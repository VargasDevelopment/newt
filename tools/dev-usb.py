#!/usr/bin/env python3
"""Bounded USB bridge client for a live local port and build."""
import argparse
from datetime import datetime, timezone
import errno
import json
from pathlib import Path
import re
import secrets
import sys
import time
import zlib

from host_common import ROOT, png_from_pbm
from bench_lease import lease_port
from dev_usb_panel_telemetry import (
    MAX_PRESS_TELEMETRY, collect_press_telemetry, decode_panel_ring)
from dev_usb_firmware_rows import MAX_LISTEN_SECONDS, FirmwareRows, listen
from dev_usb_wake import decode_wake_timing
from dev_usb_put import card_target, plan, put_files

RECORD = re.compile(rb'!N1 ([0-9]+) ([0-9]+) ([0-9]+) (.*) \*([0-9a-f]{8})\n\Z')
MAX_AUDIT_RECORDS = 2048  # Covers both directions of 32 PRESSes and two full captures.
RESYNC_QUIET_SECONDS = 4.1  # PUT's 3 s receive bound, 1 s statistics output, and cleanup margin.
FAILURE_TAIL_SECONDS = 1.1  # Includes the firmware's bounded best-effort PUT_STATS output.
CAPS = ('press,status,frame', 'press,status,frame,diag', 'press,status,frame,diag2',
        'press,status,frame,diag2,put')


def timestamp():
    return {'utc': datetime.now(timezone.utc).isoformat(), 'monotonic': time.monotonic()}


def decode_record(line):
    """Ignore ordinary logs; reject malformed/checksum-damaged protocol lines."""
    if not line.startswith(b'!N1 '):
        return None
    match = RECORD.fullmatch(line)
    if len(line) > 512 or not match or zlib.crc32(line[:line.rfind(b' *')]) != int(match[5], 16):
        raise ProtocolDirty('Damaged protocol record; restart the session')
    numbers = tuple(int(match[n]) for n in (1, 2, 3))
    if any(number > 0xffffffff or str(number).encode() != match[n]
           for n, number in enumerate(numbers, 1)):
        raise ValueError('Malformed protocol identity')
    return numbers + (match[4].decode('ascii'),)


def decode_fields(body, required):
    fields = {}
    for token in body.split()[1:]:
        if '=' not in token:
            raise ValueError('Malformed protocol metadata')
        name, value = token.split('=', 1)
        if not name or not value or name in fields:
            raise ValueError('Empty or duplicate protocol metadata')
        fields[name] = value
    if not required.issubset(fields):
        raise ValueError('Missing required protocol metadata')
    return fields


def decode_diagnostics(body, version=1):
    """Validate the exact bounded diagnostic schema before reporting boot facts."""
    if not body.startswith('DIAG '):
        raise ValueError('Malformed diagnostic response kind')
    if type(version) is not int or version not in (1, 2):
        raise ValueError('Unsupported diagnostic version')
    fields = decode_fields(body, {'available'} | ({'version'} if version == 2 else set()))
    if version == 2 and fields['version'] != '2':
        raise ValueError('Wrong diagnostic version')
    if body.split()[0] != 'DIAG' or fields['available'] not in ('0', '1'):
        raise ValueError('Malformed diagnostic availability')
    if fields['available'] == '0':
        if set(fields) != {'available'} | ({'version'} if version == 2 else set()):
            raise ValueError('Unexpected unavailable diagnostic fields')
        return {'available': False, 'version': 2} if version == 2 else {'available': False}
    boolean = {'available', 'restored', 'checkpoint_saved', 'reader_open', 'progress_ready'}
    numeric = {'reset', 'cause', 'record', 'requested', 'book', 'spine', 'character',
               'reader_status', 'actual_spine', 'actual_character', 'first_input'}
    if version == 2:
        boolean |= {'opening'}
        numeric |= {'version', 'current_book', 'heap', 'largest', 'library_count', 'library_selected'}
    if set(fields) != boolean | numeric | ({'device'} if version == 2 else set()):
        raise ValueError('Missing or unexpected diagnostic metadata')
    result = {}
    for name, value in fields.items():
        if name == 'device':
            if not re.fullmatch(r'[0-9a-f]{16}', value):
                raise ValueError('Malformed diagnostic device fingerprint')
            result[name] = value
            continue
        if not re.fullmatch(r'0|[1-9][0-9]*', value):
            raise ValueError('Malformed diagnostic integer')
        limit = 1 if name in boolean else (0xffffffffffffffff if name in ('book', 'current_book') else
                                           255 if name == 'first_input' else 0xffffffff)
        number = int(value)
        if number > limit:
            raise ValueError('Diagnostic integer out of range')
        result[name] = bool(number) if name in boolean else number
    if version == 2 and (result['library_count'] > 128 or result['library_selected'] > 127 or
                        (result['library_selected'] >= result['library_count'] if result['library_count'] else
                         result['library_selected'] != 0)):
        raise ValueError('Diagnostic Library position out of range')
    return result


class ProtocolRejected(ValueError):
    """The current peer explicitly rejected this exact request."""


class ProtocolTimeout(ValueError):
    """No complete protocol record arrived before the deadline."""


class ProtocolDirty(ValueError):
    """An unfinished prior exchange needs a bounded, read-only resynchronization."""


class ProtocolClient:
    def __init__(self, timeout=3, randbelow=secrets.randbelow):
        self.timeout = timeout
        self.buffer = bytearray()
        self.boot = self.session = self.request_id = 0
        self.capabilities = frozenset()
        self.randbelow = randbelow
        self.audit = []
        self.dirty_drain_lines = 0
        self.untraced = ()  # response prefixes summarised by their caller instead
        self.press_telemetry = []
        self.active_press = None
        self.pre_press_buffer = 0
        self.firmware_rows = FirmwareRows()
        self.connection = {'transport': 'protocol', 'closed': False, 'events': []}

    def connection_event(self, event):
        if len(self.connection['events']) < 16:
            self.connection['events'].append({'event': event, **timestamp()})

    def receipt(self):
        return {'connection': self.connection,
                'protocol': {'boot': self.boot, 'session': self.session,
                             'last_request_id': self.request_id, 'records': self.audit}}

    def trace(self, direction, boot, session, identifier, kind, body):
        if len(self.audit) >= MAX_AUDIT_RECORDS:
            raise ValueError('Protocol audit limit reached; start a new connection')
        self.audit.append({'direction': direction, 'boot': boot, 'session': session,
                           'request_id': identifier, 'kind': kind, 'body': body, **timestamp()})

    def read_data(self, remaining):
        raise NotImplementedError

    def write_data(self, data, remaining):
        raise NotImplementedError

    def record(self, deadline):
        while time.monotonic() < deadline:
            if b'\n' in self.buffer:
                line, _, rest = self.buffer.partition(b'\n')
                self.buffer = bytearray(rest)
                preexisting = self.pre_press_buffer > 0
                self.pre_press_buffer = max(0, self.pre_press_buffer - len(line) - 1)
                record = decode_record(bytes(line) + b'\n')
                if record is not None:
                    if not record[3].startswith(self.untraced):
                        self.trace('response', *record[:3], record[3].partition(' ')[0], record[3])
                    return record
                self.firmware_rows.keep(bytes(line))
                if self.active_press is not None and not preexisting:
                    # Store only the contract's literal API/driver tags, never
                    # arbitrary logs, book content or inferred refresh modes.
                    collect_press_telemetry(line,self.active_press,self.press_telemetry)
                continue
            data = self.read_data(max(0, deadline - time.monotonic()))
            self.buffer.extend(data)
            if len(self.buffer) > 8192:
                raise ValueError('Unbounded peer output')
        raise ProtocolTimeout('Protocol response timed out')

    def send(self, text, deadline):
        data = text.encode('ascii') + b'\n'
        if len(data) > 80 or b'\n' in data[:-1]:
            raise ValueError('Command exceeds the bounded wire format')
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise ValueError('Command deadline expired before write')
        tokens = text.split()
        if len(tokens) == 2:
            self.trace('request', None, None, 0, 'HELLO', text.partition(' ')[2])
        else:
            self.trace('request', *map(int, tokens[1:4]), tokens[4], text.split(' ', 4)[4])
        self.write_data(data, remaining)

    def drain_responses(self, seconds):
        """Read-only error tail; records remain in the audit and authorize nothing."""
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            try:
                self.record(deadline)
            except ProtocolDirty:
                self.dirty_drain_lines = min(self.dirty_drain_lines + 1, 0xffffffff)
                continue
            except ProtocolTimeout:
                break

    def establish_session(self, expected_build):
        try:
            return self.hello(expected_build)
        except (ProtocolDirty, ProtocolTimeout) as error:
            self.trace('recovery', None, None, 0, 'RESYNC', str(error))
        # Never replay an action or FILE/DATA/DONE. Let an interrupted payload
        # lease expire, drain its replies, then delimit an ordinary partial line.
        self.drain_responses(RESYNC_QUIET_SECONDS)
        self.buffer.clear()
        self.write_data(b'\n', self.timeout)
        return self.hello(expected_build, recovering=True)

    def hello(self, expected_build=None, *, recovering=False):
        self.capabilities = frozenset()
        previous = None
        for _ in range(2):
            deadline = time.monotonic() + self.timeout
            self.send('N1 HELLO', deadline)
            while True:
                try:
                    boot, session, identifier, body = self.record(deadline)
                except ProtocolDirty:
                    if recovering and time.monotonic() < deadline:
                        continue
                    raise
                if identifier == 0 and session != 0 and body.startswith('HELLO '):
                    break
                if not recovering or time.monotonic() >= deadline:
                    raise ProtocolDirty('Expected a fresh handshake')
            fields = decode_fields(body, {'caps', 'wake', 'build'})
            if fields.get('caps') not in CAPS or fields.get('wake') != 'physical-only':
                raise ValueError('Unsupported bridge capabilities')
            if expected_build is not None and fields.get('build') != expected_build:
                raise ValueError('Connected build differs from the explicitly selected image')
            if previous and (boot != previous[0] or session != previous[1] + 1 or body != previous[2]):
                raise ValueError('Handshake boot changed or session did not advance exactly once')
            previous = boot, session, body
        # Reserve at least 65536 monotonically increasing IDs. Historical
        # low-ID transcripts cannot satisfy this independently selected request.
        seed = self.randbelow(0x7ffeffff)
        if type(seed) is not int or not 0 <= seed < 0x7ffeffff:
            raise ValueError('Invalid request identity entropy')
        self.boot, self.session, self.request_id = boot, session, 0x80000000 + seed
        self.request('STATUS')  # Prove the random identity before any optional action.
        self.capabilities = frozenset(fields['caps'].split(','))
        return body

    def request(self, command, capture_deadline=None):
        if command not in ('STATUS', 'FRAME', 'DIAG', 'PANEL', 'WAKE') and not re.fullmatch(
                r'PRESS (previous|next|activate|back|sleep|purge-cache)|CHUNK [0-9]+ [0-9]+', command):
            raise ValueError('Unsupported bounded command')
        self.request_id += 1
        if self.request_id > 0xffffffff:
            raise ValueError('Session request IDs exhausted; request a fresh handshake')
        deadline = time.monotonic() + self.timeout
        if capture_deadline is not None:
            deadline = min(deadline, capture_deadline)
        if deadline <= time.monotonic():
            raise ValueError('Capture deadline expired')
        press = command.startswith('PRESS ')
        written = False
        records = []
        self.active_press = (self.boot, self.session, self.request_id) if press else None
        self.pre_press_buffer = len(self.buffer) if press else 0
        try:
            self.send(f'N1 {self.boot} {self.session} {self.request_id} {command}', deadline)
            written = True  # Host write returned; device consumption remains unknown.
            expected = self.boot, self.session, self.request_id
            while True:
                boot, session, identifier, body = self.record(deadline)
                if (boot, session, identifier) != expected:
                    raise ValueError('Stale or unrelated protocol response')
                if body.startswith('ERROR '):
                    raise ProtocolRejected(body)
                if press:
                    if len(records) < 2:
                        if body != ('ACCEPTED', 'CONSUMED')[len(records)]:
                            raise ValueError('Wrong press acknowledgement order')
                    elif not re.fullmatch(r'(RENDER_RETURN|NO_VISUAL_CHANGE) gen=[0-9]+', body):
                        raise ValueError('Expected terminal press completion')
                elif not body.startswith(command.split()[0] + ' '):
                    raise ValueError('Wrong response kind')
                records.append(body)
                if not press or len(records) == 3:
                    return records
        except ProtocolRejected:
            raise
        except (ValueError, OSError, KeyboardInterrupt) as error:
            if press:
                uncertain = ValueError('PRESS completion unknown; do not retry automatically. '
                                       'Establish a fresh handshake and inspect status/capture first.')
                uncertain.press_delivery = {'host_write_completed': written,
                                            'validated_replies': len(records)}
                raise uncertain from error
            raise
        finally:
            self.active_press = None
            self.pre_press_buffer = 0

    def diagnostics(self):
        # Installed baselines without DIAG remain usable; never probe an
        # unadvertised command or reinterpret old STATUS as diagnostic evidence.
        if 'diag2' in self.capabilities:
            return decode_diagnostics(self.request('DIAG')[0], version=2)
        if 'diag' not in self.capabilities:
            return {'available': False}
        return decode_diagnostics(self.request('DIAG')[0])

    def capture(self):
        deadline = time.monotonic() + 14  # Leave room under the peer's 15-second hard lease.
        metadata = self.request('FRAME', deadline)[0]
        fields = decode_fields(metadata, {'format', 'polarity', 'bytes', 'chunk',
                                          'gen', 'crc', 'width', 'height'})
        if (fields.get('format') != 'pbm1' or fields.get('polarity') != 'one-black' or
                int(fields['bytes']) != 52272 or int(fields['chunk']) != 160):
            raise ValueError('Unsupported capture format')
        generation = int(fields['gen'])
        pixels = bytearray()
        for offset in range(0, 52272, 160):
            _, frame, start, data = self.request(f'CHUNK {generation} {offset}', deadline)[0].split()
            if int(frame) != generation or int(start) != offset:
                raise ValueError('Mixed capture generations or offsets')
            chunk = bytes.fromhex(data)
            if len(chunk) != min(160, 52272 - offset):
                raise ValueError('Wrong capture chunk size')
            pixels.extend(chunk)
        if zlib.crc32(pixels) != int(fields['crc'], 16):
            raise ValueError('Whole frame checksum mismatch')
        pbm = f"P4\n{fields['width']} {fields['height']}\n".encode() + pixels
        png_from_pbm(pbm)
        return fields, pbm


class SerialClient(ProtocolClient):
    def __init__(self, serial_module, port, timeout, randbelow=secrets.randbelow):
        super().__init__(timeout, randbelow)
        self.connection.update(transport='serial', port=port, opening_may_reset_device=True)
        # Linux CDC may assert both lines on open. pySerial 3.5 applies DTR
        # first: keep it high until RTS is low to avoid the C3 reset state.
        linux = sys.platform.startswith('linux')
        self.port = None
        self.connection_event('opening')
        try:
            self.port = serial_module.Serial(port=None, baudrate=115200, timeout=0.05,
                                             write_timeout=timeout)
            self.port.dtr = linux
            self.port.rts = False
            self.port.port = port
            self.port.open()
            if linux:
                try:
                    self.port.dtr = False
                except OSError as error:
                    # Match pySerial's open-time unsupported-modem handling.
                    if error.errno not in (errno.EINVAL, errno.ENOTTY):
                        raise
            self.connection_event('opened')
            self.port.reset_input_buffer()  # Discard host receive bytes, not a reset command.
        except BaseException as error:
            try:
                self.close()
            except BaseException as closure:
                closure.bridge_receipt = self.receipt()
                closure.bridge_receipt['connection']['opening_error'] = str(error) or type(error).__name__
                raise closure from error
            error.bridge_receipt = self.receipt()
            raise

    def close(self):
        if self.connection['closed']:
            return
        self.connection_event('closing')
        try:
            if self.port is not None:
                self.port.close()
        except BaseException:
            self.connection_event('close-failed')
            raise
        self.connection['closed'] = True
        self.connection_event('closed')

    def read_data(self, remaining):
        self.port.timeout = min(0.05, remaining)
        # pyserial waits for the requested size or timeout. Wait for one byte,
        # then drain only bytes already available; never pay a fill timeout for
        # every short reply in a sequential frame transfer.
        first = self.port.read(1)
        available = min(4095, self.port.in_waiting)
        return first + (self.port.read(available) if available else b'')

    def write_data(self, data, remaining):
        # A short/failed write is ambiguous. Never resend its suffix or command.
        self.port.write_timeout = remaining
        if self.port.write(data) != len(data):
            raise ValueError('Incomplete serial command write')


def approved_serial():
    try:
        import serial
    except ImportError as error:
        raise ValueError('Use .toolchain/platformio/penv/bin/python; pyserial 3.5 is already approved and installed') from error
    approved = (ROOT / '.toolchain/platformio/penv').resolve()
    if serial.__version__ != '3.5' or not Path(serial.__file__).resolve().is_relative_to(approved):
        raise ValueError('Live mode requires the existing approved project-local pyserial 3.5')
    return serial


def run_session(client, args):
    """Return evidence only after closure; operational failures retain provenance."""
    # Receipts keep 'simulation' (always False) for their readers.
    report = {'simulation': False, 'actions': []}
    try:
        report['hello'] = client.establish_session(args.expect_build)
        if args.mode == 'put':
            report['put'] = {'folder': args.folder, 'files': []}
            put_files(client, args.folder, args.files, args.timeout, report['put'])
        if args.mode == 'panel-ring':
            report['panel_ring'] = decode_panel_ring(client.request('PANEL')[0])
        for action in args.press:
            report['actions'].append(client.request('PRESS ' + action))
        if getattr(args, 'listen', 0):
            listen(client.record, args.listen, ProtocolTimeout)
        if args.mode == 'live' and args.press and args.press[-1] == 'sleep':
            report['sleep'] = 'Entry requested; render return does not prove sleep or wake. Wake requires physical power button.'
        else:
            report['status'] = client.request('STATUS')[0]
            if args.diagnostics:
                report['diagnostics'] = client.diagnostics()
            if getattr(args, 'wake_timing', False):
                report['wake_timing'] = decode_wake_timing(client.request('WAKE')[0])
        if args.output:
            fields, pbm = client.capture()
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.with_suffix('.pbm').write_bytes(pbm)
            args.output.with_suffix('.png').write_bytes(png_from_pbm(pbm))
            report['capture'] = fields
            report['image'] = str(args.output.with_suffix('.png'))
    except (ValueError, OSError, KeyboardInterrupt) as error:
        report['error'] = str(error) or 'Interrupted by operator'
        if isinstance(error, KeyboardInterrupt) or isinstance(error.__cause__, KeyboardInterrupt):
            report['interrupted'] = True
        elif args.mode == 'put':
            # The failed lease emits its statistics after its error reply.
            # Keep that bounded tail before closing, without sending anything.
            try:
                client.drain_responses(FAILURE_TAIL_SECONDS)
            except (ValueError, OSError, KeyboardInterrupt) as tail:
                report['failure_tail_error'] = str(tail) or 'Interrupted by operator'
                if isinstance(tail, KeyboardInterrupt): report['interrupted'] = True
    finally:
        try:
            client.close()
        except (ValueError, OSError, KeyboardInterrupt) as error:
            report['error'] = report.get('error', '') + f' Closure failed: {str(error) or "Interrupted by operator"}'
            if isinstance(error, KeyboardInterrupt):
                report['interrupted'] = True
        if 'put' in report:
            report['put']['receive_tail'] = {'buffered_bytes': len(client.buffer),
                'buffered_protocol_prefix': client.buffer.startswith(b'!N1 '),
                'dirty_drain_lines': client.dirty_drain_lines}
        report.update(client.receipt(), **client.firmware_rows.report())
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=['live', 'panel-ring', 'put'])
    parser.add_argument('--port', help='explicit Linux serial port; never discovered')
    parser.add_argument('--expect-build', help='exact reviewed HELLO build identity, required live')
    parser.add_argument('--timeout', type=float, default=8, help='per-command deadline in seconds (0.1–30)')
    # purge-cache is development-only: accepted on Home with the reader closed, it
    # removes derived reader caches so the next open is cold (rust/firmware/reader_image_purge.rs).
    parser.add_argument('--press', action='append', choices=['previous', 'next', 'activate', 'back', 'sleep', 'purge-cache'],
                        default=[])
    parser.add_argument('--output', type=Path, help='host capture basename')
    parser.add_argument('--diagnostics', action='store_true', help='read boot/reader diagnostics after STATUS')
    parser.add_argument('--wake-timing', action='store_true', help='read the frozen wake timeline from an opt-in diagnostic candidate')
    parser.add_argument('--listen', type=float, default=0, help='after the last press, read firmware rows (0–600 s)')
    # PUT validates every destination before opening the port.
    parser.add_argument('--folder', help='put: any card-relative directory (e.g. Newt/Books; . for card root); apps/update/dev are aliases')
    parser.add_argument('--file', action='append', type=Path, default=[], help='put: local file, repeatable; 4 KiB raw blocks, limited by free card space')
    parser.add_argument('--name', help='put: the card name for a single --file (default: its own name)')
    args = parser.parse_args()
    if not 0.1 <= args.timeout <= 30 or len(args.press) > 32 or not 0 <= args.listen <= MAX_LISTEN_SECONDS:
        parser.error('Use a 0.1–30 second deadline, at most 32 button requests and 0–600 listening seconds')
    if not args.port or '://' in args.port or not args.expect_build or not args.port.startswith('/dev/'):
        parser.error('Live mode requires an explicit Linux --port and --expect-build')
    if args.wake_timing and args.mode != 'live':
        parser.error('--wake-timing belongs to live mode')
    if args.mode == 'panel-ring' and (args.press or args.output or args.diagnostics):
        parser.error('Panel ring read accepts no actions or capture')
    if (args.mode == 'put') != bool(args.folder or args.file or args.name):
        parser.error('--folder, --file and --name belong to put, which needs --folder and --file')
    if args.mode == 'put':
        if not args.folder or args.press or args.output or args.listen:
            parser.error('put needs --folder and takes no presses, capture or listening')
        try:
            args.files = plan(args.file, args.name)
            for _, name, _ in args.files:
                card_target(args.folder, name)
        except (ValueError, OSError) as error:
            parser.error(str(error))
    if 'sleep' in args.press and (args.press[-1] != 'sleep' or args.press.count('sleep') != 1 or args.output or args.diagnostics or args.wake_timing or args.listen):
        parser.error('Live sleep must be the final request, with no subsequent capture; wake is physical only')
    with lease_port(args.port):
        client = SerialClient(approved_serial(), args.port, args.timeout)
        report = run_session(client, args)
    print(json.dumps(report, sort_keys=True))
    if 'error' in report:
        if report.get('interrupted'):
            raise KeyboardInterrupt(report['error'])
        raise ValueError(report['error'])


if __name__ == '__main__':
    try:
        main()
    except (ValueError, OSError, KeyboardInterrupt) as error:
        if hasattr(error, 'bridge_receipt'):
            print(json.dumps({'error': str(error) or 'Interrupted by operator',
                              'interrupted': isinstance(error, KeyboardInterrupt), **error.bridge_receipt}, sort_keys=True))
        print(str(error) or 'Interrupted by operator', file=sys.stderr)
        raise SystemExit(130 if isinstance(error, KeyboardInterrupt) else 2)
