//! Single-chapter streaming producer pass for chapter cache generation.
#![forbid(unsafe_code)]
use super::super::super::{PhasedReader, reader_parameters};
use crate::{phase_storage::{Phase, producer::{Attempt, WriterView}}, producer::{Route}, producer_writer::CappedWriter, resources};
use newt_core::member_check::Checked;
use newt_core::{admission::{ChapterSource, ScheduledSource, WorkPhase, WorkScope}, chapter_layout::{engine::EngineState, session::{ChapterSession, LayoutOperation, OperationHandler}, types::{LineBreak, Page, Sink, SplitBackend}}, css::CssStylesheetBuilder, page_cache::{CacheStorage, WriterMemory}, producer_visit::CurrentOutcome, xml_session::{self, BorrowedPolicy, ProducerBackend, XmlStream}, zip_container::Status, zip_window::source::BookSource};
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
            self.writer.record_start((*pages / PAGES) as usize, page.char_start);
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
    #[inline(never)]
    fn produce_segments_and_map_internal<B: ProducerBackend + ?Sized>(
        &mut self, reader: &PhasedReader<'_, '_>, phase: &mut Phase<'_>,
        backend: &mut B, breaker: &mut dyn LineBreak, route: &mut Route<'_>,
    ) -> (CurrentOutcome, Status) {
        let (outcome, raw_status) = match self.with_writer(|write| write.produce_segments_internal(reader, phase, backend, breaker, route)) {
            Ok(pair) => pair,
            Err(status) => {
                self.writer.cancellation().fail_terminal();
                return (CurrentOutcome::NoProgress(status), status);
            }
        };
        if outcome != CurrentOutcome::Done || !self.writer.mapping { return (outcome, raw_status); }
        if self.writer.restore_points && !self.writer.starts_complete(self.state.count) {
            let collected = self
                .with_idle_window(|state, writer, store, source, window, entries, guarded| {
                    crate::resume_restore::collect_missing_starts(
                        state,
                        writer,
                        store,
                        source,
                        window,
                        entries,
                        route,
                        reader.source_bytes,
                        guarded,
                    )
                })
                .unwrap_or(Err(Status::Unsupported));
            match collected {
                Ok(crate::resume_restore::CollectStarts::Certified) => {}
                Ok(crate::resume_restore::CollectStarts::Suspended) => {
                    // Duty repeatedly reclaimed the read descriptor without error;
                    // keep retryable and never latch terminal denial on a healthy chapter.
                    if self.state.sealed > 0 {
                        return (CurrentOutcome::Progress, Status::IoError);
                    }
                    self.writer.cancellation().interrupt();
                    return (CurrentOutcome::NoProgress(Status::IoError), Status::IoError);
                }
                Ok(crate::resume_restore::CollectStarts::Defect) => {
                    // An existing on-card segment failed header/envelope certification.
                    // Never fall back to byte-0 counting to publish a map over it.
                    if self.state.sealed > 0 {
                        return (CurrentOutcome::Progress, Status::Stale);
                    }
                    self.writer.cancellation().fail_terminal();
                    return (CurrentOutcome::NoProgress(Status::Stale), Status::Stale);
                }
                Ok(crate::resume_restore::CollectStarts::Unsupported) => {
                    if self.state.sealed > 0 {
                        // Withhold map publication rather than writing unverified zero slots.
                        return (CurrentOutcome::Progress, Status::Stale);
                    }
                    // Storage lacks envelope-read support (mock test store only):
                    // fall back to a byte-0 counting pass to certify starts.
                    let count_status = match self.with_writer(|write| {
                        write.count_starts_for_map(reader, phase, backend, breaker, route)
                    }) {
                        Ok(st) => st,
                        Err(st) => st,
                    };
                    if count_status != Status::Ok || !self.writer.starts_complete(self.state.count) {
                        let st = if count_status != Status::Ok { count_status } else { Status::Unsupported };
                        let cancellation = self.writer.cancellation();
                        if cancellation.interrupted() && !cancellation.terminal() {
                            return (CurrentOutcome::NoProgress(st), st);
                        }
                        cancellation.fail_terminal();
                        return (CurrentOutcome::NoProgress(st), st);
                    }
                }
                Err(status) => {
                    let cancellation = self.writer.cancellation();
                    if cancellation.interrupted() && !cancellation.terminal() {
                        return if self.state.sealed > 0 {
                            (CurrentOutcome::Progress, status)
                        } else {
                            (CurrentOutcome::NoProgress(status), status)
                        };
                    }
                    cancellation.fail_terminal();
                    return (CurrentOutcome::NoProgress(status), status);
                }
            }
        }
        if !self.store.producer_can_begin() {
            return if self.state.sealed > 0 {
                (CurrentOutcome::Progress, Status::OutOfMemory)
            } else {
                self.writer.cancellation().fail_terminal();
                (CurrentOutcome::NoProgress(Status::OutOfMemory), Status::OutOfMemory)
            };
        }
        self.store.clear_end_committed();
        let status = self.publish_map(route, reader.info.spine);
        if status == Status::Ok { return (CurrentOutcome::Done, Status::Ok); }
        let cancellation = self.writer.cancellation();
        if cancellation.interrupted() && !cancellation.terminal() {
            if self.store.end_committed() {
                (CurrentOutcome::Done, status)
            } else if self.state.sealed > 0 {
                (CurrentOutcome::Progress, status)
            } else {
                (CurrentOutcome::NoProgress(status), status)
            }
        } else {
            cancellation.fail_terminal();
            (CurrentOutcome::NoProgress(status), status)
        }
    }
    /// Complete segments before reusing their index for a map, preserving
    /// three-way progress outcomes.
    pub fn produce_segments_and_map_outcome<B: ProducerBackend + ?Sized>(
        &mut self, reader: &PhasedReader<'_, '_>, phase: &mut Phase<'_>,
        backend: &mut B, breaker: &mut dyn LineBreak, route: &mut Route<'_>,
    ) -> CurrentOutcome {
        self.produce_segments_and_map_internal(reader, phase, backend, breaker, route).0
    }
    /// Frozen produce: complete segments before reusing their index for a map.
    pub fn produce_segments_and_map<B: ProducerBackend + ?Sized>(
        &mut self, reader: &PhasedReader<'_, '_>, phase: &mut Phase<'_>,
        backend: &mut B, breaker: &mut dyn LineBreak, route: &mut Route<'_>,
    ) -> Status {
        self.produce_segments_and_map_internal(reader, phase, backend, breaker, route).1
    }
}
impl<C: ?Sized, S: CacheStorage, F: BookSource + newt_core::producer_policy::ProducerReceipt + ?Sized> WriterView<'_, '_, '_, '_, '_, C, S, F> {
    fn classify_segment_stop(&self, status: Status) -> (CurrentOutcome, Status) {
        let cancellation = self.writer.cancellation();
        if cancellation.interrupted() && !cancellation.terminal() {
            if self.state.sealed > 0 {
                (CurrentOutcome::Progress, status)
            } else {
                (CurrentOutcome::NoProgress(status), status)
            }
        } else {
            cancellation.fail_terminal();
            (CurrentOutcome::NoProgress(status), status)
        }
    }

    #[inline(never)]
    fn produce_segments_internal<B: ProducerBackend + ?Sized>(
        &mut self, reader: &PhasedReader<'_, '_>, phase: &mut Phase<'_>,
        backend: &mut B, breaker: &mut dyn LineBreak, route: &mut Route<'_>,
    ) -> (CurrentOutcome, Status) {
        for pass in 0..2 {
            self.state.next = if pass == 0 { 0 } else { self.state.skipped };
            self.state.skipped = u32::MAX;
            self.writer.seen = 0;
            let source_valid = self.source.validate_source(reader.source_bytes);
            if !source_valid { return self.classify_segment_stop(Status::IoError); }
            let mut status = if self.writer.restore_points {
                self.end_stream();
                let target = if self.guard_attempted.is_some() {
                    crate::producer_store::with_source_store(
                        self.store,
                        self.source,
                        reader.source_bytes,
                        false,
                        self.guard_attempted.as_deref_mut(),
                        |store| self.state.select_target(store, route),
                    )
                } else {
                    self.state.select_target(self.store, route)
                };
                match target {
                    Ok(Some(target_seg)) => {
                        let st = self.finish_current_mode(
                            Some(target_seg), reader, phase, backend, breaker, route,
                        );
                        self.end_stream();
                        st
                    }
                    Ok(None) => Status::Ok,
                    Err(st) => st,
                }
            } else {
                let mut st = if self.guard_attempted.is_some() {
                    crate::producer_store::with_source_store(
                        self.store,
                        self.source,
                        reader.source_bytes,
                        false,
                        self.guard_attempted.as_deref_mut(),
                        |store| self.state.boundary(self.writer, store, &mut self.memory, route),
                    )
                } else {
                    self.state.boundary(self.writer, self.store, &mut self.memory, route)
                };
                if st == Status::Ok && (self.writer.open || self.writer.mapping) {
                    st = self.finish_current(reader, phase, backend, breaker, route);
                }
                st
            };
            if status == Status::Ok && self.state.next < self.state.count {
                status = if self.guard_attempted.is_some() {
                    crate::producer_store::with_source_store(self.store, self.source, reader.source_bytes,
                        false, self.guard_attempted.as_deref_mut(), |store|
                            self.state.scan_remaining(store, route))
                } else { self.state.scan_remaining(self.store, route) };
            }
            if status != Status::Ok { return self.classify_segment_stop(status); }
            if self.state.skipped == u32::MAX { return (CurrentOutcome::Done, Status::Ok); }
            if !self.store.producer_can_begin() { break; }
            self.end_stream();
        }
        if self.writer.cancellation().terminal() {
            (CurrentOutcome::NoProgress(Status::IoError), Status::IoError)
        } else if self.state.sealed > 0 {
            (CurrentOutcome::Progress, Status::Unsupported)
        } else {
            (CurrentOutcome::NoProgress(Status::Unsupported), Status::Unsupported)
        }
    }

    /// Byte-0 counting fallback used only when the segment map is missing,
    /// all segments already exist on disk, and verified prefix-start reads
    /// could not certify every slot.
    #[inline(never)]
    fn count_starts_for_map<B: ProducerBackend + ?Sized>(
        &mut self, reader: &PhasedReader<'_, '_>, phase: &mut Phase<'_>,
        backend: &mut B, breaker: &mut dyn LineBreak, route: &mut Route<'_>,
    ) -> Status {
        let prev_restore = self.writer.restore_points;
        let prev_capture = self.writer.capture_points;
        self.writer.restore_points = false;
        self.writer.capture_points = false;
        self.writer.open = false;
        self.writer.seen = 0;
        self.state.next = self.state.count;
        let status = self.stream_current(reader, phase, backend, breaker, route);
        self.writer.restore_points = prev_restore;
        self.writer.capture_points = prev_capture;
        self.end_stream();
        status
    }

    /// Stream missing segments in target-first order across up to two passes.
    /// Any terminal failure classifies as NoProgress before leftover/progress rules.
    pub fn produce_segments_outcome<B: ProducerBackend + ?Sized>(
        &mut self, reader: &PhasedReader<'_, '_>, phase: &mut Phase<'_>,
        backend: &mut B, breaker: &mut dyn LineBreak, route: &mut Route<'_>,
    ) -> CurrentOutcome {
        self.produce_segments_internal(reader, phase, backend, breaker, route).0
    }

    /// Frozen produceSegments370-385. Phase::layout already returns the
    /// settled style home when a stream ends; re-arm only the same buffer here.
    pub fn produce_segments<B: ProducerBackend + ?Sized>(
        &mut self, reader: &PhasedReader<'_, '_>, phase: &mut Phase<'_>,
        backend: &mut B, breaker: &mut dyn LineBreak, route: &mut Route<'_>,
    ) -> Status {
        self.produce_segments_internal(reader, phase, backend, breaker, route).1
    }

    #[inline(never)]
    fn seal_open_tail(&mut self, reader: &PhasedReader<'_, '_>, status: Status) -> Status {
        if status != Status::Ok || (self.writer.segmented && !self.writer.open) {
            return status;
        }
        if !self.source.validate_source(reader.source_bytes) { return Status::IoError; }
        self.writer.open = false;
        self.writer.cache.set_total_chars(reader.info.total_chars);
        let was_failed = self.writer.cache.failed();
        let mut committed = false;
        let finished = crate::producer_store::with_source_store(self.store, self.source, reader.source_bytes,
            false, self.guard_attempted.as_deref_mut(), |store| {
                store.clear_end_committed();
                #[cfg(feature = "page-turn-trace")]
                store.trace_segment_layout_done();
                let ok = self.writer.cache.finish(store, &self.state.name[..69], &self.memory);
                committed = ok || (!was_failed && store.end_committed());
                #[cfg(feature = "page-turn-trace")]
                if committed { store.trace_segment_end(self.writer.cache.page_count()); }
                ok
            });
        if committed {
            self.state.sealed = self.state.sealed.saturating_add(1);
            if finished
                && self.writer.segmented
                && self.writer.restore_points
                && self.writer.cache.page_count() > 0
            {
                self.writer.record_start(
                    (self.writer.first / newt_core::cache_names::segment::PAGES) as usize,
                    self.writer.cache.char_start(&self.memory, 0),
                );
            }
        }
        if finished { Status::Ok } else { Status::IoError }
    }

    /// Frozen finishWhole: close parser, then validate the original source
    /// before allowing the only/final writer to publish its certified totals.
    pub fn finish_current<B: ProducerBackend + ?Sized>(
        &mut self, reader: &PhasedReader<'_, '_>, phase: &mut Phase<'_>,
        backend: &mut B, breaker: &mut dyn LineBreak, route: &mut Route<'_>,
    ) -> Status {
        self.finish_current_mode(None, reader, phase, backend, breaker, route)
    }

    #[inline(never)]
    fn finish_current_mode<B: ProducerBackend + ?Sized>(
        &mut self,
        target: Option<u32>,
        reader: &PhasedReader<'_, '_>,
        phase: &mut Phase<'_>,
        backend: &mut B,
        breaker: &mut dyn LineBreak,
        route: &mut Route<'_>,
    ) -> Status {
        let status = self.stream_current_mode(target, reader, phase, backend, breaker, route);
        self.seal_open_tail(reader, status)
    }

    /// The enclosing owner supplies the current certified route and settled
    /// styles. This pass retains neither selection nor parser and publishes no
    /// page. The writer remains lent until the owner finishes or aborts it.
    pub fn stream_current<B: ProducerBackend + ?Sized>(
        &mut self, reader: &PhasedReader<'_, '_>, phase: &mut Phase<'_>,
        backend: &mut B, breaker: &mut dyn LineBreak, route: &mut Route<'_>,
    ) -> Status {
        self.stream_current_mode(None, reader, phase, backend, breaker, route)
    }

    #[inline(never)]
    fn stream_current_mode<B: ProducerBackend + ?Sized>(
        &mut self,
        target: Option<u32>,
        reader: &PhasedReader<'_, '_>,
        phase: &mut Phase<'_>,
        backend: &mut B,
        breaker: &mut dyn LineBreak,
        route: &mut Route<'_>,
    ) -> Status {
        let with_capture = self.writer.segmented && self.writer.capture_points;
        phase
            .layout_with_capture(backend, breaker, with_capture, |slot, lr, io, capture| {
                self.stream_in_layout(target, reader, slot, lr, io, route, capture)
            })
            .unwrap_or_else(|status| status)
    }

    /// Shared entry for both byte-0 and target-first segmented streaming.
    /// Keeps `ChapterSource`, `EngineState` and `ChapterSession` in one frame
    /// while `prepare_targeted` runs Pass A/B, positioning and replay in a
    /// separate `#[inline(never)]` frame that returns before `run_layout_loop`.
    #[inline(never)]
    fn stream_in_layout<B: ProducerBackend + ?Sized>(
        &mut self,
        target: Option<u32>,
        reader: &PhasedReader<'_, '_>,
        slot: &mut crate::styles::Slot<'_>,
        lr: &mut crate::resources::LayoutResources<'_>,
        io: &mut crate::phase_storage::ParseResources<'_, B>,
        route: &mut Route<'_>,
        mut capture: Option<&mut crate::resume_capture::CaptureState<'_>>,
    ) -> Status {
        let _work = WorkScope::new_scoped(reader.on_yield, reader.work, WorkPhase::Layout);
        let cancellation = self.writer.cancellation();
        let selected = if self.state.staged {
            &reader.selections[reader.active ^ 1]
        } else {
            reader.selection()
        };
        let params = reader_parameters(reader.page_layout, Some(slot), selected.language);
        let mut scheduled =
            ScheduledSource::new(&mut *self.source, reader.on_yield, Some(cancellation));
        let mut chapter = match ChapterSource::open(
            &mut scheduled,
            &selected.entry,
            resources::decoder(&mut io.decoder),
            reader.on_yield,
            Some(cancellation),
        ) {
            Ok(chapter) => chapter,
            Err(status) => {
                cancellation.result(false);
                return status;
            }
        };
        let raw = Checked::raw(selected.entry.uncompressed_size);
        let mut images = lr.images.for_chapter(&selected.href);
        let mut engine = match EngineState::new(
            params,
            resources::storage(&mut lr.storage),
            lr.breaker,
            &mut images,
            None,
            CssStylesheetBuilder::default(),
        ) {
            Ok(engine) => engine,
            Err(_) => {
                cancellation.result(false);
                return Status::OutOfMemory;
            }
        };
        let mut policy = cancellation;
        let stream = XmlStream::with_policy(
            &mut chapter,
            None,
            io.backend,
            BorrowedPolicy(&mut policy),
        );
        let mut session = ChapterSession::new(stream);
        let mut xml_res = xml_session::Resources {
            buffer: io.xml_buffer.take(),
            filter: io.filter.take(),
        };
        let status = match target {
            Some(target_seg) => {
                let mut ctx = TargetCtx {
                    state: self.state,
                    writer: self.writer,
                    store: self.store,
                    memory: &mut self.memory,
                    stream_buffer: self.stream_buffer,
                    guard_attempted: self.guard_attempted.as_deref_mut(),
                    reader,
                    slot,
                    route,
                };
                prepare_targeted(
                    &mut ctx,
                    target_seg,
                    selected,
                    &raw,
                    &mut session,
                    &mut engine,
                    &mut xml_res,
                    capture.as_deref_mut(),
                )
            }
            None => {
                if let Some(buffer) = self.stream_buffer.take() {
                    self.store.use_stream_buffer(buffer);
                }
                let res = core::mem::replace(
                    &mut xml_res,
                    xml_session::Resources { buffer: None, filter: None },
                );
                session.begin_state(&raw, res, &engine)
            }
        };
        if target.is_some() && status != Status::Ok {
            session.abort();
            return status;
        }
        run_layout_loop(
            self.state,
            self.writer,
            self.store,
            &mut self.memory,
            self.guard_attempted.as_deref_mut(),
            reader,
            slot,
            selected,
            route,
            &mut session,
            &mut engine,
            capture,
            status,
        )
    }
}

struct TargetCtx<
    'a,
    'pass,
    'ctx,
    'pause,
    'loan,
    'mem,
    'r1,
    'r2,
    'sl,
    'rt,
    C: ?Sized,
    S: CacheStorage,
> {
    state: &'a mut crate::producer::Segments,
    writer: &'a mut CappedWriter<'loan, 'ctx, C>,
    store: &'a mut crate::producer_store::ProducerStore<'pass, 'pass, 'ctx, 'pause, C, S>,
    memory: &'a mut WriterMemory<'mem>,
    stream_buffer: &'a mut Option<&'pass mut [u8; crate::producer_store::BUFFER_BYTES]>,
    guard_attempted: Option<&'a mut bool>,
    reader: &'a PhasedReader<'r1, 'r2>,
    slot: &'a crate::styles::Slot<'sl>,
    route: &'a mut Route<'rt>,
}

#[inline(never)]
fn open_target_segment<
    'ctx,
    C: ?Sized,
    S: CacheStorage,
    F: BookSource + newt_core::producer_policy::ProducerReceipt + ?Sized,
    B: ProducerBackend + ?Sized,
>(
    ctx: &mut TargetCtx<'_, '_, 'ctx, '_, '_, '_, '_, '_, '_, '_, C, S>,
    target: u32,
    session: &mut ChapterSession<
        '_,
        ChapterSource<'_, ScheduledSource<'_, newt_core::producer_policy::ProducerBookSource<'_, 'ctx, C, F>>>,
        B,
        BorrowedPolicy<'_>,
    >,
) -> Status {
    session.stream.with_source(|ch, _| {
        ch.with_source(|sch| {
            if ctx.guard_attempted.is_some() {
                crate::producer_store::with_source_store(
                    ctx.store,
                    sch.source,
                    ctx.reader.source_bytes,
                    false,
                    ctx.guard_attempted.as_deref_mut(),
                    |store| ctx.state.open_segment(ctx.writer, store, ctx.memory, ctx.route, target),
                )
            } else {
                ctx.state.open_segment(ctx.writer, ctx.store, ctx.memory, ctx.route, target)
            }
        })
    })
}

#[inline(never)]
fn prepare_targeted<
    's,
    'loan,
    'ctx,
    C: ?Sized,
    S: CacheStorage,
    F: BookSource + newt_core::producer_policy::ProducerReceipt + ?Sized,
    B: ProducerBackend + ?Sized,
>(
    ctx: &mut TargetCtx<'_, '_, 'ctx, '_, '_, '_, '_, '_, '_, '_, C, S>,
    target: u32,
    selected: &'s crate::Selection,
    raw: &'s Checked,
    session: &mut ChapterSession<
        's,
        ChapterSource<'s, ScheduledSource<'s, newt_core::producer_policy::ProducerBookSource<'loan, 'ctx, C, F>>>,
        B,
        BorrowedPolicy<'s>,
    >,
    engine: &mut EngineState<'_>,
    xml_res: &mut xml_session::Resources<'s>,
    mut capture: Option<&mut crate::resume_capture::CaptureState<'_>>,
) -> Status {
    for cand in (0..target).rev() {
        match try_restore_candidate(
            ctx,
            cand,
            target,
            selected,
            raw,
            session,
            engine,
            xml_res,
            capture.as_deref_mut(),
        ) {
            Ok(()) => return Status::Ok,
            Err(crate::resume_restore::RestoreError::Stop(status)) => return status,
            Err(crate::resume_restore::RestoreError::LocalReject) => {
                if xml_res.buffer.is_none() {
                    *xml_res = session.abort_returning();
                    if session.stream.with_source(|ch, _| ch.consumed() != 0) {
                        let reopen = session
                            .stream
                            .with_source(|ch, _| ch.reopen(&selected.entry));
                        if reopen != Status::Ok {
                            return reopen;
                        }
                    }
                    engine.reset();
                }
                ctx.writer.seen = 0;
                ctx.writer.refused = false;
                ctx.writer.open = false;
                if let Some(cap) = capture.as_deref_mut() {
                    cap.reset();
                }
                if let Some(buf) = ctx.store.return_stream_buffer() {
                    *ctx.stream_buffer = Some(buf);
                }
            }
        }
    }
    // Byte-0 fallback for `target`.
    ctx.writer.seen = 0;
    ctx.writer.refused = false;
    if let Some(cap) = capture.as_deref_mut() {
        cap.reset();
    }
    if let Some(buffer) = ctx.stream_buffer.take() {
        ctx.store.use_stream_buffer(buffer);
    }
    let open_status = open_target_segment(ctx, target, session);
    if open_status != Status::Ok {
        return open_status;
    }
    let res = core::mem::replace(
        xml_res,
        xml_session::Resources { buffer: None, filter: None },
    );
    session.begin_state(raw, res, engine)
}

#[inline(never)]
fn try_restore_candidate<
    's,
    'loan,
    'ctx,
    C: ?Sized,
    S: CacheStorage,
    F: BookSource + newt_core::producer_policy::ProducerReceipt + ?Sized,
    B: ProducerBackend + ?Sized,
>(
    ctx: &mut TargetCtx<'_, '_, 'ctx, '_, '_, '_, '_, '_, '_, '_, C, S>,
    cand: u32,
    target: u32,
    selected: &crate::Selection,
    raw: &'s Checked,
    session: &mut ChapterSession<
        's,
        ChapterSource<'s, ScheduledSource<'s, newt_core::producer_policy::ProducerBookSource<'loan, 'ctx, C, F>>>,
        B,
        BorrowedPolicy<'s>,
    >,
    engine: &mut EngineState<'_>,
    xml_res: &mut xml_session::Resources<'s>,
    capture: Option<&mut crate::resume_capture::CaptureState<'_>>,
) -> Result<(), crate::resume_restore::RestoreError> {
    use crate::resume_restore::{
        ExpectedPoint, GuardedLease, RestoreError, feed_replay_and_arm_shift, pass_a_verify_point,
        pass_b_binding_and_source, pass_b_load_rest,
    };
    use newt_core::{
        admission::DiscardResult,
        cache_names::segment::{ChapterSegment, PAGES},
        page_envelope::Query,
        resume_point::{Mode, PointCursor},
    };

    let guarded = ctx.guard_attempted.is_some();
    verify_session_alive(ctx.writer, ctx.store, session, ctx.reader.source_bytes, guarded)
        .map_err(RestoreError::Stop)?;
    let Some(cap) = capture else {
        return Err(RestoreError::LocalReject);
    };
    let Some(target_first) = target.checked_mul(PAGES) else {
        return Err(RestoreError::LocalReject);
    };
    if !ctx.state.name_segment(ctx.route, cand) {
        return Err(RestoreError::Stop(Status::IoError));
    }
    let expected_gen = ctx.state.generation;
    let cand_seg = ChapterSegment {
        index: cand,
        pages: ctx.route.info.page_count,
        chars: ctx.route.info.total_chars,
    };
    if ctx.store.buffered_bytes() != 0 {
        return Err(RestoreError::Stop(Status::Unsupported));
    }
    let mut query = Query::new(Some(&ctx.state.name[..69]));
    let (verified, source_failed) = {
        let Some(window) = ctx.stream_buffer.as_deref_mut() else {
            return Err(RestoreError::Stop(Status::OutOfMemory));
        };
        let Some(chunk) = ctx.memory.chunks.get_mut(0) else {
            return Err(RestoreError::Stop(Status::OutOfMemory));
        };
        session.stream.with_source(|ch, _| {
            ch.with_source(|sch| {
                let mut lease =
                    GuardedLease::new(ctx.store, &mut *sch.source, ctx.reader.source_bytes, guarded);
                let verified = if lease.begin(&ctx.state.name[..69], &mut query) {
                    pass_a_verify_point(
                        &mut query,
                        window,
                        &mut chunk.entries,
                        expected_gen,
                        ctx.route.info.total_chars,
                        cand_seg.file_pages(),
                        &mut lease,
                    )
                } else {
                    None
                };
                let failed = lease.source_failed();
                if verified.is_none() && query.bound {
                    let _ = lease.finish(&mut query);
                }
                (verified, failed)
            })
        })
    };
    if source_failed {
        return Err(RestoreError::Stop(Status::IoError));
    }
    let Some(verified) = verified else {
        verify_session_alive(ctx.writer, ctx.store, session, ctx.reader.source_bytes, guarded)
            .map_err(RestoreError::Stop)?;
        return Err(RestoreError::LocalReject);
    };
    let expected = ExpectedPoint {
        digest: ctx.route.digest,
        spine: ctx.route.info.spine,
        generation: expected_gen,
        candidate_seg: cand,
        target_first,
        page_count: ctx.route.info.page_count,
        total_chars: ctx.route.info.total_chars,
        slot: ctx.slot,
        entry: &selected.entry,
        source_bytes: ctx.reader.source_bytes,
    };
    let Some(mut cursor) = PointCursor::new(verified.range.offset, verified.range.length) else {
        let _ = ctx.store.finish_envelope_read(&mut query);
        return Err(RestoreError::LocalReject);
    };
    // Pass B Stage 1: Header + Binding + Source BEFORE calling `begin_state` (Expat).
    let stage1 = {
        let Some(window) = ctx.stream_buffer.as_deref_mut() else {
            let _ = ctx.store.finish_envelope_read(&mut query);
            return Err(RestoreError::Stop(Status::OutOfMemory));
        };
        let store = &mut *ctx.store;
        let q = &query;
        let mut raw_read =
            |off: u32, dst: &mut [u8]| store.pause_point() && store.read_envelope_raw(q, off, dst);
        let mut point_in = cursor.bind(window, &mut raw_read);
        pass_b_binding_and_source(&mut point_in, &verified, &expected)
    };
    let Some((binding, source_consumed)) = stage1 else {
        let _ = ctx.store.finish_envelope_read(&mut query);
        verify_session_alive(ctx.writer, ctx.store, session, ctx.reader.source_bytes, guarded)
            .map_err(RestoreError::Stop)?;
        return Err(RestoreError::LocalReject);
    };
    let res = core::mem::replace(
        xml_res,
        xml_session::Resources { buffer: None, filter: None },
    );
    let begin_st = session.begin_state(raw, res, engine);
    if begin_st != Status::Ok {
        let _ = ctx.store.finish_envelope_read(&mut query);
        return Err(RestoreError::Stop(begin_st));
    }
    // Pass B Stage 2: Stream + Engine + Prolog + Names + Replay + Pass B CRC.
    let prefix = {
        let Some(window) = ctx.stream_buffer.as_deref_mut() else {
            let _ = ctx.store.finish_envelope_read(&mut query);
            return Err(RestoreError::Stop(Status::OutOfMemory));
        };
        let store = &mut *ctx.store;
        let q = &query;
        let mut raw_read =
            |off: u32, dst: &mut [u8]| store.pause_point() && store.read_envelope_raw(q, off, dst);
        let point_in = cursor.bind(window, &mut raw_read);
        pass_b_load_rest(
            point_in,
            &verified,
            &binding,
            source_consumed,
            selected.entry.uncompressed_size,
            session,
            engine,
            cap,
        )
    };
    // Unconditionally close file 3 (checked exit) BEFORE positioning, Replay, or `begin_write`.
    let lease_ok = ctx.store.finish_envelope_read(&mut query);
    let Some(prefix) = prefix.filter(|_| lease_ok && ctx.store.buffered_bytes() == 0) else {
        verify_session_alive(ctx.writer, ctx.store, session, ctx.reader.source_bytes, guarded)
            .map_err(RestoreError::Stop)?;
        return Err(RestoreError::LocalReject);
    };
    verify_session_alive(ctx.writer, ctx.store, session, ctx.reader.source_bytes, guarded)
        .map_err(RestoreError::Stop)?;
    // Position ChapterSource with file 3 CLOSED so due duty during discard
    // uses file slot 3 cleanly without invalidating the loaded point.
    let positioned = match verified.layout.mode {
        Mode::Seek => {
            if session.stream.with_source(|ch, _| ch.seek_stored(source_consumed)) {
                DiscardResult::Ok
            } else {
                DiscardResult::Invalid
            }
        }
        Mode::Reinflate => {
            let scratch_len = engine.storage.breaks.len().min(2048);
            let scratch = &mut engine.storage.breaks[..scratch_len];
            let store = &mut *ctx.store;
            session.stream.with_source(|ch, _| {
                ch.discard_to(source_consumed, scratch, &mut || store.pause_point())
            })
        }
    };
    match positioned {
        DiscardResult::Ok => {}
        DiscardResult::Stopped | DiscardResult::IoError => {
            return Err(RestoreError::Stop(Status::IoError));
        }
        DiscardResult::Invalid => {
            verify_session_alive(ctx.writer, ctx.store, session, ctx.reader.source_bytes, guarded)
                .map_err(RestoreError::Stop)?;
            return Err(RestoreError::LocalReject);
        }
    }
    // Feed Replay BEFORE `begin_write` so replay refusal never burns a 1-entry grant.
    if !feed_replay_and_arm_shift(session, engine, cap, ctx.writer, &binding, &prefix, target_first) {
        verify_session_alive(ctx.writer, ctx.store, session, ctx.reader.source_bytes, guarded)
            .map_err(RestoreError::Stop)?;
        return Err(RestoreError::LocalReject);
    }
    verify_session_alive(ctx.writer, ctx.store, session, ctx.reader.source_bytes, guarded)
        .map_err(RestoreError::Stop)?;
    if let Some(buffer) = ctx.stream_buffer.take() {
        ctx.store.use_stream_buffer(buffer);
    }
    let open_st = open_target_segment(ctx, target, session);
    if open_st != Status::Ok {
        return Err(RestoreError::Stop(open_st));
    }
    Ok(())
}

#[inline(never)]
fn verify_session_alive<
    'ctx,
    C: ?Sized,
    S: CacheStorage,
    F: BookSource + newt_core::producer_policy::ProducerReceipt + ?Sized,
    B: ProducerBackend + ?Sized,
>(
    writer: &CappedWriter<'_, 'ctx, C>,
    store: &mut crate::producer_store::ProducerStore<'_, '_, 'ctx, '_, C, S>,
    session: &mut ChapterSession<
        '_,
        ChapterSource<'_, ScheduledSource<'_, newt_core::producer_policy::ProducerBookSource<'_, 'ctx, C, F>>>,
        B,
        BorrowedPolicy<'_>,
    >,
    source_bytes: u64,
    guarded: bool,
) -> Result<(), Status> {
    session.stream.with_source(|ch, _| {
        ch.with_source(|sch| {
            crate::resume_restore::verify_attempt_alive(
                writer,
                store,
                sch.source,
                source_bytes,
                guarded,
            )
        })
    })
}

#[inline(never)]
fn run_layout_loop<
    'ctx,
    C: ?Sized,
    S: CacheStorage,
    F: BookSource + newt_core::producer_policy::ProducerReceipt + ?Sized,
    B: ProducerBackend + ?Sized,
>(
    state: &mut crate::producer::Segments,
    writer: &mut CappedWriter<'_, 'ctx, C>,
    store: &mut crate::producer_store::ProducerStore<'_, '_, 'ctx, '_, C, S>,
    memory: &mut WriterMemory<'_>,
    mut guard_attempted: Option<&mut bool>,
    reader: &PhasedReader<'_, '_>,
    slot: &crate::styles::Slot<'_>,
    selected: &crate::Selection,
    route: &mut Route<'_>,
    session: &mut ChapterSession<
        '_,
        ChapterSource<'_, ScheduledSource<'_, newt_core::producer_policy::ProducerBookSource<'_, 'ctx, C, F>>>,
        B,
        BorrowedPolicy<'_>,
    >,
    engine: &mut EngineState<'_>,
    mut capture: Option<&mut crate::resume_capture::CaptureState<'_>>,
    mut status: Status,
) -> Status {
    let cancellation = writer.cancellation();
    let keep_going_for_map = writer.mapping && !writer.restore_points;
    while status == Status::Ok && !session.done() && !writer.refused {
        status = session.step_with(engine, 1, 1, |chapter, parser, engine, operation| {
            chapter.with_source(|scheduled| {
                crate::producer_store::with_source_store(store, &mut *scheduled.source,
                    reader.source_bytes, false, guard_attempted.as_deref_mut(), |store| {
                        let mut sink = Writing { writer, store, memory, counted: None };
                        let mut font = ReaderAtlas;
                        let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
                        match operation {
                            LayoutOperation::Feed { bytes, last } => {
                                if let Some(cap) = capture.as_deref_mut() {
                                    cap.tap.observe(bytes);
                                    let mut op = OperationHandler { engine, backend: &mut backend };
                                    let mut tap = cap.tap.wrap(&mut op);
                                    let ok = match parser.feed_observed(bytes, last, &mut tap, &mut cap.obs) {
                                        Some(ok) => ok,
                                        None => parser.feed(bytes, last, &mut tap),
                                    };
                                    cap.tap.finish_root(cap.obs.root_index);
                                    ok
                                } else {
                                    parser.feed(bytes, last,
                                        &mut OperationHandler { engine, backend: &mut backend })
                                }
                            }
                            LayoutOperation::Finish => engine.finish(&mut backend),
                        }
                    })
            })
        });
        if !cancellation.result(status == Status::Ok) { break; }
        if !store.pause_point() { status = Status::IoError; break; }
        if let Some(cap) = capture.as_deref_mut() {
            if !session.done() && !writer.refused && writer.open {
                let capture_route = crate::resume_capture::CaptureRoute {
                    digest: route.digest,
                    spine: route.info.spine,
                    generation: state.generation,
                    page_count: route.info.page_count,
                    total_chars: route.info.total_chars,
                };
                let outcome = session.stream.with_source_and_view(|chapter, parser, stream| {
                    let input = crate::resume_capture::BoundaryInput {
                        entry: &selected.entry,
                        source_bytes: reader.source_bytes,
                        source_consumed: chapter.consumed(),
                        source_method: chapter.method(),
                        parser_index: parser.current_byte_index(),
                        stream,
                    };
                    let cw = crate::resume_capture::CaptureWriter {
                        cache: &mut writer.cache,
                        first: writer.first,
                        seen: writer.seen,
                        open: writer.open,
                        refused: &mut writer.refused,
                    };
                    chapter.with_source(|scheduled| {
                        crate::producer_store::with_source_store(
                            store,
                            &mut *scheduled.source,
                            reader.source_bytes,
                            false,
                            guard_attempted.as_deref_mut(),
                            |guarded| {
                                let credit = guarded.producer_remaining_credit();
                                let mut write = |bytes: &[u8]| guarded.write(bytes);
                                crate::resume_capture::maybe_capture(
                                    cap, engine, slot, cw, credit, &mut write, &capture_route,
                                    &input,
                                )
                            },
                        )
                    })
                });
                if outcome == crate::resume_capture::CaptureOutcome::WriteFailed {
                    status = Status::IoError;
                    break;
                }
            }
        }
        if writer.segmented {
            status = if guard_attempted.is_some() {
                session.stream.with_source(|chapter, _| chapter.with_source(|scheduled| {
                    crate::producer_store::with_source_store(store, &mut *scheduled.source,
                        reader.source_bytes, false, guard_attempted.as_deref_mut(), |store|
                            state.boundary(writer, store, memory, route))
                }))
            } else { state.boundary(writer, store, memory, route) };
            if !writer.open && !keep_going_for_map { break; }
        }
    }
    let whole = !writer.segmented || writer.open || keep_going_for_map;
    let mut eof_verified = false;
    if status == Status::Ok && whole && !writer.refused {
        let (finished, ended) = session.stream.with_source(|chapter, _| chapter.finish_checked());
        eof_verified = ended;
        if !finished {
            status = Status::Truncated;
        }
    }
    if whole && (status == Status::Ok || eof_verified) && (writer.refused
        || engine.pages != reader.info.page_count || engine.chars != reader.info.total_chars) {
        if !writer.refused {
            cancellation.fail_terminal();
        }
        status = Status::Unsupported;
    }
    cancellation.result(status == Status::Ok);
    session.abort();
    status
}

#[path = "producer_next.rs"]
mod next;
pub use next::NextPass;


#[path = "producer_current.rs"]
mod current;
