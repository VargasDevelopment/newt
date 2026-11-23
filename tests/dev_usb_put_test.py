"""dev-usb.py put over a fake device lease: never imports pyserial or opens a device."""
import base64
import hashlib
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace
import zlib

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))

spec = importlib.util.spec_from_file_location('dev_usb_client', ROOT / 'tools/dev-usb.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
import dev_usb_put as put  # the same module object dev-usb.py imported

BUILD = 'put-fixture'
put.FILE_SECONDS = 0.2  # keep deadline tests short; the rule under test is the same


class LeasePeer(module.ProtocolClient):
    """The device's side of HELLO, STATUS and the PUT lease, in memory. Replies
    arrive a few bytes at a time; `silent`, `short` and `answers` break one
    request kind."""

    def __init__(self, caps='press,status,frame,diag2,put', build=BUILD, silent=None, short=None, answers=None):
        super().__init__(0.1, randbelow=lambda size: 17)
        self.caps, self.build = caps, build
        self.silent, self.short, self.answers = silent, short, answers or {}
        self.writes, self.pending, self.peer_session, self.longest = [], b'', 0, 0
        self.card, self.open = {}, None

    def write_data(self, data, remaining):
        tokens = data.decode().split(' ')
        kind = 'HELLO' if tokens[1] == 'HELLO\n' else tokens[4].strip()
        self.writes.append(kind)
        self.longest = max(self.longest, len(data))
        if kind == self.short:
            raise ValueError('Incomplete serial command write')
        if kind == self.silent:
            return
        identifier, body = 0, self.answer(kind, [token.strip() for token in tokens[5:]])
        if kind != 'HELLO':
            identifier = int(tokens[3])
        body = self.answers.get(kind, body)
        payload = f'!N1 4242 {self.peer_session} {identifier} {body}'.encode()
        self.pending += payload + f' *{zlib.crc32(payload):08x}\n'.encode()

    def answer(self, kind, arguments):
        if kind == 'HELLO':
            self.peer_session += 1
            return f'HELLO build={self.build} caps={self.caps} wake=physical-only'
        if kind == 'STATUS':
            return 'STATUS screen=0 gen=1 ready=1 refresh_ms=0 heap=0 dropped=0 capture=0'
        if kind == 'PUT':
            return put.OFFER
        if kind == 'FILE':
            folder, name, size, digest = arguments
            self.open = folder, name, int(size), digest, bytearray()
            return 'FILE free=1073741824'
        if kind == 'DATA':
            offset, text = arguments
            assert int(offset) == len(self.open[4])
            self.open[4].extend(base64.b64decode(text, validate=True))
            return f'DATA {len(self.open[4])}'
        folder, name, size, digest, data = self.open
        if len(data) != size or hashlib.sha256(data).hexdigest() != digest:
            return 'ERROR sha256-mismatch'
        self.card[folder, name] = bytes(data)
        return f'DONE size={size} sha256={digest}'

    def read_data(self, remaining):
        data, self.pending = self.pending[:7], self.pending[7:]
        if not data:
            time.sleep(min(0.002, remaining))
        return data

    def close(self):
        self.connection['closed'] = True


def session(peer, files, folder='dev'):
    args = SimpleNamespace(mode='put', expect_build=BUILD, press=[], diagnostics=False, output=None,
                           listen=0, folder=folder, files=files, timeout=0.1)
    return module.run_session(peer, args)


def pattern(length):
    return bytes((i * 7 + i // 251) & 255 for i in range(length))


FILES = [('local/empty', 'empty.txt', b''), ('local/runtime.bin', 'runtime.bin', pattern(36_856)),
         ('local/small', 'small.js', pattern(5_131))]


def success():
    peer = LeasePeer()
    report = session(peer, FILES)
    assert 'error' not in report, report['error']
    assert peer.card == {('dev', name): data for _, name, data in FILES}
    assert [entry['result'] for entry in report['put']['files']] == ['written'] * 3
    assert report['put']['files'][1]['sha256'] == hashlib.sha256(FILES[1][2]).hexdigest()
    assert peer.writes.count('DATA') == 0 + 246 + 35 and peer.writes[-1] == 'STATUS'
    assert peer.longest <= 256, 'each line and its LF fit the device receive queue whole'
    kinds = [record['kind'] for record in report['protocol']['records']]
    assert kinds.count('DATA') == 3, 'DATA is summarised once per file'
    assert len(kinds) < 40


def refusals_before_any_write():
    for peer, error in ((LeasePeer(build='other'), 'build differs'),
                        (LeasePeer(caps='press,status,frame,diag2'), 'lack put'),
                        (LeasePeer(answers={'PUT': 'PUT line=80 chunk=3072 cap=7798784'}), 'Unsupported card write lease'),
                        (LeasePeer(answers={'PUT': 'ERROR busy reason=reading'}), 'busy reason=reading')):
        report = session(peer, FILES)
        assert error in report['error'], report['error']
        assert 'FILE' not in peer.writes and not peer.card and report['connection']['closed']


def failures_before_done_leave_the_name_unchanged():
    for broken, error in (({'short': 'DATA'}, 'Incomplete'), ({'silent': 'DATA'}, 'timed out'),
                          ({'silent': 'FILE'}, 'timed out'),
                          ({'answers': {'DATA': 'DATA 1'}}, 'Wrong DATA acknowledgement')):
        peer = LeasePeer(**broken)
        started = time.monotonic()
        report = session(peer, FILES[1:])
        assert error in report['error'], report['error']
        assert time.monotonic() - started < 2, 'every wait has a deadline'
        assert report['put']['files'][-1]['result'] == 'unchanged'
        assert 'DONE' not in peer.writes and not peer.card


def an_unknown_commit_is_never_retried():
    for broken, written in (({'silent': 'DONE'}, True), ({'short': 'DONE'}, False),
                            ({'answers': {'DONE': 'DONE size=1 sha256=' + '0' * 64}}, True)):
        peer = LeasePeer(**broken)
        report = session(peer, FILES[1:])
        assert 'do not retry' in report['error'] or 'check the card' in report['error'], report['error']
        files = report['put']['files']
        assert len(files) == 1 and files[0]['result'] == 'unknown'
        assert files[0]['host_write_completed'] is written
        assert peer.writes.count('DONE') == 1 and peer.writes.count('PUT') == 1, 'no retry, no next file'
    peer = LeasePeer(answers={'DONE': 'ERROR sha256-mismatch'})
    report = session(peer, FILES[1:])
    assert 'sha256-mismatch' in report['error'] and report['put']['files'][0]['result'] == 'unchanged'
    assert peer.writes.count('DONE') == 1


def a_commit_left_for_a_hand_check_is_recovery_needed():
    for kind in ('DONE', 'FILE'):
        peer = LeasePeer(answers={kind: 'ERROR recovery-needed step=rename code=-5'})
        report = session(peer, FILES[1:])
        assert 'docs/dev-usb-bridge.md' in report['error'], report['error']
        assert report['put']['files'][0]['result'] == 'recovery-needed'
        assert peer.writes.count('PUT') == 1, 'the run stops there'


def plans_are_checked_before_the_port_opens():
    with tempfile.TemporaryDirectory() as directory:
        folder = Path(directory)
        for name in ('a.txt', 'b.txt', '.hidden', 'a..b', 'x.'):
            (folder / name).write_bytes(b'x')
        (folder / 'upper').mkdir()
        (folder / 'upper/A.TXT').write_bytes(b'y')
        assert put.plan([folder / 'a.txt'], 'renamed.txt')[0][1:] == ('renamed.txt', b'x')
        for paths, name, error in (([folder / '.hidden'], None, 'Card name'), ([folder / 'a..b'], None, 'Card name'),
                                   ([folder / 'a.txt'], '../x', 'Card name'), ([folder / 'a.txt'], 'a/b', 'Card name'),
                                   ([folder / 'a.txt'], 'x' * 33, 'Card name'),
                                   ([folder / 'x.'], None, 'Card name'), ([folder / 'a.txt'], 'newt.bin.', 'Card name'),
                                   ([folder / 'a.txt', folder / 'upper/A.TXT'], None, 'FAT ignores case'),
                                   ([folder / 'a.txt', folder / 'b.txt'], 'c.txt', 'exactly one'),
                                   ([folder / 'a.txt', folder / 'a.txt'], None, 'share'),
                                   ([folder / 'a.txt'] * 65, None, 'Put 1 to 64'), ([], None, 'Put 1 to 64')):
            try:
                put.plan(paths, name)
            except ValueError as refusal:
                assert error in str(refusal), str(refusal)
            else:
                raise AssertionError(f'{paths} {name} accepted')
        large = folder / 'large.bin'
        with large.open('wb') as out:
            out.truncate(put.CAP_BYTES + 1)
        try:
            put.plan([large])
        except ValueError as refusal:
            assert 'cap' in str(refusal)
        else:
            raise AssertionError('oversized file accepted')
        script = ROOT / 'tools/dev-usb.py'
        port = ['--port', '/dev/mock-only', '--expect-build', 'x']
        for args in (['put', *port, '--file', str(folder / 'a.txt')], ['put', *port, '--folder', 'dev'],
                     ['live', *port, '--folder', 'dev'], ['put', *port, '--folder', 'books', '--file', str(folder / 'a.txt')],
                     ['put', *port, '--folder', 'dev', '--file', str(folder / '.hidden')],
                     ['put', *port, '--folder', 'dev', '--file', str(folder / 'a.txt'), '--press', 'next']):
            result = subprocess.run([sys.executable, str(script), *args], capture_output=True)
            assert result.returncode == 2 and not result.stdout, args


def main():
    success()
    refusals_before_any_write()
    failures_before_done_leave_the_name_unchanged()
    an_unknown_commit_is_never_retried()
    a_commit_left_for_a_hand_check_is_recovery_needed()
    plans_are_checked_before_the_port_opens()
    print('PASS: dev-usb put: verified final SHA-256, lines within the receive queue, fragmented replies, '
          'deadlines, short writes, no retry of an unknown commit, recovery-needed and plan refusals; '
          'no device access')


if __name__ == '__main__':
    main()
