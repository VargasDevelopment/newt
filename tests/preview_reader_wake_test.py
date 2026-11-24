#!/usr/bin/env python3
"""A wake into a book, then a press or a fault while that open still runs.

The preview is the real firmware built for the host over C fakes
(rust/host/preview/fakes). Each case reads an SD book, sleeps, wakes, and lets
the card fake act at the open's first book read after the wake's first
refresh: press Back (open.back, Confirm first with open.confirm), fail the read
(open.eio), or stall past the open's budget (open.slow). A press that stops the
open is a pause for input, not a result. Each control is the same build with
one firmware line changed in its generated copy (never in rust/firmware).

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
    # The NRS2 slot's outer CRC-32C covers the header and payload.
    crc = 0xffffffff
    for byte in data[at:at + 252]:
        crc ^= byte
        for _ in range(8):
            crc = (crc >> 1) ^ (0x82f63b78 if crc & 1 else 0)
    struct.pack_into('<I', data, at + 252, crc ^ 0xffffffff)
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
        ('reader_image_boot.rs',
         'policy: boot_restore_policy(checkpoint, retained_recovery),',
         'policy: if checkpoint.is_some() { newt_reader_ui::RestorePolicy::ExactPage } '
         'else { boot_restore_policy(checkpoint, retained_recovery) },')]),
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
    for case, controls in CASES:
        for name, old, new in controls:
            control = rust_preview.build(edits=[(name, old, new)])
            assert case(control) is not None, f'{case.__name__}: passes with {name} changed'
    print(f'PASS: {len(CASES)} wake and cold-start flows run the firmware; each control fails')


if __name__ == '__main__':
    main()
