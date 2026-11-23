"""dev-usb.py put: files into one fixed card folder over the bridge's PUT lease.

Each file is one lease (rust/core/src/dev_put.rs, #26): PUT, FILE, DATA lines,
DONE. The device writes a hidden temp, reads it back and checks its size and
SHA-256 before renaming it into place; this checks the device's final size
and SHA-256 again. Until DONE is sent the final name is unchanged whatever
fails. A DONE whose answer never arrives may or may not have replaced the
file, so nothing is retried: the run stops and says so.
"""
import base64
import hashlib
from pathlib import Path
import re
import time

FOLDERS = ('apps', 'update', 'dev')
NAME = re.compile(r'[A-Za-z0-9][A-Za-z0-9._-]{0,31}')
CHUNK_BYTES = 3072
LINE_BYTES = 4160
CAP_BYTES = 0x770000  # one app slot, so a bench newt.bin fits
OFFER = f'PUT line={LINE_BYTES} chunk={CHUNK_BYTES} cap={CAP_BYTES}'
MAX_FILES = 64
FILE_SECONDS = 30  # FILE may first count the card's free clusters
READ_BACK_RATE = 100_000  # bytes/s the device reads the temp back at, at least


class Refused(ValueError):
    """The device answered this request with ERROR; the final name is unchanged."""


class CommitUnknown(ValueError):
    """DONE was sent and no answer arrived: the file may or may not be in place."""


def card_name(name):
    if not NAME.fullmatch(name) or '..' in name:
        raise ValueError(f'Card name {name!r} must be 1-32 of A-Z a-z 0-9 . _ -, '
                         'start with a letter or digit and hold no ..')
    return name


def plan(paths, name=None):
    """Each (local path, card name, bytes), all checked before the port opens."""
    if not 1 <= len(paths) <= MAX_FILES:
        raise ValueError(f'Put 1 to {MAX_FILES} files')
    if name is not None and len(paths) != 1:
        raise ValueError('--name needs exactly one --file')
    files = []
    for path in paths:
        data = Path(path).read_bytes()
        if len(data) > CAP_BYTES:
            raise ValueError(f'{path} is larger than the {CAP_BYTES}-byte card write cap')
        files.append((str(path), card_name(name or Path(path).name), data))
    if len({card for _, card, _ in files}) != len(files):
        raise ValueError('Two files share a card name')
    return files


class Lease:
    """Lease lines over the client's open session, IDs and framing."""

    def __init__(self, client):
        self.client = client
        self.written = False  # the last line's host write returned

    def exchange(self, body, seconds, kind):
        client = self.client
        client.request_id += 1
        if client.request_id > 0xffffffff:
            raise ValueError('Session request IDs exhausted; request a fresh handshake')
        identity = client.boot, client.session, client.request_id
        data = f'N1 {client.boot} {client.session} {client.request_id} {body}\n'.encode('ascii')
        if len(data) > LINE_BYTES + 1:
            raise ValueError('Lease line exceeds its bound')
        if kind != 'DATA':  # thousands of DATA lines are summarised, not traced
            client.trace('request', *identity, kind, body)
        deadline = time.monotonic() + seconds
        self.written = False
        client.write_data(data, seconds)
        self.written = True
        boot, session, identifier, reply = client.record(deadline)
        if (boot, session, identifier) != identity:
            raise ValueError('Stale or unrelated protocol response')
        if reply.startswith('ERROR '):
            raise Refused(reply)
        if reply.split(' ', 1)[0] != kind:
            raise ValueError('Wrong response kind')
        return reply


def put_file(lease, folder, name, data, timeout):
    digest = hashlib.sha256(data).hexdigest()
    started = time.monotonic()
    offer = lease.exchange('PUT', timeout, 'PUT')
    if offer != OFFER:
        raise ValueError(f'Unsupported card write lease: {offer}')
    answer = re.fullmatch(r'FILE free=(0|[1-9][0-9]*)',
                          lease.exchange(f'FILE {folder} {name} {len(data)} {digest}',
                                         max(timeout, FILE_SECONDS), 'FILE'))
    if not answer:
        raise ValueError('Malformed FILE answer')
    chunks = range(0, len(data), CHUNK_BYTES)
    for offset in chunks:
        chunk = data[offset:offset + CHUNK_BYTES]
        reply = lease.exchange(f'DATA {offset} {base64.b64encode(chunk).decode()}', timeout, 'DATA')
        if reply != f'DATA {offset + len(chunk)}':
            raise ValueError('Wrong DATA acknowledgement')
    client = lease.client
    client.trace('request', client.boot, client.session, client.request_id, 'DATA',
                 f'{len(chunks)} lines, {len(data)} bytes, each acknowledged in order')
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
            try:
                entry.update(put_file(lease, folder, name, data, timeout), result='written')
            except CommitUnknown as error:
                entry.update(result='unknown', host_write_completed=error.host_write_completed)
                raise
    finally:
        client.untraced = ()
    total = sum(entry['size'] for entry in report['files'])
    seconds = sum(entry['seconds'] for entry in report['files'])
    report['bytes_per_second'] = round(total / seconds) if seconds else None
