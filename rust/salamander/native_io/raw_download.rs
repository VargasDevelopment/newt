// SPDX-License-Identifier: MIT
//! Raw errno leaves for the frozen download's explicit first-failure policy.
use core::ffi::{c_char, CStr};
use crate::OsNative;
pub const MISSING_ERRNO: i32 = 2;
pub const EXISTS_ERRNO: i32 = 17;
// SAFETY: download.c validates these constants and forwards one synchronous
// POSIX call, preserving raw errno immediately and retaining no borrowed data.
unsafe extern "C" {
    fn newt_download_mkdir(path: *const c_char, mode: u32) -> i32;
    fn newt_download_unlink(path: *const c_char) -> i32;
    fn newt_download_open(path: *const c_char, mode: u32, fd: *mut i32) -> i32;
    fn newt_download_write(fd: i32, bytes: *const u8, count: usize, written: *mut usize) -> i32;
    fn newt_download_sync(fd: i32) -> i32;
    fn newt_download_close(fd: i32) -> i32;
    fn newt_download_millis() -> u32;
}
fn status(error: i32) -> core::result::Result<(), i32> {
    if error == 0 { Ok(()) } else { Err(error) }
}
impl OsNative<'_> {
    pub fn mkdir_mode_raw(&mut self, path: &CStr, mode: u32) -> core::result::Result<(), i32> {
        // SAFETY: terminated live path; mode is converted by the actual libc.
        status(unsafe { newt_download_mkdir(path.as_ptr(), mode) })
    }
    pub fn unlink_raw(&mut self, path: &CStr) -> core::result::Result<(), i32> {
        // SAFETY: terminated live path borrowed for one unlink, no retry.
        status(unsafe { newt_download_unlink(path.as_ptr()) })
    }
    pub fn open_truncate_raw(&mut self, path: &CStr, mode: u32) -> core::result::Result<i32, i32> {
        let mut fd = -1;
        // SAFETY: exact live path and fd output, open does not retain either.
        status(unsafe { newt_download_open(path.as_ptr(), mode, &mut fd) })?;
        Ok(fd)
    }
    pub fn write_raw(&mut self, fd: i32, bytes: &[u8]) -> core::result::Result<usize, i32> {
        let mut written = 0;
        // SAFETY: readable exact byte loan plus exclusive count, synchronous
        // POSIX write; zero remains a successful zero, not a fabricated errno.
        status(unsafe { newt_download_write(fd, bytes.as_ptr(), bytes.len(), &mut written) })?;
        Ok(written)
    }
    pub fn sync_raw(&mut self, fd: i32) -> core::result::Result<(), i32> {
        // SAFETY: libc validates the descriptor in this synchronous call.
        status(unsafe { newt_download_sync(fd) })
    }
    pub fn close_raw(&mut self, fd: i32) -> core::result::Result<(), i32> {
        // SAFETY: one close; the caller consumes ownership even on failure.
        status(unsafe { newt_download_close(fd) })
    }
    pub fn monotonic_millis(&self) -> u32 {
        // SAFETY: one scalar CLOCK_MONOTONIC observation, no storage retained.
        unsafe { newt_download_millis() }
    }
}
