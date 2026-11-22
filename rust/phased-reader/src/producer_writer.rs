//! Capped page-cache writer with cancellation checks. MIT.
#![forbid(unsafe_code)]
use newt_core::cache_names::segment::{MAX_SEGMENTS, PAGES};
use newt_core::chapter_layout::types::Page;
use newt_core::page_cache::{CacheStorage, PageCacheWriter, Profile, WriterMemory};
use newt_core::producer_policy::Cancellation;

/// The owner supplies the segment starts from its attempt storage. It must
/// finish/abort the serializer before releasing the store or index loans.
pub struct CappedWriter<'loan, 'ctx, C: ?Sized> {
    /// Underlying begin/finish/query writer state.
    /// Deliver pages/anchors through this adapter, not directly to the base.
    pub cache: PageCacheWriter,
    cancellation: &'loan Cancellation<'ctx, C>,
    pub starts: &'loan mut [u32; MAX_SEGMENTS],
    pub seen: u32,
    pub first: u32,
    pub refused: bool,
    pub segmented: bool,
    pub open: bool,
    pub mapping: bool,
}
impl<'loan, 'ctx, 'pause, C: ?Sized> CappedWriter<'loan, 'ctx, C> {
    pub fn new(
        cancellation: &'loan Cancellation<'ctx, C>,
        starts: &'loan mut [u32; MAX_SEGMENTS],
    ) -> Self {
        starts.fill(0);
        Self {
            cache: PageCacheWriter::new(Profile::Small), cancellation, starts,
            seen: 0, first: 0, refused: false, segmented: false, open: false, mapping: false,
        }
    }
    pub(crate) fn cancellation(&self) -> &'loan Cancellation<'ctx, C> { self.cancellation }
    pub fn receiving(&self, page: u32) -> bool {
        self.open && page >= self.first && page - self.first < PAGES
    }
    pub fn on_page(
        &mut self,
        store: &mut impl CacheStorage,
        memory: &mut WriterMemory<'_>,
        page: &Page<'_>,
    ) -> bool {
        if self.segmented && self.seen % PAGES == 0 && (self.seen / PAGES) < MAX_SEGMENTS as u32 {
            self.starts[(self.seen / PAGES) as usize] = page.char_start;
        }
        let skip = if self.segmented {
            let skip = !self.receiving(self.seen);
            self.seen = self.seen.wrapping_add(1);
            skip
        } else {
            false
        };
        if (!skip && self.cache.page_count() >= PAGES) || !self.cancellation.checkpoint()
            || (!skip && !self.cache.on_page(store, memory, page))
        {
            self.refused = true;
        }
        self.cancellation.result(!self.refused);
        !self.refused
    }
    pub fn on_anchor(&mut self, memory: &mut WriterMemory<'_>, hash: u32, start: u32) {
        if !self.refused && self.cancellation.checkpoint() {
            if !self.segmented || self.receiving(self.seen) {
                self.cache.on_anchor(memory, hash, start);
            }
        } else {
            self.refused = true;
        }
        self.cancellation.result(!self.refused);
    }
}
