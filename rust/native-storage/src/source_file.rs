//! Bounded file-backed `BookSource`. MIT. No file bytes or allocation are retained.
use crate::Authority;
use core::num::NonZeroU64;
use newt_core::zip_window::source::BookSource;

pub const MAX_BOOK_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_READ_BYTES: usize = 32_768;

/// The owner supplies an already-open read-only file and exclusively lends its
/// cursor. A failing native read/seek must invalidate that cursor as on X3.
pub trait File {
    fn opened(&mut self) -> bool;
    fn is_directory(&mut self) -> bool;
    fn size(&mut self) -> u64;
    fn position(&mut self) -> u64;
    fn seek(&mut self, at: u32) -> bool;
    fn read(&mut self, output: &mut [u8]) -> usize;
}

pub struct FileBookSource<'a, F: File> {
    file: Option<&'a mut F>,
    bytes: u64,
    cancel: Option<&'a dyn Authority>,
}

/// The non-Copy binding state of an actual bound source. It retains neither
/// the file nor a cancellation callback, and grants no current freshness.
pub struct SourceState {
    bytes: NonZeroU64,
}
impl SourceState {
    /// The selected-source owner must relend its SAME original open file.
    /// Like Native/File itself, this is a trusted physical-owner contract;
    /// no pointer comparison substitutes for keeping that one owner alive.
    /// This does no I/O and does not bind again or accept a new file size.
    pub fn lend<'a, F: File>(self, file: &'a mut F, cancel: Option<&'a dyn Authority>)
        -> FileBookSource<'a, F> {
        FileBookSource { file: Some(file), bytes: self.bytes.get(), cancel }
    }
}

impl<'a, F: File> FileBookSource<'a, F> {
    pub fn new(cancel: Option<&'a dyn Authority>) -> Self {
        Self {
            file: None,
            bytes: 0,
            cancel,
        }
    }
    /// The hook only observes a flag; teardown remains with the owning task.
    pub fn set_cancel(&mut self, cancel: Option<&'a dyn Authority>) {
        self.cancel = cancel;
    }
    pub fn bind(&mut self, file: &'a mut F) -> bool {
        self.detach();
        if cancelled(self.cancel) || !file.opened() || cancelled(self.cancel) || file.is_directory()
        {
            return false;
        }
        if cancelled(self.cancel) {
            return false;
        }
        let bytes = file.size();
        if cancelled(self.cancel) || bytes == 0 || bytes > MAX_BOOK_BYTES {
            return false;
        }
        self.file = Some(file);
        self.bytes = bytes;
        true
    }
    /// End the binding before the owner closes/reopens the underlying file.
    pub fn release(mut self) {
        self.detach();
    }
    pub fn detach(&mut self) {
        self.file = None;
        self.bytes = 0;
    }

    /// End a short operation loan, returning the ORIGINAL file and resulting
    /// binding state. Only an actual successful bind can mint SourceState.
    /// The owner closes the real descriptor on every selected-source exit.
    pub fn into_state(mut self) -> Result<(SourceState, &'a mut F), Self> {
        let Some(bytes) = NonZeroU64::new(self.bytes) else { return Err(self); };
        let Some(file) = self.file.take() else { return Err(self); };
        Ok((SourceState { bytes }, file))
    }
}
fn cancelled(cancel: Option<&dyn Authority>) -> bool {
    cancel.is_some_and(Authority::cancelled)
}
impl<F: File> BookSource for FileBookSource<'_, F> {
    fn size(&self) -> u64 {
        self.bytes
    }
    fn read_at(&mut self, at: u64, output: &mut [u8]) -> i32 {
        if cancelled(self.cancel) {
            return -1;
        }
        let Some(file) = self.file.as_mut() else {
            return -1;
        };
        if !file.opened() || cancelled(self.cancel) {
            return -1;
        }
        let live = file.size();
        // Empty/EOF probes still verify the live handle after the blocking stat.
        if cancelled(self.cancel) || live != self.bytes {
            return -1;
        }
        if output.is_empty() || at >= self.bytes {
            return 0;
        }
        let want = output
            .len()
            .min(MAX_READ_BYTES)
            .min((self.bytes - at) as usize);
        if cancelled(self.cancel) {
            return -1;
        }
        let position = file.position();
        if cancelled(self.cancel) || (position != at && !file.seek(at as u32)) {
            return -1;
        }
        if cancelled(self.cancel) {
            return -1;
        }
        let got = file.read(&mut output[..want]);
        if cancelled(self.cancel) || got == 0 || got > want {
            return -1;
        }
        got as i32
    }
}

#[cfg(test)]
#[path = "../../tests/source_file/state_cases.rs"]
mod state_cases;
