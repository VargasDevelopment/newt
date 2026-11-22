//! MIT. Home's clock reader over the X3 Board, wiring Wire, the RTC address
//! and Serial. It lives at the outer composition so neither the Board nor the
//! UI crate depends on the other.
use newt_c3_board::{Board, ClockSnapshot, ReaderPanel};
use newt_core::{clock_format::ClockFormat, time_zone::TimeZone};
use newt_device_ui::home_clock::{ClockReason, read_home_clock};

/// HomeClockSource's per-operation context. It owns the Board, because the
/// source's context type cannot name a shorter Board borrow. `log` is the DEV
/// line's sink, as the pinned `serial` argument: in firmware the begun
/// `SerialConsole` (serial_console.rs), which gets each line in the pinned two
/// writes. Production keeps it unused.
pub struct BoardBus<'sdk, L> {
    pub board: Board<'sdk>,
    pub log: L,
}

/// A `ClockReader<BoardBus<'sdk, L>>`: one bounded snapshot, then the existing
/// Home decode and DEV line. A failed snapshot shows --:-- and logs its partial
/// bytes. Write errors are ignored, as the pinned printf's result is.
pub fn read_board_clock<L: core::fmt::Write>(
    bus: &mut BoardBus<'_, L>,
    reason: ClockReason,
    zone: TimeZone,
    format: ClockFormat,
    text: &mut [u8; 5],
) {
    let snapshot = bus.board.clock_snapshot();
    finish_clock_read(&mut bus.log, snapshot, reason, zone, format, text);
}

/// A clock context for the existing mounted reader scope. The same panel
/// remains available to the caller for sequential paint/refresh operations.
/// In firmware `log` borrows the BoardBus's `SerialConsole`.
pub struct PanelBus<'panel, 'scope, 'sdk, L> {
    pub panel: &'panel mut ReaderPanel<'scope, 'sdk>,
    pub log: L,
}

pub fn read_panel_clock<L: core::fmt::Write>(
    bus: &mut PanelBus<'_, '_, '_, L>,
    reason: ClockReason,
    zone: TimeZone,
    format: ClockFormat,
    text: &mut [u8; 5],
) {
    let snapshot = bus.panel.clock_snapshot();
    finish_clock_read(&mut bus.log, snapshot, reason, zone, format, text);
}

pub(crate) fn finish_clock_read<L: core::fmt::Write>(
    log: &mut L,
    snapshot: ClockSnapshot,
    reason: ClockReason,
    zone: TimeZone,
    format: ClockFormat,
    text: &mut [u8; 5],
) {
    let valid = read_home_clock(snapshot.complete.then_some(&snapshot.raw), zone, text);
    #[cfg(feature = "dev-usb")]
    let _ = newt_device_ui::home_clock::write_clock_log(
        log,
        reason,
        &snapshot.raw,
        snapshot.complete,
        valid,
        zone,
        format,
        text,
    );
    #[cfg(not(feature = "dev-usb"))]
    let _ = (log, reason, format, valid);
}
