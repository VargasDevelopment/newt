//! Cancellable chapter cache producer store wrapper. MIT.
#![forbid(unsafe_code)]
use newt_core::page_cache::CacheStorage;
use newt_core::producer_policy::{Cancellation, Stop};

/// Supplied only by the outer attempt, never by a guarded write view.
pub type Pause<'callback, 'ctx, C, S> = dyn FnMut(&mut S, &Cancellation<'ctx, C>) -> Stop + 'callback;

pub const BUFFER_BYTES: usize = 1024;

/// The tail keeps these bytes while the Store's source-guard loan changes.
pub struct StoreState<'window> {
    buffer: Option<&'window mut [u8; BUFFER_BYTES]>,
    used: u16,
    end_committed: bool,
}
const _: () = assert!(core::mem::size_of::<StoreState<'_>>() == 2 * core::mem::size_of::<usize>());
impl<'window> StoreState<'window> {
    pub fn new(buffer: Option<&'window mut [u8; BUFFER_BYTES]>) -> Self {
        Self { buffer, used: 0, end_committed: false }
    }
}

/// A short view of the same base, latch and tail-owned stream state.
/// Dropping a view does no I/O and never returns the stream window.
pub struct ProducerStore<'loan, 'window, 'ctx, 'pause, C: ?Sized, S: ?Sized> {
    base: &'loan mut S,
    cancellation: &'loan Cancellation<'ctx, C>,
    state: &'loan mut StoreState<'window>,
    pause: Option<&'loan mut Pause<'pause, 'ctx, C, S>>,
}
impl<'loan, 'window, 'ctx, 'pause, C: ?Sized, S: CacheStorage + ?Sized> ProducerStore<'loan, 'window, 'ctx, 'pause, C, S> {
    pub fn new(
        base: &'loan mut S,
        cancellation: &'loan Cancellation<'ctx, C>,
        state: &'loan mut StoreState<'window>,
        pause: Option<&'loan mut Pause<'pause, 'ctx, C, S>>,
    ) -> Self {
        Self { base, cancellation, state, pause }
    }
    pub(crate) fn with_guarded<R>(&mut self,
        guard: Option<(&mut dyn FnMut(newt_core::producer_policy::ProducerCheck) -> bool, &mut bool)>,
        operation: impl FnOnce(&mut ProducerStore<'_, 'window, 'ctx, 'pause, C, dyn CacheStorage + '_>) -> R,
    ) -> R where S: Sized {
        if let Some((check, attempted)) = guard {
            let mut base = crate::guarded_store::GuardedStore::new(&mut *self.base, check, attempted);
            let mut view = ProducerStore::new(&mut base as &mut dyn CacheStorage,
                self.cancellation, &mut *self.state, None);
            operation(&mut view)
        } else {
            let mut view = ProducerStore::new(&mut *self.base as &mut dyn CacheStorage,
                self.cancellation, &mut *self.state, None);
            operation(&mut view)
        }
    }
    /// Called only after the parser step and every guarded source loan return.
    pub(crate) fn pause_point(&mut self) -> bool {
        let cancellation = self.cancellation;
        if let Some(pause) = self.pause.as_deref_mut() {
            cancellation.pause_point_with(|same| pause(&mut *self.base, same))
        } else { cancellation.pause_point() }
    }
    pub(crate) fn checkpoint(&self) -> bool {
        self.cancellation.checkpoint()
    }
    pub(crate) fn use_stream_buffer(&mut self, buffer: &'window mut [u8; BUFFER_BYTES]) { self.state.buffer = Some(buffer); }
    pub(crate) fn return_stream_buffer(&mut self) -> Option<&'window mut [u8; BUFFER_BYTES]> {
        self.state.used = 0;
        self.state.buffer.take()
    }
    pub(crate) fn clear_end_committed(&mut self) { self.state.end_committed = false; }
    pub(crate) fn end_committed(&self) -> bool { self.state.end_committed }
    pub fn flush(&mut self) -> bool {
        let used = usize::from(self.state.used);
        if used == 0 {
            return self.cancellation.checkpoint();
        }
        let Some(buffer) = self.state.buffer.as_deref() else {
            return false;
        };
        if !self.cancellation.step(|| self.base.write(&buffer[..used])) {
            return false;
        }
        self.state.used = 0;
        true
    }
    /// Remains occupied after a failed flush, even if the base accepted bytes.
    pub fn buffered_bytes(&self) -> usize {
        usize::from(self.state.used)
    }
}
impl<C: ?Sized, S: CacheStorage + ?Sized> CacheStorage for ProducerStore<'_, '_, '_, '_, C, S> {
    fn exists(&mut self, name: &[u8]) -> bool {
        if !self.cancellation.checkpoint() { return false; }
        let found = self.base.exists(name);
        self.cancellation.checkpoint() && found
    }
    fn remove(&mut self, name: &[u8]) -> bool {
        self.base.remove(name)
    }
    fn file_size(&mut self, name: &[u8]) -> i64 {
        let mut size = -1;
        if self.cancellation.step(|| { size = self.base.file_size(name); size >= 0 }) {
            size
        } else {
            -1
        }
    }
    fn read_at(&mut self, name: &[u8], offset: u32, output: &mut [u8]) -> i32 {
        let mut got = -1;
        if self.cancellation.step(|| { got = self.base.read_at(name, offset, output); got >= 0 }) {
            got
        } else {
            -1
        }
    }
    fn begin_write(&mut self, name: &[u8]) -> bool {
        self.state.end_committed = false;
        self.cancellation.step(|| self.base.begin_write(name))
    }
    fn write(&mut self, bytes: &[u8]) -> bool {
        let Some(buffer) = self.state.buffer.as_deref_mut() else {
            return self.cancellation.step(|| self.base.write(bytes));
        };
        let mut input = bytes;
        while !input.is_empty() {
            if !self.cancellation.checkpoint() {
                return false;
            }
            let used = usize::from(self.state.used);
            let take = input.len().min(BUFFER_BYTES - used);
            buffer[used..used + take].copy_from_slice(&input[..take]);
            let next_used = used + take;
            self.state.used = next_used as u16;
            input = &input[take..];
            if next_used == BUFFER_BYTES {
                if !self.cancellation.step(|| self.base.write(&buffer[..next_used])) {
                    return false;
                }
                self.state.used = 0;
            }
        }
        self.cancellation.checkpoint()
    }
    fn end_write(&mut self) -> bool {
        self.state.end_committed = false;
        if !self.flush() {
            return false;
        }
        let mut committed = false;
        let ok = self.cancellation.step(|| {
            let done = self.base.end_write();
            committed = done || self.base.last_write_published();
            done
        });
        self.state.end_committed = committed;
        ok
    }
    fn last_write_published(&self) -> bool {
        self.state.end_committed || self.base.last_write_published()
    }
    fn producer_can_begin(&self) -> bool {
        self.base.producer_can_begin()
    }
    fn producer_remaining_credit(&self) -> Option<u64> {
        self.base
            .producer_remaining_credit()
            .map(|credit| credit.saturating_sub(self.buffered_bytes() as u64))
    }
    #[cfg(feature = "page-turn-trace")]
    fn trace_micros(&self) -> u32 {
        self.base.trace_micros()
    }
    #[cfg(feature = "page-turn-trace")]
    fn trace_segment_begin(&mut self, idx: u32) {
        self.base.trace_segment_begin(idx);
    }
    #[cfg(feature = "page-turn-trace")]
    fn trace_add_tee_us(&mut self, us: u32) {
        self.base.trace_add_tee_us(us);
    }
    #[cfg(feature = "page-turn-trace")]
    fn trace_segment_layout_done(&mut self) {
        self.base.trace_segment_layout_done();
    }
    #[cfg(feature = "page-turn-trace")]
    fn trace_segment_end(&mut self, pages: u32) {
        self.base.trace_segment_end(pages);
    }
    // Inherit CacheStorage's unsupported read_back_at, as the frozen Store does.
}

/// One source loan covers one guarded Store view. The original source is not
/// exposed: current chapters lend validation only; staged chapters validate the
/// cancellable wrapper, just like frozen reader.source_ during produceNext.
pub(crate) fn with_source_store<'window, 'ctx, 'pause, C: ?Sized, S: CacheStorage, F: newt_core::zip_window::source::BookSource + newt_core::producer_policy::ProducerReceipt + ?Sized, R>(
    store: &mut ProducerStore<'_, 'window, 'ctx, 'pause, C, S>,
    source: &mut newt_core::producer_policy::ProducerBookSource<'_, 'ctx, C, F>,
    bytes: u64, wrapped: bool, attempted: Option<&mut bool>,
    operation: impl FnOnce(&mut ProducerStore<'_, 'window, 'ctx, 'pause, C, dyn CacheStorage + '_>) -> R,
) -> R {
    source.with_checks(bytes, wrapped, |check| store.with_guarded(
        attempted.map(|flag| (check, flag)), operation))
}

impl<C: ?Sized, S: crate::phase_storage::ReadScope> crate::phase_storage::ReadScope
    for ProducerStore<'_, '_, '_, '_, C, S>
{
    fn invalidate_index(&mut self, digest: &[u8; 32]) -> bool {
        self.state.used == 0 && self.cancellation.checkpoint() && self.base.invalidate_index(digest)
    }
    #[inline(always)]
    fn query<R>(&mut self, name: &[u8], opening: bool, scratch: &mut [u8],
        live: &mut dyn FnMut() -> bool, read: impl FnOnce(&mut dyn CacheStorage) -> R,
    ) -> Result<(R, bool), crate::zip_container::Status> {
        if self.state.used != 0 { return Err(crate::zip_container::Status::Unsupported); }
        let cancellation = self.cancellation;
        self.base.query(name, opening, scratch, &mut || cancellation.checkpoint() && live(), read)
    }
    #[inline(always)]
    fn query_index<R>(&mut self, name: &[u8], digest: &[u8; 32], opening: bool, scratch: &mut [u8],
        live: &mut dyn FnMut() -> bool, read: impl FnOnce(&mut dyn CacheStorage) -> R,
    ) -> Result<(R, bool), crate::zip_container::Status> {
        if self.state.used != 0 { return Err(crate::zip_container::Status::Unsupported); }
        let cancellation = self.cancellation;
        self.base.query_index(name, digest, opening, scratch, &mut || cancellation.checkpoint() && live(), read)
    }
}
