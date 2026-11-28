//! One verified read authority for foreground and producer cache loans.
use super::{ChapterCacheAccess, Native};
use newt_core::{page_cache::{CacheStorage, ReadScope}, page_cache_access::VerifiedAccess,
    page_envelope::VerifiedStore, zip_container::Status};

impl<N: Native> ReadScope for ChapterCacheAccess<'_, '_, N> {
    // A failed close only costs the next open; there is nothing to report.
    fn close_after_cancel(&mut self) { let _ = self.release_for_state(); }
    fn invalidate_index(&mut self, digest: &[u8; 32]) -> bool {
        if !self.release_for_admission() { return false; }
        let name = newt_core::book_index::FileName::new(digest, newt_core::book_index::FileKind::Index);
        self.file.port.route.remove_record(self.file.port.native, "Index", name.as_str(), false)
    }
    #[inline(always)]
    fn query<R>(&mut self, name: &[u8], opening: bool, scratch: &mut [u8],
        live: &mut dyn FnMut() -> bool, read: impl FnOnce(&mut dyn CacheStorage) -> R,
    ) -> Result<(R, bool), Status> {
        self.query_bound(name, None, opening, scratch, live, read)
    }
    #[inline(always)]
    fn query_index<R>(&mut self, name: &[u8], digest: &[u8; 32], opening: bool, scratch: &mut [u8],
        live: &mut dyn FnMut() -> bool, read: impl FnOnce(&mut dyn CacheStorage) -> R,
    ) -> Result<(R, bool), Status> {
        self.query_bound(name, Some(digest), opening, scratch, live, read)
    }
}
impl<N: Native> ChapterCacheAccess<'_, '_, N> {
    fn query_bound<R>(&mut self, name: &[u8], digest: Option<&[u8; 32]>, opening: bool,
        scratch: &mut [u8], live: &mut dyn FnMut() -> bool,
        read: impl FnOnce(&mut dyn CacheStorage) -> R,
    ) -> Result<(R, bool), Status> {
        let (directory, rest) = scratch.split_first_chunk_mut::<4160>().ok_or(Status::OutOfMemory)?;
        let window = rest.first_chunk_mut::<4096>().ok_or(Status::OutOfMemory)?;
        let mut verified = match digest {
            Some(digest) => VerifiedStore::for_index(Some(name), digest, directory, window),
            None => VerifiedStore::new(Some(name), directory, window),
        };
        let held = self.begin_page_read(opening, Some(&mut verified.query));
        let entered = held && live();
        if !entered || !verified.attach(self) {
            verified.query.finish();
            live();
            if held { self.finish_cache_read(); }
            return Err(Status::Stale);
        }
        let value = read(&mut VerifiedAccess { store: &mut verified, source: self });
        verified.query.finish();
        let source_fresh = live();
        let finished = self.finish_cache_read();
        Ok((value, source_fresh && finished))
    }
}
