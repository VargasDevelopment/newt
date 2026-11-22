// SPDX-License-Identifier: MIT
//! Directory removal primitive; traversal and purge policy stay in Rust.
use super::{OsNative, Path, Result, status};
use core::ffi::c_char;

// SAFETY: the synchronous C entry consumes a terminated path and returns the
// existing typed errno tag. It retains no pointer and performs no retry.
unsafe extern "C" { fn newt_io_rmdir(path: *const c_char) -> i32; }

impl OsNative<'_> {
    pub fn rmdir(&mut self, path: &str) -> Result<()> {
        Path::with(path, |path| {
            // SAFETY: this UI-native view holds the mounted filesystem permission;
            // Path is live and NUL terminated throughout the one SDK operation.
            status(unsafe { newt_io_rmdir(path.as_ptr()) })
        })
    }
}
