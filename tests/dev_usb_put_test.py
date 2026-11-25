"""dev-usb.py put over a fake device lease: never imports pyserial or opens a device."""
import hashlib
import importlib.util
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace
from unittest.mock import patch
import zlib

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))

spec = importlib.util.spec_from_file_location('dev_usb_client', ROOT / 'tools/dev-usb.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
import dev_usb_put as put  # the same module object dev-usb.py imported
import fatfs_case
import fatfs_case_generate
from host_common import C_COMPILER
from host_objects import compiler_environment

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
        header, _, payload = data.partition(b'\n')
        self.payload = payload
        tokens = header.decode().split(' ')
        kind = 'HELLO' if tokens[1] == 'HELLO' else tokens[4].strip()
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
            encoded, size, digest = arguments
            target = bytes.fromhex(encoded).decode()
            folder, _, name = target.rpartition("/")
            self.open = folder, name, int(size), digest, bytearray()
            return 'FILE free=1073741824'
        if kind == 'DATA':
            offset, length = arguments
            assert int(offset) == len(self.open[4])
            assert len(self.payload) == int(length)
            self.open[4].extend(self.payload)
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


def session(peer, files, folder='Newt/Dev'):
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
    assert set(report['protocol']) == {'boot', 'session', 'last_request_id', 'records'}
    assert report['put']['receive_tail'] == {'buffered_bytes': 0, 'buffered_protocol_prefix': False,
                                           'dirty_drain_lines': 0}
    assert peer.card == {('Newt/Dev', name): data for _, name, data in FILES}
    assert [entry['result'] for entry in report['put']['files']] == ['written'] * 3
    assert report['put']['files'][1]['sha256'] == hashlib.sha256(FILES[1][2]).hexdigest()
    progress = report['put']['files'][1]['protocol_progress']
    assert progress['last_data_ack']['offset'] == len(FILES[1][2]) and progress['data_frames'] == 9
    assert progress['last_ack']['kind'] == 'DONE' and progress['last_response']['body'].startswith('DONE ')
    assert progress['data_seconds'] >= 0 and progress['write_seconds'] >= 0
    assert peer.writes.count('DATA') == 0 + 9 + 2 and peer.writes[-1] == 'STATUS'
    assert peer.longest <= 4352, 'each header and raw payload fit the frame bound'
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
        progress = report['put']['files'][-1]['protocol_progress']
        assert progress['last_sent']['kind'] in ('DATA', 'FILE')
        assert 'last_data_ack' not in progress, 'a failed first chunk is never acknowledged'


def an_unknown_commit_is_never_retried():
    class InterruptedPeer(LeasePeer):
        def __init__(self, kind, phase):
            super().__init__()
            self.kind, self.phase = kind, phase

        def write_data(self, data, remaining):
            super().write_data(data, remaining)
            if self.writes[-1] == self.kind and self.phase == 'write':
                raise KeyboardInterrupt()

        def read_data(self, remaining):
            if self.writes[-1] == self.kind and self.phase == 'read':
                raise KeyboardInterrupt()
            return super().read_data(remaining)

    for kind in ('DATA', 'DONE'):
        for phase in ('write', 'read'):
            peer = InterruptedPeer(kind, phase)
            report = session(peer, FILES[1:])
            assert report['interrupted']
            files = report['put']['files']
            assert len(files) == 1
            assert files[0]['result'] == ('unknown' if kind == 'DONE' else 'unchanged')
            if kind == 'DONE': assert files[0]['host_write_completed'] is (phase == 'read')
            assert files[0]['protocol_progress']['error'] == 'Interrupted by operator'
            assert peer.writes.count(kind) == 1 and peer.writes.count('PUT') == 1
    for broken, written in (({'silent': 'DONE'}, True), ({'short': 'DONE'}, False),
                            ({'answers': {'DONE': 'DONE size=1 sha256=' + '0' * 64}}, True)):
        peer = LeasePeer(**broken)
        report = session(peer, FILES[1:])
        assert 'do not retry' in report['error'] or 'check the card' in report['error'], report['error']
        files = report['put']['files']
        assert len(files) == 1 and files[0]['result'] == 'unknown'
        assert files[0]['host_write_completed'] is written
        assert peer.writes.count('DONE') == 1 and peer.writes.count('PUT') == 1, 'no retry, no next file'
        cause = files[0]['protocol_progress'].get('error', '')
        if broken.get('silent') == 'DONE': assert 'timed out' in cause, cause
        if broken.get('short') == 'DONE': assert 'Incomplete serial command write' in cause, cause
    peer = LeasePeer(answers={'DONE': 'ERROR sha256-mismatch'})
    report = session(peer, FILES[1:])
    assert 'sha256-mismatch' in report['error'] and report['put']['files'][0]['result'] == 'unchanged'
    assert peer.writes.count('DONE') == 1


def a_commit_left_for_a_hand_check_is_recovery_needed():
    for kind in ('DONE', 'FILE'):
        peer = LeasePeer(answers={kind: 'ERROR recovery-needed step=rename code=-5'})
        report = session(peer, FILES[1:])
        assert 'hand check' in report['error'], report['error']
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
        for paths, name, error in (([folder / 'a..b'], None, 'FAT card name'),
                                   ([folder / 'a.txt'], '../x', 'FAT card name'), ([folder / 'a.txt'], 'a/b', 'FAT card name'),
                                   ([folder / 'a.txt'], 'x' * 256, 'FAT card name'),
                                   ([folder / 'x.'], None, 'FAT card name'), ([folder / 'a.txt'], 'newt.bin.', 'FAT card name'),
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
        # A file larger than the old app-slot cap is an ordinary transfer.
        large = folder / 'large.bin'
        with large.open('wb') as out:
            out.truncate(0x770001)
        assert len(put.plan([large])[0][2]) == 0x770001
        assert put.card_target('Newt/Books/nested', 'a b.epub') == 'Newt/Books/nested/a b.epub'
        assert put.card_target('.', '.hidden') == '.hidden'
        for destination in ['../Books', '/Newt/Books', 'Newt//Books', 'Newt/../Books', '.newt-put', 'bad.']:
            try:
                put.card_target(destination, 'a.epub')
            except ValueError:
                pass
            else:
                raise AssertionError(f'Invalid destination accepted: {destination}')
        script = ROOT / 'tools/dev-usb.py'
        port = ['--port', '/dev/mock-only', '--expect-build', 'x']
        for args in (['put', *port, '--file', str(folder / 'a.txt')], ['put', *port, '--folder', 'dev'],
                     ['live', *port, '--folder', 'dev'], ['put', *port, '--folder', '../Books', '--file', str(folder / 'a.txt')],
                     ['put', *port, '--folder', 'dev', '--file', str(folder / 'a..b')],
                     ['put', *port, '--folder', 'dev', '--file', str(folder / 'a.txt'), '--press', 'next']):
            result = subprocess.run([sys.executable, str(script), *args], capture_output=True)
            assert result.returncode == 2 and not result.stdout, args


def unicode_destinations():
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        paths = [root / name for name in ('σ.epub', 'ς.epub', 'Ǆ.epub', 'ǅ.epub')]
        for path in paths:
            path.write_bytes(b'x')
        # Host preflight needs neither the toolchain nor a connected device.
        with patch.object(subprocess, 'run', side_effect=AssertionError('unexpected subprocess')):
            try:
                put.plan(paths[:2])
            except ValueError as error:
                assert 'FAT ignores case' in str(error), str(error)
            else:
                raise AssertionError('FatFs-equivalent Unicode destinations accepted')
            assert 'Ǆ'.lower() == 'ǅ'.lower()
            assert len(put.plan(paths[2:])) == 2, 'FatFs preserves this titlecase distinction'
    assert fatfs_case.name_key('ß') != fatfs_case.name_key('SS'), 'no uppercase expansions'
    assert fatfs_case.name_key('é') != fatfs_case.name_key('e\u0301'), 'no normalization'
    assert fatfs_case.name_key('𐐀') != fatfs_case.name_key('𐐨'), 'non-BMP units stay unchanged'


def committed_tables_match(tables):
    assert (fatfs_case.CVT1, fatfs_case.CVT2) == tables, 'Host FatFs tables differ from pinned firmware'
    source = (ROOT / 'tools/fatfs_case.py').read_text()
    assert fatfs_case_generate.generated(tables) in source, 'Regenerated table block differs'


def short_alias_destinations():
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        long_name, alias = root / 'abcdefghijk.txt', root / 'ABCDEF~1.TXT'
        long_name.write_bytes(b'first')
        alias.write_bytes(b'second')
        assert put.plan([long_name])[0][1] == long_name.name, 'long names remain supported'
        # Refuse the whole ordered batch before the first file can be written.
        for paths, name in (([long_name, alias], None), ([alias, long_name], None),
                            ([long_name], alias.name), ([alias], None)):
            try:
                put.plan(paths, name)
            except ValueError as error:
                assert 'Invalid FAT card name' in str(error) and alias.name in str(error), str(error)
            else:
                raise AssertionError('FatFs short alias accepted as a batch destination')
        # The same boundary owns directory components, not just basenames.
        for folder in ('SHORT~1', 'Newt/SHORT~12/Books', 'Newt/Books~'):
            try:
                put.card_target(folder, long_name.name)
            except ValueError as error:
                assert 'Invalid FAT card name' in str(error), str(error)
            else:
                raise AssertionError('Short-alias directory accepted')
        result = subprocess.run([sys.executable, str(ROOT / 'tools/dev-usb.py'), 'put',
                                 '--port', '/dev/mock-only', '--expect-build', BUILD,
                                 '--folder', 'Newt/Books', '--file', str(long_name), '--file', str(alias)],
                                capture_output=True, text=True)
        assert result.returncode == 2 and not result.stdout, result
        assert alias.name in result.stderr and 'Invalid FAT card name' in result.stderr, result.stderr


def short_alias_control():
    with patch.object(put, 'FORBIDDEN_NAME_CHARS', put.FORBIDDEN_NAME_CHARS.replace('~', '')):
        try:
            short_alias_destinations()
        except AssertionError as error:
            assert str(error) == 'FatFs short alias accepted as a batch destination', str(error)
        else:
            raise AssertionError('Negative control survived: generated short aliases allowed')
    print('PASS: short-alias batch, rename and parent-path refusals; allowing tilde control fails')


def fatfs_parity_and_controls():
    tables = fatfs_case_generate.pinned_tables()
    committed_tables_match(tables)
    # Original ff_wtoupper control flow, using the very same pinned tables.
    # FatFs R0.15: Copyright (C) 2022, ChaN, all right reserved.
    # FatFs module is an open source software. Redistribution and use of FatFs in
    # source and binary forms, with or without modification, are permitted provided
    # that the following condition is met:
    # 1. Redistributions of source code must retain the above copyright notice,
    #    this condition and the following disclaimer.
    # This software is provided by the copyright holder and contributors "AS IS"
    # and any warranties related to this software are DISCLAIMED.
    # The copyright owner or contributors be NOT LIABLE for any damages caused
    # by use of this software.
    reference = r'''
#include <stdint.h>
#include <stdio.h>
static uint32_t ff_wtoupper(uint32_t uni) {
    const uint16_t *p;
    uint16_t uc, bc, nc, cmd;
    if (uni < 0x10000) {
        uc = (uint16_t)uni; p = uc < 0x1000 ? cvt1 : cvt2;
        for (;;) {
            bc = *p++;
            if (bc == 0 || uc < bc) break;
            nc = *p++; cmd = nc >> 8; nc &= 0xFF;
            if (uc < bc + nc) {
                switch (cmd) {
                case 0: uc = p[uc - bc]; break;
                case 1: uc -= (uc - bc) & 1; break;
                case 2: uc -= 16; break;
                case 3: uc -= 32; break;
                case 4: uc -= 48; break;
                case 5: uc -= 26; break;
                case 6: uc += 8; break;
                case 7: uc -= 80; break;
                case 8: uc -= 0x1C60; break;
                }
                break;
            }
            if (cmd == 0) p += nc;
        }
        uni = uc;
    }
    return uni;
}
int main(void) {
    for (uint32_t code = 0; code < 0x10000; ++code) {
        uint32_t value = ff_wtoupper(code);
        putchar(value & 255); putchar(value >> 8);
    }
    return ferror(stdout) ? 1 : 0;
}
'''
    arrays = ''.join('static const unsigned short ' + name + '[] = {' +
                     ','.join(str(value) for value in table) + '};\n'
                     for name, table in zip(('cvt1', 'cvt2'), tables, strict=True))
    environment = compiler_environment(ROOT)
    with tempfile.TemporaryDirectory(prefix='fatfs-case-') as directory:
        path = Path(directory)
        source, binary = path / 'reference.c', path / 'reference'
        source.write_text(arrays + reference)
        subprocess.run([C_COMPILER, '--no-default-config', '-std=c11', '-O1', str(source), '-o', str(binary)],
                       check=True, env=environment, capture_output=True)
        result = subprocess.run([str(binary)], check=True, env=environment, capture_output=True).stdout
    assert struct.unpack('<65536H', result) == tuple(fatfs_case._upper(code) for code in range(65536))
    for label, target, field, value, check in (
            ('Python lower', put, 'name_key', str.lower, unicode_destinations),
            ('table drift', fatfs_case, 'CVT1', (fatfs_case.CVT1[0] ^ 1, *fatfs_case.CVT1[1:]),
             lambda: committed_tables_match(tables))):
        with patch.object(target, field, value):
            try:
                check()
            except AssertionError:
                pass
            else:
                raise AssertionError(f'Negative control survived: {label}')
    print('PASS: FatFs pinned table regeneration, exhaustive BMP C parity, Unicode preflight; two controls fail')


def lease_statistics_validation():
    from dev_usb_firmware_rows import decode_firmware_row
    row = (b'PUT_STATS reason=4 id=123 offset=12132352 received=17 expected=4096 bytes=12400000 '
           b'empty=200 pauses=200 pause_ms=211 rx_ms=170 write_ms=9 ack_ms=2 commit_ms=0')
    decoded = decode_firmware_row(row)
    assert decoded['row'] == 'put_stats' and decoded['offset'] == 12132352
    assert decoded['reason'] == 4 and decoded['received'] == 17 and decoded['pause_ms'] == 211
    burst_row = row + b' gap_us=250 bursts=200 burst_max=192'
    burst = decode_firmware_row(burst_row)
    assert (burst['gap_us'], burst['bursts'], burst['burst_max']) == (250, 200, 192)
    for bad in (row + b' name=anything', row.replace(b'id=123', b'id=-1'), row.replace(b'rx_ms=170', b'rx_ms=0170')):
        assert decode_firmware_row(bad) is None
    for bad in (burst_row.replace(b'gap_us=250', b'gap_us=-1'), burst_row.replace(b'bursts=200', b'bursts=0200'),
                burst_row + b' name=anything'):
        assert decode_firmware_row(bad) is None


def a_failure_keeps_its_late_statistics_without_writing_again():
    row = (b'PUT_STATS reason=4 id=123 offset=0 received=17 expected=4096 bytes=123 '
           b'empty=200 pauses=200 pause_ms=211 rx_ms=170 write_ms=0 ack_ms=2 commit_ms=0\n')
    class Peer(LeasePeer):
        def read_data(self, remaining):
            if not self.pending and self.writes[-1:] == ['DATA'] and not getattr(self, 'stats_sent', False):
                self.pending = row
                self.stats_sent = True
            return super().read_data(remaining)
    peer = Peer(answers={'DATA': 'ERROR abandoned reason=timeout'})
    report = session(peer, FILES[1:])
    assert 'timeout' in report['error'] and report['firmware_rows'][0]['reason'] == 4
    assert peer.writes[-1] == 'DATA' and peer.writes.count('DATA') == 1
    assert report['put']['files'][0]['result'] == 'unchanged'
    with patch.object(Peer, 'drain_responses', lambda self, seconds: None):
        broken = session(Peer(answers={'DATA': 'ERROR abandoned reason=timeout'}), FILES[1:])
        assert not broken['firmware_rows'], 'tail control must lose the delayed row'
    class InterruptedTail(Peer):
        def drain_responses(self, seconds): raise KeyboardInterrupt()
    interrupted = session(InterruptedTail(answers={'DATA': 'ERROR abandoned reason=timeout'}), FILES[1:])
    assert interrupted['interrupted'] and interrupted['connection']['closed']
    assert interrupted['failure_tail_error'] == 'Interrupted by operator'
    class DirtyTail(Peer):
        def read_data(self, remaining):
            if not self.pending and self.writes[-1:] == ['DATA'] and not getattr(self, 'stats_sent', False):
                self.pending = b'!N1 incomplete error\n' + row
                self.stats_sent = True
            return super().read_data(remaining)
    dirty = session(DirtyTail(answers={'DATA': 'ERROR abandoned reason=timeout'}), FILES[1:])
    assert dirty['put']['receive_tail']['dirty_drain_lines'] == 1
    assert dirty['firmware_rows'][0]['reason'] == 4
    assert dirty['put']['receive_tail']['buffered_bytes'] == 0
    class Incomplete(LeasePeer):
        def __init__(self, prefix):
            super().__init__()
            self.prefix = prefix
        def write_data(self, data, remaining):
            super().write_data(data, remaining)
            if self.writes[-1] == 'DATA': self.pending = self.prefix
    for prefix in (b'!N1 partial', b'partial log'):
        report = session(Incomplete(prefix), FILES[1:])
        progress = report['put']['files'][0]['protocol_progress']
        assert progress['buffered_bytes'] == len(prefix)
        assert progress['buffered_protocol_prefix'] is prefix.startswith(b'!N1 ')
        assert report['put']['receive_tail']['buffered_bytes'] == len(prefix)
        assert report['put']['receive_tail']['buffered_protocol_prefix'] is prefix.startswith(b'!N1 ')
        assert report['put']['files'][0]['result'] == 'unchanged'



def main():
    lease_statistics_validation()
    a_failure_keeps_its_late_statistics_without_writing_again()
    success()
    refusals_before_any_write()
    failures_before_done_leave_the_name_unchanged()
    an_unknown_commit_is_never_retried()
    a_commit_left_for_a_hand_check_is_recovery_needed()
    plans_are_checked_before_the_port_opens()
    unicode_destinations()
    short_alias_destinations()
    short_alias_control()
    fatfs_parity_and_controls()
    print('PASS: dev-usb put: verified final SHA-256, bounded lease frames, fragmented replies, '
          'deadlines, short writes, no retry of an unknown commit, recovery-needed and plan refusals; '
          'no device access')


if __name__ == '__main__':
    main()
