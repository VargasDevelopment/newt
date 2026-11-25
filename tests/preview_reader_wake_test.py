#!/usr/bin/env python3
"""A wake into a book, then a press or a fault while that open still runs.

The preview is the real firmware built for the host over C fakes
(rust/host/preview/fakes). Each case reads an SD book, sleeps, wakes, and lets
the card fake act at the open's first book read after the wake's first
refresh: press Back (open.back, Confirm first with open.confirm), fail the read
(open.eio), or stall past the open's budget (open.slow). A press that stops the
open is a pause for input, not a result. The resume picture cases change the
saved picture's pixels, as a build that draws the page differently leaves it,
and check that the real page replaces it once (#105). Each control is the same
build with one firmware line changed in its generated copy (never in
rust/firmware).

These are host facts only. They say nothing about the X3's timing: on a
device the press must land while the open still runs.
"""
import binascii
import functools
from pathlib import Path
import re
import struct
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(ROOT / 'tools'), str(ROOT / 'rust/host/preview')]
import preview_driver  # noqa: E402
import rust_preview  # noqa: E402
from gate_tier import negative_controls  # noqa: E402

BACK, CONFIRM, RIGHT, POWER = 0, 1, 3, 6
# From Home: Library, open its first SD book, sleep.
SLEEP_IN_BOOK = (CONFIRM, CONFIRM, POWER)
WAKE_IN_BOOK = (*SLEEP_IN_BOOK, rust_preview.WAKE)
COVER = re.compile(r'HOME_COVER eligible=(\d)')
BACK_AT_OPEN = {'open.back': '/Books/'}


@functools.cache
def play(binary, keys, no_hints=False, **conf):
    """(final frame, serial log of the wake) of one fresh SD-library world.
    no_hints deletes the book's source hints before the wake, so its open
    hashes the whole book."""
    with tempfile.TemporaryDirectory(prefix='newt-wake-') as scratch:
        directory = Path(scratch)
        options = preview_driver.parse(['--library-fixture', 'healthy'])
        preview_driver.world(binary, directory, options)
        preview_driver.write_conf(directory, conf)
        boot = 'power-on'
        if no_hints:
            rust_preview.run(binary, directory, SLEEP_IN_BOOK)
            hints = list(directory.glob('card/Newt/Cache/Imported/*.source-hints'))
            assert hints, 'the first open recorded no source hints'
            for path in hints:
                path.unlink()
            keys, boot = keys[len(WAKE_IN_BOOK):], 'deep-sleep'
        frames, log = rust_preview.run(binary, directory, keys, boot)
        return frames[-1], log


def eligible(log):
    """Whether the last Home composition could show the book's cover."""
    rows = COVER.findall(log)
    return rows[-1] == '1' if rows else None


def page(binary):
    return play(binary, WAKE_IN_BOOK)[0]


# Each case returns None when the firmware behaved, else what went wrong.
def back_goes_home(binary):
    """Back during the wake's open: the first frame painted after it is Home,
    and Home shows the book's cover."""
    home, log = play(binary, WAKE_IN_BOOK, **BACK_AT_OPEN)
    if eligible(log) is not True:
        return 'Home hides the cover after Back stops the wake open'
    flashed, _ = play(binary, WAKE_IN_BOOK, **BACK_AT_OPEN, freeze='open.back')
    if flashed != home:
        return 'the stopped open paints a frame before Home'


def confirm_then_back(binary):
    """Confirm then Back during the open, which leaves the reader shown with no
    input pending: it reopens the book, and a later press still turns the page."""
    turned, _ = play(binary, (*WAKE_IN_BOOK, RIGHT), **BACK_AT_OPEN, **{'open.confirm': '1'})
    if turned != play(binary, (*WAKE_IN_BOOK, RIGHT))[0]:
        return 'the reader does not reopen the book after Confirm and Back'


def back_while_hashing(binary):
    """Back while the open hashes a book with no source hints: Home shows the
    book's cover."""
    _, log = play(binary, WAKE_IN_BOOK, no_hints=True, **BACK_AT_OPEN)
    if eligible(log) is not True:
        return 'Home hides the cover after Back stops the hashing'


def read_failure(binary):
    """A failed read after the wake paints the read-failure frame."""
    failed, _ = play(binary, WAKE_IN_BOOK, **{'open.eio': '/Books/'})
    if failed in (page(binary), play(binary, WAKE_IN_BOOK, **BACK_AT_OPEN)[0]):
        return 'a failed read after the wake paints no read-failure frame'


def timeout(binary):
    """An open past its budget is a failure, not a pause: it paints the
    read-failure frame too."""
    stalled, _ = play(binary, WAKE_IN_BOOK, **{'open.slow': '/Books/'})
    if stalled in (page(binary), play(binary, WAKE_IN_BOOK, **BACK_AT_OPEN)[0]):
        return 'a timed-out open paints no read-failure frame'


def read_after_cancel(binary):
    """Read after the cancel opens the book as a plain wake does. A successful
    open replaces the reader's backing anyway; the start-of-open reset guards
    only its early returns, so this case has no control."""
    reopened, _ = play(binary, (*WAKE_IN_BOOK, BACK), **BACK_AT_OPEN)
    if reopened != page(binary):
        return 'Read after the cancel does not show the book page'


def u32(data, offset):
    return struct.unpack_from('<I', data, offset)[0]


def durable_slot(path, magic):
    data = bytearray(path.read_bytes())
    assert len(data) == 4096 and data[:4] == b'NRS2'
    slots = [at for at in range(512, len(data), 512)
             if data[at:at + 4] == b'NRSK' and data[at + 20:at + 24] == magic]
    # This fixture starts with fresh slots and writes only a few generations.
    return data, max(slots, key=lambda at: u32(data, at + 28))


def write_slot(path, data, at):
    struct.pack_into('<I', data, at + 252, preview_driver.crc32c(data[at:at + 252]))
    path.write_bytes(data)


def save_stale_boundary(state):
    progress_path, = state.glob('nb*.nrs')
    intent_path = state / 'newt-resume.nrs'
    progress, page = durable_slot(progress_path, b'NWTR')
    intent, destination = durable_slot(intent_path, b'NWBR')
    assert progress[page + 32:page + 40] == intent[destination + 32:destination + 40]
    assert u32(progress, page + 68) == u32(intent, destination + 40)
    assert not u32(intent, destination + 44) & 8, 'fixture needs an exact checkpoint'
    # Model an older layout's boundary one character into the same page.
    character = u32(progress, page + 44)
    assert character > 0
    struct.pack_into('<I', progress, page + 44, character + 1)
    checksum = binascii.crc32(progress[page + 20:page + 68])
    struct.pack_into('<I', progress, page + 68, checksum)
    write_slot(progress_path, progress, page)
    struct.pack_into('<I', intent, destination + 40, checksum)
    fnv = 2166136261
    for byte in intent[destination + 20:destination + 48]:
        fnv = ((fnv ^ byte) * 16777619) & 0xffffffff
    struct.pack_into('<I', intent, destination + 48, fnv)
    write_slot(intent_path, intent, destination)
    return character


def cold_power_on_at_old_boundary(binary):
    with tempfile.TemporaryDirectory(prefix='newt-cold-wake-') as scratch:
        directory = Path(scratch)
        preview_driver.world(binary, directory, preview_driver.parse([]))
        rust_preview.run(binary, directory, (CONFIRM, CONFIRM, RIGHT, POWER))
        character = save_stale_boundary(directory / 'card/Newt/State')
        # An upgrade boots without retained words or a matching resume picture.
        (directory / 'retained.bin').unlink(missing_ok=True)
        (directory / 'card/Newt/Cache/resume.pic').unlink(missing_ok=True)
        _, log = rust_preview.run(binary, directory, [], boot_kind='power-on')
    diag = dict(re.findall(r'(checkpoint_saved|character|reader_open|actual_character)=(\d+)',
                           log.split(' DIAG ')[-1]))
    if diag != {'checkpoint_saved': '1', 'character': str(character + 1),
                'reader_open': '1', 'actual_character': str(character)}:
        return f'cold power-on did not restore the page containing the saved checkpoint ({diag})'


PICTURE = 'card/Newt/Cache/resume.pic'
SHOWN = 'Reader picture: result=shown'
MISS = 'Reader picture miss'
SAVES = re.compile(r'Reader picture save: result=(\d)')
WRITTEN = '0'


def redraw(path):
    """Invert a band of the picture's page text and reseal it: a valid picture
    of other pixels, under the same book, position and settings."""
    data = bytearray(path.read_bytes())
    at = 512 + (len(data) - 512) // 2
    data[at:at + 600] = bytes(byte ^ 0xff for byte in data[at:at + 600])
    struct.pack_into('<I', data, 356, binascii.crc32(data[512:]))
    struct.pack_into('<I', data, 508, binascii.crc32(data[:508]))
    path.write_bytes(data)


@functools.cache
def picture_play(binary, keys, boot='deep-sleep', stale=True):
    """(final frame, serial log, picture file) of `keys` from a `boot` after a
    sleep in the book; stale redraws the saved picture first. A power-on has
    no retained words, as after an update."""
    with tempfile.TemporaryDirectory(prefix='newt-picture-') as scratch:
        directory = Path(scratch)
        preview_driver.world(binary, directory, preview_driver.parse(['--library-fixture', 'healthy']))
        # A known battery lets the reader store its pages; a picture shows only over them.
        preview_driver.write_conf(directory, {'telemetry': 'sample'})
        rust_preview.run(binary, directory, SLEEP_IN_BOOK)
        picture = directory / PICTURE
        if stale:
            redraw(picture)
        if boot == 'power-on':
            (directory / 'retained.bin').unlink(missing_ok=True)
        frames, log = rust_preview.run(binary, directory, keys, boot)
        return frames[-1], log, picture.read_bytes()


def stale_picture_repaints_once(binary):
    """The stale picture shows at the wake, then the real page replaces it at
    the same position. The next sleep writes a fresh picture, so the wake after
    it needs no repaint."""
    page, log, fresh = picture_play(binary, (), stale=False)
    if SHOWN not in log or MISS in log:
        return 'an unchanged picture is not shown and covered at the wake'
    repainted, log, _ = picture_play(binary, ())
    if SHOWN not in log or MISS not in log or repainted != page:
        return 'the wake does not replace a stale picture with the real page'
    final, log, rewritten = picture_play(binary, (POWER, rust_preview.WAKE))
    wakes = log.split(SHOWN)
    if len(wakes) != 3:
        return 'a wake does not show the picture'
    if SAVES.findall(wakes[1]) != [WRITTEN] or rewritten != fresh:
        return 'the sleep after the repaint keeps the stale picture'
    if MISS in wakes[2] or final != page:
        return 'the wake after the fresh picture repaints again'


def cold_picture_is_checked(binary):
    """A power-on has no mark from an earlier boot, so its picture is checked
    against the real page in the same boot that shows it."""
    page, log, _ = picture_play(binary, (), 'power-on', stale=False)
    if SHOWN not in log or MISS in log:
        return 'an unchanged picture is not shown and covered at power-on'
    repainted, log, _ = picture_play(binary, (), 'power-on')
    if SHOWN not in log or MISS not in log or repainted != page:
        return 'power-on does not replace a stale picture with the real page'


@functools.cache
def play_cw(binary, keys, **conf):
    """Same fresh SD-library world as play(), configured for clockwise landscape."""
    with tempfile.TemporaryDirectory(prefix='newt-wake-cw-') as scratch:
        directory = Path(scratch)
        options = preview_driver.parse(['--library-fixture', 'healthy', '--reader-orientation', 'landscape-cw'])
        preview_driver.world(binary, directory, options)
        preview_driver.write_conf(directory, conf)
        frames, log = rust_preview.run(binary, directory, keys, 'power-on')
        return frames[-1], log


def clockwise_wake_open_buttons(binary):
    """In clockwise landscape, once a wake publishes its clockwise working frame,
    front 1 (Back) is Page forward and must not cancel the open, while front 4
    (Right) cancels the open and goes cleanly Home."""
    page1, _ = play_cw(binary, WAKE_IN_BOOK)
    page2, _ = play_cw(binary, (*WAKE_IN_BOOK, BACK))
    if page2 == page1:
        return 'clockwise front 1 does not advance after wake'
    after_front1, log = play_cw(binary, WAKE_IN_BOOK, **BACK_AT_OPEN)
    if after_front1 != page2:
        return 'clockwise front 1 during wake open does not advance to page 2'
    tail = log.split(' DIAG ')[-1]
    if 'STATUS screen=4 ' not in tail or 'orientation=landscape-cw ' not in tail:
        return 'clockwise front 1 during wake open did not stay on clockwise reading screen'
    home, log = play_cw(binary, WAKE_IN_BOOK, **BACK_AT_OPEN, **{'open.button': str(RIGHT)})
    if eligible(log) is not True or 'STATUS screen=0 ' not in log.split(' DIAG ')[-1]:
        return 'Home hides the cover or did not open after clockwise front 4 cancels the wake open'
    flashed, _ = play_cw(binary, WAKE_IN_BOOK, **BACK_AT_OPEN,
                         **{'open.button': str(RIGHT), 'freeze': 'open.back'})
    if flashed != home:
        return 'clockwise front 4 cancel paints a frame before Home'


@functools.cache
def reopen_cw_from_home(binary, keys, **conf):
    """Open a clockwise book once to populate warm pages and resume.pic, leave
    to portrait Home via front 4 (Right), then boot onto Home and run `keys`."""
    with tempfile.TemporaryDirectory(prefix='newt-home-cw-') as scratch:
        directory = Path(scratch)
        options = preview_driver.parse(['--library-fixture', 'healthy', '--reader-orientation', 'landscape-cw'])
        preview_driver.world(binary, directory, options)
        preview_driver.write_conf(directory, {'telemetry': 'sample'})
        rust_preview.run(binary, directory, (CONFIRM, CONFIRM, RIGHT), 'power-on')
        assert (directory / PICTURE).is_file(), 'clockwise exit to Home did not save resume.pic'
        preview_driver.write_conf(directory, {'telemetry': 'sample', **conf})
        frames, log = rust_preview.run(binary, directory, keys, 'power-on')
        return frames[-1], log


def clockwise_reopen_from_home_picture_buttons(binary):
    """Reopening a clockwise book from portrait Home publishes its clockwise
    resume picture inside with_open before the reader finishes loading. Front 1
    pressed in that window must advance to page 2 (not cancel), while front 4
    must cancel the open and return Home."""
    home, _ = reopen_cw_from_home(binary, ())
    page1, log = reopen_cw_from_home(binary, (BACK,))
    if SHOWN not in log:
        return 'reopen from Home did not publish the clockwise resume picture'
    page2, _ = reopen_cw_from_home(binary, (BACK, BACK))
    if page2 == page1:
        return 'clockwise front 1 does not advance after reopen from Home'
    after_front1, log = reopen_cw_from_home(
        binary, (BACK,), **{'card.back': '/Pages/', 'card.after_refresh': '2', 'card.button': str(BACK)})
    if SHOWN not in log or after_front1 != page2:
        return 'clockwise front 1 after resume picture publish during Home reopen did not advance to page 2'
    cancelled_home, log = reopen_cw_from_home(
        binary, (BACK,), **{'card.back': '/Pages/', 'card.after_refresh': '2', 'card.button': str(RIGHT)})
    if SHOWN not in log or cancelled_home != home or eligible(log) is not True:
        return 'clockwise front 4 after resume picture publish did not cancel cleanly to Home'
    if not re.search(r'Reader route: [^\n]*ready=0 [^\n]*cancel=1 active=1', log):
        return 'clockwise front 4 did not cancel the in-flight open'


# The reader trusts any shown picture without comparing it with the real page.
UNCHECKED = ('reader_image_paint.rs',
             '        let covered = check == newt_core::resume_picture::Check::Covered;',
             '        let covered = check != newt_core::resume_picture::Check::None;')
PUBLISH_BACK_SYNC = ('app_runtime/display.rs',
                     '        control.set_clockwise_back(orientation == Orientation::LandscapeCw);',
                     '        let _ = control;')
RELEASE = ('reader_image_session.rs',
           '        if self.close_runtime(home).is_err() { crate::app_power::stop(); }\n'
           '        self.memory = ReaderBacking::Interrupted;',
           '        self.memory = ReaderBacking::Interrupted;')
# (case, its controls: (generated firmware file, exact text, replacement))
CASES = [
    (back_goes_home, [
        # The stopped open keeps its session, so Home keeps the cover hidden.
        RELEASE,
        # The interrupted reader paints its closed frame.
        ('reader_image_session.rs',
         '        matches!(self.memory, ReaderBacking::Interrupted) || self.session.as_ref()',
         '        self.session.as_ref()'),
        # The press is recorded as a failure.
        ('reader_image_session.rs',
         '        if !events.control.cancelled() || events.control.timed_out() { return false; }',
         '        if true { return false; }')]),
    (confirm_then_back, [
        # Idle never reopens an interrupted open.
        ('reader_image_work.rs', '            && self.reopen_owed() {',
         '            && self.session.as_ref().is_some_and(|session| session.verification.state()\n'
         '                == crate::reader_verification::Verification::Changed) {')]),
    (back_while_hashing, [RELEASE]),
    (read_failure, []),
    (timeout, [
        # A timeout counts as a press.
        ('reader_image_session.rs',
         '        if !events.control.cancelled() || events.control.timed_out() { return false; }',
         '        if !events.control.cancelled() { return false; }')]),
    (read_after_cancel, []),
    (cold_power_on_at_old_boundary, [
        # Boot asks a durable reopen for its exact page.
        ('reader_image_boot.rs',
         'policy: boot_restore_policy(durable.then(|| DurableCheckpoint::of(&target)),\n'
         '                    report.restored.then_some(&boot.record), self.settings),',
         'policy: if durable { newt_reader_ui::RestorePolicy::ExactPage } '
         'else { boot_restore_policy(None,\n'
         '                    report.restored.then_some(&boot.record), self.settings) },')]),
    (stale_picture_repaints_once, [
        UNCHECKED,
        # A save skips the picture whose binding and footer match.
        ('reader_image_picture.rs',
         '        let holds = !self.picture.stale() && home.with_storage_io(|_, io| {',
         '        let holds = home.with_storage_io(|_, io| {')]),
    (cold_picture_is_checked, [UNCHECKED]),
    (clockwise_wake_open_buttons, [PUBLISH_BACK_SYNC]),
    (clockwise_reopen_from_home_picture_buttons, [PUBLISH_BACK_SYNC]),
]


def main():
    # The gate passes its one preview build; controls relink from it.
    binary = rust_preview.FIRMWARE if len(sys.argv) == 2 else rust_preview.build()
    for case, _ in CASES:
        problem = case(binary)
        assert problem is None, f'{case.__name__}: {problem}'
    if not negative_controls(__file__):
        print(f'PASS: {len(CASES)} wake and cold-start flows run the firmware (controls not in this tier)')
        return
    # Cases that share a control share its build. The preview's clock is fake,
    # so these assertions don't depend on CPU speed and controls skip optimization.
    built = {}
    for case, controls in CASES:
        for edit in controls:
            if edit not in built:
                built[edit] = rust_preview.control([edit], optimize=False)
            assert case(built[edit]) is not None, f'{case.__name__}: passes with {edit[0]} changed'
    print(f'PASS: {len(CASES)} wake and cold-start flows run the firmware; each control fails')


if __name__ == '__main__':
    main()
