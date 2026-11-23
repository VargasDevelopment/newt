//! Frozen chapter_cache_producer.h:218-275, staged selection only. MIT.
#![forbid(unsafe_code)]
use super::{PhasedReader, Writing, reader_parameters};
use crate::{phase_storage::{Phase, producer::{Producer, WriterView}}, resources, watch::Verdict};
use newt_core::{admission::{ChapterSource, WorkPhase, WorkScope}, cache_names::segment::{MAX_SEGMENTS, PAGES}, chapter_layout::{engine::EngineState, session::{ChapterSession, LayoutOperation, OperationHandler}, types::{LineBreak, SplitBackend}}, css::CssStylesheetBuilder, page_cache::{CacheStorage, WriterMemory}, xml_session::{self, BorrowedPolicy, ProducerBackend, XmlStream}, zip_container::{Entry, Status}, zip_window::source::BookSource};
use newt_reader_atlas::ReaderAtlas;

/// Only copied facts cross a parser/window return; no bank view escapes.
#[derive(Clone, Copy, Debug)]
pub struct NextPass {
    pub status: Status,
    pub pages: u32,
    pub chars: u32,
    pub restyle: bool,
}
struct Parsed {
    status: Status,
    pages: u32,
    chars: u32,
    verdict: Verdict,
}
impl<C: ?Sized, S: CacheStorage, F: BookSource + newt_core::producer_policy::ProducerReceipt + ?Sized> Producer<'_, '_, '_, C, S, F> {
    /// The enclosing scheduler already admitted the cache/SDK lease. Selection
    /// stages the inactive entry and never publishes reader metadata or plan.
    pub fn select_next<B: ProducerBackend + ?Sized>(&mut self, reader: &mut PhasedReader<'_, '_>,
        spine: u32, backend: &mut B,
    ) -> Status {
        self.with_unbuffered(|phase, a| {
            reader.select_physical(a.source, spine, phase, backend, false)?;
            Ok(if phase.ready(spine) { Status::Ok } else { Status::OutOfMemory })
        }).unwrap_or_else(|status| status)
    }

    /// Counting has the full bank. A writing window dies before settlement;
    /// the same Layout work scope includes both, as in frozen streamNext.
    pub(crate) fn next_pass<B: ProducerBackend + ?Sized>(&mut self, reader: &PhasedReader<'_, '_>,
        spine: u32, backend: &mut B, breaker: &mut dyn LineBreak, prepare: bool, memory: &mut WriterMemory<'_>,
    ) -> Result<NextPass, Status> {
        let _work = WorkScope::new_scoped(reader.on_yield, reader.work, WorkPhase::Layout);
        let parsed = self.with_pass(!prepare, |phase, mut a| {
            a.with_memory(memory, |write| write.stream_next(reader, phase, backend, breaker, prepare))
        })?;
        let (status, restyle) = self.with_unbuffered(|phase, a| {
            phase.settle_watched(a.source, backend, &reader.selections[reader.active ^ 1],
                spine, parsed.verdict, parsed.status, reader.on_yield)
        })?;
        Ok(NextPass { status, pages: parsed.pages, chars: parsed.chars, restyle })
    }
}

#[path = "producer_next_finish.rs"]
mod finish;
impl<C: ?Sized, S: CacheStorage, F: BookSource + newt_core::producer_policy::ProducerReceipt + ?Sized> WriterView<'_, '_, '_, '_, '_, C, S, F> {
    fn stream_next<B: ProducerBackend + ?Sized>(&mut self, reader: &PhasedReader<'_, '_>,
        phase: &mut Phase<'_>, backend: &mut B, breaker: &mut dyn LineBreak, prepare: bool,
    ) -> Result<Parsed, Status> {
        let cancellation = self.writer.cancellation();
        let selected = &reader.selections[reader.active ^ 1];
        let mut parsed = phase.watched_layout(backend, breaker, |slot, lr, io, backing| {
            let mut failed = Parsed { status: Status::Ok, pages: 0, chars: 0,
                verdict: Verdict { state: slot.state, stopped: false, cleared: false } };
            if !prepare { self.begin_stream(); }
            let mut chapter = match ChapterSource::open(&mut *self.source, &selected.entry,
                resources::decoder(&mut io.decoder), reader.on_yield, Some(cancellation)) {
                Ok(chapter) => chapter,
                Err(status) => { failed.status = status; return Ok(failed); }
            };
            crate::phase_storage::with_style_watch(backing, &mut chapter, slot.state, |watch| {
                let raw = Entry::raw(selected.entry.uncompressed_size);
                let mut images = lr.images.for_chapter(&selected.href);
                let mut engine = match EngineState::new(reader_parameters(reader.landscape, Some(slot), selected.language),
                    resources::storage(&mut lr.storage), lr.breaker, &mut images, None,
                    CssStylesheetBuilder::default()) {
                    Ok(engine) => engine,
                    Err(_) => { failed.status = Status::OutOfMemory; return failed; }
                };
                let mut policy = cancellation;
                let stream = XmlStream::with_policy(&mut *watch, None, io.backend, BorrowedPolicy(&mut policy));
                let mut session = ChapterSession::new(stream);
                let mut counted = 0;
                let mut status = session.begin_state(&raw, xml_session::Resources {
                    buffer: io.xml_buffer.as_deref_mut(), filter: io.filter.as_deref_mut(),
                }, &engine);
                while status == Status::Ok && !session.done() && !self.writer.refused
                    && (prepare || !self.writer.segmented || self.writer.seen <= PAGES) {
                    status = session.step_with(&mut engine, 1, 1, |watch, parser, engine, operation| {
                        watch.with_source(|chapter| chapter.with_source(|source| {
                            crate::producer_store::with_source_store(self.store, source,
                                reader.source_bytes, true, self.guard_attempted.as_deref_mut(), |store| {
                                    let mut sink = Writing { writer: self.writer, store,
                                        memory: &mut self.memory, counted: if prepare { Some(&mut counted) } else { None } };
                                    let mut font = ReaderAtlas;
                                    let mut split = SplitBackend { font: &mut font, sink: &mut sink };
                                    match operation {
                                        LayoutOperation::Feed { bytes, last } => parser.feed(bytes, last,
                                            &mut OperationHandler { engine, backend: &mut split }),
                                        LayoutOperation::Finish => engine.finish(&mut split),
                                    }
                                })
                        }))
                    });
                    if session.stream.with_source(|watch, _| watch.verdict().stopped) { break; }
                    if !cancellation.result(status == Status::Ok) || !self.store.pause_point() {
                        status = Status::IoError; break;
                    }
                }
                let verdict = session.stream.with_source(|watch, _| watch.verdict());
                if status == Status::Ok && !verdict.stopped
                    && (prepare || !self.writer.segmented || self.writer.seen <= PAGES)
                    && !session.stream.with_source(|watch, _| watch.with_source(|chapter| chapter.finish())) {
                    status = Status::Truncated;
                }
                if counted > MAX_SEGMENTS as u32 * PAGES || (status == Status::Ok && self.writer.refused) {
                    status = Status::Unsupported;
                }
                let result = Parsed { status, pages: engine.pages, chars: engine.chars, verdict };
                session.abort();
                result
            })
        })??;
        if parsed.status == Status::Ok && !crate::producer_store::with_source_store(self.store,
            self.source, reader.source_bytes, true, self.guard_attempted.as_deref_mut(), |store| store.flush()) {
            parsed.status = Status::IoError;
        }
        self.end_stream();
        Ok(parsed)
    }
}
