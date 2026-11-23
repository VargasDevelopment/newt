//! Chapter page cache writer half. MIT.
//! Child of reader.rs: private selection/validation stay inside the reader.
use super::*;
use newt_core::{
    chapter_layout::types::{Page, Sink, SplitBackend},
    page_cache::{CacheStorage, PageCacheWriter, Profile, WriterMemory},
};

include!("cache_build_result.rs");

// One operation lends the same authenticated source to sequential XML reads
// and storage probes. Runtime loans end at each read/probe; no arena lifetime,
// retained source, second file handle or heap allocation is introduced.
struct SourceView<'loan, 'source, S: ?Sized>(&'loan core::cell::RefCell<&'source mut S>);
impl<S: BookSource + ?Sized> BookSource for SourceView<'_, '_, S> {
    fn size(&self) -> u64 {
        self.0.try_borrow().map_or(0, |source| source.size())
    }
    fn read_at(&mut self, offset: u64, output: &mut [u8]) -> i32 {
        match self.0.try_borrow_mut() {
            Ok(mut source) => source.read_at(offset, output),
            Err(_) => -1,
        }
    }
}
use crate::guarded_store::GuardedStore;
struct WritingSink<'a, 'm, S> {
    writer: &'a mut PageCacheWriter,
    store: &'a mut S,
    memory: &'a mut WriterMemory<'m>,
    failed: bool,
}
impl<S: CacheStorage> Sink for WritingSink<'_, '_, S> {
    fn page(&mut self, page: Page<'_>) -> bool {
        if !self.failed {
            self.failed = !self.writer.on_page(self.store, self.memory, &page);
        }
        true // Writer failure must never stop XML or certify a prefix.
    }
    fn anchor(&mut self, hash: u32, chars: u32) {
        if !self.failed {
            self.writer.on_anchor(self.memory, hash, chars);
        }
    }
}
impl CacheBuild<'_> {
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    pub fn build<S: BookSource + ?Sized, B: ParserBackend + ?Sized, C: CacheStorage>(
        &self,
        reader: &mut PhasedReader<'_, '_>,
        source: &mut S,
        spine: u32,
        store: &mut C,
        memory: &mut WriterMemory<'_>,
        metadata: &mut Arena<'_>,
        parse: &mut Arena<'_>,
        r: &mut canonical::Resources<'_, B>,
        toc: &mut [TocEntry],
        sr: &mut StyleResources<'_, '_>,
        sources: &mut Sources<'_>,
        slot: &mut Slot<'_>,
        lr: &mut LayoutResources<'_>,
        lease: &Lease,
    ) -> BuildResult {
        if !reader.opened || reader.plan.is_none() || spine >= reader.selection().spines {
            return BuildResult::status(Status::NotFound);
        }
        // Never overwrite an in-flight staged publication. Writer memory is a
        // separate typed loan; native plan/index phase rebinding is not claimed.
        if reader.pending.is_some() {
            return BuildResult::status(Status::Unsupported);
        }
        if self.name.is_empty() || self.name.contains(&0) {
            return BuildResult::status(Status::IoError);
        }
        let loan = core::cell::RefCell::new(source);
        let mut source = SourceView(&loan);
        let mut guard_source = SourceView(&loan);
        let bytes = reader.source_bytes;
        let mut check = |kind| matches!(kind, newt_core::producer_policy::ProducerCheck::Receipt)
            || validate(&mut guard_source, bytes) == Status::Ok;
        let mut attempted = false;
        let mut store = GuardedStore::new(store, &mut check, &mut attempted);
        let mut writer = PageCacheWriter::new(Profile::Small);
        let mut again = false;
        loop {
            // begin() resets all active index counts; borrowed backing is reused.
            if !writer.begin(&mut store, self.name, self.generation, memory) {
                store.remove(self.name);
                return BuildResult::status(Status::OutOfMemory);
            }
            let mut sink = WritingSink {
                writer: &mut writer,
                store: &mut store,
                memory,
                failed: false,
            };
            let result = reader.cache_stream(
                &mut source,
                spine,
                again,
                &mut sink,
                metadata,
                parse,
                r,
                toc,
                sr,
                sources,
                slot,
                lr,
                lease,
            );
            let failed = sink.failed;
            drop(sink);
            let (status, pages, chars, retry) = match result {
                Ok(v) => v,
                Err(e) => (e, 0, 0, false),
            };
            if status == Status::Ok && retry {
                again = true;
                continue; // begin_write restarts/truncates the stopped style pass.
            }
            if status == Status::Ok && !failed && pages == 0 {
                store.remove(self.name);
                return BuildResult {
                    status: Status::NotFound,
                    verified_empty: true,
                };
            }
            if status != Status::Ok || failed || writer.page_count() != pages {
                store.remove(self.name);
                return BuildResult::status(if status != Status::Ok {
                    status
                } else {
                    Status::OutOfMemory
                });
            }
            writer.set_total_chars(chars);
            return BuildResult::status(if writer.finish(&mut store, self.name, memory) {
                Status::Ok
            } else {
                Status::IoError
            });
        }
    }
}
impl PhasedReader<'_, '_> {
    #[allow(clippy::too_many_arguments)]
    fn cache_stream<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
        &mut self,
        source: &mut S,
        spine: u32,
        again: bool,
        sink: &mut dyn Sink,
        metadata: &mut Arena<'_>,
        parse: &mut Arena<'_>,
        r: &mut canonical::Resources<'_, B>,
        toc: &mut [TocEntry],
        sr: &mut StyleResources<'_, '_>,
        sources: &mut Sources<'_>,
        slot: &mut Slot<'_>,
        lr: &mut LayoutResources<'_>,
        lease: &Lease,
    ) -> Result<(Status, u32, u32, bool), Status> {
        let status = self.validate(source);
        if status != Status::Ok {
            return Err(status);
        }
        let held = lease.hold()?;
        let selected = &mut self.selections[self.active ^ 1];
        if !again {
            // The actual canonical Book/select/style path; never copy active
            // selection or publish the inactive one merely to prepare a cache.
            self.styles_ready = false;
            let selection = select(
                source,
                spine,
                selected,
                metadata,
                r,
                toc,
                sr,
                sources,
                slot,
                self.on_yield,
                self.work,
            );
            metadata.reset();
            parse.reset();
            self.undeclared_styles = selection?;
            self.styles_ready = slot.state != StyleState::Empty;
        }
        if slot.spine != spine || slot.state == StyleState::Empty {
            return Err(Status::OutOfMemory);
        }
        let result = {
            let _work = WorkScope::new_scoped(self.on_yield, self.work, WorkPhase::Layout);
            cache_pass(
                source,
                selected,
                self.landscape,
                slot,
                lr,
                r,
                sink,
                self.on_yield,
            )
        };
        // cache_pass has aborted/dropped Session, Engine, Watch and ChapterSource.
        metadata.reset();
        parse.reset();
        let (mut status, pages, chars, verdict) = result?;
        let mut retry = false;
        if verdict.stopped {
            status = settle_stopped_style(
                source,
                selected,
                spine,
                metadata,
                r,
                sr,
                sources,
                slot,
                verdict,
                self.on_yield,
            );
            retry = status == Status::Ok;
            self.styles_ready = retry;
        } else if status == Status::Ok && slot.issues & styles::MALFORMED != 0 {
            self.styles_ready = false;
            status = Status::OutOfMemory;
        } else if status == Status::Ok && verdict.cleared {
            slot.state = StyleState::Settled;
        }
        drop(held); // parser teardown precedes lease return; guard follows it.
        if status == Status::Ok && !retry {
            status = self.validate(source);
        }
        Ok((status, pages, chars, retry))
    }
}
#[allow(clippy::too_many_arguments)]
fn cache_pass<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    source: &mut S,
    selected: &Selection,
    landscape: bool,
    slot: &Slot<'_>,
    lr: &mut LayoutResources<'_>,
    r: &mut canonical::Resources<'_, B>,
    sink: &mut dyn Sink,
    on_yield: newt_core::admission::Yield<'_>,
) -> Result<(Status, u32, u32, crate::watch::Verdict), Status> {
    let mut scheduled = ScheduledSource::new(source, on_yield, None);
    let mut chapter = ChapterSource::open(
        &mut scheduled,
        &selected.entry,
        resources::decoder(&mut r.decoder),
        on_yield,
        None,
    )?;
    let mut watch = Watch::begin(&mut chapter, slot.state);
    let raw = Entry::raw(selected.entry.uncompressed_size);
    let (mut status, pages, chars, complete) = {
        let mut font = ReaderAtlas;
        let mut backend = SplitBackend {
            font: &mut font,
            sink,
        };
        let mut images = lr.images.for_chapter(&selected.href);
        let mut engine = Engine::new(
            reader_parameters(landscape, Some(slot), selected.language),
            resources::storage(&mut lr.storage),
            &mut backend,
            lr.breaker,
            &mut images,
            None,
            CssStylesheetBuilder::default(),
        )
        .map_err(|_| Status::OutOfMemory)?;
        let stream = XmlStream::new(&mut watch, None, r.backend);
        let mut session = ChapterSession::new(stream);
        let mut status = session.begin(
            &raw,
            xml_session::Resources {
                buffer: r.xml_buffer.as_deref_mut(),
                filter: r.filter.as_deref_mut(),
            },
            &engine,
        );
        while status == Status::Ok && !session.done() {
            status = session.step(
                &mut LayoutHandler {
                    engine: &mut engine,
                },
                1,
            );
            if status == Status::Ok && !session.done() {
                if let Some(f) = on_yield {
                    f.yield_now();
                }
            }
        }
        // Done alone also includes Engine's early-stop branch. It cannot
        // certify XML; require no early stop plus ChapterSource::finish below.
        let complete = session.done() && !engine.stopped;
        let pages = engine.pages;
        let chars = engine.chars;
        session.abort();
        (status, pages, chars, complete)
    };
    let verdict = watch.verdict();
    drop(watch);
    if status == Status::Ok && (!complete || !chapter.finish()) {
        status = Status::Truncated;
    }
    // Stopped Watch is settled by the owner; no success leaves this pass as final.
    Ok((status, pages, chars, verdict))
}


/// Owns writer state, never a source or a layout Backend. Every guard uses the
/// original authenticated source for one synchronous operation only.
struct PhysicalWriter<'a, 'm, C> {
    writer: PageCacheWriter,
    store: &'a mut C,
    memory: &'a mut WriterMemory<'m>,
    bytes: u64,
    attempted: bool,
}
impl<C: CacheStorage> PhysicalWriter<'_, '_, C> {
    fn guarded<S: BookSource + ?Sized, R>(
        &mut self,
        source: &mut S,
        operation: impl FnOnce(&mut PageCacheWriter, &mut GuardedStore<'_, '_, C>, &mut WriterMemory<'_>) -> R,
    ) -> R {
        let mut check = |kind| matches!(kind, newt_core::producer_policy::ProducerCheck::Receipt)
            || validate(source, self.bytes) == Status::Ok;
        let mut store = GuardedStore::new(&mut *self.store, &mut check, &mut self.attempted);
        operation(&mut self.writer, &mut store, self.memory)
    }
    fn layout<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
        &mut self,
        source: &mut S,
        parser: &mut B,
        engine: &mut newt_core::chapter_layout::engine::EngineState<'_>,
        operation: newt_core::chapter_layout::session::LayoutOperation<'_>,
    ) -> bool {
        use newt_core::chapter_layout::session::{LayoutOperation, OperationHandler};
        let result = self.guarded(source, |writer, store, memory| {
            let failed = writer.failed();
            let mut sink = WritingSink { writer, store, memory, failed };
            let mut font = ReaderAtlas;
            let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
            let result = match operation {
                LayoutOperation::Feed { bytes, last } => parser.feed(bytes, last,
                    &mut OperationHandler { engine, backend: &mut backend }),
                LayoutOperation::Finish => engine.finish(&mut backend),
            };
            result
        });
        result
    }
}
impl CacheBuild<'_> {
    // Both entry points reject before identity or storage effects.
    pub(crate) fn physical_ready(reader: &PhasedReader<'_, '_>, spine: u32) -> Result<(), Status> {
        if !reader.opened || reader.plan.is_none() || spine >= reader.selection().spines {
            return Err(Status::NotFound);
        }
        if reader.has_page || reader.pending.is_some() {
            return Err(Status::Unsupported);
        }
        Ok(())
    }
    /// Physical build: canonical selection, actual bank loans and real writer.
    /// The inactive selection is scratch only; no pending/page/proof is published.
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    pub fn build_phased<S: BookSource + ?Sized, B: ParserBackend + ?Sized, C: CacheStorage>(
        &self,
        reader: &mut PhasedReader<'_, '_>,
        source: &mut S,
        spine: u32,
        store: &mut C,
        storage: &mut crate::phase_storage::Storage<'_>,
        lender: &mut impl crate::phase_storage::BuildLoan,
        backend: &mut B,
        breaker: &mut dyn newt_core::chapter_layout::types::LineBreak,
    ) -> BuildResult {
        if let Err(status) = Self::physical_ready(reader, spine) {
            return BuildResult::status(status);
        }
        if self.name.is_empty() || self.name.contains(&0) {
            return BuildResult::status(Status::IoError);
        }
        // Detach the plan loan only for the callback, permitting select_physical
        // to borrow the reader without a self-reference or a second plan owner.
        let plan = reader.plan.take().expect("checked plan");
        reader.proofs.plan = false;
        // A build may replace the style home for an inactive selection. Future
        // ordinary layout must establish its own matching style readiness.
        reader.styles_ready = false;
        let result = crate::phase_storage::with_writer_index(plan, |memory| {
            let mut writer = PhysicalWriter {
                writer: PageCacheWriter::new(Profile::Small), store, memory,
                bytes: reader.source_bytes, attempted: false,
            };
            let mut again = false;
            let mut passes = 0;
            loop {
                passes += 1;
                if passes > 3 {
                    writer.guarded(source, |_, store, _| store.remove(self.name));
                    return BuildResult::status(Status::OutOfMemory);
                }
                if !writer.guarded(source, |w, store, m| w.begin(store, self.name, self.generation, m)) {
                    writer.guarded(source, |_, store, _| store.remove(self.name));
                    return BuildResult::status(Status::OutOfMemory);
                }
                // Selection, layout and settlement are inside the ACTUAL loan.
                let pass = storage.with_loan(lender, |phase| {
                    let status = reader.validate(source);
                    if status != Status::Ok { return Err(status); }
                    if !again {
                        reader.select_physical(source, spine, phase, backend, false)?;
                    }
                    if !phase.ready(spine) { return Err(Status::OutOfMemory); }
                    let selected = &reader.selections[reader.active ^ 1];
                    let result = {
                        let _work = WorkScope::new_scoped(reader.on_yield, reader.work, WorkPhase::Layout);
                        phase.layout(backend, breaker, |slot, lr, io| {
                            physical_cache_pass(source, selected, reader.landscape, slot,
                                lr, io, &mut writer, reader.on_yield)
                        })??
                    };
                    let (status, pages, chars, verdict) = result;
                    if verdict.stopped {
                        let settled = phase.settle(source, backend, selected, spine, verdict, reader.on_yield)?;
                        return if settled == Status::Ok { Ok((0, 0, true)) } else { Err(settled) };
                    }
                    if status != Status::Ok { return Err(status); }
                    if phase.edit_slot(|slot| slot.issues & styles::MALFORMED != 0)? {
                        return Err(Status::OutOfMemory);
                    }
                    if verdict.cleared {
                        phase.edit_slot(|slot| slot.state = StyleState::Settled)?;
                    }
                    Ok((pages, chars, false))
                }); // parser/source/bank views end; lender really returns here
                let (pages, chars, retry) = match pass {
                    Ok(v) => v,
                    Err(e) => {
                        writer.guarded(source, |_, store, _| store.remove(self.name));
                        return BuildResult::status(e); // parse/source beats writer failure
                    }
                };
                if retry {
                    again = true;
                    continue; // next begin resets counts and truncates the prefix
                }
                // A BuildLoan implementation can mutate/revoke source on return.
                // Do not finalize a cache or certify empty until this guard passes.
                let status = reader.validate(source);
                if status != Status::Ok {
                    writer.guarded(source, |_, store, _| store.remove(self.name));
                    return BuildResult::status(status);
                }
                if !writer.writer.failed() && pages == 0 {
                    writer.guarded(source, |_, store, _| store.remove(self.name));
                    return BuildResult { status: Status::NotFound, verified_empty: true };
                }
                if writer.writer.failed() || writer.writer.page_count() != pages {
                    writer.guarded(source, |_, store, _| store.remove(self.name));
                    return BuildResult::status(Status::OutOfMemory);
                }
                writer.writer.set_total_chars(chars);
                let committed = writer.guarded(source, |w, store, m| w.finish(store, self.name, m));
                return BuildResult::status(if committed { Status::Ok } else { Status::IoError });
            }
        });
        reader.plan = Some(plan);
        result.unwrap_or_else(BuildResult::status)
    }
}
#[allow(clippy::too_many_arguments)]
fn physical_cache_pass<S: BookSource + ?Sized, B: ParserBackend + ?Sized, C: CacheStorage>(
    source: &mut S,
    selected: &Selection,
    landscape: bool,
    slot: &Slot<'_>,
    lr: &mut LayoutResources<'_>,
    io: &mut crate::phase_storage::ParseResources<'_, B>,
    writer: &mut PhysicalWriter<'_, '_, C>,
    on_yield: newt_core::admission::Yield<'_>,
) -> Result<(Status, u32, u32, crate::watch::Verdict), Status> {
    use newt_core::chapter_layout::engine::EngineState;
    let mut scheduled = ScheduledSource::new(source, on_yield, None);
    let mut chapter = ChapterSource::open(&mut scheduled, &selected.entry,
        resources::decoder(&mut io.decoder), on_yield,
        None,
        )?;
    let mut watch = Watch::begin(&mut chapter, slot.state);
    let raw = Entry::raw(selected.entry.uncompressed_size);
    let (mut status, pages, chars, complete) = {
        let mut images = lr.images.for_chapter(&selected.href);
        let mut engine = EngineState::new(reader_parameters(landscape, Some(slot), selected.language),
            resources::storage(&mut lr.storage), lr.breaker, &mut images, None,
            CssStylesheetBuilder::default()).map_err(|_| Status::OutOfMemory)?;
        let stream = XmlStream::new(&mut watch, None, io.backend);
        let mut session = ChapterSession::new(stream);
        let mut status = session.begin_state(&raw, xml_session::Resources {
            buffer: io.xml_buffer.as_deref_mut(), filter: io.filter.as_deref_mut(),
        }, &engine);
        while status == Status::Ok && !session.done() {
            status = session.step_with(&mut engine, 1, u32::MAX, |watch, parser, state, operation| {
                watch.with_source(|chapter| chapter.with_source(|scheduled| {
                    writer.layout(&mut *scheduled.source, parser, state, operation)
                }))
            });
            if status == Status::Ok && !session.done() {
                if let Some(f) = on_yield { f.yield_now(); }
            }
        }
        let complete = session.done() && !engine.stopped;
        let result = (status, engine.pages, engine.chars, complete);
        session.abort();
        result
    };
    let verdict = watch.verdict();
    drop(watch);
    if status == Status::Ok && (!complete || !chapter.finish()) {
        status = Status::Truncated;
    }
    Ok((status, pages, chars, verdict))
}

#[cfg(all(test, feature = "native-host"))]
#[path = "physical_engine_cases.rs"]
pub(super) mod physical_engine_cases;
