use core::fmt::{self, Write};
use super::TraceSink;

// The frozen char[160] includes its NUL; fmt::Write needs no terminator, but
// retains that same exclusive 160-byte limit before touching the transport.
struct Row {
    bytes: [u8; 160],
    len: usize,
}

impl Write for Row {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(fmt::Error)?;
        if end >= self.bytes.len() { return Err(fmt::Error); }
        self.bytes[self.len..end].copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

pub(crate) fn emit(sink: &mut (impl TraceSink + ?Sized), maximum: usize, args: fmt::Arguments<'_>) -> bool {
    let mut row = Row { bytes: [0; 160], len: 0 };
    if row.write_fmt(args).is_err() || row.len == 0 || row.len > maximum { return false; }
    if sink.available_for_write() < row.len as i32 { return false; }
    sink.write(&row.bytes[..row.len]) == row.len
}
