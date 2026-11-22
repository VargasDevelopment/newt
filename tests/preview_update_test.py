#!/usr/bin/env python3
"""The preview's update, Wi-Fi and firmware-switch flows run the firmware.

The preview is the real firmware built for the host over C fakes
(rust/host/preview/fakes). Each case drives it with presses from a fixture
world (rust/host/preview/update_fixtures.py) and checks what the firmware
did: its serial log, its frames and what it wrote. Every case has a
negative control: the same build with one firmware line changed in its
generated copy (never in rust/firmware). The control must fail the case.

These are host facts only. They say nothing about the X3's radio, TLS,
flash or panel.
"""
from pathlib import Path
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(ROOT / 'tools'), str(ROOT / 'rust/host/preview')]
import rust_preview  # noqa: E402
from gate_tier import negative_controls  # noqa: E402
import update_fixtures  # noqa: E402

BACK, CONFIRM, UP, DOWN = 0, 1, 4, 5
# A new card shows Quick Start: Start, keep Upright, skip the time zone; the
# Wi-Fi step's Set up Wi-Fi restarts into update mode on its networks page.
TO_WIFI_STEP = [CONFIRM, CONFIRM, DOWN, CONFIRM]
SET_UP_WIFI = [*TO_WIFI_STEP, CONFIRM]
# The open "Tower 7" is the fifth listed network; Confirm joins and saves it.
PICK_OPEN = [*[DOWN] * 5, CONFIRM]
# From Home on a card that ran Newt: Settings, then a row of its list.
def settings_row(row):
    return [DOWN, DOWN, CONFIRM, *[DOWN] * row]
SWITCH_ROW, UPDATE_ROW, WIFI_ROW = 5, 6, 7
# Settings' Diagnostics row, About, five Selects on its Version (the first
# newt), then Previous, Next, Previous, Next: the hidden Developer page. Select
# there switches Test builds.
UNLOCK = [*settings_row(3), CONFIRM, CONFIRM, *[CONFIRM] * 5, UP, DOWN, UP, DOWN]
SWITCH_TEST_BUILDS = [*UNLOCK, CONFIRM]
# Settings' Update row restarts into update mode; Select checks GitHub.
CHECK = [*settings_row(UPDATE_ROW), CONFIRM, CONFIRM]
READY, LATEST = 3, 4  # newt_core::update_online::OnlineCheck


def world(directory, binary, update=None, other_firmware='crossink', new_card=False):
    update_fixtures.write(directory, update=update, other_firmware=other_firmware,
                          run=lambda keys: rust_preview.run(binary, directory, keys))
    if new_card:
        for path in sorted((directory / 'card/Newt').rglob('*'), reverse=True):
            path.rmdir() if path.is_dir() else path.unlink()
        (directory / 'card/Newt').rmdir()


def play(binary, keys, **fixture):
    """(frames, log) of one fresh fixture world driven by `keys`."""
    with tempfile.TemporaryDirectory() as scratch:
        directory = Path(scratch)
        world(directory, binary, **fixture)
        return rust_preview.run(binary, directory, keys)


# Each case returns None when the firmware behaved, else what went wrong.
def check_after_save(binary):
    """#1: Quick Start's Wi-Fi setup saves a network, then checks GitHub."""
    _, log = play(binary, [*SET_UP_WIFI, *PICK_OPEN], new_card=True)
    if 'Newt Wi-Fi network save: record 0' not in log:
        return 'the network was not saved'
    if 'Newt update online: result' not in log:
        return 'no online check after the save'


def resumes_quick_start(binary):
    """#2: leaving update mode restarts into the unfinished Quick Start."""
    step = play(binary, TO_WIFI_STEP, new_card=True)[0][-1]
    frames, _ = play(binary, [*SET_UP_WIFI, BACK], new_card=True)
    if frames[-1] != step:
        return 'the restart did not resume Quick Start at its Wi-Fi step'


def save_outcomes(binary):
    """A refused key write is "not saved"; a lost record sync "unconfirmed"."""
    keys = [*settings_row(WIFI_ROW), CONFIRM, *PICK_OPEN]
    pages = {name: play(binary, keys, update=name)[0][-1]
             for name in ('wifi-saved', 'wifi-not-saved', 'wifi-unconfirmed')}
    if len(set(pages.values())) != 3:
        return 'saved, not saved and unconfirmed draw the same page'


def cancel_or_finish(binary):
    """Back out of setup, or save a network: Quick Start says which."""
    cancelled = play(binary, [*SET_UP_WIFI, BACK], new_card=True)[0][-1]
    finished = play(binary, [*SET_UP_WIFI, *PICK_OPEN, BACK], new_card=True)[0][-1]
    if cancelled == finished:
        return 'a finished setup resumes like a cancelled one'


def switch_notice(binary):
    """The switch row restarts into firmware that is present, else says so."""
    keys = [*settings_row(SWITCH_ROW), CONFIRM, CONFIRM]
    # The run ends at a restart into the other slot: that boot is not Newt.
    present, restarted = play(binary, keys, other_firmware='crossink')
    missing, stayed = play(binary, keys, other_firmware='missing')
    if 'Newt restarting into the other firmware' not in restarted:
        return 'the switch to CrossInk did not restart'
    if 'restarting into the other firmware' in stayed or len(missing) != 1 or present[0] == missing[0]:
        return 'a missing slot draws the switch page'


def check_result(log):
    results = [line for line in log.splitlines() if line.startswith('Newt update online: result ')]
    return int(results[-1].split()[4].rstrip(',')) if results else None


def test_builds(binary):
    """Test builds On asks for the newest release, which is a test build. Each
    run below is a power-on, so RAM and RTC are gone and only the card holds
    the choice. An explicit Off, and a corrupt record, read as Off."""
    def check(*switches, corrupt=False):
        with tempfile.TemporaryDirectory() as scratch:
            directory = Path(scratch)
            world(directory, binary, update='test-builds')
            for _ in range(switches[0] if switches else 0):
                rust_preview.run(binary, directory, SWITCH_TEST_BUILDS)
            if corrupt:
                record = directory / 'card/Newt/State/newt-builds.nrs'
                record.write_bytes(bytes(b ^ 0x5a for b in record.read_bytes()))
            return check_result(rust_preview.run(binary, directory, CHECK)[1])
    if check() != LATEST:
        return 'Test builds Off offered more than the latest release'
    if check(1) != READY:
        return 'Test builds On did not offer the test build after a cold boot'
    if check(2) != LATEST:
        return 'an explicit Off did not survive a cold boot'
    if check(1, corrupt=True) != LATEST:
        return 'a corrupt Test builds record did not read as Off'


def test_builds_unconfirmed(binary):
    """A switch whose save cannot be confirmed says so, and keeps the old choice."""
    off = play(binary, UNLOCK, update='test-builds')[0][-1]
    saved = play(binary, SWITCH_TEST_BUILDS, update='test-builds')[0][-1]
    unconfirmed = play(binary, SWITCH_TEST_BUILDS, update='test-builds-unconfirmed')[0][-1]
    if len({off, saved, unconfirmed}) != 3:
        return 'an unconfirmed switch draws the Off or the saved page'


# (case, generated firmware file, exact text, replacement)
CASES = [
    (check_after_save, 'update_mode.rs',
     'if changed.is_ok() { intent = update::check_after_save(view); }', ''),
    (resumes_quick_start, 'home_startup.rs', 'if !resume.show { return false; }', 'return false;'),
    (save_outcomes, 'update_runtime.rs', '    view.commit = report.record;\n', ''),
    (cancel_or_finish, 'home_startup.rs',
     'Load::Loaded(sealed) if sealed.network)', 'Load::Loaded(sealed) if sealed.network && false)'),
    (switch_notice, 'firmware_switch.rs',
     'board.with_image_partitions(false, |parts| locate(parts, name).is_some())', '{ let _ = (board, name); false }'),
    # Update mode no longer restores the card's choices at its boot.
    (test_builds, 'update_state.rs',
     '|store| { clock.restore_preferences(store); !store.reading_state_unavailable() })',
     '|store| { let _ = &clock; !store.reading_state_unavailable() })'),
    (test_builds_unconfirmed, 'device_lifecycle.rs',
     '{SettingsNotice::None}else{SettingsNotice::TestBuildsSaveFailed}', '{SettingsNotice::None}else{SettingsNotice::None}'),
]


def main():
    # The gate passes its one preview build; controls relink from it.
    binary = rust_preview.FIRMWARE if len(sys.argv) == 2 else rust_preview.build()
    for case, *_ in CASES:
        problem = case(binary)
        assert problem is None, f'{case.__name__}: {problem}'
    if not negative_controls(__file__):
        print(f'PASS: {len(CASES)} update, Wi-Fi, switch and test-build flows run the firmware (controls not in this tier)')
        return
    for case, name, old, new in CASES:
        control = rust_preview.build(edits=[(name, old, new)])
        assert case(control) is not None, f'{case.__name__}: passes with {name} changed'
    print(f'PASS: {len(CASES)} update, Wi-Fi, switch and test-build flows run the firmware; each control fails')


if __name__ == '__main__':
    main()
