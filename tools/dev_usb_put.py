"""DEV PUT: validated card-relative paths and acknowledged raw blocks.

Only DONE can replace the final name, after sync and card read-back SHA-256.
An unknown DONE is never retried. Raw blocks use the scoped hardware FIFO
receiver with USB backpressure; the ordinary SDK RX queue stays unchanged.
"""
import hashlib
from pathlib import Path
import re
import time
from fatfs_case import name_key

CHUNK_BYTES = 4096
LINE_BYTES = 4351
OFFER = f'PUT line={LINE_BYTES} chunk={CHUNK_BYTES} path=hex data=raw'
MAX_FILES = 64
# Generated FatFs short aliases contain '~'; a batch must never target one.
FORBIDDEN_NAME_CHARS = '<>:"/\\|?*~'
FILE_SECONDS = 30  # FILE may first count the card's free clusters
READ_BACK_RATE = 100_000  # bytes/s the device reads the temp back at, at least


class Refused(ValueError):
    """The device answered this request with ERROR; the final name is unchanged
    unless the answer is recovery-needed."""


class RecoveryNeeded(ValueError):
    """The device cannot prove a commit finished or undone; the card needs a filesystem check."""


class CommitUnknown(ValueError):
    """DONE was sent and no answer arrived: the file may or may not be in place."""


def card_name(name):
    stem = name.split('.', 1)[0].upper()
    device = stem in ('CON', 'PRN', 'AUX', 'NUL') or re.fullmatch(r'(COM|LPT)[1-9]', stem)
    if (not name or name.lower() in ('newt.put', 'newt.old', 'newt.mrk', '.newt-put', '.newt-old', '.newt-old-name') or len(name.encode('utf-16-le')) > 510 or device or '..' in name
            or name.endswith(('.', ' ')) or any(ord(c) < 32 or 127 <= ord(c) <= 159 or c in FORBIDDEN_NAME_CHARS for c in name)):
        raise ValueError(f'Invalid FAT card name {name!r}')
    return name


def card_folder(folder):
    # Preserve the original CLI spellings; all other paths are card-relative.
    folder = {'apps': 'Newt/Apps', 'update': 'Newt/Update', 'dev': 'Newt/Dev'}.get(folder, folder)
    if folder == '.':
        return ''
    for component in folder.split('/'):
        card_name(component)
    return folder


def card_target(folder, name):
    folder = card_folder(folder)
    target = f'{folder}/{card_name(name)}' if folder else card_name(name)
    if len(('/newt-sd/' + target).encode()) > 511:
        raise ValueError('Card path exceeds the native 511-byte path bound')
    return target


def plan(paths, name=None):
    """Each (local path, card name, bytes), all checked before the port opens."""
    if not 1 <= len(paths) <= MAX_FILES:
        raise ValueError(f'Put 1 to {MAX_FILES} files')
    if name is not None and len(paths) != 1:
        raise ValueError('--name needs exactly one --file')
    files = []
    for path in paths:
        data = Path(path).read_bytes()
        files.append((str(path), card_name(name or Path(path).name), data))
    # Use the card's table, including Unicode aliases Python lower() misses.
    if len({name_key(card) for _, card, _ in files}) != len(files):
        raise ValueError('Two files share a card name (FAT ignores case)')
    return files


class Lease:
    """Lease frames over the client's open session, IDs and framing."""

    def __init__(self, client):
        self.client = client
        self.written = False  # the last line's host write returned
        self.progress = {}

    def exchange(self, body, seconds, kind, payload=b''):
        client = self.client
        client.request_id += 1
        if client.request_id > 0xffffffff:
            raise ValueError('Session request IDs exhausted; request a fresh handshake')
        identity = client.boot, client.session, client.request_id
        started = time.monotonic()
        self.progress['last_sent'] = {'kind': kind, 'id': client.request_id, 'monotonic': started,
                                      'write_completed': False}
        self.progress['write_seconds'] = 0
        if kind == 'DATA':
            _, offset, length = body.split()
            self.progress['last_sent'].update(offset=int(offset), length=int(length))
        data = f'N1 {client.boot} {client.session} {client.request_id} {body}\n'.encode('ascii')
        if len(data) + len(payload) > LINE_BYTES + 1:
            raise ValueError('Lease frame exceeds its bound')
        data += payload
        if kind != 'DATA':  # thousands of DATA frames are summarised, not traced
            client.trace('request', *identity, kind, body)
        deadline = time.monotonic() + seconds
        self.written = False
        try:
            client.write_data(data, seconds)
            self.written = True
            self.progress['last_sent']['write_completed'] = True
            self.progress['write_seconds'] = time.monotonic() - started
            boot, session, identifier, reply = client.record(deadline)
        except (ValueError, OSError, KeyboardInterrupt) as error:
            self.progress.update(error=str(error) or 'Interrupted by operator',
                                 elapsed_seconds=time.monotonic() - started,
                                 buffered_bytes=len(client.buffer),
                                 buffered_protocol_prefix=client.buffer.startswith(b'!N1 '))
            raise
        self.progress.update(elapsed_seconds=time.monotonic() - started,
                             last_response={'boot': boot, 'session': session, 'id': identifier, 'body': reply,
                                            'monotonic': time.monotonic()})
        if (boot, session, identifier) != identity:
            raise ValueError('Stale or unrelated protocol response')
        if reply.startswith('ERROR '):
            raise Refused(reply)
        if reply.split(' ', 1)[0] != kind:
            raise ValueError('Wrong response kind')
        if kind == 'DATA' and reply != f'DATA {int(offset) + int(length)}':
            raise ValueError('Wrong DATA acknowledgement')
        self.progress['last_ack'] = {'kind': kind, 'id': identifier, 'monotonic': time.monotonic()}
        if kind == 'DATA':
            self.progress['last_ack']['offset'] = int(offset) + int(length)
            self.progress['last_data_ack'] = dict(self.progress['last_ack'])
            self.progress['data_frames'] = self.progress.get('data_frames', 0) + 1
            self.progress['data_seconds'] = self.progress.get('data_seconds', 0) + time.monotonic() - started
        return reply


def put_file(lease, folder, name, data, timeout):
    target = card_target(folder, name)
    digest = hashlib.sha256(data).hexdigest()
    started = time.monotonic()
    offer = lease.exchange('PUT', timeout, 'PUT')
    if offer != OFFER:
        raise ValueError(f'Unsupported card write lease: {offer}')
    answer = re.fullmatch(r'FILE free=(0|[1-9][0-9]*)',
                          lease.exchange(f'FILE {target.encode().hex()} {len(data)} {digest}',
                                         max(timeout, FILE_SECONDS), 'FILE'))
    if not answer:
        raise ValueError('Malformed FILE answer')
    chunks = range(0, len(data), CHUNK_BYTES)
    for offset in chunks:
        chunk = data[offset:offset + CHUNK_BYTES]
        lease.exchange(f'DATA {offset} {len(chunk)}', timeout, 'DATA', chunk)
    client = lease.client
    client.trace('request', client.boot, client.session, client.request_id, 'DATA',
                 f'{len(chunks)} frames, {len(data)} bytes, each acknowledged in order')
    try:
        reply = lease.exchange('DONE', max(timeout, FILE_SECONDS) + len(data) / READ_BACK_RATE, 'DONE')
    except Refused:
        raise
    except (ValueError, OSError, KeyboardInterrupt) as error:
        unknown = CommitUnknown('DONE completion unknown; do not retry automatically. '
                                'Establish a fresh handshake and check the card first.')
        unknown.host_write_completed = lease.written
        raise unknown from error
    if reply != f'DONE size={len(data)} sha256={digest}':
        unknown = CommitUnknown(f'The device placed a different file ({reply}); check the card.')
        unknown.host_write_completed = True
        raise unknown
    seconds = time.monotonic() - started
    return {'free_before': int(answer[1]), 'seconds': round(seconds, 3)}


def put_files(client, folder, files, timeout, report):
    """Each file in order; the first failure stops the run with its result."""
    if 'put' not in client.capabilities:
        raise ValueError('This build has no card write (HELLO caps lack put)')
    lease = Lease(client)
    client.untraced = ('DATA ',)
    try:
        for local, name, data in files:
            entry = {'local': local, 'name': name, 'size': len(data),
                     'sha256': hashlib.sha256(data).hexdigest(), 'result': 'unchanged'}
            report['files'].append(entry)
            lease.progress = entry.setdefault('protocol_progress', {})
            try:
                entry.update(put_file(lease, folder, name, data, timeout), result='written')
            except CommitUnknown as error:
                entry.update(result='unknown', host_write_completed=error.host_write_completed)
                raise
            except Refused as error:
                if not str(error).startswith('ERROR recovery-needed'):
                    raise
                entry['result'] = 'recovery-needed'
                raise RecoveryNeeded(f'{error}: the card keeps its reserved files and marker for a '
                                     'hand check; see "Card write" in docs/dev-usb-bridge.md.') from error
    except (ValueError, OSError, KeyboardInterrupt) as error:
        lease.progress.setdefault('error', str(error) or 'Interrupted by operator')
        raise
    finally:
        client.untraced = ()
    total = sum(entry['size'] for entry in report['files'])
    seconds = sum(entry['seconds'] for entry in report['files'])
    report['bytes_per_second'] = round(total / seconds) if seconds else None
