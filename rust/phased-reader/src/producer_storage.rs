//! Producer controls in the existing SDK tail; scoped views of the parse bank.
#![forbid(unsafe_code)]
use super::{BANK_BYTES, BuildLoan, Parts, Phase, Storage};
use crate::{producer::{Route, Segments}, producer_store::{BUFFER_BYTES, Pause, ProducerStore, StoreState}, producer_writer::CappedWriter};
use newt_core::{cache_names::segment::{MAX_BYTES, MAX_SEGMENTS}, page_cache::{Anchor, CacheStorage, IndexChunk, WriterMemory}, producer_policy::{Cancellation, ProducerBookSource}, zip_container::Status, zip_window::source::BookSource};

/// One owner per raw region. No bank-derived view survives a phase callback.
pub struct Producer<'loan, 'ctx, 'pause, C: ?Sized, S: ?Sized, F: BookSource + newt_core::producer_policy::ProducerReceipt + ?Sized> {
    pub state: &'loan mut Segments,
    pub writer: &'loan mut CappedWriter<'loan, 'ctx, C>,
    pub source: &'loan mut ProducerBookSource<'loan, 'ctx, C, F>,
    base: &'loan mut S,
    cancellation: &'loan Cancellation<'ctx, C>,
    pause: Option<&'loan mut Pause<'pause, 'ctx, C, S>>,
    shared: &'loan mut [u8],
    parse: &'loan mut [u8],
    held: &'loan mut bool,
    index: &'loan mut [u8],
    store_bytes: &'loan mut [u8],
    store_state_bytes: &'loan mut [u8],
    guard_attempted: &'loan mut bool,
    source_guard: bool,
    guard_bytes: u64,
}
/// Store is the same adapter over the same base, initialized in its tail slot.
/// Its window and all parser/index views end before another pass can start.
pub struct Attempt<'pass, 'loan, 'ctx, 'pause, C: ?Sized, S: ?Sized, F: BookSource + newt_core::producer_policy::ProducerReceipt + ?Sized> {
    pub state: &'pass mut Segments,
    pub writer: &'pass mut CappedWriter<'loan, 'ctx, C>,
    pub store: &'pass mut ProducerStore<'pass, 'pass, 'ctx, 'pause, C, S>,
    pub source: &'pass mut ProducerBookSource<'loan, 'ctx, C, F>,
    index: &'pass mut [u8],
    stream_buffer: Option<&'pass mut [u8; BUFFER_BYTES]>,
    pub(crate) guard_attempted: Option<&'pass mut bool>,
    guard_bytes: u64,
}
/// The index alone is lent for less than a pass, then reused for the map.
pub struct WriterView<'write, 'pass, 'loan, 'ctx, 'pause, C: ?Sized, S: ?Sized, F: BookSource + newt_core::producer_policy::ProducerReceipt + ?Sized> {
    pub state: &'write mut Segments,
    pub writer: &'write mut CappedWriter<'loan, 'ctx, C>,
    pub store: &'write mut ProducerStore<'pass, 'pass, 'ctx, 'pause, C, S>,
    pub source: &'write mut ProducerBookSource<'loan, 'ctx, C, F>,
    pub memory: WriterMemory<'write>,
    pub(crate) stream_buffer: &'write mut Option<&'pass mut [u8; BUFFER_BYTES]>,
    pub(crate) guard_attempted: Option<&'write mut bool>,
}
impl<'loan, 'ctx, 'pause, C: ?Sized, S: CacheStorage + ?Sized, F: BookSource + newt_core::producer_policy::ProducerReceipt + ?Sized> Producer<'loan, 'ctx, 'pause, C, S, F> {
    /// The enclosing production path supplies the real Pages policy and source
    /// authority. This scope keeps source checks attached across every pass.
    pub fn with_source_guard<R>(&mut self, bytes: u64, operation: impl FnOnce(&mut Self) -> R) -> R {
        let previous = (self.source_guard, self.guard_bytes);
        self.source_guard = true;
        self.guard_bytes = bytes;
        let result = operation(self);
        (self.source_guard, self.guard_bytes) = previous;
        result
    }
    pub(crate) fn drop_styles(&mut self) { *self.held = false; }
    /// Keep the writer index across parse/window phases. Removing its raw
    /// region makes another writer or map loan unavailable until this returns.
    pub(crate) fn with_index<R>(&mut self,
        operation: impl for<'index> FnOnce(&mut Self, WriterMemory<'index>) -> Result<R, Status>,
    ) -> Result<R, Status> {
        let index = core::mem::take(&mut self.index);
        let result = (|| {
            let mut parts = Parts(&mut *index);
            let anchors = parts.array::<Anchor>(192)?;
            let chunks = parts.array::<IndexChunk>(1)?;
            operation(self, WriterMemory { chunks, anchors })
        })();
        self.index = index;
        result
    }
    pub fn with_buffered<R>(&mut self,
        operation: impl for<'pass> FnOnce(&mut Phase<'pass>, Attempt<'pass, 'loan, 'ctx, 'pause, C, S, F>) -> Result<R, Status>,
    ) -> Result<R, Status> { self.with_pass(true, operation) }
    pub fn with_unbuffered<R>(&mut self,
        operation: impl for<'pass> FnOnce(&mut Phase<'pass>, Attempt<'pass, 'loan, 'ctx, 'pause, C, S, F>) -> Result<R, Status>,
    ) -> Result<R, Status> { self.with_pass(false, operation) }
    pub(crate) fn with_pass<R>(&mut self, buffered: bool,
        operation: impl for<'pass> FnOnce(&mut Phase<'pass>, Attempt<'pass, 'loan, 'ctx, 'pause, C, S, F>) -> Result<R, Status>,
    ) -> Result<R, Status> {
        let (window, parse): (Option<&mut [u8; BUFFER_BYTES]>, &mut [u8]) = if buffered {
            let (window, parse) = self.parse.split_at_mut(BUFFER_BYTES);
            (Some(window.try_into().map_err(|_| Status::OutOfMemory)?), parse)
        } else { (None, &mut *self.parse) };
        let state = Parts(&mut *self.store_state_bytes).one(StoreState::new(None))?;
        let store = Parts(&mut *self.store_bytes).one(ProducerStore::new(&mut *self.base, self.cancellation, state, self.pause.as_deref_mut()))?;
        let mut phase = Phase { policy: Some(self.cancellation), shared: &mut *self.shared, parse, held: &mut *self.held };
        operation(&mut phase, Attempt { state: &mut *self.state, writer: &mut *self.writer,
            store, source: &mut *self.source, index: &mut *self.index, stream_buffer: window,
            guard_attempted: if self.source_guard { Some(&mut *self.guard_attempted) } else { None }, guard_bytes: self.guard_bytes })
    }
}
impl<'pass, 'loan, 'ctx, 'pause, C: ?Sized, S: CacheStorage + ?Sized, F: BookSource + newt_core::producer_policy::ProducerReceipt + ?Sized> Attempt<'pass, 'loan, 'ctx, 'pause, C, S, F> {
    pub(crate) fn with_memory<R>(&mut self, memory: &mut WriterMemory<'_>,
        operation: impl for<'write> FnOnce(&mut WriterView<'write, 'pass, 'loan, 'ctx, 'pause, C, S, F>) -> R,
    ) -> R {
        operation(&mut WriterView { state: &mut *self.state, writer: &mut *self.writer,
            store: &mut *self.store, source: &mut *self.source,
            memory: WriterMemory { chunks: &mut *memory.chunks, anchors: &mut *memory.anchors },
            stream_buffer: &mut self.stream_buffer, guard_attempted: self.guard_attempted.as_deref_mut() })
    }
    pub fn with_writer<R>(&mut self,
        operation: impl for<'write> FnOnce(&mut WriterView<'write, 'pass, 'loan, 'ctx, 'pause, C, S, F>) -> R,
    ) -> Result<R, Status> {
        let mut parts = Parts(&mut *self.index);
        let anchors = parts.array::<Anchor>(192)?;
        let chunks = parts.array::<IndexChunk>(1)?;
        Ok(operation(&mut WriterView { state: &mut *self.state, writer: &mut *self.writer,
            store: &mut *self.store, source: &mut *self.source,
            memory: WriterMemory { chunks, anchors }, stream_buffer: &mut self.stream_buffer, guard_attempted: self.guard_attempted.as_deref_mut() }))
    }
    /// Frozen writeMap resets only the index; Store's window stays attached.
    pub fn publish_map(&mut self, route: &mut Route<'_>, spine: u32) -> Status where S: Sized {
        self.publish_map_checked(route, spine, false)
    }
    /// Reclaim the idle 1 KiB store window if currently attached to `self.store`
    /// with zero buffered bytes, returning it to `self.stream_buffer`.
    pub(crate) fn reclaim_window(&mut self) -> bool {
        if self.store.buffered_bytes() != 0 {
            return false;
        }
        if self.stream_buffer.is_none() {
            if let Some(buf) = self.store.return_stream_buffer() {
                self.stream_buffer = Some(buf);
            }
        }
        self.stream_buffer.is_some()
    }
    /// Borrow `self.stream_buffer` and the idle 128-entry index chunk alongside
    /// the attempt fields for verified prefix-start collection while no writer is open.
    pub(crate) fn with_idle_window<R>(
        &mut self,
        f: impl FnOnce(
            &mut Segments,
            &mut CappedWriter<'loan, 'ctx, C>,
            &mut ProducerStore<'pass, 'pass, 'ctx, 'pause, C, S>,
            &mut ProducerBookSource<'loan, 'ctx, C, F>,
            &mut [u8; BUFFER_BYTES],
            &mut [newt_core::page_cache::Entry; 128],
            bool,
        ) -> R,
    ) -> Option<R> {
        if !self.reclaim_window() {
            return None;
        }
        let window = self.stream_buffer.as_deref_mut()?;
        let mut parts = Parts(&mut *self.index);
        let _anchors = parts.array::<Anchor>(192).ok()?;
        let chunks = parts.array::<IndexChunk>(1).ok()?;
        let entries = &mut chunks.get_mut(0)?.entries;
        let guarded = self.guard_attempted.is_some();
        Some(f(
            &mut *self.state,
            &mut *self.writer,
            &mut *self.store,
            &mut *self.source,
            window,
            entries,
            guarded,
        ))
    }
    pub(crate) fn publish_map_checked(&mut self, route: &mut Route<'_>, spine: u32, wrapped: bool) -> Status where S: Sized {
        let record = self.index.get_mut(..MAX_BYTES).and_then(|bytes| bytes.try_into().ok());
        crate::producer_store::with_source_store(self.store, self.source, self.guard_bytes, wrapped,
            self.guard_attempted.as_deref_mut(), |store| self.state.write_map(self.writer, store, record, route, spine))
    }
}
impl<C: ?Sized, S: CacheStorage + ?Sized, F: BookSource + newt_core::producer_policy::ProducerReceipt + ?Sized> WriterView<'_, '_, '_, '_, '_, C, S, F> {
    pub fn begin_stream(&mut self) {
        if let Some(buffer) = self.stream_buffer.take() { self.store.use_stream_buffer(buffer); }
    }
    pub(crate) fn end_stream(&mut self) {
        if let Some(buffer) = self.store.return_stream_buffer() { *self.stream_buffer = Some(buffer); }
    }
}
impl Storage<'_> {
    /// One SDK lend/return encloses every pass; return poisons its frame.
    /// The owner must repaint the whole retained plan before publication.
    pub fn with_producer<'ctx, 'pause, C: ?Sized, S: CacheStorage + ?Sized, F: BookSource + newt_core::producer_policy::ProducerReceipt + ?Sized, R>(
        &mut self, lender: &mut impl BuildLoan, cancellation: &Cancellation<'ctx, C>,
        base: &mut S, source: &mut F, certified_pages: u32,
        pause: Option<&mut Pause<'pause, 'ctx, C, S>>,
        operation: impl for<'loan> FnOnce(&mut Producer<'loan, 'ctx, 'pause, C, S, F>) -> Result<R, Status>,
    ) -> Result<R, Status> {
        lender.with_parse(|native| {
            let padding = native.as_ptr().align_offset(16);
            let native = native.get_mut(padding..newt_phased_sink::FRAME_BYTES).ok_or(Status::OutOfMemory)?;
            if native.len() < BANK_BYTES { return Err(Status::OutOfMemory); }
            let (parse, tail) = native.split_at_mut(BANK_BYTES);
            let mut parts = Parts(tail);
            let state = parts.one(Segments::new(certified_pages))?;
            let starts = parts.array::<u32>(MAX_SEGMENTS)?.try_into().map_err(|_| Status::OutOfMemory)?;
            let index = parts.bytes(192 * core::mem::size_of::<Anchor>() + core::mem::size_of::<IndexChunk>(), core::mem::align_of::<IndexChunk>())?;
            let writer = parts.one(CappedWriter::new(cancellation, starts))?;
            let store_state_bytes = parts.bytes(core::mem::size_of::<StoreState<'_>>(), core::mem::align_of::<StoreState<'_>>())?;
            let store_bytes = parts.bytes(core::mem::size_of::<ProducerStore<'_, '_, '_, '_, C, S>>(), core::mem::align_of::<ProducerStore<'_, '_, '_, '_, C, S>>())?;
            let source = parts.one(ProducerBookSource::new(source, cancellation))?;
            let guard_attempted = parts.one(false)?;
            operation(&mut Producer { state, writer, source, base, cancellation, pause,
                shared: &mut *self.shared, parse, held: &mut self.held, index, store_bytes, store_state_bytes, guard_attempted, source_guard: false, guard_bytes: 0 })
        })?
    }
}

#[cfg(test)]
#[path = "producer_storage_cases.rs"]
mod tests;
