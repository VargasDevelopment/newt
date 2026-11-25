#!/usr/bin/env python3
"""Host preview tests and negative controls for #124 (interrupted changed-book reimport)."""
import binascii
import os
from pathlib import Path
import re
import struct
import sys
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(ROOT / 'tools'), str(ROOT / 'rust/host/preview')]
import preview_driver  # noqa: E402
import rust_preview  # noqa: E402
from gate_tier import negative_controls  # noqa: E402

BACK, CONFIRM, LEFT, RIGHT, POWER = 0, 1, 2, 3, 6
COVER = re.compile(r'HOME_COVER eligible=(\d)')
STATUS = re.compile(r'STATUS screen=(\d+) gen=(\d+) ')
DIAG_FIELD = re.compile(r'(reader_status|reader_open|actual_character|current_book)=(\d+)')


def pad_epub(book):
    """Pad chapter 2 with an 8 KiB comment so unread block 1 sits between block 0 and ZIP tail."""
    with zipfile.ZipFile(book, 'r') as src:
        entries = [(info, src.read(info)) for info in src.infolist()]
    pad = b'<!-- ' + b'0' * 8192 + b' -->'
    with zipfile.ZipFile(book, 'w', zipfile.ZIP_STORED) as dst:
        for info, data in entries:
            if info.filename.endswith('two.xhtml'):
                data = data.replace(b'</body>', pad + b'</body>')
            dst.writestr(info, data, compress_type=zipfile.ZIP_STORED)


def crc_neutral_ascii(base, flip_index):
    """Flip bit 0 at flip_index and solve 32 following '0'/'1' bytes to keep exact CRC-32."""
    b0 = bytearray(base)
    c0 = binascii.crc32(b0)
    b1 = bytearray(b0)
    b1[flip_index] ^= 1
    target = c0 ^ binascii.crc32(b1)
    pivots = {}
    for k in range(32):
        tmp = bytearray(b0)
        tmp[flip_index + 1 + k] ^= 1
        v, m = c0 ^ binascii.crc32(tmp), 1 << k
        while v:
            bit = v.bit_length() - 1
            if bit in pivots:
                pv, pm = pivots[bit]
                v ^= pv
                m ^= pm
            else:
                pivots[bit] = (v, m)
                break
    v, m = target, 0
    while v:
        bit = v.bit_length() - 1
        pv, pm = pivots[bit]
        v ^= pv
        m ^= pm
    for k in range(32):
        if (m >> k) & 1:
            b1[flip_index + 1 + k] ^= 1
    assert binascii.crc32(b1) == c0 and bytes(b1) != bytes(b0)
    return bytes(b1)


def mutate_epub_block1(book):
    """Edit chapter 2 strictly inside block 1 (4096..8192) keeping exact CRC-32, size, and mtime."""
    st = book.stat()
    raw = bytearray(book.read_bytes())
    with zipfile.ZipFile(book, 'r') as z:
        info = [i for i in z.infolist() if i.filename.endswith('two.xhtml')][0]
        data = z.read(info)
        lhoff = info.header_offset
        fnlen, exlen = struct.unpack_from('<HH', raw, lhoff + 26)
        data_off = lhoff + 30 + fnlen + exlen
    marker = data.index(b'<!-- ' + b'0' * 100) + 5
    flip_in_data = max(marker, 6600 - data_off)
    assert 4096 <= data_off + flip_in_data and data_off + flip_in_data + 33 < 8192
    raw[data_off:data_off + len(data)] = crc_neutral_ascii(data, flip_in_data)
    book.write_bytes(raw)
    os.utime(book, ns=(st.st_atime_ns, st.st_mtime_ns))


def eligible(log):
    rows = COVER.findall(log)
    return rows[-1] == '1' if rows else None


def screen_of(log):
    rows = STATUS.findall(log)
    return int(rows[-1][0]) if rows else None


def diag_of(log):
    return dict(DIAG_FIELD.findall(log.split(' DIAG ')[-1]))


def selected_book_id(directory):
    """Return the committed book_id (first 8 bytes LE of digest) in newt-selected.nrs."""
    path = directory / 'card/Newt/State/newt-selected.nrs'
    data = path.read_bytes()
    slots = [at for at in range(512, len(data), 512)
             if data[at:at + 4] == b'NRSK' and data[at + 20:at + 24] == b'NWLS']
    assert slots, 'newt-selected.nrs has no valid NWLS slot'
    at = max(slots, key=lambda pos: struct.unpack_from('<I', data, pos + 28)[0])
    return str(struct.unpack_from('<Q', data, at + 32)[0])


def run_path_a(binary, keys=(), *, orientation=None, mutate=True, **conf):
    """Warm fast-open -> idle verification detects block-1 edit -> reimport_changed runs prepare_image."""
    with tempfile.TemporaryDirectory(prefix='newt-reimport-a-') as td:
        d = Path(td)
        args = ['--library-fixture', 'healthy']
        if orientation:
            args += ['--reader-orientation', orientation]
        preview_driver.world(binary, d, preview_driver.parse(args))
        book = sorted((d / 'card/Newt/Books').glob('*.epub'))[0]
        pad_epub(book)
        preview_driver.write_conf(d, {'telemetry': 'sample'})
        turn = BACK if orientation == 'landscape-cw' else RIGHT
        rust_preview.run(binary, d, (CONFIRM, CONFIRM, turn, POWER))
        if mutate:
            mutate_epub_block1(book)
        preview_driver.write_conf(d, {'telemetry': 'sample', **conf})
        frames, log = rust_preview.run(binary, d, keys, 'deep-sleep')
        return frames[-1], log, selected_book_id(d)


def run_path_b(binary, keys=(), *, mutate=True, **conf):
    """Stale wake -> open_selected_armed prepares new digest -> reimport_changed adopts and runs open_selected_profile."""
    with tempfile.TemporaryDirectory(prefix='newt-reimport-b-') as td:
        d = Path(td)
        preview_driver.world(binary, d, preview_driver.parse(['--library-fixture', 'healthy']))
        book = sorted((d / 'card/Newt/Books').glob('*.epub'))[0]
        preview_driver.write_conf(d, {'telemetry': 'sample'})
        rust_preview.run(binary, d, (CONFIRM, CONFIRM, RIGHT, POWER))
        if mutate:
            pad_epub(book)
        preview_driver.write_conf(d, {'telemetry': 'sample', **conf})
        frames, log = rust_preview.run(binary, d, keys, 'deep-sleep')
        return frames[-1], log, selected_book_id(d)


def reimport_prepare_back_goes_home(binary):
    """Back during reimport_changed's prepare_image returns cleanly to Home with cover eligible and no flash."""
    clean_home, clean_log, _ = run_path_a(binary, (BACK,), mutate=False)
    assert screen_of(clean_log) == 0 and eligible(clean_log) is True
    base_page, base_log, new_book = run_path_a(binary)
    assert diag_of(base_log).get('reader_open') == '1' and diag_of(base_log).get('current_book') == new_book

    conf = {'open.back': '/Books/', 'open.after_refresh': '3'}
    home, log, _ = run_path_a(binary, **conf)
    if screen_of(log) != 0 or eligible(log) is not True:
        return f'Back during reimport prepare hid cover or did not go Home (screen={screen_of(log)}, eligible={eligible(log)})'
    if home != clean_home:
        return 'Home after Back during reimport prepare differed from clean Home (stale warning or title)'
    flashed, _, _ = run_path_a(binary, **conf, freeze='open.back')
    if flashed != home:
        return 'Back during reimport prepare flashed a closed frame before Home'
    reopened, rlog, rbook = run_path_a(binary, (BACK,), **conf)
    if diag_of(rlog).get('reader_open') != '1' or diag_of(rlog).get('current_book') != new_book or rbook != new_book:
        return f'Read after Back during reimport prepare did not reopen changed book ({diag_of(rlog)}, sel={rbook})'
    if reopened != base_page:
        return 'Read after Back during reimport prepare did not show the reopened page'


def reimport_prepare_confirm_back_retries(binary):
    """Confirm then Back during reimport_changed's prepare_image retries automatically and turns the page."""
    expected, base_log, new_book = run_path_a(binary, (RIGHT,))
    expected_char = diag_of(base_log).get('actual_character')
    assert diag_of(base_log).get('reader_open') == '1' and int(expected_char) > 0

    conf = {'open.back': '/Books/', 'open.confirm': '1', 'open.after_refresh': '3'}
    turned, log, sel_book = run_path_a(binary, (RIGHT,), **conf)
    got = diag_of(log)
    if got.get('reader_open') != '1' or got.get('actual_character') != expected_char or sel_book != new_book:
        return f'Confirm+Back during reimport prepare did not retry and turn page ({got}, want char={expected_char})'
    if turned != expected:
        return 'Confirm+Back during reimport prepare rendered a different page after Right'


def reimport_prepare_failures_stay_closed(binary):
    """Read error and timeout during reimport_changed's prepare_image fail honestly instead of pausing."""
    page, _, _ = run_path_a(binary)
    home, _, _ = run_path_a(binary, **{'open.back': '/Books/', 'open.after_refresh': '3'})
    for kind, key in (('eio', 'open.eio'), ('timeout', 'open.slow')):
        frame, log, _ = run_path_a(binary, **{key: '/Books/', 'open.after_refresh': '3'})
        got = diag_of(log)
        if got.get('reader_open') != '0' or screen_of(log) != 4:
            return f'{kind} during reimport prepare did not stay closed on reading screen ({got}, screen={screen_of(log)})'
        if frame in (page, home):
            return f'{kind} during reimport prepare did not paint a read-failure frame'


def reimport_open_back_goes_home_cleanly(binary):
    """Back during reimport_changed's open_selected_profile (after adopted digest published) rolls back cleanly to Home."""
    clean_home, clean_log, old_book = run_path_b(binary, mutate=False, **{'open.back': '/Books/'})
    assert screen_of(clean_log) == 0 and eligible(clean_log) is True
    base_page, base_log, new_book = run_path_b(binary)
    assert new_book != old_book and diag_of(base_log).get('reader_open') == '1'

    conf = {'open.back': '/Books/', 'open.after_refresh': '1', 'open.skip': '3'}
    home, log, sel_book = run_path_b(binary, **conf)
    if screen_of(log) != 0 or eligible(log) is not True:
        return f'Back during reimport open hid cover or did not go Home (screen={screen_of(log)}, eligible={eligible(log)})'
    if sel_book != old_book:
        return f'Back during reimport open unexpectedly modified durable selection ({sel_book} != {old_book})'
    if home != clean_home:
        return 'Home after Back during reimport open differed from clean Home (leaked notice or wiped title)'
    flashed, _, _ = run_path_b(binary, **conf, freeze='open.back')
    if flashed != home:
        return 'Back during reimport open flashed a closed frame before Home'
    _, slog, _ = run_path_b(binary, (POWER,), **conf)
    if 'storage-warning=0' not in slog:
        return 'Back during reimport open leaked a stale storage warning onto Sleep/Device'
    reopened, rlog, rbook = run_path_b(binary, (BACK,), **conf)
    if diag_of(rlog).get('reader_open') != '1' or diag_of(rlog).get('current_book') != new_book or rbook != new_book:
        return f'Read after Back during reimport open did not reopen and commit changed book ({diag_of(rlog)}, sel={rbook})'
    if reopened != base_page:
        return 'Read after Back during reimport open did not show the reopened page'


def reimport_open_confirm_back_commits_selection(binary):
    """Confirm then Back during reimport_changed's open_selected_profile retries, turns page, and commits new digest."""
    expected, base_log, new_book = run_path_b(binary, (RIGHT,))
    expected_char = diag_of(base_log).get('actual_character')
    assert diag_of(base_log).get('reader_open') == '1' and int(expected_char) > 0

    conf = {'open.back': '/Books/', 'open.confirm': '1', 'open.after_refresh': '1', 'open.skip': '3'}
    turned, log, sel_book = run_path_b(binary, (RIGHT,), **conf)
    got = diag_of(log)
    if got.get('reader_open') != '1' or got.get('actual_character') != expected_char:
        return f'Confirm+Back during reimport open did not retry and turn page ({got}, want char={expected_char})'
    if sel_book != new_book:
        return f'Confirm+Back during reimport open left new digest uncommitted in newt-selected.nrs ({sel_book} != {new_book})'
    if turned != expected:
        return 'Confirm+Back during reimport open rendered a different page after Right'


def reimport_open_timeout_stays_closed(binary):
    """Timeout during reimport_changed's open_selected_profile fails honestly instead of pausing."""
    page, _, _ = run_path_b(binary)
    home, _, _ = run_path_b(binary, **{'open.back': '/Books/', 'open.after_refresh': '1', 'open.skip': '3'})
    frame, log, _ = run_path_b(binary, **{'open.slow': '/Books/', 'open.after_refresh': '1', 'open.skip': '3'})
    got = diag_of(log)
    if got.get('reader_open') != '0' or screen_of(log) != 4:
        return f'timeout during reimport open did not stay closed ({got}, screen={screen_of(log)})'
    if frame in (page, home):
        return 'timeout during reimport open did not paint a read-failure frame'


def reimport_clockwise_and_power_interrupt(binary):
    """Clockwise book reimport shows portrait FirstOpen so Back cancels cleanly to Home; Power enters sleep and resumes."""
    conf_cancel = {'open.back': '/Books/', 'open.button': str(BACK), 'open.after_refresh': '3'}
    home, log, _ = run_path_a(binary, orientation='landscape-cw', **conf_cancel)
    if screen_of(log) != 0 or eligible(log) is not True:
        return f'Back during clockwise FirstOpen reimport did not go Home with cover eligible (screen={screen_of(log)}, eligible={eligible(log)})'
    flashed, _, _ = run_path_a(binary, orientation='landscape-cw', **conf_cancel, freeze='open.back')
    if flashed != home:
        return 'Back during clockwise FirstOpen reimport flashed a frame before Home'

    # Power during reimport_changed pauses the open cleanly into sleep (not a closed failure frame),
    # and the next wake reimports and opens the changed book.
    with tempfile.TemporaryDirectory(prefix='newt-reimport-pwr-') as td:
        d = Path(td)
        preview_driver.world(binary, d, preview_driver.parse(['--library-fixture', 'healthy']))
        book = sorted((d / 'card/Newt/Books').glob('*.epub'))[0]
        pad_epub(book)
        preview_driver.write_conf(d, {'telemetry': 'sample'})
        rust_preview.run(binary, d, (CONFIRM, CONFIRM, RIGHT, POWER))
        mutate_epub_block1(book)
        preview_driver.write_conf(d, {'telemetry': 'sample', 'open.back': '/Books/', 'open.button': str(POWER), 'open.after_refresh': '3'})
        rust_preview.run(binary, d, (), 'deep-sleep')
        if (d / 'exit.txt').read_text().strip() != 'sleep':
            return f'Power during reimport did not enter sleep cleanly (exit={(d / "exit.txt").read_text()!r})'
        preview_driver.write_conf(d, {'telemetry': 'sample'})
        preview_driver.fresh(d)
        _, wlog = rust_preview.run(binary, d, (), 'deep-sleep')
        if screen_of(wlog) != 0 or eligible(wlog) is not True:
            return f'wake after Power-interrupted reimport did not land cleanly on Home (screen={screen_of(wlog)}, eligible={eligible(wlog)})'
        preview_driver.fresh(d)
        _, rlog = rust_preview.run(binary, d, (BACK,), 'power-on')
        if diag_of(rlog).get('reader_open') != '1' or screen_of(rlog) != 4:
            return f'Read after Power-interrupted reimport did not open changed book ({diag_of(rlog)}, screen={screen_of(rlog)})'


CASES = (
    reimport_prepare_back_goes_home,
    reimport_prepare_confirm_back_retries,
    reimport_prepare_failures_stay_closed,
    reimport_open_back_goes_home_cleanly,
    reimport_open_confirm_back_commits_selection,
    reimport_open_timeout_stays_closed,
    reimport_clockwise_and_power_interrupt,
)

DISABLE_PREPARE_INTERRUPT = (
    'reader_image_changed.rs',
    '            if !imported {\n                if self.interrupted_open(home, events) {',
    '            if !imported {\n                if false && self.interrupted_open(home, events) {',
)
DISABLE_OPEN_INTERRUPT = (
    'reader_image_changed.rs',
    '                if matches!(self.memory, ReaderBacking::Interrupted) || self.interrupted_open(home, events) {',
    '                if false && (matches!(self.memory, ReaderBacking::Interrupted) || self.interrupted_open(home, events)) {',
)
DROP_READER_ISSUE_CLEAR = (
    'reader_image_changed.rs',
    '        self.saves.set_reader(newt_core::save_notice::ReaderIssue::None);',
    '        let _ = newt_core::save_notice::ReaderIssue::None;',
)
TIMEOUT_AS_CANCEL = (
    'reader_image_session.rs',
    '        if !events.control.cancelled() || events.control.timed_out() { return false; }',
    '        if !events.control.cancelled() { return false; }',
)

CONTROLS = (
    (reimport_prepare_back_goes_home, *DISABLE_PREPARE_INTERRUPT),
    (reimport_prepare_confirm_back_retries, *DISABLE_PREPARE_INTERRUPT),
    (reimport_open_back_goes_home_cleanly, *DISABLE_OPEN_INTERRUPT),
    (reimport_open_confirm_back_commits_selection, *DISABLE_OPEN_INTERRUPT),
    (reimport_open_back_goes_home_cleanly, *DROP_READER_ISSUE_CLEAR),
    (reimport_prepare_failures_stay_closed, *TIMEOUT_AS_CANCEL),
    (reimport_open_timeout_stays_closed, *TIMEOUT_AS_CANCEL),
)


def main():
    binary = rust_preview.FIRMWARE if len(sys.argv) == 2 else rust_preview.build()
    for case in CASES:
        problem = case(binary)
        if problem:
            raise SystemExit(f'FAIL {case.__name__}: {problem}')
        print(f'PASS {case.__name__}')
    if negative_controls(__file__):
        built = {}
        for case, source, old, new in CONTROLS:
            key = (source, old, new)
            if key not in built:
                built[key] = rust_preview.control([key], optimize=False)
            problem = case(built[key])
            if not problem:
                raise SystemExit(f'FAIL control {source} -> {case.__name__} did not notice the mutation')
            print(f'PASS control {source} -> {case.__name__}: {problem}')
    print('PASS preview_reader_reimport_test')


if __name__ == '__main__':
    main()
