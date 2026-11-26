//! Shared frozen chapter_page_cache.h GuardedStore, one scoped source validator. MIT.
#![forbid(unsafe_code)]
use newt_core::{page_cache::CacheStorage, producer_policy::ProducerCheck};

// Exact build-side original GuardedStore policy, including pre-begin cleanup.
// A direct source loan also works inside a source-lending callback. The legacy
// caller alone supplies SourceView; guard probes finish before storage calls.
pub(crate) struct GuardedStore<'store, 'check, C: ?Sized> {
    inner: &'store mut C,
    check: &'check mut dyn FnMut(ProducerCheck) -> bool,
    attempted: &'check mut bool,
}
impl<'store, 'check, C: ?Sized> GuardedStore<'store, 'check, C> {
    pub(crate) fn new(inner: &'store mut C, check: &'check mut dyn FnMut(ProducerCheck) -> bool,
        attempted: &'check mut bool) -> Self { Self { inner, check, attempted } }
    fn good(&mut self) -> bool { (self.check)(ProducerCheck::Source) }
}
impl<C: CacheStorage + ?Sized> CacheStorage for GuardedStore<'_, '_, C> {
    fn exists(&mut self, name: &[u8]) -> bool {
        self.good() && self.inner.exists(name) && self.good()
    }
    fn remove(&mut self, name: &[u8]) -> bool {
        if !*self.attempted {
            return true;
        }
        *self.attempted = false;
        self.inner.remove(name)
    }
    fn file_size(&mut self, name: &[u8]) -> i64 {
        if !self.good() {
            return -1;
        }
        let bytes = self.inner.file_size(name);
        if self.good() { bytes } else { -1 }
    }
    fn read_at(&mut self, name: &[u8], offset: u32, out: &mut [u8]) -> i32 {
        if !self.good() {
            return -1;
        }
        let count = self.inner.read_at(name, offset, out);
        if self.good() { count } else { -1 }
    }
    fn begin_write(&mut self, name: &[u8]) -> bool {
        *self.attempted = false;
        if !self.good() {
            return false;
        }
        *self.attempted = true;
        self.inner.begin_write(name) && self.good()
    }
    fn write(&mut self, bytes: &[u8]) -> bool {
        self.good() && self.inner.write(bytes) && self.good()
    }
    fn end_write(&mut self) -> bool {
        if !self.good() { return false; }
        let written = self.inner.end_write_checked(&mut || (self.check)(ProducerCheck::Receipt));
        written && self.good()
    }
    #[cfg(feature = "page-turn-trace")]
    fn trace_micros(&self) -> u32 {
        self.inner.trace_micros()
    }
    #[cfg(feature = "page-turn-trace")]
    fn trace_segment_begin(&mut self, idx: u32) {
        self.inner.trace_segment_begin(idx);
    }
    #[cfg(feature = "page-turn-trace")]
    fn trace_add_tee_us(&mut self, us: u32) {
        self.inner.trace_add_tee_us(us);
    }
    #[cfg(feature = "page-turn-trace")]
    fn trace_segment_layout_done(&mut self) {
        self.inner.trace_segment_layout_done();
    }
    #[cfg(feature = "page-turn-trace")]
    fn trace_segment_end(&mut self, pages: u32) {
        self.inner.trace_segment_end(pages);
    }
}
