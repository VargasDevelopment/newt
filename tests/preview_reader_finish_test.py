#!/usr/bin/env python3
"""Host preview tests and negative controls for #125 (Mark as finished during pending verification)."""
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

BACK, CONFIRM, LEFT, RIGHT, UP, DOWN, POWER, WAIT_MINUTE, QUEUED = range(9)
GEN = re.compile(r'STATUS screen=(\d+) gen=(\d+) ')
# Reading menu rows: 0=Return, 1=Chapters, 2=Reading settings, 3=Reading statistics,
#                    4=Mark as finished / not finished, 5=Home.
QUEUED_MENU_OPEN = (CONFIRM, QUEUED, RIGHT, QUEUED, RIGHT, QUEUED, RIGHT, QUEUED, RIGHT)
QUEUED_MENU_MARK = (*QUEUED_MENU_OPEN, QUEUED, CONFIRM)
QUEUED_EXIT_HOME = (*QUEUED_MENU_OPEN, QUEUED, RIGHT, QUEUED, CONFIRM)


def status_gen(directory):
    m = GEN.search((directory / 'receipt.txt').read_text())
    assert m, 'missing STATUS line in receipt.txt'
    return int(m.group(1)), int(m.group(2))


def parse_pbm(data):
    _, size, pixels = data.split(b'\n', 2)
    w, h = map(int, size.split())
    stride = (w + 7) // 8
    return [pixels[y * stride:(y + 1) * stride] for y in range(h)]


def crop(rows, x0, y0, w, h):
    return bytes(
        (rows[y][x >> 3] >> (7 - (x & 7))) & 1
        for y in range(y0, y0 + h) for x in range(x0, x0 + w)
    )


def footer_strip(frame_bytes):
    # Portrait footer (where PG SAVE ERR / STORAGE or Book changed appears).
    return crop(parse_pbm(frame_bytes), 0, 750, 528, 42)


def finish_row_strip(frame_bytes):
    # Portrait Reading menu row 4 (Mark as finished / Mark as not finished), above hint line (y=550).
    return crop(parse_pbm(frame_bytes), 40, 450, 440, 45)


def latest_slot(path, magic):
    """Return (sequence, book_u64, checkpoint_crc_u32) from the newest valid NRS2 slot, or None."""
    if not path.is_file():
        return None
    data = path.read_bytes()
    if len(data) != 4096 or data[:4] != b'NRS2':
        return None
    slots = [at for at in range(512, len(data), 512)
             if data[at:at + 4] == b'NRSK' and data[at + 20:at + 24] == magic]
    if not slots:
        return None
    at = max(slots, key=lambda pos: struct.unpack_from('<I', data, pos + 28)[0])
    seq = struct.unpack_from('<I', data, at + 28)[0]
    book = struct.unpack_from('<Q', data, at + 32)[0]
    crc_offset = 68 if magic == b'NWTR' else 40
    crc = struct.unpack_from('<I', data, at + crc_offset)[0]
    return seq, book, crc


def progress_and_resume(directory):
    state = directory / 'card/Newt/State'
    nbs = sorted(state.glob('nb*.nrs'))
    nb = latest_slot(nbs[0], b'NWTR') if nbs else None
    resume = latest_slot(state / 'newt-resume.nrs', b'NWBR')
    return nb, resume


def cold_boot_menu(binary, directory, *, from_home=False):
    """Discard retained RTC and resume picture, cold-boot, open Reading menu on row 4."""
    (directory / 'retained.bin').unlink(missing_ok=True)
    (directory / 'card/Newt/Cache/resume.pic').unlink(missing_ok=True)
    preview_driver.fresh(directory)
    keys = (BACK, *QUEUED_MENU_OPEN) if from_home else QUEUED_MENU_OPEN
    rust_preview.run(binary, directory, keys, 'power-on')
    return (directory / 'frame.pbm').read_bytes(), status_gen(directory)[0]


def crc_neutral_ascii(base, flip_index):
    """Flip bit 0 at flip_index and adjust 32 following '0'/'1' bytes to keep exact CRC-32."""
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


def seed_warm_home_multiblock(binary, directory):
    """Seed a warm book whose chapter 2 spans unread block 1 (4096..8192) during warm spine-0 open."""
    options = preview_driver.parse(['--library-fixture', 'healthy'])
    preview_driver.world(binary, directory, options)
    book = sorted((directory / 'card/Newt/Books').glob('*.epub'))[0]
    with zipfile.ZipFile(book, 'r') as src:
        entries = [(info, src.read(info)) for info in src.infolist()]
    pad = b'<!-- ' + b'0' * 8192 + b' -->'
    with zipfile.ZipFile(book, 'w', zipfile.ZIP_STORED) as dst:
        for info, data in entries:
            if info.filename.endswith('two.xhtml'):
                data = data.replace(b'</body>', pad + b'</body>')
            dst.writestr(info.filename, data, compress_type=zipfile.ZIP_STORED)
    preview_driver.write_conf(directory, {'telemetry': 'sample'})
    rust_preview.run(binary, directory, (CONFIRM, CONFIRM, WAIT_MINUTE, *QUEUED_EXIT_HOME))
    return book


def mutate_epub_block1(book):
    """Modify chapter 2 strictly inside block 1 (4096..8192) with identical CRC-32, size, and mtime.
    Warm open (block 0 + ZIP tail) admits Verification::Pending; full SHA-256 verification detects Changed."""
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


def seed_warm_home(binary, directory):
    """Open the fixture book once, let Pages/ cache build on healthy battery, and return to Home."""
    options = preview_driver.parse(['--library-fixture', 'healthy'])
    preview_driver.world(binary, directory, options)
    preview_driver.write_conf(directory, {'telemetry': 'sample'})
    rust_preview.run(binary, directory, (CONFIRM, CONFIRM, WAIT_MINUTE, *QUEUED_EXIT_HOME))


def seed_warm_sleep(binary, directory):
    """Open the fixture book once, turn one page, let Pages/ cache build on healthy battery, and sleep."""
    options = preview_driver.parse(['--library-fixture', 'healthy'])
    preview_driver.world(binary, directory, options)
    preview_driver.write_conf(directory, {'telemetry': 'sample'})
    rust_preview.run(binary, directory, (CONFIRM, CONFIRM, RIGHT, WAIT_MINUTE, POWER))


def stats_files(directory):
    root = directory / 'card/Newt/Stats'
    if not root.exists():
        return set()
    return {p.name for p in root.iterdir() if p.is_file()}


def canonical_menu_frames(binary, directory, *, open_keys, boot='power-on'):
    """Wait past verification, then capture verified Mark-succeeded (Undo) and Mark-not-finished frames."""
    preview_driver.write_conf(directory, {'telemetry': '', 'store.refuse': '', 'store.drop': '', 'open.slow': ''})
    preview_driver.fresh(directory)
    rust_preview.run(binary, directory, (*open_keys, WAIT_MINUTE, *QUEUED_MENU_MARK), boot)
    undo_frame = (directory / 'frame.pbm').read_bytes()
    if open_keys:
        preview_driver.fresh(directory)
        rust_preview.run(binary, directory, QUEUED_EXIT_HOME)
    preview_driver.fresh(directory)
    rust_preview.run(binary, directory, (*open_keys, WAIT_MINUTE, *QUEUED_MENU_MARK), boot)
    mark_frame = (directory / 'frame.pbm').read_bytes()
    if undo_frame == mark_frame or finish_row_strip(undo_frame) == finish_row_strip(mark_frame):
        raise AssertionError('canonical finished vs not-finished menu frames did not differ')
    return undo_frame, mark_frame


def mark_finished_immediately_across_open_paths(binary):
    """Mark as finished settles verification + owed selection/progress commits across every open path."""
    with tempfile.TemporaryDirectory(prefix='newt-finish-ref-') as ref_td:
        ref = Path(ref_td)
        seed_warm_home(binary, ref)
        home_undo, home_mark = canonical_menu_frames(binary, ref, open_keys=(BACK,))
    with tempfile.TemporaryDirectory(prefix='newt-finish-ref-wake-') as ref_td:
        ref = Path(ref_td)
        seed_warm_sleep(binary, ref)
        wake_undo, _ = canonical_menu_frames(binary, ref, open_keys=(), boot='deep-sleep')

    # 1. Home -> Read fast-open: older acknowledgement exists in nb*.nrs AND ACCEPTED_OPEN_OWED is set
    #    (NWBR is Home tombstone book=0). Mark as finished must explicitly settle progress + NWBR first.
    with tempfile.TemporaryDirectory(prefix='newt-finish-home-') as td:
        d = Path(td)
        seed_warm_home(binary, d)
        before_nb, before_resume = progress_and_resume(d)
        assert before_nb is not None and before_resume is not None and before_resume[1] == 0
        preview_driver.write_conf(d, {'telemetry': ''})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (BACK, *QUEUED_MENU_MARK))
        got = (d / 'frame.pbm').read_bytes()
        after_nb, after_resume = progress_and_resume(d)
        if got != home_undo or not any(n.startswith('sd') for n in stats_files(d)):
            return 'Home fast-open Mark as finished did not complete in one press'
        if after_nb is None or after_nb[0] <= before_nb[0]:
            return f'Home fast-open with older acknowledgement skipped owed progress commit ({before_nb} -> {after_nb})'
        if after_resume is None or after_resume[1] != after_nb[1] or after_resume[2] != after_nb[2]:
            return f'Home fast-open did not commit reading destination NWBR ({after_nb} vs {after_resume})'
        cold_frame, cold_screen = cold_boot_menu(binary, d)
        if cold_screen != 4 or cold_frame != home_undo:
            return 'cold restart after Home fast-open Mark as finished did not restore book and finished state'
        # Toggle back to not finished right after another fast open from Home.
        preview_driver.fresh(d)
        rust_preview.run(binary, d, QUEUED_EXIT_HOME)
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (BACK, *QUEUED_MENU_MARK))
        if (d / 'frame.pbm').read_bytes() != home_mark:
            return 'Home fast-open Mark as not finished did not complete in one press'

    # 2. Books -> Open (TITLE_DEFERRED defers COMMIT_OWED and ACCEPTED_OPEN_OWED).
    with tempfile.TemporaryDirectory(prefix='newt-finish-books-') as td:
        d = Path(td)
        seed_warm_home(binary, d)
        preview_driver.write_conf(d, {'telemetry': ''})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (CONFIRM, CONFIRM, *QUEUED_MENU_MARK))
        got = (d / 'frame.pbm').read_bytes()
        after_nb, after_resume = progress_and_resume(d)
        if got != home_undo or not any(n.startswith('sd') for n in stats_files(d)):
            return 'Books fast-open Mark as finished did not complete in one press'
        if after_nb is None or after_resume is None or after_resume[1] != after_nb[1] or after_resume[2] != after_nb[2]:
            return f'Books fast-open did not commit progress + destination ({after_nb} vs {after_resume})'

    # 3. Deep-sleep wake into book (accepted_open_owed == false).
    with tempfile.TemporaryDirectory(prefix='newt-finish-wake-') as td:
        d = Path(td)
        seed_warm_sleep(binary, d)
        preview_driver.write_conf(d, {'telemetry': ''})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, QUEUED_MENU_MARK, 'deep-sleep')
        got = (d / 'frame.pbm').read_bytes()
        if got != wake_undo or not any(n.startswith('sd') for n in stats_files(d)):
            return 'deep-sleep wake Mark as finished did not complete in one press'

    # 5. Cold-boot recovery into book (retained.bin + resume.pic removed, accepted_open_owed == false).
    with tempfile.TemporaryDirectory(prefix='newt-finish-cold-') as td:
        d = Path(td)
        seed_warm_sleep(binary, d)
        (d / 'retained.bin').unlink(missing_ok=True)
        (d / 'card/Newt/Cache/resume.pic').unlink(missing_ok=True)
        preview_driver.write_conf(d, {'telemetry': ''})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, QUEUED_MENU_MARK, 'power-on')
        got = (d / 'frame.pbm').read_bytes()
        if got != wake_undo or not any(n.startswith('sd') for n in stats_files(d)):
            return 'cold-boot recovery Mark as finished did not complete in one press'

    # 6. Book open with unseeded reading stats (no Newt/Stats/newt-stats.nrs yet): seeds on demand.
    with tempfile.TemporaryDirectory(prefix='newt-finish-unseeded-') as td:
        d = Path(td)
        seed_warm_sleep(binary, d)
        for p in (d / 'card/Newt/Stats').glob('*'):
            p.unlink()
        preview_driver.write_conf(d, {'telemetry': ''})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, QUEUED_MENU_MARK, 'deep-sleep')
        got = (d / 'frame.pbm').read_bytes()
        if got != wake_undo or 'newt-stats.nrs' not in stats_files(d):
            return 'book open with unseeded stats failed to seed and mark finished'


def books_no_nb_commits_progress_and_destination(binary):
    """Books fast-open with warm Pages/ cache and no nb*.nrs commits progress and NWBR despite synthetic zero ack."""
    with tempfile.TemporaryDirectory(prefix='newt-finish-ref-nonb-') as ref_td:
        ref = Path(ref_td)
        seed_warm_home(binary, ref)
        home_undo, _ = canonical_menu_frames(binary, ref, open_keys=(BACK,))
    with tempfile.TemporaryDirectory(prefix='newt-finish-books-no-nb-') as td:
        d = Path(td)
        seed_warm_home(binary, d)
        for nb in (d / 'card/Newt/State').glob('nb*.nrs'):
            nb.unlink()
        preview_driver.write_conf(d, {'telemetry': ''})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (CONFIRM, CONFIRM, *QUEUED_MENU_MARK))
        got = (d / 'frame.pbm').read_bytes()
        after_nb, after_resume = progress_and_resume(d)
        if got != home_undo or not any(n.startswith('sd') for n in stats_files(d)):
            return 'Books fast-open without prior nb*.nrs did not save checkpoint and mark finished'
        if after_nb is None or after_nb[0] < 1:
            return f'Books fast-open without prior nb*.nrs did not write nb*.nrs progress record ({after_nb})'
        if after_resume is None or after_resume[1] != after_nb[1] or after_resume[2] != after_nb[2]:
            return f'Books fast-open without prior nb*.nrs did not commit NWBR destination ({after_nb} vs {after_resume})'
        cold_frame, cold_screen = cold_boot_menu(binary, d)
        if cold_screen != 4 or cold_frame != home_undo:
            return 'cold restart after no-nb Books fast-open Mark as finished did not restore book and finished state'


def books_open_after_idle_verify_commits_selection(binary):
    """After idle verification finishes on a Books warm open (<30s due), Mark as finished commits selection, progress, and stats."""
    with tempfile.TemporaryDirectory(prefix='newt-finish-books-idle-') as td:
        d = Path(td)
        seed_warm_home(binary, d)
        preview_driver.write_conf(d, {'telemetry': 'sample'})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (CONFIRM, CONFIRM, WAIT_MINUTE, *QUEUED_MENU_MARK))
        expected_undo = (d / 'frame.pbm').read_bytes()

    with tempfile.TemporaryDirectory(prefix='newt-finish-books-idle2-') as td:
        d = Path(td)
        seed_warm_home(binary, d)
        preview_driver.write_conf(d, {'telemetry': 'sample'})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (CONFIRM, CONFIRM, *QUEUED_MENU_MARK))
        got = (d / 'frame.pbm').read_bytes()
        after_nb, after_resume = progress_and_resume(d)
        if got != expected_undo or not any(n.startswith('sd') for n in stats_files(d)):
            return 'Books open after idle verification (<30s due) failed to commit selection and mark finished'
        if after_nb is None or after_resume is None or after_resume[1] != after_nb[1] or after_resume[2] != after_nb[2]:
            return 'Books open after idle verification (<30s due) did not commit progress + destination'


def menu_does_not_stall_background_verification(binary):
    """Opening the Reading menu on healthy battery continues idle verification without waiting for 30s due."""
    with tempfile.TemporaryDirectory(prefix='newt-finish-menu-pre-') as td_pre:
        d_pre = Path(td_pre)
        seed_warm_home(binary, d_pre)
        preview_driver.write_conf(d_pre, {'telemetry': 'sample'})
        preview_driver.fresh(d_pre)
        rust_preview.run(binary, d_pre, (BACK, CONFIRM, RIGHT, RIGHT, RIGHT, RIGHT, CONFIRM))
        expected_undo = (d_pre / 'frame.pbm').read_bytes()
        _, gen_pre = status_gen(d_pre)

    with tempfile.TemporaryDirectory(prefix='newt-finish-menu-imm-') as td_imm:
        d_imm = Path(td_imm)
        seed_warm_home(binary, d_imm)
        preview_driver.write_conf(d_imm, {'telemetry': 'sample'})
        preview_driver.fresh(d_imm)
        rust_preview.run(binary, d_imm, (BACK, QUEUED, CONFIRM, RIGHT, RIGHT, RIGHT, RIGHT, CONFIRM))
        if (d_imm / 'frame.pbm').read_bytes() != expected_undo:
            return 'Mark as finished after opening Menu immediately did not produce finished state'
        _, gen_imm = status_gen(d_imm)
        if gen_imm >= gen_pre:
            return (f'Menu stalled idle verification and triggered extra foreground work: '
                    f'immediate gen={gen_imm} vs pre-verified gen={gen_pre}')

    with tempfile.TemporaryDirectory(prefix='newt-finish-menu-chg-') as td_chg:
        d_chg = Path(td_chg)
        seed_warm_home_multiblock(binary, d_chg)
        preview_driver.write_conf(d_chg, {'telemetry': 'sample'})
        preview_driver.fresh(d_chg)
        rust_preview.run(binary, d_chg, (BACK, QUEUED, CONFIRM))
        unmodified_menu = (d_chg / 'frame.pbm').read_bytes()

    with tempfile.TemporaryDirectory(prefix='newt-finish-menu-chg2-') as td_chg2:
        d_chg2 = Path(td_chg2)
        book = seed_warm_home_multiblock(binary, d_chg2)
        mutate_epub_block1(book)
        preview_driver.write_conf(d_chg2, {'telemetry': 'sample'})
        preview_driver.fresh(d_chg2)
        rust_preview.run(binary, d_chg2, (BACK, QUEUED, CONFIRM))
        changed_menu = (d_chg2 / 'frame.pbm').read_bytes()
        if changed_menu == unmodified_menu:
            return 'background verification while Reading menu was open did not detect mutated EPUB before 30s due'


def cancelled_and_error_notices_stay_honest(binary):
    """Timeout, write refusal, seeding sync-loss, unconfirmed progress commit, and changed EPUB stay honest."""
    # Canonical unfinished (clean_menu) and finished (undo_frame) frames on deep-sleep wake.
    with tempfile.TemporaryDirectory(prefix='newt-finish-clean-') as td:
        d = Path(td)
        seed_warm_sleep(binary, d)
        preview_driver.write_conf(d, {'telemetry': ''})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, QUEUED_MENU_OPEN, 'deep-sleep')
        clean_menu = (d / 'frame.pbm').read_bytes()
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (WAIT_MINUTE, *QUEUED_MENU_MARK), 'power-on')
        undo_frame = (d / 'frame.pbm').read_bytes()
    assert finish_row_strip(clean_menu) != finish_row_strip(undo_frame)

    # 1. Timed-out verification on wake shows FinishOpening without PG SAVE ERR and stays unfinished.
    with tempfile.TemporaryDirectory(prefix='newt-finish-cancel-') as td:
        d = Path(td)
        seed_warm_sleep(binary, d)
        preview_driver.write_conf(d, {'telemetry': '', 'card.slow': '.epub', 'card.after_refresh': '2'})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, QUEUED_MENU_MARK, 'deep-sleep')
        opening_frame = (d / 'frame.pbm').read_bytes()
        screen, _ = status_gen(d)
        if screen != 4:
            return f'timed-out MarkFinished verification left Reader screen (screen={screen})'
        preview_driver.write_conf(d, {'telemetry': '', 'card.slow': '', 'card.after_refresh': ''})
        after_cancel_menu, _ = cold_boot_menu(binary, d)

    # 2. Refused stats write: verify unfinished before, error + unfinished row after, and unfinished after cold boot.
    with tempfile.TemporaryDirectory(prefix='newt-finish-refuse-') as td:
        d = Path(td)
        seed_warm_sleep(binary, d)
        preview_driver.write_conf(d, {'telemetry': ''})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, QUEUED_MENU_OPEN, 'deep-sleep')
        if (d / 'frame.pbm').read_bytes() != clean_menu:
            return 'refused-write fixture did not start unfinished'
        preview_driver.write_conf(d, {'telemetry': '', 'store.refuse': 'card-write:/newt-sd/Newt/Stats'})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (WAIT_MINUTE, *QUEUED_MENU_MARK), 'power-on')
        failed_frame = (d / 'frame.pbm').read_bytes()
        preview_driver.write_conf(d, {'telemetry': '', 'store.refuse': ''})
        after_refuse_cold, _ = cold_boot_menu(binary, d)

    if after_cancel_menu != clean_menu:
        return 'cancelled verification unexpectedly marked the book finished'
    if opening_frame in (undo_frame, clean_menu, failed_frame):
        return 'cancelled verification did not show distinct FinishOpening notice'
    if finish_row_strip(opening_frame) != finish_row_strip(clean_menu):
        return 'cancelled verification changed the finish row'
    if footer_strip(opening_frame) != footer_strip(clean_menu):
        return 'cancelled MarkFinished verification falsely dirtied the reader footer (PG SAVE ERR)'
    if failed_frame in (undo_frame, clean_menu, opening_frame):
        return 'refused stats write did not show distinct FinishSaveFailed notice'
    if finish_row_strip(failed_frame) != finish_row_strip(clean_menu):
        return 'refused stats write falsely changed the finish row to Undo'
    if after_refuse_cold != clean_menu:
        return 'cold restart after refused stats write did not remain unfinished'

    # 3. Sync-loss during on-demand seeding on an unseeded card: allow .tmp envelope creation to
    #    rename into newt-stats.nrs, then drop fsync on newt-stats.nrs itself (trailing '$'). Bytes
    #    land on disk, but readback_durable() is false so firmware must show FinishUnconfirmed
    #    (never success on readback alone); cold restart restores complete old or new state cleanly.
    with tempfile.TemporaryDirectory(prefix='newt-finish-seed-drop-') as td:
        d = Path(td)
        seed_warm_sleep(binary, d)
        ledger = d / 'card/Newt/Stats/newt-stats.nrs'
        for p in (d / 'card/Newt/Stats').glob('*'):
            p.unlink()
        preview_driver.write_conf(d, {'telemetry': '', 'store.drop': 'card-sync:/newt-sd/Newt/Stats/newt-stats.nrs$'})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, QUEUED_MENU_MARK, 'deep-sleep')
        unconfirmed_frame = (d / 'frame.pbm').read_bytes()
        if not ledger.is_file():
            return 'sync-loss fixture did not reach newt-stats.nrs slot write before fsync dropped'
        if unconfirmed_frame in (undo_frame, clean_menu, failed_frame, opening_frame):
            return 'sync-loss during on-demand seeding did not show distinct FinishUnconfirmed notice'
        if finish_row_strip(unconfirmed_frame) != finish_row_strip(clean_menu):
            return 'sync-loss during on-demand seeding falsely updated finish row on readback alone'
        preview_driver.write_conf(d, {'telemetry': '', 'store.drop': ''})
        after_drop_cold, _ = cold_boot_menu(binary, d)
        if after_drop_cold not in (clean_menu, undo_frame):
            return 'cold restart after seeding sync-loss did not restore a complete state'
        if after_drop_cold != clean_menu:
            return 'cold restart after seeding sync-loss unexpectedly marked book finished'
        # Simulate power loss dropping the unsynced ledger file too: cold restart stays clean and retry succeeds.
        for p in (d / 'card/Newt/Stats').glob('*'):
            p.unlink()
        after_lost_cold, _ = cold_boot_menu(binary, d)
        if after_lost_cold != clean_menu:
            return 'cold restart after lost unsynced ledger did not restore clean unfinished state'
        preview_driver.fresh(d)
        rust_preview.run(binary, d, QUEUED_MENU_MARK, 'power-on')
        if (d / 'frame.pbm').read_bytes() != undo_frame:
            return 'retry after seeding sync-loss failed to seed and mark finished'

    # 4. Unconfirmed progress save during owed accepted-open commit on Home fast-open:
    #    must keep honest unconfirmed notice and not mark finished.
    with tempfile.TemporaryDirectory(prefix='newt-finish-prog-drop-') as td:
        d = Path(td)
        seed_warm_home(binary, d)
        preview_driver.write_conf(d, {'telemetry': ''})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (BACK, WAIT_MINUTE, *QUEUED_MENU_OPEN))
        home_clean = (d / 'frame.pbm').read_bytes()
        preview_driver.fresh(d)
        rust_preview.run(binary, d, QUEUED_EXIT_HOME)
        preview_driver.write_conf(d, {'telemetry': '', 'store.drop': 'card-sync:/newt-sd/Newt/State'})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (BACK, *QUEUED_MENU_MARK))
        prog_unconfirmed = (d / 'frame.pbm').read_bytes()
        if finish_row_strip(prog_unconfirmed) != finish_row_strip(home_clean):
            return 'unconfirmed owed progress commit still changed the finish row'
        if prog_unconfirmed == home_clean or footer_strip(prog_unconfirmed) == footer_strip(home_clean):
            return 'unconfirmed owed progress commit hid the unconfirmed save notice'

    # 5. Mutated EPUB detected during MarkFinished: warm open admits Pending, on-demand verification
    #    reimports and reopens replacement UNFINISHED (never replays mark onto the new digest).
    with tempfile.TemporaryDirectory(prefix='newt-finish-mut-ref-') as ref_td:
        ref = Path(ref_td)
        book = seed_warm_home_multiblock(binary, ref)
        mutate_epub_block1(book)
        preview_driver.write_conf(ref, {'telemetry': ''})
        preview_driver.fresh(ref)
        rust_preview.run(binary, ref, (BACK, WAIT_MINUTE))
        assert status_gen(ref)[0] == 4 and 'reader_open=1' in (ref / 'receipt.txt').read_text()
        preview_driver.fresh(ref)
        rust_preview.run(binary, ref, (BACK, *QUEUED_MENU_OPEN))
        mutated_clean_menu = (ref / 'frame.pbm').read_bytes()
        preview_driver.fresh(ref)
        rust_preview.run(binary, ref, (BACK, *QUEUED_MENU_MARK))
        mutated_undo_frame = (ref / 'frame.pbm').read_bytes()
    assert finish_row_strip(mutated_clean_menu) != finish_row_strip(mutated_undo_frame)

    with tempfile.TemporaryDirectory(prefix='newt-finish-changed-') as td:
        d = Path(td)
        book = seed_warm_home_multiblock(binary, d)
        mutate_epub_block1(book)
        preview_driver.write_conf(d, {'telemetry': ''})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (BACK, *QUEUED_MENU_MARK))
        reopened_page = (d / 'frame.pbm').read_bytes()
        if status_gen(d)[0] != 4 or 'reader_open=1' not in (d / 'receipt.txt').read_text():
            return 'MarkFinished on a changed EPUB did not reopen the replacement book'
        if reopened_page in (mutated_clean_menu, mutated_undo_frame):
            return 'MarkFinished on a changed EPUB left the Reading menu on screen instead of closing to the reopened page'
        # Open Menu on the reopened replacement (via power-on -> Home -> Read, or direct cold boot) and confirm UNFINISHED.
        changed_menu_before, cold_screen = cold_boot_menu(binary, d, from_home=True)
        if cold_screen != 4 or changed_menu_before != mutated_clean_menu or changed_menu_before == mutated_undo_frame:
            return 'MarkFinished on a changed EPUB did not leave the reopened replacement unfinished'
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (BACK, *QUEUED_MENU_MARK))
        if (d / 'frame.pbm').read_bytes() != mutated_undo_frame:
            return 'explicit MarkFinished on the reopened replacement did not mark it finished'


CASES = (
    mark_finished_immediately_across_open_paths,
    books_no_nb_commits_progress_and_destination,
    books_open_after_idle_verify_commits_selection,
    menu_does_not_stall_background_verification,
    cancelled_and_error_notices_stay_honest,
)

CONTROLS = (
    (mark_finished_immediately_across_open_paths, 'reader_image_action.rs',
     '        if needs_verify && !self.finish_verification_core(home, ui, logical, events, false, false) {',
     '        if false && needs_verify && !self.finish_verification_core(home, ui, logical, events, false, false) {'),
    (mark_finished_immediately_across_open_paths, 'reader_image_action.rs',
     '        if progress_owed {',
     '        if self.checkpoint.acknowledged_for(&before_digest).is_none() {'),
    (books_no_nb_commits_progress_and_destination, 'reader_image_action.rs',
     '        if progress_owed {',
     '        if self.checkpoint.acknowledged_for(&before_digest).is_none() {'),
    (mark_finished_immediately_across_open_paths, 'reading_stats_runtime.rs',
     'if self.condition == StatsCondition::Unseeded && !page_unconfirmed {',
     'if false && self.condition == StatsCondition::Unseeded && !page_unconfirmed {'),
    (menu_does_not_stall_background_verification, 'reader_image_verification.rs',
     'let optional = ui.displayed.valid',
     'let optional = ui.displayed.valid && self.book().is_some_and(|b| b.ui.panel == newt_reader_ui::ReaderPanel::Page)'),
    (cancelled_and_error_notices_stay_honest, 'reader_image_action.rs',
     '                self.reader_ui.notice = if still_pending {\n                    ReaderUiNotice::FinishOpening',
     '                self.reader_ui.notice = if still_pending {\n                    ReaderUiNotice::FinishSaveFailed'),
    (cancelled_and_error_notices_stay_honest, 'reading_stats_runtime.rs',
     '        if report.result == StatsResult::Done {\n'
     '            show_finish(ui, report.finish);\n'
     '        }\n'
     '        ui.notice = match report.result {',
     '        show_finish(ui, if finish { BookFinish::Yes } else { BookFinish::No });\n'
     '        ui.notice = match StatsResult::Done {'),
    (cancelled_and_error_notices_stay_honest, 'reading_stats_runtime.rs',
     '                ui.notice = if seeded.result == StatsResult::Unconfirmed {\n'
     '                    ReaderUiNotice::FinishUnconfirmed',
     '                ui.notice = if seeded.result == StatsResult::Unconfirmed {\n'
     '                    ReaderUiNotice::FinishSaveFailed'),
    (cancelled_and_error_notices_stay_honest, 'reader_image_action.rs',
     '        if *self.selection.digest() != before_digest\n'
     '            || self.reader_ui.notice == ReaderUiNotice::BookChanged\n'
     '            || !self.opened() {',
     '        if !self.opened() {'),
)


def main():
    # The gate passes its one preview CLI; raw runs use its compiled firmware binary.
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
                built[key] = rust_preview.control([key])
            problem = case(built[key])
            if not problem:
                raise SystemExit(f'FAIL control {source} -> {case.__name__} did not notice the mutation')
            print(f'PASS control {source} -> {case.__name__}: {problem}')
    print('PASS preview_reader_finish_test')


if __name__ == '__main__':
    main()
