//! Single-chapter streaming producer pass for chapter cache generation.
#![forbid(unsafe_code)]
use super::super::super::{PhasedReader, reader_parameters};
use crate::{phase_storage::{Phase, producer::{Attempt, WriterView}}, producer::{Route}, producer_writer::CappedWriter, resources};
use newt_core::{admission::{ChapterSource, ScheduledSource, WorkPhase, WorkScope}, chapter_layout::{engine::EngineState, session::{ChapterSession, LayoutOperation, OperationHandler}, types::{LineBreak, Page, Sink, SplitBackend}}, css::CssStylesheetBuilder, page_cache::{CacheStorage, WriterMemory}, xml_session::{self, BorrowedPolicy, ProducerBackend, XmlStream}, zip_container::{Entry, Status}, zip_window::source::BookSource};
use newt_reader_atlas::ReaderAtlas;

struct Writing<'a, 'index, 'loan, 'ctx, C: ?Sized, S> {
    writer: &'a mut CappedWriter<'loan, 'ctx, C>,
    store: &'a mut S,
    memory: &'a mut WriterMemory<'index>,
    counted: Option<&'a mut u32>,
}
impl<C: ?Sized, S: CacheStorage> Sink for Writing<'_, '_, '_, '_, C, S> {
    fn page(&mut self, page: Page<'_>) -> bool {
        let Some(pages) = self.counted.as_deref_mut() else {
            return self.writer.on_page(self.store, self.memory, &page);
        };
        use newt_core::cache_names::segment::{MAX_PAGES, MAX_SEGMENTS, PAGES};
        if *pages % PAGES == 0 && (*pages / PAGES) < MAX_SEGMENTS as u32 {
            self.writer.starts[(*pages / PAGES) as usize] = page.char_start;
        }
        *pages += 1;
        if *pages > MAX_PAGES {
            return self.writer.cancellation().result(false);
        }
        self.writer.cancellation().checkpoint()
    }
    fn anchor(&mut self, hash: u32, chars: u32) {
        if self.counted.is_none() { self.writer.on_anchor(self.memory, hash, chars); }
    }
}
impl<C: ?Sized, S: CacheStorage, F: BookSource + newt_core::producer_policy::ProducerReceipt + ?Sized> Attempt<'_, '_, '_, '_, C, S, F> {
    /// Frozen produce: complete segments before reusing their index for a map.
    pub fn produce_segments_and_map<B: ProducerBackend + ?Sized>(
        &mut self, reader: &PhasedReader<'_, '_>, phase: &mut Phase<'_>,
        backend: &mut B, breaker: &mut dyn LineBreak, route: &mut Route<'_>,
    ) -> Status {
        let status = self.with_writer(|write| write.produce_segments(reader, phase, backend, breaker, route))
            .unwrap_or_else(|status| status);
        if status != Status::Ok || !self.writer.mapping { return status; }
        self.publish_map(route, reader.info.spine)
    }
}
impl<C: ?Sized, S: CacheStorage, F: BookSource + newt_core::producer_policy::ProducerReceipt + ?Sized> WriterView<'_, '_, '_, '_, '_, C, S, F> {
    /// Frozen produceSegments370-385. Phase::layout already returns the
    /// settled style home when a stream ends; re-arm only the same buffer here.
    pub fn produce_segments<B: ProducerBackend + ?Sized>(
        &mut self, reader: &PhasedReader<'_, '_>, phase: &mut Phase<'_>,
        backend: &mut B, breaker: &mut dyn LineBreak, route: &mut Route<'_>,
    ) -> Status {
        for pass in 0..2 {
            self.state.next = if pass == 0 { 0 } else { self.state.skipped };
            self.state.skipped = u32::MAX;
            self.writer.seen = 0;
            let source_valid = self.source.validate_source(reader.source_bytes);
            if !source_valid { return Status::IoError; }
            let mut status = if self.guard_attempted.is_some() {
                crate::producer_store::with_source_store(self.store, self.source, reader.source_bytes,
                    false, self.guard_attempted.as_deref_mut(), |store|
                        self.state.boundary(self.writer, store, &mut self.memory, route))
            } else { self.state.boundary(self.writer, self.store, &mut self.memory, route) };
            if status == Status::Ok && (self.writer.open || self.writer.mapping) {
                status = self.finish_current(reader, phase, backend, breaker, route);
            }
            if status != Status::Ok || self.state.skipped == u32::MAX { return status; }
            self.end_stream();
        }
        Status::Unsupported
    }

    /// Frozen finishWhole: close parser, then validate the original source
    /// before allowing the only/final writer to publish its certified totals.
    pub fn finish_current<B: ProducerBackend + ?Sized>(
        &mut self, reader: &PhasedReader<'_, '_>, phase: &mut Phase<'_>,
        backend: &mut B, breaker: &mut dyn LineBreak, route: &mut Route<'_>,
    ) -> Status {
        let status = self.stream_current(reader, phase, backend, breaker, route);
        if status != Status::Ok || (self.writer.segmented && !self.writer.open) {
            return status;
        }
        if !self.source.validate_source(reader.source_bytes) { return Status::IoError; }
        self.writer.open = false;
        self.writer.cache.set_total_chars(reader.info.total_chars);
        if crate::producer_store::with_source_store(self.store, self.source, reader.source_bytes,
            false, self.guard_attempted.as_deref_mut(), |store|
                self.writer.cache.finish(store, &self.state.name[..69], &self.memory)) {
            Status::Ok
        } else {
            Status::IoError
        }
    }

    /// The enclosing owner supplies the current certified route and settled
    /// styles. This pass retains neither selection nor parser and publishes no
    /// page. The writer remains lent until the owner finishes or aborts it.
    pub fn stream_current<B: ProducerBackend + ?Sized>(
        &mut self, reader: &PhasedReader<'_, '_>, phase: &mut Phase<'_>,
        backend: &mut B, breaker: &mut dyn LineBreak, route: &mut Route<'_>,
    ) -> Status {
        let cancellation = self.writer.cancellation();
        let selected = if self.state.staged { &reader.selections[reader.active ^ 1] }
            else { reader.selection() };
        phase.layout(backend, breaker, |slot, lr, io| {
            let _work = WorkScope::new_scoped(reader.on_yield, reader.work, WorkPhase::Layout);
            let params = reader_parameters(reader.page_layout, Some(slot), selected.language);
            self.begin_stream();
            let mut scheduled = ScheduledSource::new(&mut *self.source, reader.on_yield, Some(cancellation));
            let mut chapter = match ChapterSource::open(&mut scheduled, &selected.entry,
                resources::decoder(&mut io.decoder), reader.on_yield,
                Some(cancellation),
                ) {
                Ok(chapter) => chapter,
                Err(status) => { cancellation.result(false); return status; }
            };
            let raw = Entry::raw(selected.entry.uncompressed_size);
            let mut images = lr.images.for_chapter(&selected.href);
            let mut engine = match EngineState::new(params, resources::storage(&mut lr.storage),
                lr.breaker, &mut images, None, CssStylesheetBuilder::default()) {
                Ok(engine) => engine,
                Err(_) => { cancellation.result(false); return Status::OutOfMemory; }
            };
            let mut policy = cancellation;
            let stream = XmlStream::with_policy(&mut chapter, None, io.backend, BorrowedPolicy(&mut policy));
            let mut session = ChapterSession::new(stream);
            let mut status = session.begin_state(&raw, xml_session::Resources {
                buffer: io.xml_buffer.as_deref_mut(), filter: io.filter.as_deref_mut(),
            }, &engine);
            while status == Status::Ok && !session.done() && !self.writer.refused {
                status = session.step_with(&mut engine, 1, 1, |chapter, parser, engine, operation| {
                    chapter.with_source(|scheduled| {
                        crate::producer_store::with_source_store(self.store, &mut *scheduled.source,
                            reader.source_bytes, false, self.guard_attempted.as_deref_mut(), |store| {
                                let mut sink = Writing { writer: self.writer, store, memory: &mut self.memory, counted: None };
                                let mut font = ReaderAtlas;
                                let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
                                match operation {
                                    LayoutOperation::Feed { bytes, last } => parser.feed(bytes, last,
                                        &mut OperationHandler { engine, backend: &mut backend }),
                                    LayoutOperation::Finish => engine.finish(&mut backend),
                                }
                            })
                    })
                });
                if !cancellation.result(status == Status::Ok) { break; }
                if !self.store.pause_point() { status = Status::IoError; break; }
                if self.writer.segmented {
                    status = if self.guard_attempted.is_some() {
                        session.stream.with_source(|chapter, _| chapter.with_source(|scheduled| {
                            crate::producer_store::with_source_store(self.store, &mut *scheduled.source,
                                reader.source_bytes, false, self.guard_attempted.as_deref_mut(), |store|
                                    self.state.boundary(self.writer, store, &mut self.memory, route))
                        }))
                    } else { self.state.boundary(self.writer, self.store, &mut self.memory, route) };
                    if !self.writer.open && !self.writer.mapping { break; }
                }
            }
            let whole = !self.writer.segmented || self.writer.open || self.writer.mapping;
            if status == Status::Ok && whole && !self.writer.refused
                && !session.stream.with_source(|chapter, _| chapter.finish()) {
                status = Status::Truncated;
            }
            if status == Status::Ok && whole && (self.writer.refused
                || engine.pages != reader.info.page_count || engine.chars != reader.info.total_chars) {
                status = Status::Unsupported;
            }
            cancellation.result(status == Status::Ok);
            session.abort();
            status
        }).unwrap_or_else(|status| status)
    }
}

#[path = "producer_next.rs"]
mod next;
pub use next::NextPass;


#[path = "producer_current.rs"]
mod current;
