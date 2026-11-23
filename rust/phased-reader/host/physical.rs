// SPDX-License-Identifier: MIT
//! Host I/O ports for synthetic physical-reader fixtures.
use newt_core::{page_cache::CacheStorage, zip_container::Status, zip_window::source::BookSource};
use newt_phased_reader::{CachePresence, hash_provider::CacheHash,
    phase_storage::{BANK_BYTES, BuildLoan, ReadScope}};
use std::{io::Read, path::Path};

pub struct Source(std::io::Cursor<Vec<u8>>);
impl Source {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        Ok(Self(std::io::Cursor::new(std::fs::read(path)?)))
    }
}
impl BookSource for Source {
    fn size(&self) -> u64 { self.0.get_ref().len() as u64 }
    fn read_at(&mut self, at: u64, bytes: &mut [u8]) -> i32 {
        self.0.set_position(at);
        self.0.read(bytes).map_or(-1, |n| n as i32)
    }
}

#[repr(align(16))]
pub struct Bank(pub [u8; BANK_BYTES]);
impl BuildLoan for Bank {
    fn with_parse<R>(&mut self, operation: impl for<'a> FnOnce(&'a mut [u8]) -> R) -> Result<R, Status> {
        let result = operation(&mut self.0);
        self.0.fill(0);
        Ok(result)
    }
}

#[derive(Default)]
pub struct Store {
    pub name: Vec<u8>,
    pub file: Vec<u8>,
}
impl CacheStorage for Store {
    fn file_size(&mut self, name: &[u8]) -> i64 {
        if self.name == name { self.file.len() as i64 } else { -1 }
    }
    fn read_at(&mut self, name: &[u8], at: u32, bytes: &mut [u8]) -> i32 {
        if name != self.name { return -1; }
        self.file.get(at as usize..).map_or(-1, |mut file| file.read(bytes).map_or(-1, |n| n as i32))
    }
    fn begin_write(&mut self, name: &[u8]) -> bool {
        self.name = name.to_vec();
        self.file.clear();
        true
    }
    fn write(&mut self, bytes: &[u8]) -> bool {
        self.file.extend_from_slice(bytes);
        true
    }
    fn end_write(&mut self) -> bool { true }
    fn remove(&mut self, _: &[u8]) -> bool { false }
}
impl ReadScope for Store {
    fn query<R>(&mut self, name: &[u8], _: bool, _: &mut [u8], live: &mut dyn FnMut() -> bool,
        read: impl FnOnce(&mut dyn CacheStorage) -> R,
    ) -> Result<(R, bool), Status> {
        if !live() || !self.present(name, false) { return Err(Status::NotFound); }
        Ok((read(self), live()))
    }
}
impl CachePresence for Store {
    fn present(&mut self, name: &[u8], _: bool) -> bool { self.name == name && !self.file.is_empty() }
}

pub struct Key;
impl CacheHash for Key {
    fn hash(&mut self, bytes: &[u8], key: &mut [u8; 32]) -> bool {
        // Preserve the spine, generation, engine revision and view settings
        // from the production key encoding. The fixture holds the digest fixed.
        let Some(layout) = bytes.get(84..99) else { return false; };
        key[..layout.len()].copy_from_slice(layout);
        true
    }
}

