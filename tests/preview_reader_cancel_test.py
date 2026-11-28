#!/usr/bin/env python3
"""Real-firmware preview tests for the #103 cancel contract, with negative controls.

- Back that cancels a live turn exits Home and saves the shown page to SD.
- A stalled turn over a strict source still lets Back and Power exit and save.
- A turn into a short cached chapter shows no Loading screen.
- A Contents jump that runs out of time keeps the reader open on its page,
  and Back then saves it, over a sealed receipt or the strict fallback (#199).
- Back that cancels an initial open from Library goes Home and keeps the
  previous book's selection and position; a later tagged Back in the same
  queue leaves the screen the presses between them opened.

Each test reloads the position from a cold boot with RTC and RAM discarded.
"""
from pathlib import Path
import re
import shutil
import struct
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(ROOT / 'tools'), str(ROOT / 'rust/host/preview')]
import page_turn_fixtures  # noqa: E402
import preview_driver  # noqa: E402
import rust_preview  # noqa: E402
from gate_tier import negative_controls  # noqa: E402

BACK, CONFIRM, LEFT, RIGHT, UP, DOWN, POWER = range(7)
HOME, READER, LIBRARY = 0, 4, 6
STATUS = re.compile(r'STATUS screen=(\d+) ')
GENERATION = re.compile(r'STATUS screen=\d+ gen=(\d+)')
ACTION_TOTAL = re.compile(r'Reader action: schema=\d+ total=(\d+) ')
ROUTE_FLAGS = re.compile(r'Reader route: flags=(\d+) ')
STRICT_FALLBACK = 1 << 10  # ReaderOpenRoute::StrictFallback
# A live action's time budget (salamander/input OPEN_BUDGET_MS), in microseconds.
BUDGET_US = 20_000_000
DIAG_FIELD = re.compile(r'(reader_open|actual_spine|actual_character|current_book)=(\d+)')
# From Home: Read, then turn to the first chapter's last page, which only an
# exit saves.
TO_CHAPTER_END = (BACK, RIGHT, RIGHT, RIGHT)
# The next turn crosses into the second chapter, short and cached by then.
CROSS_CHAPTER_TURN = RIGHT
# From a page: the menu, Contents, then the second row, a chapter whose
# opening anchor needs the producer.
TO_CONTENTS_ROW = (CONFIRM, RIGHT, CONFIRM, RIGHT)
# A receipt that can't seal: the reader opens over the strict fallback, which
# reads the held book under its full digest (#199).
SEAL_FAILS = (
    'sd_revision.rs',
    'file.sync() && file.commit(MAX_RECEIPT_BYTES, true)',
    '{ let _ = &mut file; false }',
)
# Without a session, live actions use the strict source, as when the session
# cannot be allocated.
NO_SESSION = (
    'reader_image_open.rs',
    'if context.is_none() { *context = session::Session::try_new(&mut native, key, &cancel, millis); }',
    'let _ = (&mut native, key, &cancel);',
)


def screen_of(log):
    rows = STATUS.findall(log)
    return int(rows[-1]) if rows else None


def diag_of(log):
    return dict(DIAG_FIELD.findall(log.split(' DIAG ')[-1]))


def cold_boot(directory, conf=None):
    """Forget run files and RTC retained words; set only this run's faults."""
    preview_driver.fresh(directory)
    (directory / 'retained.bin').unlink(missing_ok=True)
    preview_driver.write_conf(directory, {'telemetry': 'sample', 'card.back': '', 'card.slow': '',
                                          'card.after_refresh': '', 'card.button': '', **(conf or {})})


def run(binary, directory, keys, conf=None, *, from_seed=True):
    """Run `keys` from a cold boot; by default over the seeded card, so each
    run starts from the same saved state."""
    if from_seed:
        shutil.rmtree(directory / 'card')
        shutil.copytree(directory / 'seed-card', directory / 'card')
    cold_boot(directory, conf)
    return rust_preview.run(binary, directory, keys, 'power-on')[1]


def reopened(binary, directory):
    """(book, spine, character) the SD record restores on a cold boot, which
    resumes an open book or else waits on Home for Read."""
    got = diag_of(run(binary, directory, (), from_seed=False))
    if got.get('reader_open') != '1':
        got = diag_of(run(binary, directory, (BACK,), from_seed=False))
    assert got.get('reader_open') == '1', f'Read did not reopen the book ({got})'
    return got['current_book'], got['actual_spine'], got['actual_character']


def selected_book(directory):
    """The committed book id in newt-selected.nrs (its newest valid slot)."""
    data = (directory / 'card/Newt/State/newt-selected.nrs').read_bytes()
    slots = [at for at in range(512, len(data), 512)
             if data[at:at + 4] == b'NRSK' and data[at + 20:at + 24] == b'NWLS']
    assert slots, 'newt-selected.nrs has no valid slot'
    at = max(slots, key=lambda pos: struct.unpack_from('<I', data, pos + 28)[0])
    return str(struct.unpack_from('<Q', data, at + 32)[0])


def seed(binary, directory, fixture, turns):
    """Open the first Library book, turn `turns` pages and go Home, which
    saves; later runs start from this card."""
    preview_driver.world(binary, directory, preview_driver.parse(['--library-fixture', fixture]))
    run(binary, directory, (CONFIRM, CONFIRM) + (RIGHT,) * turns + (BACK,), from_seed=False)
    shutil.copytree(directory / 'card', directory / 'seed-card')
    return reopened(binary, directory)


def first_refresh_after(binary, directory, keys):
    """The card fault's refresh count that first fires after `keys`: the
    smallest one that leaves those keys' outcome alone, so it fires at the
    next action's Loading or Opening refresh."""
    clean = run(binary, directory, keys)
    want = (screen_of(clean), diag_of(clean))
    for after in range(1, 64):
        log = run(binary, directory, keys, {'card.back': '/Books/', 'card.after_refresh': str(after)})
        if (screen_of(log), diag_of(log)) == want:
            return str(after)
    raise AssertionError(f'no refresh count leaves {keys} alone')


def back_cancels_live_turn_and_saves(binary):
    """Back during a live turn exits Home through the cancelled-exit save."""
    with tempfile.TemporaryDirectory(prefix='newt-cancel-turn-') as td:
        d = Path(td)
        book, *saved = seed(binary, d, 'healthy', 0)
        shown = diag_of(run(binary, d, TO_CHAPTER_END))
        want = [shown['actual_spine'], shown['actual_character']]
        assert want != saved, (want, saved)
        after = first_refresh_after(binary, d, TO_CHAPTER_END)
        log = run(binary, d, TO_CHAPTER_END + (CROSS_CHAPTER_TURN,),
                  {'card.back': '/Books/', 'card.after_refresh': after})
        if screen_of(log) != HOME:
            return f'Back during a live turn did not go Home (screen={screen_of(log)})'
        got = reopened(binary, d)
        if list(got[1:]) != want:
            return f'cold reload after a cancelled turn restored {got[1:]} instead of the shown page {want}'


def stalled_turn_exits_and_saves(binary):
    """A turn that times out on a stalled card read still lets Back and Power
    exit and save the shown page, with or without a reader session."""
    for name, exit_key in (('Back', BACK), ('Power', POWER)):
        with tempfile.TemporaryDirectory(prefix=f'newt-stall-{name.lower()}-') as td:
            d = Path(td)
            book, *saved = seed(binary, d, 'healthy', 0)
            shown = diag_of(run(binary, d, TO_CHAPTER_END))
            want = [shown['actual_spine'], shown['actual_character']]
            assert want != saved, (want, saved)
            after = first_refresh_after(binary, d, TO_CHAPTER_END)
            log = run(binary, d, TO_CHAPTER_END + (CROSS_CHAPTER_TURN, exit_key),
                      {'card.slow': '/Books/', 'card.after_refresh': after})
            exit_kind = (d / 'exit.txt').read_text().strip() if (d / 'exit.txt').exists() else ''
            if exit_key == BACK and screen_of(log) != HOME:
                return f'Back after a stalled turn did not go Home (screen={screen_of(log)})'
            if exit_key == POWER and exit_kind != 'sleep':
                return f'Power after a stalled turn did not sleep (exit={exit_kind!r}, screen={screen_of(log)})'
            got = reopened(binary, d)
            if list(got[1:]) != want:
                return f'{name} after a stalled turn: cold reload restored {got[1:]} instead of {want}'


def library_cancel_goes_home_and_keeps_book(binary):
    """Back during an initial open from Library goes Home with the previous book kept."""
    with tempfile.TemporaryDirectory(prefix='newt-cancel-library-') as td:
        d = Path(td)
        saved = seed(binary, d, 'four', 2)
        assert int(saved[2]) > 0, saved
        to_second_book = (CONFIRM, DOWN)
        after = first_refresh_after(binary, d, to_second_book)
        log = run(binary, d, to_second_book + (CONFIRM,), {'card.back': '/Books/', 'card.after_refresh': after})
        if screen_of(log) != HOME:
            return f'Back during an open from Library stayed on screen {screen_of(log)} instead of Home'
        if selected_book(d) != saved[0]:
            return f'Back during an open from Library replaced the selected book ({selected_book(d)} != {saved[0]})'
        got = reopened(binary, d)
        if got != saved:
            return f'Read after a Library cancel restored {got} instead of {saved}'


def library_cancel_queue_keeps_later_screen(binary):
    """Back, Next, Confirm, Back queued during an open from Library: the first
    Back goes Home, Next and Confirm open a Home card, and the second tagged
    Back leaves that screen open."""
    with tempfile.TemporaryDirectory(prefix='newt-cancel-queue-') as td:
        d = Path(td)
        seed(binary, d, 'four', 2)
        to_second_book = (CONFIRM, DOWN)
        opened = screen_of(run(binary, d, to_second_book + (BACK, RIGHT, CONFIRM)))
        assert opened not in (HOME, LIBRARY, None), opened
        after = first_refresh_after(binary, d, to_second_book)
        queue = ','.join(map(str, (BACK, RIGHT, CONFIRM, BACK)))
        log = run(binary, d, to_second_book + (CONFIRM,),
                  {'card.back': '/Books/', 'card.after_refresh': after, 'card.button': queue})
        if screen_of(log) != opened:
            return f'a queued Back after a Library cancel left screen {screen_of(log)} instead of {opened}'


def timed_out_jump_keeps_page(binary, *, strict=False):
    """A Contents jump that a stalled card runs out of time keeps the reader
    open on an unsaved page. A cold boot of that run's own card restores a
    page the reader showed, never the jump's target; Back after the timeout
    saves the shown page."""
    with tempfile.TemporaryDirectory(prefix='newt-jump-timeout-') as td:
        d = Path(td)
        preview_driver.world(binary, d, preview_driver.parse(['--library-fixture', 'empty']))
        books = d / 'card/Newt/Books'
        books.mkdir(parents=True, exist_ok=True)
        (books / 'book-l.epub').write_bytes(page_turn_fixtures.build_book('l'))
        run(binary, d, (CONFIRM, CONFIRM, RIGHT, RIGHT, BACK), from_seed=False)
        shutil.copytree(d / 'card', d / 'seed-card')
        book, *saved = reopened(binary, d)
        # Read, then one turn: a page that only an exit saves.
        to_page = (BACK, RIGHT)
        log = run(binary, d, to_page)
        fallback = any(int(flags) & STRICT_FALLBACK for flags in ROUTE_FLAGS.findall(log))
        assert fallback == strict, f'the open took the strict fallback: {fallback}'
        shown = diag_of(log)
        page = [shown['actual_spine'], shown['actual_character']]
        to_row = to_page + TO_CONTENTS_ROW
        clean = diag_of(run(binary, d, to_row + (CONFIRM,)))
        target = [clean['actual_spine'], clean['actual_character']]
        # Otherwise a jump that saved its target, or a lost turn, would pass.
        assert len({tuple(saved), tuple(page), tuple(target)}) == 3, (saved, page, target)
        # The first stall that leaves the way to the row alone and runs the jump out of time.
        for after in range(1, 64):
            stall = {'card.slow': '/Books/', 'card.after_refresh': str(after)}
            log = run(binary, d, to_row + (CONFIRM,), stall)
            if int(ACTION_TOTAL.findall(log)[-1]) >= BUDGET_US:
                break
        else:
            raise AssertionError('no stall runs the Contents jump out of time')
        reached = run(binary, d, to_row, stall)
        assert diag_of(reached) == diag_of(run(binary, d, to_row)), f'the stall at {after} fired before the jump'
        # The timed-out run again, so the cold boot below reads this run's card.
        log = run(binary, d, to_row + (CONFIRM,), stall)
        shown = diag_of(log)
        if screen_of(log) != READER or shown.get('reader_open') != '1':
            return f'a timed-out jump left the reader (screen={screen_of(log)}, {shown})'
        if [shown['actual_spine'], shown['actual_character']] != page:
            return f'a timed-out jump showed {shown} instead of the page {page}'
        got = reopened(binary, d)
        if got[0] != book or list(got[1:]) not in (saved, page):
            return f'a cold boot after a timed-out jump restored {got}, not a page the reader showed ({saved} or {page})'
        log = run(binary, d, to_row + (CONFIRM, BACK), stall)
        if screen_of(log) != HOME:
            return f'Back after a timed-out jump did not go Home (screen={screen_of(log)})'
        got = reopened(binary, d)
        if list(got) != [book, *page]:
            return f'Back after a timed-out jump: a cold boot restored {got} instead of the shown page {[book, *page]}'


def strict_timed_out_jump_keeps_page(binary):
    """The same over the strict fallback, whose reload hashes the book again."""
    return timed_out_jump_keeps_page(binary, strict=True)


def short_cached_chapter_turns_without_loading(binary):
    """A turn into a short cached chapter draws its page as the only new frame."""
    with tempfile.TemporaryDirectory(prefix='newt-no-loading-') as td:
        d = Path(td)
        seed(binary, d, 'healthy', 0)
        shown = run(binary, d, TO_CHAPTER_END)
        turned = run(binary, d, TO_CHAPTER_END + (CROSS_CHAPTER_TURN,))
        spines = int(diag_of(shown)['actual_spine']), int(diag_of(turned)['actual_spine'])
        assert spines[1] == spines[0] + 1, spines
        frames = int(GENERATION.findall(turned)[-1]) - int(GENERATION.findall(shown)[-1])
        if frames != 1:
            return f'a turn into a short cached chapter drew {frames} frames, so Loading showed first'


EARLY_CLEARED_CANCEL = (
    'reader_image_action.rs',
    '            Some(ReaderUiIntent::Home) => {\n'
    '                self.reflow_anchor = None;',
    '            Some(ReaderUiIntent::Home) => {\n'
    '                self.reflow_anchor = None;\n'
    '                self.closed_exit = ClosedExit::None;',
)
TIMED_OUT_EXIT_TAKES_LOAN = (
    'reader_image_action.rs',
    '|| (self.closed_exit == ClosedExit::TimedOut && matches!(action, Action::Back | Action::Sleep)) {',
    '|| false {',
)
# An open owner's timeout clears the exit state, as before the fix.
TIMED_OUT_CLEARED_WHILE_OPEN = (
    'reader_image_action.rs',
    'if had_progress && events.control.timed_out() && self.has_progress() {',
    'if !self.opened() && had_progress && events.control.timed_out() && self.has_progress() {',
)
# The exit after a stopped action skips its checkpoint.
STOPPED_EXIT_SKIPS_CHECKPOINT = (
    'reader_image_exit.rs',
    'core::mem::replace(&mut self.closed_exit, ClosedExit::None) != ClosedExit::None;',
    'core::mem::replace(&mut self.closed_exit, ClosedExit::None) != ClosedExit::None;\n'
    '        if allow_cancelled { return Ok(()); }',
)
TIMED_OUT_EXIT_REFUSED = (
    'reader_image_exit.rs',
    'core::mem::replace(&mut self.closed_exit, ClosedExit::None) != ClosedExit::None;',
    'core::mem::replace(&mut self.closed_exit, ClosedExit::None) == ClosedExit::Cancelled;',
)
# Every screen but Home dispatches a cancel Back, as before the fix.
CANCEL_BACK_EVERYWHERE = (
    'app_runtime/run_loop.rs',
    '        _ => input::Screen::Other,',
    '        _ => input::Screen::Library,',
)
# Loading before every cross-chapter Next, as the first #103 fix did.
LOADING_EVERY_CROSS_CHAPTER_TURN = (
    'reader_image_action.rs',
    '                failed.spine > 0\n            };\n        }\n        false\n',
    '                failed.spine > 0\n            };\n        }\n'
    '        action == Action::Next && book.ui.panel == ReaderPanel::Page\n'
    '            && book.reader.info().page + 1 >= book.reader.info().page_count\n',
)
# #199: a receipt that never reports its cancel reads the timed-out jump as a changed book.
CANCEL_READS_AS_CHANGED = (
    'sd_revision.rs',
    '!self.file.operation().allowed()',
    'false',
)
# #199: the exit after a cancel leaves the closed source unread, so Back can't save.
LIBRARY_KEEPS_CANCEL_BACK = (
    'app_runtime/run_loop.rs',
    'dispatch_back.then_some(input::Action::Back)',
    '(dispatch_back && ui.app.screen == Screen::EpubReading).then_some(input::Action::Back)',
)


def check(case, binary):
    problem = case(binary)
    if problem:
        raise SystemExit(f'FAIL {case.__name__}: {problem}')
    print(f'PASS {case.__name__}')


def main():
    binary = rust_preview.FIRMWARE if len(sys.argv) == 2 else rust_preview.build()
    check(back_cancels_live_turn_and_saves, binary)
    check(library_cancel_goes_home_and_keeps_book, binary)
    check(library_cancel_queue_keeps_later_screen, binary)
    check(short_cached_chapter_turns_without_loading, binary)
    check(timed_out_jump_keeps_page, binary)
    check(strict_timed_out_jump_keeps_page, rust_preview.control([SEAL_FAILS], optimize=False))
    check(stalled_turn_exits_and_saves, binary)
    check(stalled_turn_exits_and_saves, rust_preview.control([NO_SESSION], optimize=False))
    if not negative_controls(__file__):
        return
    for case, edits, label in (
        (back_cancels_live_turn_and_saves, [EARLY_CLEARED_CANCEL], 'early-cleared-cancel'),
        (stalled_turn_exits_and_saves, [NO_SESSION, TIMED_OUT_EXIT_TAKES_LOAN], 'timed-out-exit-takes-loan'),
        (stalled_turn_exits_and_saves, [NO_SESSION, TIMED_OUT_EXIT_REFUSED], 'timed-out-exit-refused'),
        (stalled_turn_exits_and_saves, [STOPPED_EXIT_SKIPS_CHECKPOINT], 'session-exit-skips-checkpoint'),
        (stalled_turn_exits_and_saves, [NO_SESSION, STOPPED_EXIT_SKIPS_CHECKPOINT], 'strict-exit-skips-checkpoint'),
        (stalled_turn_exits_and_saves, [TIMED_OUT_CLEARED_WHILE_OPEN], 'timed-out-cleared-while-open'),
        (library_cancel_goes_home_and_keeps_book, [LIBRARY_KEEPS_CANCEL_BACK], 'library-keeps-cancel-back'),
        (library_cancel_queue_keeps_later_screen, [CANCEL_BACK_EVERYWHERE], 'cancel-back-everywhere'),
        (short_cached_chapter_turns_without_loading, [LOADING_EVERY_CROSS_CHAPTER_TURN],
         'loading-every-cross-chapter-turn'),
        (timed_out_jump_keeps_page, [CANCEL_READS_AS_CHANGED], 'cancel-reads-as-changed'),
    ):
        problem = case(rust_preview.control(edits, optimize=False))
        if not problem:
            raise SystemExit(f'FAIL control {label}: {case.__name__} did not notice')
        print(f'PASS control {label}: {problem}')


if __name__ == '__main__':
    main()
