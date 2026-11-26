// SPDX-License-Identifier: MIT
//! In-memory index/work files shared by portable corpus and native host fixtures.
use newt_core::{book_index as index, book_index_build as builder,
    page_cache::CacheStorage, zip_container::Status};
use newt_phased_reader::{CachePresence, index_build, phase_storage::ReadScope};
use std::vec::Vec;

#[derive(Default)]
pub struct BuildStore { pub work: Vec<u8>, pub cache: Vec<u8>, pending: Vec<u8>, pub index_opens: usize }
impl builder::WorkFile for BuildStore {
    fn append(&mut self, bytes: &[u8]) -> bool { self.work.extend_from_slice(bytes); true }
    fn read_at(&mut self, at: u32, out: &mut [u8]) -> bool {
        let Some(input) = self.work.get(at as usize..at as usize + out.len()) else { return false; };
        out.copy_from_slice(input); true
    }
    fn remove_work(&mut self) -> bool { self.work.clear(); true }
    fn sync_work(&mut self) -> bool { true }
}
impl CacheStorage for BuildStore {
    fn remove(&mut self, _: &[u8]) -> bool { self.cache.clear(); true }
    fn file_size(&mut self, _: &[u8]) -> i64 { self.cache.len() as i64 }
    fn read_at(&mut self, _: &[u8], at: u32, out: &mut [u8]) -> i32 {
        if at == 0 && out.len() == index::HEADER_BYTES as usize { self.index_opens += 1; }
        let Some(input) = self.cache.get(at as usize..at as usize + out.len()) else { return -1; };
        out.copy_from_slice(input); out.len() as i32
    }
    fn begin_write(&mut self, _: &[u8]) -> bool { self.pending.clear(); true }
    fn write(&mut self, bytes: &[u8]) -> bool { self.pending.extend_from_slice(bytes); true }
    fn end_write(&mut self) -> bool { self.cache = std::mem::take(&mut self.pending); true }
}

impl ReadScope for BuildStore {
    fn query<R>(&mut self, _: &[u8], _: bool, _: &mut [u8], live: &mut dyn FnMut() -> bool,
        read: impl FnOnce(&mut dyn CacheStorage) -> R) -> Result<(R, bool), Status> {
        if !live() { return Err(Status::Stale); }
        let result = read(self);
        Ok((result, live()))
    }
}

impl CachePresence for BuildStore {
    fn with_index<R>(&mut self, _: &[u8; 32], operation: impl FnOnce(&mut dyn index_build::Store) -> R) -> Option<R> { Some(operation(self)) }
    fn present(&mut self, _: &[u8], _: bool) -> bool { !self.cache.is_empty() }
}

impl index_build::Store for BuildStore {
    fn census(&mut self, cancelled: &dyn Fn() -> bool) { cancelled(); }
    fn spilled(&self) -> bool { !self.work.is_empty() }
    fn release_pages(&mut self) -> bool { self.pending.clear(); true }
}
