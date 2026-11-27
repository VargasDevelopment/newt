//! Chapter cache producer segment boundaries and map writes; no parser or bank owner.
#![forbid(unsafe_code)]
use super::{identity_at_generation, identity_generation};
use crate::{BookLanguage, PageInfo, imported_revision::{Digest}, producer_store::ProducerStore, producer_writer::CappedWriter};
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
    pub(crate) language: Option<BookLanguage>,
    pub(crate) start: u8,
    pub(crate) sealed: u8,
}
const _: () = assert!(core::mem::size_of::<Segments>() == 148);
impl Segments {
    pub fn new(certified_pages: u32) -> Self {
        Self { name: [0; 128], generation: 0, next: 0,
            count: if certified_pages > segment::MAX_PAGES { 0 } else { ChapterSegment::count(certified_pages) },
            skipped: u32::MAX, staged: false,
            language: None, start: 0, sealed: 0 }
    }
    pub fn set_language(&mut self, language: Option<BookLanguage>) {
        self.language = language;
    }
    pub(crate) fn set_start(&mut self, segment: u32) {
        self.start = if self.count == 0 {
            0
        } else {
            segment.min(self.count - 1) as u8
        };
    }
    pub(crate) fn segment_index(&self, step: u32) -> u32 {
        let start = u32::from(self.start);
        if start == 0 || self.count == 0 {
            step
        } else {
            let idx = start + step;
            if idx >= self.count { idx - self.count } else { idx }
        }
    }
    fn identify(&mut self, route: &mut Route<'_>, spine: u32) -> bool {
        // Frozen identity publishes the generation before the fallible hash.
        let Some(language) = self.language else { return false; };
        self.generation = identity_generation(route.settings, language);
        let Ok(name) = identity_at_generation(route.settings, route.digest, spine, route.hash, self.generation) else {
            return false;
        };
        self.name[..69].copy_from_slice(&name);
        self.name[69] = 0;
        true
    }
    pub(crate) fn name_segment(&mut self, route: &mut Route<'_>, index: u32) -> bool {
        let segment = ChapterSegment {
            index,
            pages: route.info.page_count,
            chars: route.info.total_chars,
        };
        self.identify(route, route.info.spine)
            && segment::segment_name(segment, &mut self.name, &mut |b, d| route.hash.hash(b, d))
    }
    /// Select the first missing segment in D3 order that can open under the
    /// active grant, without calling `begin_version` yet so restore can open
    /// and close its predecessor read lease before opening the target temp.
    #[inline(never)]
    pub(crate) fn select_target<C: ?Sized, S: CacheStorage + ?Sized>(
        &mut self,
        store: &mut ProducerStore<'_, '_, '_, '_, C, S>,
        route: &mut Route<'_>,
    ) -> Result<Option<u32>, Status> {
        while self.next < self.count {
            let step = self.next;
            self.next = self.next.wrapping_add(1);
            let index = self.segment_index(step);
            if !self.name_segment(route, index) {
                return Err(Status::IoError);
            }
            if store.exists(&self.name[..69]) {
                continue;
            }
            if !store.checkpoint() {
                return Err(Status::IoError);
            }
            if !store.producer_can_begin() {
                self.skipped = self.skipped.min(step);
                continue;
            }
            return Ok(Some(index));
        }
        Ok(None)
    }
    /// Open the temp writer for an already selected target segment `index`,
    /// recomputing `self.name` first (for callers after restore candidate scans
    /// that may have overwritten `self.name`).
    #[inline(never)]
    pub(crate) fn open_segment<C: ?Sized, S: CacheStorage + ?Sized>(
        &mut self,
        writer: &mut CappedWriter<'_, '_, C>,
        store: &mut ProducerStore<'_, '_, '_, '_, C, S>,
        memory: &mut WriterMemory<'_>,
        route: &mut Route<'_>,
        index: u32,
    ) -> Status {
        if !self.name_segment(route, index) {
            return Status::IoError;
        }
        self.open_named_segment(writer, store, memory, route, index)
    }
    /// Open the temp writer for `index` when `self.name` and `self.generation`
    /// already identify `index` (avoids re-hashing on ordinary segment boundaries).
    fn open_named_segment<C: ?Sized, S: CacheStorage + ?Sized>(
        &mut self,
        writer: &mut CappedWriter<'_, '_, C>,
        store: &mut ProducerStore<'_, '_, '_, '_, C, S>,
        memory: &mut WriterMemory<'_>,
        route: &mut Route<'_>,
        index: u32,
    ) -> Status {
        let segment = ChapterSegment {
            index,
            pages: route.info.page_count,
            chars: route.info.total_chars,
        };
        let version = if writer.capture_points
            && segment.first().wrapping_add(PAGES) < route.info.page_count
        {
            newt_core::page_cache::VERSION_POINT
        } else {
            newt_core::page_cache::VERSION_PAGES
        };
        if !writer.cache.begin_version(store, &self.name[..69], self.generation, memory, version) {
            return Status::OutOfMemory;
        }
        #[cfg(feature = "page-turn-trace")]
        store.trace_segment_begin(index);
        writer.first = segment.first();
        writer.open = true;
        Status::Ok
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
            let was_failed = writer.cache.failed();
            store.clear_end_committed();
            #[cfg(feature = "page-turn-trace")]
            store.trace_segment_layout_done();
            let finished = writer.cache.finish(store, &self.name[..69], memory);
            let committed = finished || (!was_failed && store.end_committed());
            if committed {
                self.sealed = self.sealed.saturating_add(1);
                if finished && writer.segmented && writer.restore_points && writer.cache.page_count() > 0 {
                    writer.record_start(
                        (writer.first / PAGES) as usize,
                        writer.cache.char_start(memory, 0),
                    );
                }
                #[cfg(feature = "page-turn-trace")]
                store.trace_segment_end(writer.cache.page_count());
            }
            if !finished { return Status::IoError; }
        }
        while self.next < self.count {
            let step = self.next;
            self.next = self.next.wrapping_add(1);
            let index = self.segment_index(step);
            let segment = ChapterSegment { index, pages: route.info.page_count, chars: route.info.total_chars };
            if !self.name_segment(route, index) {
                return Status::IoError;
            }
            if store.exists(&self.name[..69]) { continue; }
            if !store.checkpoint() { return Status::IoError; }
            if segment.first() < writer.seen { self.skipped = self.skipped.min(step); continue; }
            if !store.producer_can_begin() { self.skipped = self.skipped.min(step); continue; }
            return self.open_named_segment(writer, store, memory, route, index);
        }
        Status::Ok
    }
    /// After EOF finishes the final physical segment under a wrapped `start > 0`,
    /// earlier segments (`0..start`) remain in `self.next..self.count`. Record any
    /// absent step in `self.skipped` so a later pass or attempt fills them.
    pub(crate) fn scan_remaining<C: ?Sized, S: CacheStorage + ?Sized>(
        &mut self, store: &mut ProducerStore<'_, '_, '_, '_, C, S>, route: &mut Route<'_>,
    ) -> Status {
        while self.next < self.count {
            let step = self.next;
            self.next = self.next.wrapping_add(1);
            let index = self.segment_index(step);
            let segment = ChapterSegment { index, pages: route.info.page_count, chars: route.info.total_chars };
            let spine = route.info.spine;
            if !self.identify(route, spine) || !segment::segment_name(segment, &mut self.name, &mut |b, d| route.hash.hash(b, d)) {
                return Status::IoError;
            }
            if store.exists(&self.name[..69]) { continue; }
            if !store.checkpoint() { return Status::IoError; }
            self.skipped = self.skipped.min(step);
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
        let count = ChapterSegment::count(route.info.page_count);
        if !writer.starts_complete(count) {
            return Status::Unsupported;
        }
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
