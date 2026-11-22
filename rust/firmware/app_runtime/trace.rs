// SPDX-License-Identifier: MIT
//! Frozen reader_trace_row.h: one bounded, nonblocking Serial write.
use crate::serial_console::SerialConsole;
use core::fmt::{self, Write};

#[inline(never)]
pub fn row(log: &SerialConsole, maximum: usize, arguments: fmt::Arguments<'_>) -> bool {
    let mut line = Line { bytes: [0; 160], length: 0 };
    if fmt::write(&mut line, arguments).is_err() || line.length == 0 || line.length > maximum
        || newt_c3_board::dev_usb::available_for_write() < line.length as i32 { return false; }
    let Ok(text) = core::str::from_utf8(&line.bytes[..line.length]) else { return false; };
    log.write_bytes(text.as_bytes()) == line.length
}
struct Line { bytes: [u8; 160], length: usize }
impl Write for Line {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.length.checked_add(text.len()).ok_or(fmt::Error)?;
        if end >= self.bytes.len() { return Err(fmt::Error); }
        self.bytes[self.length..end].copy_from_slice(text.as_bytes());
        self.length = end;
        Ok(())
    }
}
