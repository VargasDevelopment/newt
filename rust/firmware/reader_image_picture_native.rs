// SPDX-License-Identifier: MIT
//! Short picture-slot loan of the existing UI-task native port.
use core::ffi::CStr;
use newt_native_io::{cover, OsNative, UiIo};
use newt_native_storage::{Kind, Native, Open, Result, Stat};
use super::picture_slot::PictureNative;

pub(super) struct PictureIo<'ui> {
    io: &'ui UiIo,
    native: OsNative<'ui>,
}

impl<'ui> PictureIo<'ui> {
    pub(super) fn new(io: &'ui UiIo) -> Self {
        Self { io, native: io.port() }
    }
}

impl PictureNative for PictureIo<'_> {
    fn pending_metadata(&mut self) -> Result<(Kind, u64)> {
        let path = c"/newt-sd/Newt/Cache/resume.pic.tmp";
        // Shared frozen primitive: host lstat; pinned symlink-free FAT stat.
        let facts = cover::stat(self.io, path)?;
        let kind = if facts.regular() { Kind::Regular }
            else if facts.directory() { Kind::Directory }
            else { Kind::Other };
        Ok((kind, facts.links))
    }
}

impl Native for PictureIo<'_> {
    fn open(&mut self, path: &str, mode: Open) -> Result<i32> {
        self.native.open(path, mode)
    }
    fn close(&mut self, fd: i32) -> Result<()> {
        self.native.close(fd)
    }
    fn read(&mut self, fd: i32, out: &mut [u8]) -> Result<usize> {
        self.native.read(fd, out)
    }
    fn write(&mut self, fd: i32, input: &[u8]) -> Result<usize> {
        self.native.write(fd, input)
    }
    fn seek(&mut self, fd: i32, offset: u32) -> Result<u64> {
        self.native.seek(fd, offset)
    }
    fn truncate(&mut self, fd: i32, bytes: u32) -> Result<()> {
        self.native.truncate(fd, bytes)
    }
    fn sync(&mut self, fd: i32) -> Result<()> {
        self.native.sync(fd)
    }
    fn fstat(&mut self, fd: i32) -> Result<Stat> {
        self.native.fstat(fd)
    }
    fn stat(&mut self, path: &str, no_follow: bool) -> Result<Stat> {
        self.native.stat(path, no_follow)
    }
    fn mkdir(&mut self, path: &str) -> Result<()> {
        self.native.mkdir(path)
    }
    fn unlink(&mut self, path: &str) -> Result<()> {
        self.native.unlink(path)
    }
    fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        self.native.rename(from, to)
    }
    fn rename_cstr(&mut self, from: &CStr, to: &CStr) -> Result<()> {
        self.native.rename_cstr(from, to)
    }
    fn next_nonce(&mut self) -> u32 {
        self.native.next_nonce()
    }
}
