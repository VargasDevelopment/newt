#![forbid(unsafe_code)]
use super::engine::Engine;
use crate::{zip_container::Status, zip_window::source::BookSource};
/// Same byte/chunk behavior as layoutPlainText, including first-read-only BOM
/// recognition and CR preserving the newline counter across chunk boundaries.
pub fn layout(
    source: &mut dyn BookSource,
    engine: &mut Engine<'_>,
    buffer: &mut [u8; 4096],
) -> Status {
    if engine.params.width <= 0 || engine.params.height <= 0 {
        return Status::Unsupported;
    }
    engine.start(b"body", None, None, None, None, None);
    engine.start(b"p", None, None, None, None, None);
    let (mut offset, mut newlines, mut first) = (0, 0, true);
    let mut status = Status::Ok;
    loop {
        let n = source.read_at(offset, buffer);
        if n < 0 {
            status = Status::IoError;
            break;
        }
        if n == 0 {
            break;
        }
        let n = n as usize;
        offset += n as u64;
        let mut start = 0;
        if first {
            first = false;
            if n >= 3 && buffer[..3] == [0xef, 0xbb, 0xbf] {
                start = 3
            }
        }
        let mut run = start;
        for i in start..n {
            if buffer[i] == 10 {
                newlines += 1;
                if newlines >= 2 {
                    if i > run {
                        engine.text(&buffer[run..i])
                    }
                    engine.end(b"p");
                    engine.start(b"p", None, None, None, None, None);
                    run = i + 1;
                    newlines = 0
                }
            } else if buffer[i] != 13 {
                newlines = 0
            }
        }
        if n > run {
            engine.text(&buffer[run..n])
        }
        if engine.stopped {
            break;
        }
    }
    engine.end(b"p");
    engine.end(b"body");
    if status == Status::Ok && !engine.finish() {
        status = Status::OutOfMemory
    }
    status
}
