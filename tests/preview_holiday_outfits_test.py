#!/usr/bin/env python3
"""Host preview tests and negative controls for #128 (opt-in holiday outfits).

Every holiday outfit (Halloween, Thanksgiving, Christmas) is off by default,
persisted in the SD card record `newt_holiday` (`card/Newt/State/newt-holiday.nrs`,
never NVS), and shown only when enabled and the local clock is trusted on a
matching date. Full AGENTS.md Rule 5 power-lifecycle coverage:
- off by default (including with a pre-existing 0.5.0 `newt_sleep` record);
- each outfit enabled independently on its date(s) only, and never with an unset clock;
- cold boot with all RTC and RAM discarded keeps the saved choice;
- explicit Off persists and supersedes an older On across cold boots;
- durable SD Off overrides a stale retained RTC image from an earlier On boot;
- missing or corrupt `newt-holiday.nrs` falls back to all Off without repairing;
- failed write or unconfirmed sync keeps the live runtime choice and shows the notice;
- compiled negative controls for durable restore, opt-in gating, notice and publication.
"""
from pathlib import Path
import struct
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(ROOT / 'tools'), str(ROOT / 'rust/host/preview')]
import preview_driver  # noqa: E402
import rust_preview  # noqa: E402
from gate_tier import negative_controls  # noqa: E402

BACK, CONFIRM, LEFT, RIGHT, UP, DOWN, POWER, WAIT_MINUTE, QUEUED = range(9)

# From Home: Settings -> Sleep screen -> Holiday outfits (row 3).
TO_SLEEP_SCREEN = (RIGHT, RIGHT, CONFIRM, RIGHT, RIGHT, CONFIRM)
TO_HOLIDAYS = (*TO_SLEEP_SCREEN, RIGHT, RIGHT, RIGHT, CONFIRM)
TOGGLE_HALLOWEEN = (*TO_HOLIDAYS, CONFIRM)
TOGGLE_THANKSGIVING = (*TO_HOLIDAYS, RIGHT, CONFIRM)
TOGGLE_CHRISTMAS = (*TO_HOLIDAYS, RIGHT, RIGHT, CONFIRM)


def seed_world(binary, directory, *, date='2026-11-25', clock='12:00'):
    args = ['--date', date]
    if clock is not None:
        args += ['--clock', clock]
    options = preview_driver.parse(args)
    preview_driver.world(binary, directory, options)


def set_clock(directory, *, date, clock='12:00'):
    if clock is None:
        (directory / 'clock.bin').unlink(missing_ok=True)
        return
    year, month, day = preview_driver.date_value(date)
    hour, minute = preview_driver.clock_value(clock)
    (directory / 'clock.bin').write_bytes(preview_driver.clock_registers(year, month, day, hour, minute))


def run_step(binary, directory, keys, *, boot='power-on', conf=None, date='2026-11-25', clock='12:00'):
    if conf is not None:
        preview_driver.write_conf(directory, conf)
    preview_driver.fresh(directory)
    set_clock(directory, date=date, clock=clock)
    frames, log = rust_preview.run(binary, directory, keys, boot)
    return frames[-1], log


def durable_holiday_slot(directory):
    """Return newest valid NWTH slot {seq, value} from card/Newt/State/newt-holiday.nrs."""
    path = directory / 'card/Newt/State/newt-holiday.nrs'
    if not path.is_file():
        return None
    data = path.read_bytes()
    if len(data) != 4096 or data[:4] != b'NRS2':
        return None
    slots = [at for at in range(512, len(data), 512)
             if data[at:at + 4] == b'NRSK' and data[at + 20:at + 24] == b'NWTH']
    if not slots:
        return None
    at = max(slots, key=lambda pos: struct.unpack_from('<I', data, pos + 28)[0])
    seq, value = struct.unpack_from('<II', data, at + 28)
    return {'seq': seq, 'value': value}


def sleep_frame_on(binary, directory, *, date, clock='12:00', boot='power-on', discard_rtc=False):
    if discard_rtc:
        (directory / 'retained.bin').unlink(missing_ok=True)
        (directory / 'card/Newt/Cache/resume.pic').unlink(missing_ok=True)
    frame, _ = run_step(binary, directory, (POWER,), boot=boot, date=date, clock=clock)
    return frame


def off_by_default_and_upgrade_compat(binary):
    """All holiday outfits stay off by default, including on cards upgraded from 0.5.0."""
    with tempfile.TemporaryDirectory(prefix='newt-holiday-default-') as scratch:
        directory = Path(scratch)
        seed_world(binary, directory, date='2026-11-25', clock='12:00')
        # Simulate a 0.5.0 card that has newt_sleep saved (Book progress On), but no newt_holiday.
        run_step(binary, directory, (*TO_SLEEP_SCREEN, CONFIRM))
        if not (directory / 'card/Newt/State/newt-sleep.nrs').is_file():
            return 'setup did not persist newt-sleep.nrs'
        if (directory / 'card/Newt/State/newt-holiday.nrs').exists():
            return 'unexpected newt-holiday.nrs created before toggling any holiday'
        plain = sleep_frame_on(binary, directory, date='2026-11-25', discard_rtc=True)
        for label, date in (('Halloween', '2026-10-31'), ('Thanksgiving', '2026-11-26'),
                            ('Christmas Eve', '2026-12-24'), ('Christmas Day', '2026-12-25')):
            got = sleep_frame_on(binary, directory, date=date, discard_rtc=True)
            if got != plain:
                return f'{label} ({date}) dressed up while off by default'


def each_holiday_independent_and_sd_only(binary):
    """Enabling one outfit shows it only on its own date(s) with a set clock, stored on SD only."""
    cases = [
        ('Halloween', TOGGLE_HALLOWEEN, 1, ('2026-10-31',), ('2026-11-26', '2026-12-24', '2026-12-25')),
        ('Thanksgiving', TOGGLE_THANKSGIVING, 2, ('2026-11-26',), ('2026-10-31', '2026-11-25', '2026-12-25')),
        ('Christmas', TOGGLE_CHRISTMAS, 4, ('2026-12-24', '2026-12-25'), ('2026-10-31', '2026-11-26', '2026-12-26')),
    ]
    for name, toggle, expected_bits, active_dates, plain_dates in cases:
        with tempfile.TemporaryDirectory(prefix=f'newt-holiday-{name.lower()}-') as scratch:
            directory = Path(scratch)
            seed_world(binary, directory, date='2026-11-25', clock='12:00')
            plain = sleep_frame_on(binary, directory, date='2026-11-25', discard_rtc=True)
            plain_unset = sleep_frame_on(binary, directory, date='2026-11-25', clock=None, discard_rtc=True)

            run_step(binary, directory, toggle)
            slot = durable_holiday_slot(directory)
            if slot is None or slot['value'] != expected_bits:
                return f'{name}: expected SD newt-holiday.nrs value {expected_bits}, got {slot}'
            if (directory / 'nvs/newt_holiday').exists():
                return f'{name}: newt_holiday must not be written to NVS'

            dressed_frames = []
            for date in active_dates:
                frame = sleep_frame_on(binary, directory, date=date, discard_rtc=True)
                if frame == plain:
                    return f'{name} enabled did not dress up on {date}'
                dressed_frames.append(frame)
            if len(set(dressed_frames)) != 1:
                return f'{name}: matching holiday dates drew different outfits'
            for date in plain_dates:
                frame = sleep_frame_on(binary, directory, date=date, discard_rtc=True)
                if frame != plain:
                    return f'{name} enabled unexpectedly dressed up on {date}'
            # Unset/untrusted clock must still stay plain even when enabled.
            if sleep_frame_on(binary, directory, date=active_dates[0], clock=None, discard_rtc=True) != plain_unset:
                return f'{name} dressed up with an unset clock'


def cold_boot_explicit_off_and_stale_rtc(binary):
    """Cold boot keeps On, explicit Off persists and supersedes On, and durable SD Off
    overrides a stale retained RTC image from an earlier On boot."""
    with tempfile.TemporaryDirectory(prefix='newt-holiday-power-') as scratch:
        directory = Path(scratch)
        seed_world(binary, directory, date='2026-10-31', clock='12:00')
        plain = sleep_frame_on(binary, directory, date='2026-10-31', discard_rtc=True)

        # 1. Enable Halloween and sleep, capturing the retained RTC state from an On boot.
        on_frame, _ = run_step(binary, directory, (*TOGGLE_HALLOWEEN, POWER), date='2026-10-31')
        if on_frame == plain:
            return 'Halloween On did not dress up before power loss'
        stale_rtc = (directory / 'retained.bin').read_bytes() if (directory / 'retained.bin').is_file() else None
        slot_on = durable_holiday_slot(directory)
        if slot_on is None or slot_on['value'] != 1:
            return f'expected durable value 1 after enabling Halloween, got {slot_on}'

        # 2. Cold boot with RTC and global state discarded must restore On from SD.
        cold_on = sleep_frame_on(binary, directory, date='2026-10-31', discard_rtc=True)
        if cold_on != on_frame:
            return 'cold boot with RTC discarded lost saved Halloween On'

        # 3. Explicitly turn Halloween Off and confirm explicit 0 is persisted at a newer seq.
        run_step(binary, directory, TOGGLE_HALLOWEEN, date='2026-10-31')
        slot_off = durable_holiday_slot(directory)
        if slot_off is None or slot_off['value'] != 0 or slot_off['seq'] <= slot_on['seq']:
            return f'explicit Off did not persist 0 at newer sequence (on={slot_on}, off={slot_off})'
        cold_off = sleep_frame_on(binary, directory, date='2026-10-31', discard_rtc=True)
        if cold_off != plain:
            return 'explicit persisted Off did not override older On after cold boot'

        # 4. Durable SD Off must override a stale retained RTC copy from the earlier On boot
        #    on both cold power-on and warm wake boots.
        if stale_rtc is not None:
            for boot_kind in ('power-on', 'wake'):
                (directory / 'retained.bin').write_bytes(stale_rtc)
                frame, _ = run_step(binary, directory, (POWER,), boot=boot_kind, date='2026-10-31')
                if frame != plain:
                    return f'durable Off failed to override stale RTC On on {boot_kind} boot'


def corrupt_record_falls_back_off_unrepaired(binary):
    """A corrupt newt-holiday.nrs falls back to all Off without repairing the file."""
    with tempfile.TemporaryDirectory(prefix='newt-holiday-corrupt-') as scratch:
        directory = Path(scratch)
        seed_world(binary, directory, date='2026-10-31', clock='12:00')
        plain = sleep_frame_on(binary, directory, date='2026-10-31', discard_rtc=True)

        run_step(binary, directory, TOGGLE_HALLOWEEN)
        record = directory / 'card/Newt/State/newt-holiday.nrs'
        corrupted = bytes(b ^ 0x5a for b in record.read_bytes())
        record.write_bytes(corrupted)

        got = sleep_frame_on(binary, directory, date='2026-10-31', discard_rtc=True)
        if got != plain:
            return 'corrupt newt-holiday.nrs did not fall back to Off'
        if record.read_bytes() != corrupted:
            return 'corrupt newt-holiday.nrs was modified instead of left unrepaired'


def unconfirmed_save_keeps_runtime_choice(binary):
    """Failed write and unconfirmed sync keep the prior live choice and show the notice."""
    def run_fresh(keys, *, enable_first=False, conf=None):
        with tempfile.TemporaryDirectory(prefix='newt-holiday-unconf-') as scratch:
            directory = Path(scratch)
            seed_world(binary, directory, date='2026-10-31', clock='12:00')
            if enable_first:
                run_step(binary, directory, TOGGLE_HALLOWEEN, date='2026-10-31')
            frame, _ = run_step(binary, directory, keys, conf=conf, date='2026-10-31')
            return frame

    plain = run_fresh((POWER,))
    off_page = run_fresh(TO_HOLIDAYS)
    on_page = run_fresh(TOGGLE_HALLOWEEN)
    on_sleep = run_fresh((POWER,), enable_first=True)
    if on_sleep == plain:
        return 'saved On did not dress up'

    for fault_name, conf in (
        ('card-write', {'store.refuse': 'card-write:/newt-sd/Newt/State/newt-holiday.nrs'}),
        ('card-sync', {'store.drop': 'card-sync:/newt-sd/Newt/State/newt-holiday.nrs'}),
    ):
        # 1. Fail Off -> On: notice draws, backing out and reopening keeps Off, sleep stays plain.
        unconfirmed_page = run_fresh(TOGGLE_HALLOWEEN, conf=conf)
        if len({off_page, on_page, unconfirmed_page}) != 3:
            return f'{fault_name}: off, saved On, and unconfirmed Off->On did not draw distinct frames (did not draw the notice)'
        if run_fresh((*TOGGLE_HALLOWEEN, BACK, CONFIRM), conf=conf) != off_page:
            return f'{fault_name}: unconfirmed Off->On changed the live choice'
        if run_fresh((*TOGGLE_HALLOWEEN, POWER), conf=conf) != plain:
            return f'{fault_name}: unconfirmed Off->On dressed up on sleep'

        # 2. Fail On -> Off: notice draws, backing out and reopening keeps On, sleep keeps outfit.
        unconfirmed_off = run_fresh(TOGGLE_HALLOWEEN, enable_first=True, conf=conf)
        if unconfirmed_off in (on_page, off_page):
            return f'{fault_name}: unconfirmed On->Off did not draw the notice'
        if run_fresh((*TOGGLE_HALLOWEEN, BACK, CONFIRM), enable_first=True, conf=conf) != on_page:
            return f'{fault_name}: unconfirmed On->Off changed the live On choice'
        if run_fresh((*TOGGLE_HALLOWEEN, POWER), enable_first=True, conf=conf) != on_sleep:
            return f'{fault_name}: unconfirmed On->Off dropped the live outfit on sleep'


NO_RESTORE = (
    'home_startup.rs',
    '            self.clock.restore_preferences(&mut store);',
    '            let _ = &mut store;',
)
NO_CHOSEN_GATE = (
    'reader_image_paint.rs',
    '        ui.info.hat = newt_device_ui::HolidayHat::chosen(today, home.clock.holiday_outfits());',
    '        ui.info.hat = newt_device_ui::HolidayHat::on(today);',
)
NO_UNCONFIRMED_NOTICE = (
    'device_lifecycle.rs',
    '0=>SettingsNotice::HolidayHalloweenSaveFailed,1=>SettingsNotice::HolidayThanksgivingSaveFailed,_=>SettingsNotice::HolidayChristmasSaveFailed',
    '0=>SettingsNotice::None,1=>SettingsNotice::None,_=>SettingsNotice::None',
)
PUBLISH_UNCONFIRMED = (
    'home_startup.rs',
    '            Self::HolidayOutfits(choices) => clock.commit_holiday_outfits(store, choices),',
    '            Self::HolidayOutfits(choices) => { use newt_core::durable::{BlobRead, DurableWrite, StorageOpen}; '
    'struct Saves([u8; 128], usize); impl Backend for Saves { '
    'fn begin(&mut self, _: &str, _: bool) -> StorageOpen { StorageOpen::Opened } '
    'fn end(&mut self) {} fn readback_durable(&self) -> bool { true } '
    'fn read_blob(&mut self, _: &str, out: &mut [u8]) -> BlobRead { if self.1 == 0 { return BlobRead::Missing; } '
    'out[..self.1].copy_from_slice(&self.0[..self.1]); BlobRead::Present(self.1) } '
    'fn put_bytes(&mut self, _: DurableWrite, _: &str, bytes: &[u8]) -> usize { '
    'self.0[..bytes.len()].copy_from_slice(bytes); self.1 = bytes.len(); bytes.len() } } '
    'let c = clock.commit_holiday_outfits(store, choices); '
    'if c != Commit::Saved { let _ = clock.commit_holiday_outfits(&mut Saves([0; 128], 0), choices); } c }',
)

CASES = [
    (off_by_default_and_upgrade_compat, [(NO_CHOSEN_GATE, 'dressed up while off by default')]),
    (each_holiday_independent_and_sd_only, []),
    (cold_boot_explicit_off_and_stale_rtc, [(NO_RESTORE, 'cold boot with RTC discarded lost saved Halloween On')]),
    (corrupt_record_falls_back_off_unrepaired, []),
    (unconfirmed_save_keeps_runtime_choice, [
        (NO_UNCONFIRMED_NOTICE, 'did not draw the notice'),
        (PUBLISH_UNCONFIRMED, 'changed the live choice'),
    ]),
]


def main():
    binary = rust_preview.FIRMWARE if len(sys.argv) == 2 else rust_preview.build()
    for case, _ in CASES:
        problem = case(binary)
        assert problem is None, f'{case.__name__}: {problem}'
    if not negative_controls(__file__):
        print(f'PASS: {len(CASES)} holiday outfit cases run the firmware (controls not in this tier)')
        return
    built = {}
    for case, controls in CASES:
        for edit, expected in controls:
            if edit not in built:
                built[edit] = rust_preview.control([edit], optimize=False)
            problem = case(built[edit])
            assert problem is not None, f'{case.__name__}: passes with {edit[0]} changed'
            assert expected in problem, (
                f'{case.__name__}: unexpected failure with {edit[0]} changed: '
                f'expected {expected!r}, got {problem!r}'
            )
    print(f'PASS: {len(CASES)} holiday outfit cases run the firmware; each control fails')


if __name__ == '__main__':
    main()
