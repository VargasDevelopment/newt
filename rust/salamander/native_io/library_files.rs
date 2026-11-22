// SPDX-License-Identifier: MIT
//! alpha15 LibraryFiles: exact single transfers and native cursor side effects.
use super::{UiIo, WireStat, Open, open_mode, newt_io_open, newt_io_close,
    newt_io_read, newt_io_write, newt_io_mkdir, newt_io_stat, newt_io_unlink};
use core::{ffi::CStr, marker::PhantomData};
use newt_library_current::list::Files;

// SAFETY: link matching native_io.c; UiIo serializes descriptor access and
// expected is one live exclusive i64. Neither synchronous call retains pointers.
unsafe extern "C" {
    fn newt_io_seek_end(fd: i32) -> i64;
    fn newt_io_seek_set_raw(fd: i32, offset: u32, expected: *mut i64) -> i64;
}

/// Stateless synchronous view. The scan owns all descriptors and closes them;
/// this adapter retains no path, buffer, directory or extra native owner.
pub struct LibraryFiles<'ui> { _owner: PhantomData<&'ui UiIo> }
impl<'ui> LibraryFiles<'ui> {
    pub fn new(_: &'ui UiIo) -> Self { Self { _owner: PhantomData } }
    fn seek(fd: i32, offset: u32) -> bool {
        let mut expected = 0;
        // SAFETY: scalar output is live. Native returns both raw lseek and
        // the actual off_t conversion, preserving the frozen signed compare
        // even outside normal list offsets; no success reclassification.
        let at = unsafe { newt_io_seek_set_raw(fd, offset, &mut expected) };
        at == expected
    }
}
impl Files for LibraryFiles<'_> {
    fn open(&mut self, path: &[u8], write: bool) -> i32 {
        let Ok(path) = CStr::from_bytes_until_nul(path) else { return -1; };
        let mode = if write { Open::CreateTruncateWriteOnly } else { Open::SourceReadOnly };
        let mut fd = -1;
        // SAFETY: caller lends its terminated bytes, without UTF-8 conversion
        // or a second path buffer. Native retains no pointer and uses0600.
        let result = unsafe { newt_io_open(path.as_ptr(), open_mode(mode), &mut fd) };
        if result == 0 { fd } else { -1 }
    }
    fn read(&mut self, fd: i32, offset: u32, out: &mut [u8]) -> bool {
        if !Self::seek(fd, offset) { return false; }
        let mut got = 0;
        // SAFETY: exact exclusive output extent, valid through one read. A
        // short transfer retains its actual prefix and cursor, without retry.
        unsafe { newt_io_read(fd, out.as_mut_ptr(), out.len(), &mut got) == 0 && got == out.len() }
    }
    fn write(&mut self, fd: i32, offset: u32, data: &[u8]) -> bool {
        if !Self::seek(fd, offset) { return false; }
        let mut wrote = 0;
        // SAFETY: exact immutable input extent lives through one write.
        unsafe { newt_io_write(fd, data.as_ptr(), data.len(), &mut wrote) == 0 && wrote == data.len() }
    }
    fn size(&mut self, fd: i32) -> i64 {
        // SAFETY: one lseek(SEEK_END), including its cursor side effect and
        // frozen cast to native long. It is deliberately not fstat.
        unsafe { newt_io_seek_end(fd) }
    }
    fn close(&mut self, fd: i32) -> bool {
        // SAFETY: caller explicitly closes its native descriptor once.
        unsafe { newt_io_close(fd) == 0 }
    }
    fn make_folder(&mut self, path: &[u8]) -> bool {
        let Ok(path) = CStr::from_bytes_until_nul(path) else { return false; };
        // SAFETY: live terminated path. Every failed mkdir, not only EEXIST,
        // proceeds to stat, exactly as the original LibraryFiles does.
        if unsafe { newt_io_mkdir(path.as_ptr()) == 0 } { return true; }
        let mut info = WireStat::default();
        // SAFETY: exclusive ABI-sized output; no path or output is retained.
        unsafe { newt_io_stat(path.as_ptr(), 0, &mut info) == 0 && info.kind == 2 }
    }
    fn remove(&mut self, path: &[u8]) -> bool {
        let Ok(path) = CStr::from_bytes_until_nul(path) else { return false; };
        // SAFETY: synchronous unlink of the caller's terminated path. Only
        // success and the native ENOENT mapping count as already removed.
        matches!(unsafe { newt_io_unlink(path.as_ptr()) }, 0 | 2)
    }
}
