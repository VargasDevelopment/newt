// SPDX-License-Identifier: MIT
//! Frozen main.cpp fail(): a start failure logs and shows its reason, then
//! halts. An abort would restart into the same failure: a silent loop that
//! drains the battery with no reason on screen (#324).
use core::fmt::Write;
use newt_c3_board::{Board, Error, Refresh};
use newt_core::{app::{AppState, Screen}, frame::{self, Frame}, preferences::Orientation};
use newt_device_ui::{DeviceInfo, FIRMWARE_BUILD, SystemBar, render_device_ui};

/// RTC retained words could not be read or cleared.
pub const RETAINED: &str = "retained state unavailable";

/// Log "Newt stopped: <reason>", paint the reason on Diagnostics when a begun
/// panel is lent, then halt. Without a panel only the log remains, as frozen
/// paint() returned when the display was not ready. Every start failure ends
/// before the input task starts, so there is no task to suspend.
pub fn stop(log: &mut impl Write, display: Option<(&mut Board<'_>, &mut Frame)>, reason: &str) -> ! {
    let _ = writeln!(log, "Newt stopped: {reason}");
    if let Some((board, logical)) = display { let _ = show(board, logical, reason); }
    halt()
}

fn show(board: &mut Board<'_>, logical: &mut Frame, reason: &str) -> Result<(), Error> {
    // Frozen fail(): the Device screen with its row open is Diagnostics, whose
    // Display row shows the reason in place of the panel description.
    let state = AppState { screen: Screen::Device, row_open: true, ..AppState::default() };
    let mut info = DeviceInfo::default();
    info.panel = Some(reason.as_bytes());
    render_device_ui(&state, &info, &SystemBar::default(), Some(logical), Default::default(),
        None, None, FIRMWARE_BUILD, Default::default(), Default::default(), Default::default());
    let native = board.frame()?.try_into().map_err(|_| Error::Length)?;
    frame::to_native_frame(logical, native, Orientation::Portrait);
    board.update(Refresh::Full, false)
}

/// Frozen `for (;;) delay(1000)`: the setup task blocks in the scheduler, so
/// the idle task still runs and feeds the task watchdog; nothing restarts.
pub fn halt() -> ! {
    loop { crate::app_power::delay_ms(1000) }
}
