// SPDX-License-Identifier: MIT
//! Shared crashable index card. Descriptor and path timestamps model FAT.
use super::{model, ROOT};
use model::Model;
use newt_native_storage::*;
use std::collections::BTreeMap;

/// The FAT model plus what it lacks: file times for eviction order, a count
/// of open files for the card's small descriptor budget, and a power cut that
/// stops the card, so no cleanup runs between the cut and the next boot.
#[derive(Default)]
pub(super) struct Card {
    pub(super) model: Model,
    pub(super) times: BTreeMap<String, i64>,
    pub(super) open: u32,
    pub(super) most_open: u32,
    pub(super) creates: u32,
    pub(super) cstr_opens: u32,
    pub(super) writes: u32,
    pub(super) armed: bool,
    pub(super) off: bool,
}
impl Card {
    fn live<T>(&mut self, op: impl FnOnce(&mut Model) -> Result<T>) -> Result<T> {
        if self.off {
            return Err(Error::Unavailable);
        }
        let result = op(&mut self.model);
        self.off = self.armed && self.model.cut_after.is_none();
        result
    }
    /// Cut power `steps` card operations from now.
    pub(super) fn cut_in(&mut self, steps: u32) {
        self.model.cut_after = Some(self.model.steps + steps);
        self.armed = true;
    }
    /// Boot again on what was durable at the cut.
    pub(super) fn boot(&mut self) {
        (self.armed, self.off, self.open) = (false, false, 0);
        self.model.power();
    }
}
impl Native for Card {
    fn open_cstr(&mut self, path: &core::ffi::CStr, mode: Open) -> Result<i32> {
        self.cstr_opens += 1;
        self.open(path.to_str().map_err(|_| Error::Other)?, mode)
    }
    fn open(&mut self, path: &str, mode: Open) -> Result<i32> {
        if matches!(mode, Open::CreateExclusive) {
            self.creates += 1;
            if let Some(name)=path.strip_prefix(&format!("{ROOT}/Index/")) {
                assert!(newt_core::book_index::FileName::parse(name.as_bytes()).is_some(),"new index file must be one 8.3 entry: {name}");
            }
        }
        let fd = self.live(|m| m.open(path, mode))?;
        self.open += 1;
        self.most_open = self.most_open.max(self.open);
        Ok(fd)
    }
    fn close(&mut self, fd: i32) -> Result<()> {
        self.live(|m| m.close(fd))?;
        self.open -= 1;
        Ok(())
    }
    fn read(&mut self, fd: i32, out: &mut [u8]) -> Result<usize> { self.live(|m| m.read(fd, out)) }
    fn write(&mut self, fd: i32, input: &[u8]) -> Result<usize> { self.writes += 1; self.live(|m| m.write(fd, input)) }
    fn seek(&mut self, fd: i32, offset: u32) -> Result<u64> { self.live(|m| m.seek(fd, offset)) }
    fn truncate(&mut self, fd: i32, bytes: u32) -> Result<()> { self.live(|m| m.truncate(fd, bytes)) }
    fn sync(&mut self, fd: i32) -> Result<()> { self.live(|m| m.sync(fd)) }
    fn fstat(&mut self, fd: i32) -> Result<Stat> {
        let mut stat = self.live(|m| m.fstat(fd))?;
        // FAT fstat has no directory entry, so it supplies no modification time.
        (stat.seconds, stat.nanos) = (0, 0);
        Ok(stat)
    }
    fn stat(&mut self, path: &str, no_follow: bool) -> Result<Stat> {
        let mut stat = self.live(|m| m.stat(path, no_follow))?;
        // Path stat reads the directory entry. Keep its time different from
        // fstat by default so every index publication exercises the FAT contract.
        stat.seconds = self.times.get(path).copied().unwrap_or(1);
        Ok(stat)
    }
    fn mkdir(&mut self, path: &str) -> Result<()> { self.live(|m| m.mkdir(path)) }
    fn unlink(&mut self, path: &str) -> Result<()> { self.live(|m| m.unlink(path)) }
    fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        if let Some(name)=to.strip_prefix(&format!("{ROOT}/Index/")) {
            assert!(newt_core::book_index::FileName::parse(name.as_bytes()).is_some(),"renamed index file must be one 8.3 entry: {name}");
        }
        self.live(|m| m.rename(from, to))
    }
    fn next_nonce(&mut self) -> u32 { self.model.next_nonce() }
}
impl Directory for Card {
    type Listing = u32;
    fn open_dir(&mut self, path: &str) -> Result<u32> { self.live(|m| m.open_dir(path)) }
    fn read_dir(&mut self, listing: &mut u32, name: &mut [u8]) -> Result<Option<usize>> {
        self.live(|m| m.read_dir(listing, name))
    }
    fn close_dir(&mut self, listing: u32) -> Result<()> { self.live(|m| m.close_dir(listing)) }
}
impl page_entries::PageEntries for Card {
    fn walk_entries(&mut self, folder: &core::ffi::CStr, sized: bool,
        visit: &mut dyn FnMut(&mut Self, page_entries::PageEntry<'_>) -> bool,
    ) -> bool { page_entries::walk_directory(self, folder, sized, visit) }
}
impl Card {
    pub(super) fn new() -> Self {
        let mut card = Self::default();
        card.model.dirs.extend(["/sd".into(), ROOT.into()]);
        card
    }
    /// Files in Cache/Index, by leaf name.
    pub(super) fn indexes(&self) -> Vec<String> {
        let folder = format!("{ROOT}/Index/");
        self.model.files.keys().filter_map(|p| p.strip_prefix(&folder)).map(String::from).collect()
    }
    pub(super) fn seed(&mut self, leaf: &str, bytes: usize, time: i64) {
        let path = format!("{ROOT}/Index/{leaf}");
        self.model.dirs.insert(format!("{ROOT}/Index"));
        self.model.files.insert(path.clone(), model::File { durable: vec![0; bytes], readable: vec![0; bytes] });
        self.times.insert(path, time);
    }
}

