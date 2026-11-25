// SPDX-License-Identifier: MIT
//! Cache store adapter over `ChapterCacheAccess` for the phased reader.
//! Every call takes one short access; the descriptor, lease and read policy
//! stay in `ChapterCacheAccess`.
use newt_core::page_cache::CacheStorage;
use newt_core::page_cache_access::VerifiedAccess;
use newt_core::page_envelope::VerifiedStore;
use newt_core::zip_container::Status;
use newt_native_storage::Native;
use newt_native_storage::chapter_cache::ChapterCacheStorage;
use newt_phased_reader::CachePresence;
use newt_phased_reader::phase_storage::ReadScope;

pub struct ReaderPages<'route, N: Native>(pub ChapterCacheStorage<'route, N>);

impl<N: Native> CacheStorage for ReaderPages<'_, N> {
    fn exists(&mut self, name: &[u8]) -> bool { self.0.access().exists(name) }
    fn remove(&mut self, name: &[u8]) -> bool {
        self.0.access().remove(name)
    }
    fn file_size(&mut self, name: &[u8]) -> i64 {
        self.0.access().file_size(name)
    }
    fn read_at(&mut self, name: &[u8], offset: u32, dst: &mut [u8]) -> i32 {
        self.0.access().read_at(name, offset, dst)
    }
    fn begin_write(&mut self, name: &[u8]) -> bool {
        self.0.access().begin_write(name)
    }
    fn write(&mut self, bytes: &[u8]) -> bool {
        self.0.access().write(bytes)
    }
    fn end_write(&mut self) -> bool {
        self.0.access().end_write()
    }
    fn read_back_at(&mut self, offset: u32, dst: &mut [u8]) -> i32 {
        self.0.access().read_back_at(offset, dst)
    }
}

/// presentIn: a presence probe inside its own read scope, so a file over the
/// opening cap is not cached for the opening.
impl<N: Native + newt_native_storage::page_entries::PageEntries> CachePresence for ReaderPages<'_, N> {
    fn with_index<R>(&mut self, digest: &[u8; 32], operation: impl FnOnce(&mut dyn newt_phased_reader::index_build::Store) -> R) -> Option<R> {
        let mut work = newt_native_storage::chapter_cache::IndexWorkState::new();
        let inner = newt_native_storage::chapter_cache::IndexBuildStore::lend(&mut self.0, &mut work, digest)?;
        Some(operation(&mut ReaderIndex(inner)))
    }
    fn present(&mut self, name: &[u8], opening: bool) -> bool {
        let mut access = self.0.access();
        if !access.begin_page_read(opening, None) {
            return false;
        }
        let found = access.exists(name);
        access.finish_cache_read() && found
    }
}

/// The verified query sits in the parse loan beyond the index, as pinned
/// `new(scratch) VerifiedPageStore(name)`. A failed attach still runs the
/// checked exit, and its read is Stale.
impl<N: Native> ReadScope for ReaderPages<'_, N> {
    fn query<R>(
        &mut self,
        name: &[u8],
        opening: bool,
        scratch: &mut [u8],
        source_live: &mut dyn FnMut() -> bool,
        read: impl FnOnce(&mut dyn CacheStorage) -> R,
    ) -> Result<(R, bool), Status> {
        self.query_bound(name,None,opening,scratch,source_live,read)
    }
    fn query_index<R>(&mut self,name:&[u8],digest:&[u8;32],opening:bool,scratch:&mut[u8],
        source_live:&mut dyn FnMut()->bool,read:impl FnOnce(&mut dyn CacheStorage)->R)->Result<(R,bool),Status> {
        self.query_bound(name,Some(digest),opening,scratch,source_live,read)
    }
}
impl<N:Native> ReaderPages<'_,N> {
    fn query_bound<R>(&mut self,name:&[u8],digest:Option<&[u8;32]>,opening:bool,scratch:&mut[u8],
        source_live:&mut dyn FnMut()->bool,read:impl FnOnce(&mut dyn CacheStorage)->R)->Result<(R,bool),Status> {
        let (directory, rest) = scratch
            .split_first_chunk_mut::<4160>()
            .ok_or(Status::OutOfMemory)?;
        let window = rest.first_chunk_mut::<4096>().ok_or(Status::OutOfMemory)?;
        let mut access = self.0.access();
        let mut verified=match digest {
            Some(digest)=>VerifiedStore::for_index(Some(name),digest,directory,window),
            None=>VerifiedStore::new(Some(name),directory,window),
        };
        let held = access.begin_page_read(opening, Some(&mut verified.query));
        let entered = held && source_live();
        if !entered || !verified.attach(&mut access) {
            verified.query.finish();
            source_live();
            if held { access.finish_cache_read(); }
            return Err(Status::Stale);
        }
        let value = read(&mut VerifiedAccess {
            store: &mut verified,
            source: &mut access,
        });
        verified.query.finish();
        let source_fresh = source_live();
        let finished = access.finish_cache_read();
        Ok((value, source_fresh && finished))
    }

}

impl<N: Native> newt_phased_reader::producer_probe::ProducerProbe for ReaderPages<'_, N> {
    fn producer_probe(&mut self, name: &[u8], cancelled: &dyn Fn() -> bool)
        -> newt_core::producer_visit::MissingProbe
    {
        use newt_native_storage::{Authority, MissingProbe, ProducerPolicy};
        struct Check<'a>(&'a dyn Fn() -> bool);
        impl Authority for Check<'_> {
            fn cancelled(&self) -> bool { (self.0)() }
        }
        let cancel = Check(cancelled);
        let mut resolved = false;
        let hint = self.0.access().probe_page(name, &cancel, &mut resolved);
        if resolved { return hint; }
        // A filename hit still needs the checked NTBE read before it can skip a rebuild.
        let policy = ProducerPolicy::new(&cancel);
        let mut uncertain = false;
        let Some(size) = self.0.produce(&policy, |access| {
            let size = access.identity_file_size(name, &mut uncertain);
            if uncertain { policy.record_failure(); }
            size
        }) else { return MissingProbe::Denied; };
        if policy.cancellation_observed() { return MissingProbe::Stopped; }
        if policy.failed() || uncertain { return MissingProbe::Denied; }
        if cancelled() { return MissingProbe::Stopped; }
        if size >= 0 { MissingProbe::Complete } else { MissingProbe::Ready }
    }
    fn release_for_admission(&mut self) -> bool {
        self.0.access().release_for_admission()
    }
}


struct ReaderIndex<'op, 'route, N: Native>(newt_native_storage::chapter_cache::IndexBuildStore<'op, 'route, N>);
impl<N: Native + newt_native_storage::page_entries::PageEntries> newt_phased_reader::index_build::Store for ReaderIndex<'_, '_, N> {
    fn census(&mut self, cancelled: &dyn Fn() -> bool) { self.0.census(cancelled); }
    fn spilled(&self) -> bool { self.0.spilled() }
    fn release_pages(&mut self) -> bool { self.0.release_pages() }
}
impl<N: Native> newt_core::book_index_build::WorkFile for ReaderIndex<'_, '_, N> {
    fn append(&mut self, bytes: &[u8]) -> bool { self.0.append(bytes) }
    fn read_at(&mut self, at: u32, out: &mut [u8]) -> bool { newt_core::book_index_build::WorkFile::read_at(&mut self.0, at, out) }
    fn sync_work(&mut self) -> bool { self.0.sync_work() }
    fn remove_work(&mut self) -> bool { self.0.remove_work() }
}
impl<N: Native> CacheStorage for ReaderIndex<'_, '_, N> {
    fn remove(&mut self, name: &[u8]) -> bool { self.0.remove(name) }
    fn file_size(&mut self, name: &[u8]) -> i64 { self.0.file_size(name) }
    fn read_at(&mut self, name: &[u8], at: u32, out: &mut [u8]) -> i32 { CacheStorage::read_at(&mut self.0, name, at, out) }
    fn begin_write(&mut self, name: &[u8]) -> bool { self.0.begin_write(name) }
    fn write(&mut self, bytes: &[u8]) -> bool { self.0.write(bytes) }
    fn end_write(&mut self) -> bool { self.0.end_write() }
    fn end_write_checked(&mut self, live: &mut dyn FnMut() -> bool) -> bool { self.0.end_write_checked(live) }
    fn read_back_at(&mut self, at: u32, out: &mut [u8]) -> i32 { self.0.read_back_at(at, out) }
}
