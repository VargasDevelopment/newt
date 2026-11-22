// SPDX-License-Identifier: MIT
//! Reading-statistics RTC clock adapter over Salamander's Board snapshot.
use newt_c3_board::{Board, ClockSnapshot, ReaderPanel};
use newt_core::stats_clock::{StatsClock, stats_session_clock};
use newt_core::time_zone::TimeZone;

/// The same bounded register read as Home, with the published zone. There is
/// no RTC write or second clock decoder. Match the pinned pre-read output clear;
/// an incomplete snapshot remains absent to the core policy.
#[inline(never)]
pub fn read_stats_session_clock(
    board: &mut Board<'_>,
    zone: TimeZone,
    utc_seconds: Option<&mut u32>,
) -> StatsClock {
    read_clock(|| board.clock_snapshot(), zone, utc_seconds)
}

/// Statistics inside the existing mounted reader scope, after parser/frame
/// loans end. No second Board or independent Wire authority is created.
pub fn read_panel_stats_session_clock(
    panel: &mut ReaderPanel<'_, '_>,
    zone: TimeZone,
    utc_seconds: Option<&mut u32>,
) -> StatsClock {
    read_clock(|| panel.clock_snapshot(), zone, utc_seconds)
}

pub fn read_hardware_stats_session_clock(
    hardware: &mut newt_c3_board::hardware::HardwareReader<'_>,
    zone: TimeZone, utc_seconds: Option<&mut u32>,
) -> StatsClock {
    read_clock(|| hardware.clock_snapshot(), zone, utc_seconds)
}

fn read_clock(
    snapshot: impl FnOnce() -> ClockSnapshot,
    zone: TimeZone,
    mut utc_seconds: Option<&mut u32>,
) -> StatsClock {
    if let Some(seconds) = utc_seconds.as_deref_mut() {
        *seconds = 0;
    }
    let snapshot = snapshot();
    stats_session_clock(
        snapshot.complete.then_some(&snapshot.raw),
        zone,
        utc_seconds,
    )
}
