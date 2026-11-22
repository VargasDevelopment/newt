//! Chapter cache producer segment boundaries and map writes; no parser or bank owner.
#![forbid(unsafe_code)]
use super::{identity_at_generation, identity_generation};
use crate::{PageInfo, imported_revision::{Digest}, producer_store::ProducerStore, producer_writer::CappedWriter};
use newt_core::{cache_names::segment::{self, ChapterSegment, PAGES}, page_cache::{CacheStorage, WriterMemory}, preferences::ViewSettings, zip_container::Status};

/// Certified published metadata and real route provider, lent for one call.
/// The enclosing producer must validate its source before entering this seam.
pub struct Route<'a> {
    pub settings: ViewSettings,
    pub digest: &'a Digest,
    pub info: PageInfo,
    pub hash: &'a mut dyn crate::hash_provider::CacheHash,
}
pub struct Segments {
    pub name: [u8; 128],
    pub generation: u32,
    pub next: u32,
    pub count: u32,
    pub skipped: u32,
    pub(crate) staged: bool,
}
impl Segments {
    pub fn new(certified_pages: u32) -> Self {
        Self { name: [0; 128], generation: 0, next: 0,
            count: ChapterSegment::count(certified_pages), skipped: u32::MAX, staged: false }
    }
    fn identify(&mut self, route: &mut Route<'_>, spine: u32) -> bool {
        // Frozen identity publishes the generation before the fallible hash.
        self.generation = identity_generation(route.settings);
        let Ok(name) = identity_at_generation(route.settings, route.digest, spine, route.hash, self.generation) else {
            return false;
        };
        self.name[..69].copy_from_slice(&name);
        self.name[69] = 0;
        true
    }
    /// Call between parser steps. Index backing is already lent; begin resets
    /// its active counters in place.
    pub fn boundary<C: ?Sized, S: CacheStorage + ?Sized>(
        &mut self, writer: &mut CappedWriter<'_, '_, C>, store: &mut ProducerStore<'_, '_, '_, '_, C, S>,
        memory: &mut WriterMemory<'_>, route: &mut Route<'_>,
    ) -> Status {
        if writer.open {
            if writer.cache.page_count() < PAGES || writer.first.wrapping_add(PAGES) >= route.info.page_count {
                return Status::Ok;
            }
            writer.open = false;
            writer.cache.set_total_chars(route.info.total_chars);
            if !writer.cache.finish(store, &self.name[..69], memory) { return Status::IoError; }
        }
        while self.next < self.count {
            let index = self.next;
            self.next = self.next.wrapping_add(1);
            let segment = ChapterSegment { index, pages: route.info.page_count, chars: route.info.total_chars };
            let spine = route.info.spine;
            if !self.identify(route, spine) || !segment::segment_name(segment, &mut self.name, &mut |b, d| route.hash.hash(b, d)) {
                return Status::IoError;
            }
            if store.exists(&self.name[..69]) { continue; }
            if !store.checkpoint() { return Status::IoError; }
            if segment.first() < writer.seen { self.skipped = self.skipped.min(index); continue; }
            if !writer.cache.begin(store, &self.name[..69], self.generation, memory) { return Status::OutOfMemory; }
            writer.first = segment.first();
            writer.open = true;
            return Status::Ok;
        }
        Status::Ok
    }
    /// The owner returns the writer-index loan before lending map bytes. None
    /// represents a failed tail allocation; no backing is allocated here.
    pub fn write_map<C: ?Sized, S: CacheStorage + ?Sized>(
        &mut self, writer: &CappedWriter<'_, '_, C>, store: &mut ProducerStore<'_, '_, '_, '_, C, S>,
        record: Option<&mut [u8; segment::MAX_BYTES]>, route: &mut Route<'_>, spine: u32,
    ) -> Status {
        let Some(record) = record else { return Status::IoError; };
        if !self.identify(route, spine) || !segment::map_name(&mut self.name, &mut |b, d| route.hash.hash(b, d)) {
            return Status::IoError;
        }
        let bytes = segment::encode(record, self.generation, route.info.page_count, route.info.total_chars,
            &*writer.starts, &mut |b, d| route.hash.hash(b, d));
        if bytes == 0 { return Status::Unsupported; }
        if !store.begin_write(&self.name[..69]) { return Status::IoError; }
        if store.write(&record[..bytes as usize]) && store.end_write() { Status::Ok } else { Status::IoError }
    }
}

#[path = "producer_stream.rs"]
mod stream;
pub use stream::NextPass;
