//! One attempt's work file alongside the existing index publication port.
//! This module is a child of chapter_cache so both operations use its one Native loan.
use super::*;
use crate::{Error, Open};
use newt_core::book_index_build::WorkFile;

/// Caller-owned only for one build, relended to the same native card and book.
/// Census before building; resident publication may keep the source open.
/// End every attempt with WorkFile::remove_work, including admission failures.
pub struct IndexWorkState {
    path: [u8; 256], path_bytes: usize, fd: Option<i32>, bytes: u32,
}
impl IndexWorkState {
    pub fn new() -> Self { Self { path: [0; 256], path_bytes: 0, fd: None, bytes: 0 } }
}
pub struct IndexBuildStore<'op, 'a, N: Native> {
    cache: &'op mut ChapterCacheStorage<'a, N>,
    work: &'op mut IndexWorkState,
}
impl<'op, 'a, N: Native> IndexBuildStore<'op, 'a, N> {
    pub fn lend(cache: &'op mut ChapterCacheStorage<'a, N>, work: &'op mut IndexWorkState, digest: &[u8; 32]) -> Option<Self> {
        if work.path_bytes != 0 { return Some(Self { cache, work }); }
        use core::fmt::Write;
        let mut suffix = [0; 69];
        newt_core::book_index::name(digest, &mut suffix);
        let suffix = core::str::from_utf8(&suffix).ok()?;
        let mut name = cache.file.route.name("Index", suffix, false)?;
        let nonce = cache.file.native.next_nonce();
        write!(name, ".{nonce:08x}.tmp").ok()?;
        let mut path = [0; 256];
        path[..name.len()].copy_from_slice(name.as_bytes());
        let (parent, _) = name.rsplit_once('/')?;
        match cache.file.native.mkdir(parent) {
            Ok(()) | Err(Error::Exists) => {},
            Err(Error::Missing) => {
                // A fresh card may not have the cache root yet. Create its
                // ancestors here too, before any parse callback can spill.
                for end in (1..=parent.len()).filter(|&i| i == parent.len() || parent.as_bytes()[i] == b'/') {
                    if !cache.file.route.allowed()
                        || !matches!(cache.file.native.mkdir(&parent[..end]), Ok(()) | Err(Error::Exists)) {
                        return None;
                    }
                }
            }
            Err(_) => return None,
        }
        work.path = path;
        work.path_bytes = name.len();
        Some(Self { cache, work })
    }
    pub fn spilled(&self) -> bool { self.work.fd.is_some() }
    pub fn release_pages(&mut self) -> bool { self.cache.access().release_for_admission() }
    pub fn census(&mut self, cancelled: &dyn Fn() -> bool) where N: crate::page_entries::PageEntries {
        struct Live<'a>(&'a dyn Fn() -> bool);
        impl crate::Authority for Live<'_> { fn cancelled(&self) -> bool { (self.0)() } }
        self.cache.index_census(&Live(cancelled), None, None, None);
    }
    /// The live work descriptor, creating the file on first use.
    fn open(&mut self) -> Option<i32> {
        if self.work.fd.is_some() { return self.work.fd; }
        let path = core::ffi::CStr::from_bytes_with_nul(&self.work.path[..=self.work.path_bytes]).ok()?;
        self.work.fd = self.cache.file.native.open_cstr(path, Open::CreateExclusive).ok();
        self.work.fd
    }
}
impl<N: Native> WorkFile for IndexBuildStore<'_, '_, N> {
    fn append(&mut self, input: &[u8]) -> bool {
        let Some(bytes) = u32::try_from(input.len()).ok().and_then(|n| self.work.bytes.checked_add(n)) else { return false; };
        if u64::from(bytes) > newt_core::cache_names::namespace::INDEX_QUOTA { return false; }
        let Some(fd) = self.open() else { return false; };
        if self.cache.file.native.seek(fd, self.work.bytes) != Ok(u64::from(self.work.bytes)) { return false; }
        if self.cache.file.native.write(fd, input) != Ok(input.len()) { return false; }
        self.work.bytes = bytes;
        true
    }
    fn read_at(&mut self, offset: u32, output: &mut [u8]) -> bool {
        let Some(fd) = self.work.fd else { return false; };
        if offset > self.work.bytes || output.len() > (self.work.bytes - offset) as usize { return false; }
        self.cache.file.native.seek(fd, offset) == Ok(u64::from(offset))
            && self.cache.file.native.read(fd, output) == Ok(output.len())
    }
    fn sync_work(&mut self) -> bool {
        self.work.fd.is_none_or(|fd| self.cache.file.native.sync(fd).is_ok())
    }
    /// Also forgets the name, so a reused state picks a fresh one for its next build.
    fn remove_work(&mut self) -> bool {
        let path_bytes = core::mem::take(&mut self.work.path_bytes);
        self.work.bytes = 0;
        let Some(fd) = self.work.fd.take() else { return true; };
        let closed = self.cache.file.native.close(fd).is_ok();
        let removed = core::str::from_utf8(&self.work.path[..path_bytes])
            .is_ok_and(|name| matches!(self.cache.file.native.unlink(name), Ok(()) | Err(Error::Missing)));
        closed && removed
    }
}
impl<N: Native> CacheStorage for IndexBuildStore<'_, '_, N> {
    fn remove(&mut self, name: &[u8]) -> bool { self.cache.access().remove(name) }
    fn file_size(&mut self, name: &[u8]) -> i64 { self.cache.access().file_size(name) }
    fn read_at(&mut self, name: &[u8], offset: u32, output: &mut [u8]) -> i32 {
        self.cache.access().read_at(name, offset, output)
    }
    fn begin_write(&mut self, name: &[u8]) -> bool { self.cache.access().begin_write(name) }
    fn write(&mut self, bytes: &[u8]) -> bool { self.cache.access().write(bytes) }
    fn end_write(&mut self) -> bool { self.cache.access().end_write() }
    fn end_write_checked(&mut self, receipt_live: &mut dyn FnMut() -> bool) -> bool {
        self.cache.access().end_write_checked(receipt_live)
    }
    fn read_back_at(&mut self, offset: u32, output: &mut [u8]) -> i32 {
        self.cache.access().read_back_at(offset, output)
    }
}
