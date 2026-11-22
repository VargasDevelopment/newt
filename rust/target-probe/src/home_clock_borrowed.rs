// SPDX-License-Identifier: MIT
// Every clock, store and choice comes from the caller, so each linked entry
// holds the shipped generic bodies rather than folded constants. Built without
// dev-usb: the production graph carries no DEV clock log.
use crate::stats_service_probe::Borrowed;
use core::ffi::CStr;
use newt_core::clock_format::ClockFormat;
use newt_core::durable::Commit;
use newt_core::time_zone::TimeZone;
use newt_device_ui::home_clock::{
    ClockReader, ClockReason, HomeClockSource, HomeClockText, read_home_clock,
};
use newt_device_ui::{DeviceInfo, SleepScreenChoices, SystemBar};

// The board's register snapshot stands in for Board as the reader context.
type Registers = [u8; 16];

// Its nm size is the resident snapshot: 12 bytes on RV32.
#[unsafe(no_mangle)]
pub static NEWT_HOME_CLOCK_SOURCE_BYTES: [u8; size_of::<HomeClockSource<Registers>>()] =
    [0; size_of::<HomeClockSource<Registers>>()];

fn registers(
    raw: &mut Registers,
    _: ClockReason,
    zone: TimeZone,
    _: ClockFormat,
    text: &mut [u8; 5],
) {
    read_home_clock(Some(&*raw), zone, text);
}

#[inline(never)]
#[unsafe(no_mangle)]
pub fn newt_home_clock_begin_borrowed(
    clock: &mut HomeClockSource<Registers>,
    store: &mut Borrowed<'_>,
    raw: &mut Registers,
    read: bool,
) {
    let reader: Option<ClockReader<Registers>> = read.then_some(registers);
    clock.begin_home_clock(store, raw, reader);
}

#[inline(never)]
#[unsafe(no_mangle)]
pub fn newt_home_clock_paint_borrowed<'a>(
    clock: &'a mut HomeClockSource<Registers>,
    raw: &mut Registers,
    other: Option<Option<&'a CStr>>,
) -> Option<&'a CStr> {
    let shown = match other {
        None => clock.begin_paint(),
        Some(text) => HomeClockText::Other(text),
    };
    clock.paint(raw, shown);
    clock.shown(shown)
}

#[inline(never)]
#[unsafe(no_mangle)]
pub fn newt_home_clock_system_bar_borrowed(
    clock: &HomeClockSource<Registers>,
    raw: &mut Registers,
    info: &DeviceInfo<'_>,
    radio_on: bool,
) -> SystemBar {
    clock.read_system_bar(raw, info, radio_on)
}

#[inline(never)]
#[unsafe(no_mangle)]
pub fn newt_home_clock_commit_borrowed(
    clock: &mut HomeClockSource<Registers>,
    store: &mut Borrowed<'_>,
    zone: Option<TimeZone>,
    format: Option<ClockFormat>,
    choices: SleepScreenChoices,
) -> Commit {
    match (zone, format) {
        (Some(zone), _) => clock.commit_time_zone(store, zone),
        (None, Some(format)) => clock.commit_clock_format(store, format),
        (None, None) => clock.commit_sleep_screen(store, choices),
    }
}

#[inline(never)]
#[unsafe(no_mangle)]
pub fn newt_home_clock_read_borrowed(
    raw: Option<&Registers>,
    zone: TimeZone,
    text: &mut [u8; 5],
) -> bool {
    read_home_clock(raw, zone, text)
}
