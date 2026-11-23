#!/usr/bin/env python3
"""A card app in /Newt/Apps through the real firmware's startup and Apps entry.

The preview is the real firmware built for the host over C fakes
(rust/host/preview/fakes), in a world from tools/preview_driver.py
--app-package. The card fake's card.trace lists every open and listing under
/Newt/Apps, and NVS is one file per key. Each control is the same build with
one firmware line changed in its generated copy (never in rust/firmware).

These are host facts only: the X3's card, NVS and power cycles are bench
checks.
"""
from pathlib import Path
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(ROOT / 'tools'), str(ROOT / 'rust/host/preview')]
import preview_driver  # noqa: E402
import rust_preview  # noqa: E402
from gate_tier import negative_controls  # noqa: E402

BACK, CONFIRM, RIGHT, POWER = 0, 1, 3, 6
APPS = (RIGHT, CONFIRM)  # From Home after a cold boot.


def play(binary, package, *boots):
    """(last frame of each boot, the card trace, whether the record exists).
    Each boot is a power-on of the same world, with all RAM gone."""
    with tempfile.TemporaryDirectory(prefix='newt-card-app-') as scratch:
        directory = Path(scratch)
        preview_driver.world(binary, directory, preview_driver.parse(['--app-package', package]))
        preview_driver.write_conf(directory, {'card.trace': '/Newt/Apps'})
        frames = []
        for keys in boots:
            preview_driver.fresh(directory)
            frames.append(rust_preview.run(binary, directory, keys)[0][-1])
        trace = directory / 'card-trace.txt'
        record = (directory / 'nvs/newt_apps').exists()
        return frames, trace.read_text() if trace.exists() else '', record


# Each case returns None when the firmware behaved, else what went wrong.
def startup_reads_no_app_card(binary):
    """Neither a power-on nor a wake reads /Newt/Apps; entering Apps does."""
    _, trace, _ = play(binary, 'offer', (), (POWER, rust_preview.WAKE))
    if trace:
        return f'startup read the app card: {trace.strip()}'
    _, trace, _ = play(binary, 'offer', APPS)
    if 'list /newt-sd/Newt/Apps' not in trace or 'open /newt-sd/Newt/Apps/quiet-hour.newt' not in trace:
        return f'entering Apps did not read the app card: {trace.strip()!r}'


def nothing_is_written_before_install(binary):
    """The offer and Later write nothing; Install writes the record."""
    (offer, declined), _, record = play(binary, 'offer', APPS, (*APPS, BACK))
    if record:
        return 'the record was written before Install'
    (installed,), _, record = play(binary, 'offer', (*APPS, CONFIRM))
    if not record or installed in (offer, declined):
        return 'Install did not write and show the installed app'


def a_cold_boot_runs_the_installed_app(binary):
    """After Install, a power-on boot's Apps is the installed app, and the
    unchanged card file is no new offer."""
    (opened, restarted), _, _ = play(binary, 'offer', (*APPS, CONFIRM, CONFIRM), APPS)
    (pause,), _, _ = play(binary, 'offer', (*APPS, BACK))
    if restarted != opened or restarted == pause:
        return 'a cold boot does not run the installed app'


# (case, its controls: (generated firmware file, exact text, replacement))
CASES = [
    (startup_reads_no_app_card, [
        # Plan control 5: a /Newt/Apps scan at startup.
        ('reader_image_app.rs', '        self.boot_restore(home, ui, boot, trace)\n',
         '        ui.enter_apps(home);\n        self.boot_restore(home, ui, boot, trace)\n')]),
    (nothing_is_written_before_install, [
        # Plan control 6: entering Apps installs without Install.
        ('reader_image_work.rs', '            Work::EnterApps => ui.enter_apps(home),\n',
         '            Work::EnterApps => ui.enter_apps(home) && ui.install_app(home),\n')]),
    # The record as the only authority is rust/tests/app_store/offer.rs's
    # cold-boot test and its control: InstalledApp::restore is the one way
    # Application::new holds an app, and it is not a generated top-level file.
    (a_cold_boot_runs_the_installed_app, []),
]


def main():
    # The gate passes its one preview build; controls relink from it.
    binary = rust_preview.FIRMWARE if len(sys.argv) == 2 else rust_preview.build()
    for case, _ in CASES:
        problem = case(binary)
        assert problem is None, f'{case.__name__}: {problem}'
    if not negative_controls(__file__):
        print(f'PASS: {len(CASES)} card app flows run the firmware (controls not in this tier)')
        return
    for case, controls in CASES:
        for name, old, new in controls:
            control = rust_preview.build(edits=[(name, old, new)])
            assert case(control) is not None, f'{case.__name__}: passes with {name} changed'
    print(f'PASS: {len(CASES)} card app flows run the firmware; each control fails')


if __name__ == '__main__':
    main()
