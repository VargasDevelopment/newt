"""Fake serial-driver tests: never imports pyserial or opens a device."""
import errno
import importlib.util
from pathlib import Path
import subprocess
import sys
import time
import zlib
from types import SimpleNamespace
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))

spec = importlib.util.spec_from_file_location('dev_usb_client', ROOT / 'tools/dev-usb.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def rejects(callback, text):
    try:
        callback()
    except (ValueError, OSError) as error:
        assert text in str(error), str(error)
    else:
        raise AssertionError('Expected bounded protocol failure')


def serial_open_validation(platform='linux', post_error=None, linux_driver=True):
    device = SimpleNamespace(lines={'dtr': False, 'rts': False}, resets=0, opens=0)
    instances = []

    class Driver:
        """Linux CDC asserts both lines; pinned pySerial applies DTR first."""
        def __init__(self, *, port, **settings):
            assert port is None
            self.is_open = False
            self._dtr = self._rts = True
            self.lines = device.lines
            self.states = []
            self.post_writes = []
            self.close_calls = 0
            self.input_resets = 0
            instances.append(self)

        def apply(self, name, value):
            self.lines[name] = value
            self.states.append((self.lines['rts'], self.lines['dtr']))
            device.resets += self.lines == {'rts': True, 'dtr': False}

        @property
        def dtr(self):
            return self._dtr

        @dtr.setter
        def dtr(self, value):
            self._dtr = value
            if self.is_open:
                self.post_writes.append(value)
                if post_error is not None:
                    raise OSError(post_error, 'mock modem ioctl')
                self.apply('dtr', value)

        @property
        def rts(self):
            return self._rts

        @rts.setter
        def rts(self, value):
            self._rts = value
            if self.is_open:
                self.apply('rts', value)

        def open(self):
            device.opens += 1
            self.presets = (self.dtr, self.rts)
            if linux_driver:
                self.lines.update(dtr=True, rts=True)
            self.is_open = True
            self.apply('dtr', self.dtr)
            self.apply('rts', self.rts)

        def reset_input_buffer(self):
            self.input_resets += 1
            assert not self.lines['rts']

        def close(self):
            self.close_calls += 1
            self.lines.update(dtr=False, rts=False)
            self.is_open = False

    with patch.object(module.sys, 'platform', platform):
        linux = platform.startswith('linux')
        fails = linux and post_error == errno.EIO
        for _ in range(2):
            try:
                peer = module.SerialClient(SimpleNamespace(Serial=Driver), '/dev/mock-only', 1)
            except OSError as error:
                assert fails and error.errno == errno.EIO, error
                driver = instances[-1]
                assert not driver.is_open and driver.close_calls == 1 and driver.input_resets == 0
                assert error.bridge_receipt['connection']['closed']
                return
            assert not fails, 'EIO must fail closed'
            assert device.resets == 0, 'opening reset the C3'
            assert peer.port.presets == (linux, False)
            assert peer.port.post_writes == ([False] if linux else [])
            states = [(True, True), (False, True)] if linux_driver else [(False, False)] * 2
            if linux and post_error is None:
                states.append((False, False))
            assert peer.port.states == states
            assert peer.port.input_resets == 1
            peer.close()
            assert not peer.port.is_open and peer.connection['closed'] and peer.port.close_calls == 1
            assert device.lines == {'dtr': False, 'rts': False}
        assert device.opens == 2 and len(instances) == 2


def serial_open_control():
    try:
        # The unchanged non-Linux preset reproduces the old Linux ordering.
        serial_open_validation(platform='darwin')
    except AssertionError as error:
        assert str(error) == 'opening reset the C3', str(error)
    else:
        raise AssertionError('Control did not detect the old reset sequence')


def command_bound_validation():
    class ObservedClient(module.ProtocolClient):
        def __init__(self):
            super().__init__(1)
            self.writes = []
        def write_data(self, data, remaining):
            assert remaining > 0
            self.writes.append(data)
    peer = ObservedClient()
    longest = 'N1 4294967295 4294967295 4294967295 CHUNK 4294967295 4294967295'
    assert len(longest) == 63
    peer.send(longest, time.monotonic() + 1)
    assert peer.writes == [longest.encode() + b'\n']
    assert peer.audit[-1]['body'] == 'CHUNK 4294967295 4294967295'
    for command in ('STATUS', 'FRAME', 'DIAG', 'PRESS previous', 'PRESS activate',
                    'PRESS next', 'PRESS back', 'PRESS sleep'):
        text = 'N1 4294967295 4294967295 4294967295 ' + command
        peer.send(text, time.monotonic() + 1)
        assert peer.writes[-1] == text.encode() + b'\n' and peer.audit[-1]['body'] == command
    prefix = 'N1 4294967295 4294967295 4294967295 '
    boundary = prefix + 'A' * (79 - len(prefix))
    peer.send(boundary, time.monotonic() + 1)
    assert len(peer.writes[-1]) == 80 and peer.audit[-1]['body'] == boundary.split(' ', 4)[4]
    writes, records = len(peer.writes), len(peer.audit)
    rejects(lambda: peer.send(boundary + 'A', time.monotonic() + 1), 'bounded wire format')
    assert len(peer.writes) == writes and len(peer.audit) == records


class DiagnosticPeer(module.ProtocolClient):
    """Bounded in-memory wire peer: no serial driver or application mutation."""
    def __init__(self, diagnostic, boot=12345, capabilities='press,status,frame,diag', stale=False):
        super().__init__(0.1, randbelow=lambda size: 17)
        self.diagnostic = diagnostic
        self.peer_boot = boot
        self.peer_capabilities = capabilities
        self.stale = stale
        self.writes = []
        self.pending = b''
        self.peer_session = 0

    def write_data(self, data, remaining):
        self.writes.append(data)
        tokens = data.decode().split()
        if tokens == ['N1', 'HELLO']:
            self.peer_session += 1
            identifier = 0
            body = f'HELLO build=diagnostic-fixture caps={self.peer_capabilities} wake=physical-only'
        else:
            identifier = int(tokens[3])
            if tokens[4] == 'STATUS':
                body = 'STATUS screen=0 gen=1 ready=1 refresh_ms=0 heap=0 dropped=0 capture=0'
            else:
                assert tokens[4:] == ['DIAG']
                if self.diagnostic is None:
                    return
                body = self.diagnostic
                if self.stale:
                    identifier -= 1
        payload = f'!N1 {self.peer_boot} {self.peer_session} {identifier} {body}'.encode()
        self.pending = payload + f' *{zlib.crc32(payload):08x}\n'.encode()

    def read_data(self, remaining):
        result, self.pending = self.pending, b''
        if not result:
            time.sleep(min(0.005, remaining))
        return result


def diagnostic_validation():
    body = ('DIAG available=1 reset=3 cause=2 record=1 requested=1 restored=1 '
            'checkpoint_saved=1 book=18446744073709551615 spine=7 character=123 '
            'reader_status=2 reader_open=1 progress_ready=1 actual_spine=7 '
            'actual_character=123 first_input=255')
    # Fresh sessions after reconnect retain each boot's exact facts and never
    # send a PRESS, FRAME, or CHUNK while obtaining status and diagnostics.
    for boot, reset in ((12345, 3), (12346, 1)):
        peer = DiagnosticPeer(body.replace('reset=3', f'reset={reset}'), boot)
        peer.hello('diagnostic-fixture')
        status = peer.request('STATUS')
        fields = peer.diagnostics()
        assert fields['reset'] == reset and fields['book'] == 0xffffffffffffffff
        assert fields['first_input'] == 255 and fields['actual_character'] == 123
        assert peer.request('STATUS') == status
        assert [data.decode().split()[-1] for data in peer.writes] == ['HELLO', 'HELLO', 'STATUS', 'STATUS', 'DIAG', 'STATUS']
    old = DiagnosticPeer(None, capabilities='press,status,frame')
    old.hello('diagnostic-fixture')
    old.request('STATUS')
    assert old.diagnostics() == {'available': False} and len(old.writes) == 4
    unavailable = DiagnosticPeer('DIAG available=0')
    unavailable.hello()
    assert unavailable.diagnostics() == {'available': False}
    for malformed in ('', 'STATUS available=1', 'DIAG available=2',
                      'DIAG available=0 reset=1', body.replace(' spine=7', ''),
                      body + ' extra=1', body + ' reset=3',
                      body.replace('reset=3', 'reset=03'),
                      body.replace('restored=1', 'restored=2'),
                      body.replace('first_input=255', 'first_input=256'),
                      body.replace('character=123', 'character=4294967296'),
                      body.replace('book=18446744073709551615', 'book=18446744073709551616')):
        try:
            module.decode_diagnostics(malformed)
        except ValueError:
            pass
        else:
            raise AssertionError('Malformed diagnostic facts accepted')
    for diagnostic, stale, error in ((body, True, 'Stale'),
                                     (None, False, 'timed out'),
                                     (body.replace(' spine=7', ''), False, 'diagnostic metadata')):
        peer = DiagnosticPeer(diagnostic, stale=stale)
        peer.hello()
        status = peer.request('STATUS')
        started = time.monotonic()
        rejects(peer.diagnostics, error)
        assert time.monotonic() - started < 0.5
        assert peer.request('STATUS') == status
        assert sum(b' DIAG' in data for data in peer.writes) == 1


def diagnostic_v2_validation():
    def framed(boot, session, identifier, body):
        payload = f'!N1 {boot} {session} {identifier} {body}'.encode()
        return payload + f' *{zlib.crc32(payload):08x}\n'.encode()
    legacy = ('DIAG available=1 reset=4294967295 cause=4294967295 record=4294967295 '
              'requested=4294967295 restored=1 checkpoint_saved=1 book=18446744073709551615 '
              'spine=4294967295 character=4294967295 reader_status=4294967295 reader_open=1 '
              'progress_ready=1 actual_spine=4294967295 actual_character=4294967295 first_input=255')
    body = legacy.replace('DIAG ', 'DIAG version=2 ', 1) + (
        ' current_book=18446744073709551615 heap=4294967295 largest=4294967295 opening=1'
        ' library_count=128 library_selected=127 device=ffffffffffffffff')
    assert len(body.encode()) == 459
    wire = framed(0xffffffff, 0xffffffff, 0xffffffff, body)
    assert len(wire) == 507 and module.decode_record(wire) == (0xffffffff, 0xffffffff, 0xffffffff, body)
    decoded = module.decode_diagnostics(body, version=2)
    assert decoded['version'] == 2 and decoded['current_book'] == 0xffffffffffffffff
    assert decoded['heap'] == decoded['largest'] == 0xffffffff and decoded['opening'] is True
    assert decoded['library_count'] == 128 and decoded['library_selected'] == 127 and decoded['device'] == 'ffffffffffffffff'
    for count in (0, 1):
        empty_or_first = body.replace('library_count=128', f'library_count={count}').replace('library_selected=127', 'library_selected=0')
        fields = module.decode_diagnostics(empty_or_first, version=2)
        assert fields['library_count'] == count and fields['library_selected'] == 0
    for boot in (12345, 12346):
        peer = DiagnosticPeer(body, boot, capabilities='press,status,frame,diag2')
        peer.hello('diagnostic-fixture')
        assert peer.diagnostics() == decoded
        assert peer.audit[-1]['body'] == body and len(peer.audit[-1]['body']) == 459
        assert [data.decode().split()[-1] for data in peer.writes] == ['HELLO', 'HELLO', 'STATUS', 'DIAG']
    unavailable = DiagnosticPeer('DIAG version=2 available=0', capabilities='press,status,frame,diag2')
    unavailable.hello()
    assert unavailable.diagnostics() == {'available': False, 'version': 2}
    for negotiated, response, error in (('diag2', legacy, 'metadata'), ('diag', body, 'diagnostic'),
                                        ('diag2', 'DIAG available=0', 'metadata'),
                                        ('diag', 'DIAG version=2 available=0', 'diagnostic')):
        peer = DiagnosticPeer(response, capabilities='press,status,frame,' + negotiated)
        peer.hello()
        rejects(peer.diagnostics, error)
        assert sum(b' DIAG' in data for data in peer.writes) == 1
    malformed = [body.replace('version=2', 'version=1'), body.replace('version=2', 'version=02'),
                 body + ' version=2', body + ' extra=1', body.replace('opening=1', 'opening=2'),
                 body.replace('current_book=18446744073709551615', 'current_book=18446744073709551616'),
                 body.replace('heap=4294967295', 'heap=4294967296'),
                 body.replace('largest=4294967295', 'largest=04294967295'),
                 body.replace('library_count=128', 'library_count=129'),
                 body.replace('library_selected=127', 'library_selected=128'),
                 body.replace('library_count=128', 'library_count=127'),
                 body.replace('library_count=128', 'library_count=0'),
                 body.replace('device=ffffffffffffffff', 'device=FFFFFFFFFFFFFFFF'),
                 body.replace('device=ffffffffffffffff', 'device=fffffffffffffff'),
                 body.replace('device=ffffffffffffffff', 'device=fffffffffffffffff'),
                 body.replace('device=ffffffffffffffff', 'device=fffffffffffffffg'),
                 'DIAG version=2 available=0 opening=0']
    for token in ('version=2', 'current_book=18446744073709551615', 'heap=4294967295',
                  'largest=4294967295', 'opening=1', 'library_count=128', 'library_selected=127',
                  'device=ffffffffffffffff'):
        rejects(lambda token=token: module.decode_diagnostics(body.replace(' ' + token, ''), version=2), 'metadata')
    for response in malformed:
        try:
            module.decode_diagnostics(response, version=2)
        except ValueError:
            pass
        else:
            raise AssertionError('Malformed DIAG2 facts accepted')
    rejects(lambda: module.decode_diagnostics(body), 'diagnostic')
    rejects(lambda: module.decode_diagnostics(legacy, version=2), 'metadata')
    rejects(lambda: module.decode_diagnostics(body, version=3), 'version')
    rejects(lambda: module.decode_record(framed(0xffffffff, 0xffffffff, 0xffffffff, body + ' extra=1')), 'Damaged')


def freshness_validation():
    def framed(boot, session, identifier, body):
        payload = f'!N1 {boot} {session} {identifier} {body}'.encode()
        return payload + f' *{zlib.crc32(payload):08x}\n'.encode()

    hello = 'HELLO build=diagnostic-fixture caps=press,status,frame,diag wake=physical-only'
    status = 'STATUS screen=0 gen=1 ready=1 refresh_ms=0 heap=0 dropped=0 capture=0'

    class Replay(DiagnosticPeer):
        def __init__(self, transcript):
            super().__init__('DIAG available=0')
            self.transcript = bytearray(b''.join(transcript))

        def write_data(self, data, remaining):
            self.writes.append(data)  # No newly generated peer replies.

        def read_data(self, remaining):
            data, self.transcript = bytes(self.transcript), bytearray()
            return data

        def close(self):
            self.connection['closed'] = True
            self.connection_event('closed')

    # The formerly accepted complete transcript now fails at the second HELLO.
    replay = Replay([framed(12345, 1, 0, hello), framed(12345, 1, 1, status),
                     framed(12345, 1, 2, 'DIAG available=0')])
    rejects(replay.hello, 'fresh handshake')
    assert replay.writes == [b'N1 HELLO\n', b'N1 HELLO\n']
    # Even two historical HELLOs cannot make their low-ID STATUS/DIAG current.
    replay = Replay([framed(12345, 1, 0, hello), framed(12345, 2, 0, hello),
                     framed(12345, 2, 1, status), framed(12345, 2, 2, 'DIAG available=0')])
    rejects(replay.hello, 'Stale')
    assert len(replay.writes) == 3 and replay.writes[-1].endswith(b'2147483666 STATUS\n')
    replay = Replay([framed(12345, 1, 0, hello), framed(12345, 2, 0, hello),
                     framed(12345, 2, 1, status), framed(12345, 2, 2, 'DIAG available=0')])
    args = SimpleNamespace(mode='live', expect_build='diagnostic-fixture', press=['next'],
                           diagnostics=True, output=Path('/outside-project/not-written'))
    report = module.run_session(replay, args)
    assert 'Stale' in report['error'] and report['connection']['closed']
    assert [data.decode().split()[-1] for data in replay.writes] == ['HELLO', 'HELLO', 'STATUS']
    for second_boot, second_session in ((12346, 2), (12345, 1), (12345, 3)):
        peer = Replay([framed(12345, 1, 0, hello), framed(second_boot, second_session, 0, hello)])
        rejects(peer.hello, 'boot changed or session')
        assert len(peer.writes) == 2 and not peer.capabilities
    rollover = Replay([framed(12345, 0xffffffff, 0, hello), framed(12345, 0, 0, hello)])
    rejects(rollover.hello, 'fresh handshake')
    changed_caps = Replay([framed(12345, 1, 0, hello),
                           framed(12345, 2, 0, hello.replace(',diag', ''))])
    rejects(changed_caps.hello, 'boot changed or session')
    for seed in (0, 0x7ffefffe):
        peer = DiagnosticPeer('DIAG available=0')
        peer.randbelow = lambda size, seed=seed: seed
        peer.hello()
        assert 0x80000001 <= peer.request_id <= 0xfffeffff
        assert 0xffffffff - peer.request_id >= 65536
        peer.request('STATUS')
    for seed in (-1, 0x7ffeffff, 'bad', True):
        peer = DiagnosticPeer('DIAG available=0')
        peer.randbelow = lambda size, seed=seed: seed
        rejects(peer.hello, 'entropy')
        assert not peer.capabilities and len(peer.writes) == 2
    peer = DiagnosticPeer('DIAG available=0')
    peer.hello()
    while len(peer.audit) < module.MAX_AUDIT_RECORDS:
        peer.request('STATUS')
    writes = len(peer.writes)
    rejects(lambda: peer.request('STATUS'), 'audit limit')
    assert len(peer.writes) == writes
    assert module.MAX_AUDIT_RECORDS >= 6 + 32 * 4 + 2 + 2 * (2 + 327 * 2)
    for boot, session, identifier in ((0x100000000, 1, 1), (1, 0x100000000, 1),
                                     (1, 1, 0x100000000), ('01', 1, 1)):
        rejects(lambda: module.decode_record(framed(boot, session, identifier, status)), 'identity')


def dirty_session_recovery():
    class DirtyPeer(DiagnosticPeer):
        def __init__(self):
            super().__init__('DIAG available=0')
            self.partial = True
        def write_data(self, data, remaining):
            if data == b'\n':
                self.partial = False
                self.writes.append(data)
                return
            if self.partial:
                self.writes.append(data)
                payload = b'!N1 12345 0 0 ERROR syntax'
                self.pending += payload + f' *{zlib.crc32(payload):08x}\n'.encode()
                return
            super().write_data(data, remaining)
    with patch.object(module, 'RESYNC_QUIET_SECONDS', 0.002):
        peer = DirtyPeer()
        assert peer.establish_session('diagnostic-fixture').startswith('HELLO ')
        assert peer.writes.count(b'\n') == 1
        assert peer.writes[-3:-1] == [b'N1 HELLO\n'] * 2
        assert b' STATUS\n' in peer.writes[-1]
        assert not any(b'PUT' in write or b'DATA' in write or b'DONE' in write for write in peer.writes)
        assert any(row['kind'] == 'RESYNC' for row in peer.audit)
        # Breaking the boundary leaves HELLO appended to the old partial input.
        class NoBoundary(DirtyPeer):
            def write_data(self, data, remaining):
                if data == b'\n':
                    return
                super().write_data(data, remaining)
        broken = NoBoundary()
        started = time.monotonic()
        rejects(lambda: broken.establish_session('diagnostic-fixture'), 'timed out')
        assert time.monotonic() - started < 1, 'resync remains bounded under persistent dirty input'


def main():
    from dev_usb_telemetry_test import firmware_row_validation, telemetry_validation
    telemetry_validation()
    firmware_row_validation()
    diagnostic_validation()
    diagnostic_v2_validation()
    command_bound_validation()
    freshness_validation()
    dirty_session_recovery()
    serial_open_validation()
    for number in (errno.ENOTTY, errno.EINVAL, errno.EIO):
        serial_open_validation(post_error=number)
    for platform in ('darwin', 'win32'):
        serial_open_validation(platform=platform, post_error=errno.EIO, linux_driver=False)
    serial_open_control()
    for body in ('FRAME format=pbm1', 'FRAME gen=1 gen=2', 'FRAME gen=', 'FRAME broken'):
        rejects(lambda: module.decode_fields(body, {'gen'}), 'metadata')
    script = ROOT / 'tools/dev-usb.py'
    for args in (['live'], ['live', '--port', 'COM3', '--expect-build', 'x'],
                 ['live', '--port', 'rfc2217://example.invalid', '--expect-build', 'x'],
                 ['live', '--port', '/dev/mock-only'], ['simulate'],
                 ['live', '--port', '/dev/mock-only', '--expect-build', 'x', '--press', 'sleep', '--press', 'next'],
                 ['live', '--port', '/dev/mock-only', '--expect-build', 'x', '--press', 'sleep', '--output', '/outside-project/not-written'],
                 ['live', '--port', '/dev/mock-only', '--expect-build', 'x', '--press', 'sleep', '--diagnostics'],
                 ['live', '--port', '/dev/mock-only', '--expect-build', 'x', '--timeout', 'nan'],
                 ['live', '--port', '/dev/mock-only', '--expect-build', 'x', '--listen', '601'],
                 ['live', '--port', '/dev/mock-only', '--expect-build', 'x', '--press', 'sleep', '--listen', '1']):
        result = subprocess.run([sys.executable, str(script), *args], capture_output=True)
        assert result.returncode == 2 and not result.stdout
    print('PASS: fake serial protocol: Linux open without a reset (old-sequence control fails), double HELLO/random-ID freshness, bounded commands, strict diagnostics and CLI refusals; no device access')


if __name__ == '__main__':
    main()
