// SPDX-License-Identifier: MIT
//! The DEV `USB link` row (#170): link and session-end totals since boot.
//! Written only when a total changed, on a quiet pass with ring room, so it
//! never waits and is never skipped. Each row carries every total, so a later
//! row stands in for any the host missed.
use crate::{dev_usb::Bridge, serial_console::{Row, SerialConsole}};
use core::fmt::Write;
use newt_core::dev_link::LinkWatch;

/// The reader's link watch and session-end totals. It lives once in the
/// non-static Application, which bridge polls lend it from, so it adds no
/// static RAM. Counters are `u16` because this is UI-task stack.
#[derive(Default)]
pub struct UsbLinkRow {
    pub watch: LinkWatch,
    /// Live sessions ended by: link down, 60 s idle, transmit stall, tail flush.
    ended: [u16; 4],
    /// The totals sum last written.
    shown: u16,
}
const _: () = assert!(core::mem::size_of::<UsbLinkRow>() <= 28);

impl UsbLinkRow {
    pub fn publish(&mut self, bridge: &mut Bridge, log: &mut SerialConsole) -> bool {
        let taken = bridge.take_ended();
        for (bit, total) in self.ended.iter_mut().enumerate() {
            if taken & (1 << bit) != 0 { *total = total.wrapping_add(1); }
        }
        let link = self.watch.counts();
        let skipped = log.skipped();
        let [end_link, end_idle, end_tx, end_tail] = self.ended;
        // Every total only grows, so their sum changes whenever one does.
        let sum = [link.blips, link.lost, link.latched, skipped, end_link, end_idle, end_tx, end_tail]
            .into_iter().fold(0u16, u16::wrapping_add);
        if sum == self.shown || bridge.output_pending() || bridge.request_pending() { return false; }
        let mut bytes = [0; 192];
        let mut row = Row::new(&mut bytes);
        if writeln!(row, "USB link: blips={} lost={} latched={} skipped={} end_link={} end_idle={} end_tx={} end_tail={}",
            link.blips, link.lost, link.latched, skipped, end_link, end_idle, end_tx, end_tail).is_err() {
            return false;
        }
        let Ok(text) = core::str::from_utf8(&row.bytes[..row.length]) else { return false };
        if newt_c3_board::dev_usb::available_for_write() < row.length as i32 { return false; }
        self.shown = sum;
        log.write_str(text).is_ok()
    }
}
