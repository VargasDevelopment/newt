// SPDX-License-Identifier: MIT
//! CrossInk's read-only card view; Core owns traversal, retries and records.
use crate::{Error, Native, OsNative, UiIo, WireStat, posix_directory::Listing};
use core::ffi::CStr;
use newt_core::crossink_import::{Entry, EntryKind, Files, IoError};

// SAFETY: exact synchronous POSIX wire calls. Paths and outputs are borrowed
// for one call; only the actual fd result escapes, owned by ReadFile below.
unsafe extern "C" {
    fn newt_io_readonly_open_errno(path: *const core::ffi::c_char, fd: *mut i32) -> i32;
    fn newt_io_readonly_stat_errno(path: *const core::ffi::c_char, out: *mut WireStat) -> i32;
}

/// Neither this short view nor its files expose a write, rename or delete.
pub struct CrossInkFiles<'ui> {
    io: &'ui UiIo,
    native: OsNative<'ui>,
}
/// Move-only fd; Core explicitly closes it on every completed/error path.
pub struct ReadFile(i32);

fn errno(error: i32) -> IoError {
    // native_io.c asserts these POSIX values against the linked SDK headers.
    match error {
        2 | 20 => IoError::Missing,
        4 => IoError::Interrupted,
        _ => IoError::Failed,
    }
}
fn failed(error: Error) -> IoError {
    match error {
        Error::Interrupted => IoError::Interrupted,
        _ => IoError::Failed,
    }
}
impl<'ui> CrossInkFiles<'ui> {
    pub fn new(io: &'ui UiIo) -> Self {
        Self {
            io,
            native: io.port(),
        }
    }
}
impl<'ui> Files for CrossInkFiles<'ui> {
    type File = ReadFile;
    type Directory = Listing<'ui>;
    fn open(&mut self, path: &CStr) -> Result<Self::File, IoError> {
        let mut fd = -1;
        // SAFETY: the acquired UI domain and terminated path remain live for
        // one O_RDONLY open. No filename bytes or Rust pointer are retained.
        let error = unsafe { newt_io_readonly_open_errno(path.as_ptr(), &mut fd) };
        if error != 0 {
            return Err(errno(error));
        }
        if fd < 0 {
            return Err(IoError::Failed);
        }
        Ok(ReadFile(fd))
    }
    fn size(&mut self, file: &mut Self::File) -> Result<u64, IoError> {
        self.native
            .fstat(file.0)
            .map(|info| info.size as u64)
            .map_err(failed)
    }
    fn read(&mut self, file: &mut Self::File, output: &mut [u8]) -> Result<usize, IoError> {
        self.native.read(file.0, output).map_err(failed)
    }
    fn close(&mut self, file: Self::File) -> Result<(), IoError> {
        self.native.close(file.0).map_err(failed)
    }
    fn open_directory(&mut self, path: &CStr) -> Result<Self::Directory, IoError> {
        Listing::open_errno(self.io, path).map_err(errno)
    }
    fn next<'dir>(
        &mut self,
        directory: &'dir mut Self::Directory,
    ) -> Result<Option<Entry<'dir>>, IoError> {
        directory
            .next_entry()
            .map(|entry| {
                entry.map(|(name, kind)| Entry {
                    name,
                    kind: match kind {
                        0 => EntryKind::Unknown,
                        1 => EntryKind::Directory,
                        _ => EntryKind::Other,
                    },
                })
            })
            .map_err(failed)
    }
    fn close_directory(&mut self, directory: Self::Directory) -> Result<(), IoError> {
        if directory.close() {
            Ok(())
        } else {
            Err(IoError::Failed)
        }
    }
    fn is_directory(&mut self, path: &CStr) -> Result<bool, IoError> {
        let mut info = WireStat::default();
        // SAFETY: exact existing stat wire, with a complete borrowed path and
        // exclusive output. The wrapper preserves the same call's raw errno.
        let error = unsafe { newt_io_readonly_stat_errno(path.as_ptr(), &mut info) };
        if error != 0 {
            return Err(errno(error));
        }
        Ok(info.kind == 2)
    }
}
