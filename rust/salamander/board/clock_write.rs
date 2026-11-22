// SPDX-License-Identifier: MIT
//! One bounded Wire transaction; Core owns network-time validation and order.
use super::Board;
// SAFETY: native uses this exact initialized readable byte extent only during
// one synchronous sensor-bus transaction and retains no pointer.
unsafe extern "C" { fn nb_clock_write(first: u8, bytes: *const u8, count: usize) -> u8; }
impl Board<'_> {
    pub fn write_clock_registers(&mut self, first: u8, bytes: &[u8]) -> bool {
        // SAFETY: Board holds the unique sensor bus and the bytes outlive this
        // single SDK call, including its timeout restoration on every result.
        unsafe { nb_clock_write(first, bytes.as_ptr(), bytes.len()) != 0 }
    }
}
