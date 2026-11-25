//! Unbuffered read-only source descriptor from pinned sd_source_file.h. MIT.
use crate::source_file::File;
use crate::{Kind, Native, Open, Stat};

/// Persistent unbuffered source descriptor/cursor, without a native loan.
#[must_use = "explicitly close the source at the enclosing source boundary"]
pub struct SdSourceState {
    descriptor: i32,
    position: usize,
}
impl SdSourceState {
    pub fn new() -> Self { Self { descriptor: -1, position: usize::MAX } }
    pub fn lend<'op, S: Native>(&'op mut self, native: &'op mut S) -> SdSourceFile<'op, S> {
        SdSourceFile { native, state: self }
    }
    pub fn close<S: Native>(&mut self, native: &mut S) { self.lend(native).close(); }
}
/// Short cursor/backend loan; no Drop I/O. The backend must own this fd's
/// domain, as required by the existing CardPort contract.
pub struct SdSourceFile<'a, S: Native> {
    native: &'a mut S,
    state: &'a mut SdSourceState,
}
impl<S: Native> SdSourceFile<'_, S> {
    pub fn open(&mut self, path: &str) -> bool {
        self.close();
        let Ok(fd) = self.native.open(path, Open::SourceReadOnly) else {
            return false;
        };
        if fd < 0 {
            return false;
        }
        if !matches!(self.native.fstat(fd), Ok(info) if info.kind == Kind::Regular) {
            let _ = self.native.close(fd);
            return false;
        }
        self.state.descriptor = fd;
        self.state.position = 0;
        true
    }
    pub fn close(&mut self) { let _ = self.release_checked(); }
    /// A failed/uncertain close cannot grant another publication descriptor.
    pub fn release_checked(&mut self) -> bool {
        let closed = self.state.descriptor < 0 || self.native.close(self.state.descriptor).is_ok();
        self.state.descriptor = -1;
        self.state.position = usize::MAX;
        closed
    }
    /// X3 fstat lacks modification timestamps: freshness uses path stat.
    pub fn stat_path(&mut self, path: &str) -> Option<Stat> {
        self.native
            .stat(path, false)
            .ok()
            .filter(|info| info.kind == Kind::Regular && info.size >= 0)
    }
}
impl<S: Native> File for SdSourceFile<'_, S> {
    fn opened(&mut self) -> bool {
        self.state.descriptor >= 0
    }
    fn is_directory(&mut self) -> bool {
        false
    }
    fn size(&mut self) -> u64 {
        if self.state.descriptor < 0 {
            return 0;
        }
        match self.native.fstat(self.state.descriptor) {
            Ok(info) if info.size >= 0 => (info.size as usize) as u64,
            _ => 0,
        }
    }
    fn position(&mut self) -> u64 {
        self.state.position as u64
    }
    fn seek(&mut self, at: u32) -> bool {
        self.state.position = usize::MAX;
        if self.state.descriptor < 0 || self.native.seek(self.state.descriptor, at) != Ok(u64::from(at)) {
            return false;
        }
        self.state.position = at as usize;
        true
    }
    fn read(&mut self, output: &mut [u8]) -> usize {
        if self.state.descriptor < 0 || output.is_empty() || self.state.position == usize::MAX {
            return 0;
        }
        match self.native.read(self.state.descriptor, output) {
            Ok(got) if got != 0 && got <= output.len() => {
                self.state.position = self.state.position.wrapping_add(got);
                got
            }
            _ => {
                self.state.position = usize::MAX;
                0
            }
        }
    }
}
