// SPDX-License-Identifier: MIT
//! The frozen single pending runtime-observation byte and USB row.
use core::fmt::{self, Write};
use crate::{dev_usb::Bridge, serial_console::SerialConsole};
use newt_c3_board::hardware::HardwareReader;
use newt_core::app::Screen;

#[repr(u8)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Kind { #[default] None, Reader, StageRegional, StageDirect, StageMono, StageOverlay }
#[derive(Default)]
pub struct RuntimeLows { pending: Kind }
impl RuntimeLows {
    pub fn painted(&mut self, screen: Screen) {
        self.pending = if matches!(screen, Screen::EpubReading | Screen::Library) { Kind::Reader } else { Kind::None };
    }
    pub fn stage_painted(&mut self, gray: bool, full: bool, overlay: bool) {
        self.pending = if !gray { Kind::StageMono } else if !full { Kind::StageRegional }
            else if overlay { Kind::StageOverlay } else { Kind::StageDirect };
    }
    /// Called after housekeeping, with no parser or protocol publication live.
    /// Backpressure keeps the pending observation for the next quiet pass.
    pub fn publish(&mut self, bridge: &Bridge, log: &mut SerialConsole, hardware: &mut HardwareReader<'_>) -> bool {
        if self.pending == Kind::None || bridge.output_pending() || bridge.request_pending()
            || newt_c3_board::dev_usb::available_for_write() < 128 { return false; }
        let Ok(stack) = hardware.current_task_stack_low_bytes() else { return false; };
        let Ok(heap) = hardware.min_free_heap_bytes() else { return false; };
        let name = match self.pending {
            Kind::Reader => "reader", Kind::StageRegional => "stage-regional",
            Kind::StageDirect => "stage-full-direct", Kind::StageMono => "stage-monochrome",
            Kind::StageOverlay => "stage-full-overlay", Kind::None => "none",
        };
        let mut bytes = [0;128];
        let mut row = Row { bytes: &mut bytes, length: 0 };
        if writeln!(row, "Runtime: kind={} stack-size=20480 stack-min-free={} heap-min-free={}", name, stack, heap).is_err() {
            return false;
        }
        let text = match core::str::from_utf8(&row.bytes[..row.length]) { Ok(text) => text, Err(_) => return false };
        self.pending = Kind::None;
        log.write_str(text).is_ok()
    }
}
struct Row<'a> { bytes: &'a mut [u8], length: usize }
impl Write for Row<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.length.checked_add(text.len()).ok_or(fmt::Error)?;
        let out = self.bytes.get_mut(self.length..end).ok_or(fmt::Error)?;
        out.copy_from_slice(text.as_bytes()); self.length = end; Ok(())
    }
}
const _: () = assert!(core::mem::size_of::<RuntimeLows>() == 1);
