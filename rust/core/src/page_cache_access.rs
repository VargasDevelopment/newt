//! Short-lived SDK cache access over an already attached verified query.
use crate::page_cache::CacheStorage;
use crate::page_envelope::{RawRead, VerifiedStore};

/// Drop this view before returning the native query/parse-storage lease.
pub struct VerifiedAccess<'s, 'n, 'a, R: RawRead> {
    pub store: &'s mut VerifiedStore<'n, 'a>,
    pub source: &'s mut R,
}
impl<R: RawRead> CacheStorage for VerifiedAccess<'_, '_, '_, R> {
    fn remove(&mut self, _name: &[u8]) -> bool {
        false
    }
    fn file_size(&mut self, name: &[u8]) -> i64 {
        self.store.file_size(Some(name))
    }
    fn read_at(&mut self, name: &[u8], offset: u32, out: &mut [u8]) -> i32 {
        self.store.read_at(self.source, Some(name), offset, out)
    }
    fn begin_write(&mut self, _name: &[u8]) -> bool {
        false
    }
    fn write(&mut self, _bytes: &[u8]) -> bool {
        false
    }
    fn end_write(&mut self) -> bool {
        false
    }
}
