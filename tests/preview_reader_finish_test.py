#!/usr/bin/env python3
"""Host preview tests and negative controls for #125 (Mark as finished during pending verification)."""
import hashlib
import os
from pathlib import Path
import re
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
QUEUED_MENU_MARK = (CONFIRM, QUEUED, RIGHT, QUEUED, RIGHT, QUEUED, RIGHT, QUEUED, RIGHT, QUEUED, CONFIRM)
QUEUED_EXIT_HOME = (CONFIRM, QUEUED, RIGHT, QUEUED, RIGHT, QUEUED, RIGHT, QUEUED, RIGHT, QUEUED, RIGHT, QUEUED, CONFIRM)


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


def mutate_epub_same_size(book):
    """Rewrite a valid EPUB keeping exact file size and mtime, changing its SHA-256."""
    st = book.stat()
    orig = book.read_bytes()
    with zipfile.ZipFile(book, 'r') as src:
        entries = [(info, src.read(info)) for info in src.infolist()]
    changed, new_entries = False, []
    for info, data in entries:
        if not changed and info.filename.endswith('.xhtml') and b'lantern' in data.lower():
            idx = data.lower().find(b'lantern')
            b = bytearray(data)
            b[idx] = ord('B') if b[idx] == ord('L') else ord('b')
            data = bytes(b)
            changed = True
        new_entries.append((info, data))
    assert changed
    with zipfile.ZipFile(book, 'w') as dst:
        for info, data in new_entries:
            dst.writestr(info.filename, data, compress_type=info.compress_type)
    cur = book.read_bytes()
    if len(cur) > st.st_size:
        with zipfile.ZipFile(book, 'w', zipfile.ZIP_DEFLATED) as dst:
            for info, data in new_entries:
                ctype = zipfile.ZIP_STORED if info.filename == 'mimetype' else zipfile.ZIP_DEFLATED
                dst.writestr(info.filename, data, compress_type=ctype)
        cur = book.read_bytes()
    if len(cur) < st.st_size:
        with zipfile.ZipFile(book, 'a') as dst:
            dst.comment = b' ' * (st.st_size - len(cur))
    assert book.stat().st_size == st.st_size and book.read_bytes() != orig
    os.utime(book, (st.st_atime, st.st_mtime))
    return hashlib.sha256(book.read_bytes()).hexdigest()


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
    if undo_frame == mark_frame:
        raise AssertionError('canonical finished vs not-finished menu frames did not differ')
    return undo_frame, mark_frame


def mark_finished_immediately_across_open_paths(binary):
    """Mark as finished completes verification and commits in one press across Home, Books, wake, and cold boot."""
    with tempfile.TemporaryDirectory(prefix='newt-finish-ref-') as ref_td:
        ref = Path(ref_td)
        seed_warm_home(binary, ref)
        home_undo, home_mark = canonical_menu_frames(binary, ref, open_keys=(BACK,))
    with tempfile.TemporaryDirectory(prefix='newt-finish-ref-wake-') as ref_td:
        ref = Path(ref_td)
        seed_warm_sleep(binary, ref)
        wake_undo, _ = canonical_menu_frames(binary, ref, open_keys=(), boot='deep-sleep')

    # 1. Home -> Read (warm cache, telemetry='' so verification is Pending).
    with tempfile.TemporaryDirectory(prefix='newt-finish-home-') as td:
        d = Path(td)
        seed_warm_home(binary, d)
        preview_driver.write_conf(d, {'telemetry': ''})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (BACK, *QUEUED_MENU_MARK))
        got = (d / 'frame.pbm').read_bytes()
        if got != home_undo or not any(n.startswith('sd') for n in stats_files(d)):
            return 'Home fast-open Mark as finished did not complete in one press'
        # Toggle back to not finished right after another fast open from Home.
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (BACK, *QUEUED_MENU_MARK))
        if (d / 'frame.pbm').read_bytes() != home_mark:
            return 'Home fast-open Mark as not finished did not complete in one press'

    # 2. Books -> Open (TITLE_DEFERRED clears accepted_open_owed and defers COMMIT_OWED).
    with tempfile.TemporaryDirectory(prefix='newt-finish-books-') as td:
        d = Path(td)
        seed_warm_home(binary, d)
        preview_driver.write_conf(d, {'telemetry': ''})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (CONFIRM, CONFIRM, *QUEUED_MENU_MARK))
        got = (d / 'frame.pbm').read_bytes()
        if got != home_undo or not any(n.startswith('sd') for n in stats_files(d)):
            return 'Books fast-open Mark as finished did not complete in one press'

    # 3. Books -> Open when warm Pages/ exists but nb*.nrs progress record is missing.
    with tempfile.TemporaryDirectory(prefix='newt-finish-books-no-nb-') as td:
        d = Path(td)
        seed_warm_home(binary, d)
        for nb in (d / 'card/Newt/State').glob('nb*.nrs'):
            nb.unlink()
        preview_driver.write_conf(d, {'telemetry': ''})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (CONFIRM, CONFIRM, *QUEUED_MENU_MARK))
        got = (d / 'frame.pbm').read_bytes()
        if got != home_undo or not any(n.startswith('sd') for n in stats_files(d)):
            return 'Books fast-open without prior nb*.nrs did not save checkpoint and mark finished'

    # 4. Deep-sleep wake into book (accepted_open_owed == false).
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

    # 6. Book open with unseeded reading stats (no Newt/Stats/newt-stats.nrs yet, e.g. fresh card
    #    opened before Home idle ran, or boot directly into a book): Mark as finished seeds on demand.
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


def books_open_after_idle_verify_commits_selection(binary):
    """After idle verification finishes on a Books warm open (<30s due), Mark as finished commits selection and saves."""
    with tempfile.TemporaryDirectory(prefix='newt-finish-books-idle-') as td:
        d = Path(td)
        seed_warm_home(binary, d)
        # Capture reference verified frame (with telemetry=sample so battery footer matches).
        preview_driver.write_conf(d, {'telemetry': 'sample'})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (CONFIRM, CONFIRM, WAIT_MINUTE, *QUEUED_MENU_MARK))
        expected_undo = (d / 'frame.pbm').read_bytes()

    with tempfile.TemporaryDirectory(prefix='newt-finish-books-idle2-') as td:
        d = Path(td)
        seed_warm_home(binary, d)
        preview_driver.write_conf(d, {'telemetry': 'sample'})
        preview_driver.fresh(d)
        # Open from Books, wait 1 settle window (4s, <30s due) so idle verify finishes while COMMIT_OWED remains,
        # then open Menu and select Mark as finished.
        frames, _ = rust_preview.run(binary, d, (CONFIRM, CONFIRM, *QUEUED_MENU_MARK))
        got = (d / 'frame.pbm').read_bytes()
        if got != expected_undo or not any(n.startswith('sd') for n in stats_files(d)):
            return 'Books open after idle verification (<30s due) failed to commit selection and mark finished'


def menu_does_not_stall_background_verification(binary):
    """Opening the Reading menu on healthy battery continues idle verification without waiting for 30s due."""
    # Check 1: Opening Menu immediately (QUEUED) on a warm book with telemetry=sample still verifies during Menu
    # idle, so pressing Mark as finished does not need a foreground Saving working screen.
    with tempfile.TemporaryDirectory(prefix='newt-finish-menu-pre-') as td_pre:
        d_pre = Path(td_pre)
        seed_warm_home(binary, d_pre)
        preview_driver.write_conf(d_pre, {'telemetry': 'sample'})
        preview_driver.fresh(d_pre)
        # Let verification finish on Page first (1 settle window), then open Menu and mark.
        rust_preview.run(binary, d_pre, (BACK, CONFIRM, RIGHT, RIGHT, RIGHT, RIGHT, CONFIRM))
        expected_undo = (d_pre / 'frame.pbm').read_bytes()
        _, gen_pre = status_gen(d_pre)

    with tempfile.TemporaryDirectory(prefix='newt-finish-menu-imm-') as td_imm:
        d_imm = Path(td_imm)
        seed_warm_home(binary, d_imm)
        preview_driver.write_conf(d_imm, {'telemetry': 'sample'})
        preview_driver.fresh(d_imm)
        # Open Menu immediately (QUEUED before idle runs on Page), step 4 times (~16s < 30s due), press CONFIRM.
        rust_preview.run(binary, d_imm, (BACK, QUEUED, CONFIRM, RIGHT, RIGHT, RIGHT, RIGHT, CONFIRM))
        if (d_imm / 'frame.pbm').read_bytes() != expected_undo:
            return 'Mark as finished after opening Menu immediately did not produce finished state'
        _, gen_imm = status_gen(d_imm)
        # QUEUED skips one Page-only paint, so without a Saving working refresh gen_imm == gen_pre - 1.
        if gen_imm >= gen_pre:
            return (f'Menu stalled idle verification and triggered extra foreground work: '
                    f'immediate gen={gen_imm} vs pre-verified gen={gen_pre}')

    # Check 2: Mutating the EPUB (same size & mtime) is detected during Menu idle within 4s (<30s due).
    with tempfile.TemporaryDirectory(prefix='newt-finish-menu-chg-') as td_chg:
        d_chg = Path(td_chg)
        seed_warm_home(binary, d_chg)
        preview_driver.write_conf(d_chg, {'telemetry': 'sample'})
        preview_driver.fresh(d_chg)
        rust_preview.run(binary, d_chg, (BACK, QUEUED, CONFIRM))
        unmodified_menu = (d_chg / 'frame.pbm').read_bytes()

    with tempfile.TemporaryDirectory(prefix='newt-finish-menu-chg2-') as td_chg2:
        d_chg2 = Path(td_chg2)
        seed_warm_home(binary, d_chg2)
        book = sorted((d_chg2 / 'card/Newt/Books').glob('*.epub'))[0]
        mutate_epub_same_size(book)
        preview_driver.write_conf(d_chg2, {'telemetry': 'sample'})
        preview_driver.fresh(d_chg2)
        rust_preview.run(binary, d_chg2, (BACK, QUEUED, CONFIRM))
        changed_menu = (d_chg2 / 'frame.pbm').read_bytes()
        if changed_menu == unmodified_menu:
            return 'background verification while Reading menu was open did not detect mutated EPUB before 30s due'


def cancelled_and_error_notices_stay_honest(binary):
    """Timed-out verification on wake shows FinishOpening without PG SAVE ERR; write errors show SaveFailed; changed book reopens."""
    # 1. Deep-sleep wake + card.slow=.epub after the Saving refresh times out verification inside MarkFinished (still Pending).
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
        # Verify the book was NOT marked finished: clearing the fault and reopening Menu still shows Mark.
        preview_driver.write_conf(d, {'telemetry': '', 'card.slow': '', 'card.after_refresh': ''})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (CONFIRM, QUEUED, RIGHT, QUEUED, RIGHT, QUEUED, RIGHT, QUEUED, RIGHT), 'power-on')
        after_cancel_menu = (d / 'frame.pbm').read_bytes()

    # 2. Genuine stats write refusal shows FinishSaveFailed (distinct from FinishOpening and Undo).
    with tempfile.TemporaryDirectory(prefix='newt-finish-refuse-') as td:
        d = Path(td)
        seed_warm_sleep(binary, d)
        preview_driver.write_conf(d, {'telemetry': '', 'store.refuse': 'card-write:/newt-sd/Newt/Stats'})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (WAIT_MINUTE, *QUEUED_MENU_MARK), 'deep-sleep')
        failed_frame = (d / 'frame.pbm').read_bytes()

    # 3. Clean menu (no notice) and verified Undo frame on deep-sleep wake.
    with tempfile.TemporaryDirectory(prefix='newt-finish-clean-') as td:
        d = Path(td)
        seed_warm_sleep(binary, d)
        preview_driver.write_conf(d, {'telemetry': ''})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (CONFIRM, QUEUED, RIGHT, QUEUED, RIGHT, QUEUED, RIGHT, QUEUED, RIGHT), 'deep-sleep')
        clean_menu = (d / 'frame.pbm').read_bytes()
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (WAIT_MINUTE, *QUEUED_MENU_MARK), 'power-on')
        undo_frame = (d / 'frame.pbm').read_bytes()

    if after_cancel_menu != clean_menu:
        return 'cancelled verification unexpectedly marked the book finished'
    if opening_frame == undo_frame:
        return 'cancelled verification unexpectedly rendered the finished menu'
    if opening_frame == failed_frame:
        return 'cancelled verification on deep-sleep wake showed FinishSaveFailed instead of FinishOpening'
    if opening_frame == clean_menu:
        return 'cancelled verification on deep-sleep wake showed no notice'
    if footer_strip(opening_frame) != footer_strip(clean_menu):
        return 'cancelled MarkFinished verification falsely dirtied the reader footer (PG SAVE ERR)'

    # 4. Mutated EPUB detected during on-demand MarkFinished reopens changed book without marking it finished.
    with tempfile.TemporaryDirectory(prefix='newt-finish-changed-') as td:
        d = Path(td)
        seed_warm_home(binary, d)
        book = sorted((d / 'card/Newt/Books').glob('*.epub'))[0]
        mutate_epub_same_size(book)
        preview_driver.write_conf(d, {'telemetry': ''})
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (BACK, *QUEUED_MENU_MARK))
        reopened_page = (d / 'frame.pbm').read_bytes()
        if reopened_page == clean_menu or reopened_page == undo_frame:
            return 'MarkFinished on a changed EPUB left the stale Reading menu on screen'
        # Open the Menu on the reopened book and confirm it is NOT finished (still Mark, not Undo).
        preview_driver.fresh(d)
        rust_preview.run(binary, d, (CONFIRM, QUEUED, RIGHT, QUEUED, RIGHT, QUEUED, RIGHT, QUEUED, RIGHT))
        changed_menu_before = (d / 'frame.pbm').read_bytes()
        preview_driver.fresh(d)
        rust_preview.run(binary, d, QUEUED_MENU_MARK)
        changed_menu_after = (d / 'frame.pbm').read_bytes()
        if changed_menu_before == changed_menu_after:
            return 'MarkFinished on a changed EPUB unexpectedly left the new revision already marked finished'


CASES = (
    mark_finished_immediately_across_open_paths,
    books_open_after_idle_verify_commits_selection,
    menu_does_not_stall_background_verification,
    cancelled_and_error_notices_stay_honest,
)

CONTROLS = (
    (mark_finished_immediately_across_open_paths, 'reader_image_action.rs',
     'self.finish_pending_mark_image(home, ui, logical, events, action, &mut exit);',
     'let _ = (&before, &exit);'),
    (mark_finished_immediately_across_open_paths, 'reading_stats_runtime.rs',
     'if self.condition == StatsCondition::Unseeded && !page_unconfirmed {',
     'if false && self.condition == StatsCondition::Unseeded && !page_unconfirmed {'),
    (menu_does_not_stall_background_verification, 'reader_image_verification.rs',
     'let optional = ui.displayed.valid',
     'let optional = ui.displayed.valid && self.book().is_some_and(|b| b.ui.panel == newt_reader_ui::ReaderPanel::Page)'),
    (cancelled_and_error_notices_stay_honest, 'reader_image_action.rs',
     '                self.reader_ui.notice = if still_pending {\n                    ReaderUiNotice::FinishOpening',
     '                self.reader_ui.notice = if still_pending {\n                    ReaderUiNotice::FinishSaveFailed'),
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
        for case, source, old, new in CONTROLS:
            broken = rust_preview.control([(source, old, new)])
            problem = case(broken)
            if not problem:
                raise SystemExit(f'FAIL control {source} -> {case.__name__} did not notice the mutation')
            print(f'PASS control {source} -> {case.__name__}: {problem}')
    print('PASS preview_reader_finish_test')


if __name__ == '__main__':
    main()
