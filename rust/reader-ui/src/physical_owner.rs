// SPDX-License-Identifier: MIT
//! Resource-backed external owner. No frame exists during execute's Session.
use super::*;
use crate::lifecycle::{MenuOperation, handle_reader_ui_with};
use crate::physical_source::{Interrupted, PhysicalSourceRef, ReaderSourceMode};
use newt_core::{chapter_layout::types::LineBreak, admission::ActionScope,
    reader_action_profile::{ActionStage, CacheMissReason}, reader_work_profile::ReaderOpenRoute};
use newt_phased_reader::imported_revision::{
    Digest, ImportedRevision,
    session::{Cache, Session},
};
use newt_phased_reader::phase_storage::{BuildLoan, ReadScope, Storage};
use newt_phased_reader::{CachePresence, CacheRequest, OperationPhased, PhasedChapterCache, QUICK_VERIFY_BYTES};
#[path = "physical_owner/verification.rs"]
mod verification;
#[path = "physical_owner/actions.rs"]
mod actions;
#[path = "physical_owner/receipt.rs"]
mod receipt;
use receipt::{begin_receipt_action, finish_receipt_action, finish_reader_action, reject_changed_receipt, Shown};

/// One SDK context supplies BOTH loans; it cannot expose a frame from with_parse.
/// Frame bytes are logical until compose_native consumes a completed UI paint.
/// No refresh is performed here. SDK callers require the existing panic=abort contract.
pub trait ReaderPanelLoan: BuildLoan {
    fn with_frame<R>(
        &mut self,
        paint: impl for<'frame> FnOnce(&'frame mut Frame) -> R,
    ) -> Result<R, Status>;
}
#[derive(Clone, Copy)]
pub enum PhysicalOpen {
    /// Caller has successfully begun the real revision import.
    Prepared,
    /// Full cache-building import; requires the separately reviewed writer patch.
    Import,
    /// Caller loaded the real receipt/source. Session verifies identity/ranges.
    Loaded { digest: Digest, spine: u32 },
    /// Caller selected the receipt over the complete current source
    /// (prepareSelection). Session warm-opens `spine` and strictly reimports
    /// the held source on any miss (openRevision).
    Selected { digest: Digest, spine: u32 },
}
/// Construct per owner call or UI transaction. These are resource-owner borrows,
/// not bank views. Source authority is passed separately, at each operation.
pub struct PhysicalOwner<'op, 'bank, 'cache, B: ParserBackend + ?Sized, L, C> {
    pub storage: &'op mut Storage<'bank>,
    pub panel: &'op mut L,
    pub backend: &'op mut B,
    pub breaker: &'op mut dyn LineBreak,
    pub cache: PhasedChapterCache<'cache, C>,
    pub open: PhysicalOpen,
    pub cache_only: bool,
}
impl<B: ParserBackend + ?Sized, L: ReaderPanelLoan, C: ReadScope + CachePresence>
    PhysicalOwner<'_, '_, '_, B, L, C>
{
    fn execute_revision(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        revision: &mut ImportedRevision<'_>,
        request: PageRequest,
        retain_source: bool,
    ) -> PageWork {
        begin_receipt_action(revision, request);
        let work = self.execute_revision_body(reader, revision, request, retain_source,
            self.cache_only);
        finish_receipt_action(reader, revision, request, work)
    }

    fn execute_revision_body(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        revision: &mut ImportedRevision<'_>,
        request: PageRequest,
        retain_source: bool,
        cache_only: bool,
    ) -> PageWork {
        let work = (|| {
            let mut publication = ActionScope::inactive();
            let mut cached = false;
            // OperationPhased is constructed only here, with source/lender supplied
            // for THIS operation. All real parser/query/native loans end below.
            let mut op =
                OperationPhased::new(reader, self.storage, self.panel, self.backend, self.breaker);
            let capture = {
                let mut session = Session {
                    revision,
                    reader: &mut op,
                    cache: &mut self.cache,
                };
                match request {
                    PageRequest::Open => {
                        let status = match self.open {
                            PhysicalOpen::Prepared => {
                                if retain_source {
                                    session.finish_prepared_open_retaining_source()
                                } else {
                                    session.finish_prepared_open()
                                }
                            }
                            PhysicalOpen::Import => session.finish_prepared_import(),
                            PhysicalOpen::Loaded { digest, spine } => {
                                session.open_loaded_owned(digest, spine)
                            }
                            PhysicalOpen::Selected { digest, spine } => {
                                if retain_source {
                                    session.open_selected_owned_retaining_source(digest, spine)
                                } else {
                                    session.open_selected_owned(digest, spine)
                                }
                            }
                        };
                        // Open publishes selection, not a page. Owner restore is next.
                        drop(session);
                        drop(op);
                        return completed(status);
                    }
                    PageRequest::Menu(focus) => {
                        let status = session.prepare_menu_owned(focus);
                        drop(session);
                        drop(op);
                        return prepared(status);
                    }
                    PageRequest::Chapter(spine, toc) => session.capture_chapter_owned(spine, toc),
                    PageRequest::Page(spine, target) | PageRequest::Anchor(spine, target) => {
                        if session.revision.provisional() {
                            return PageWork::failed(Status::Unsupported);
                        }
                        let staged = if spine != session.reader.reader.info().spine {
                            let digest = *session.revision.digest();
                            if session.cache.stored(&digest, spine, session.reader) {
                                // Staging verifies the whole chapter. A cache-only probe
                                // verifies only a short one; a longer one, or any failure,
                                // retries after Loading shows, which reports it (#103).
                                if cache_only {
                                    session.reader.limit_verification(QUICK_VERIFY_BYTES);
                                }
                                let status = session.stage_owned(spine);
                                if cache_only && status != Status::Ok {
                                    return retry_after_loading(session.revision);
                                }
                                if status != Status::Ok {
                                    return PageWork::failed(status);
                                }
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                        let result = session.capture_page_owned_traced(CacheRequest {
                            spine,
                            target,
                            anchor: matches!(request, PageRequest::Anchor(..)),
                            staged,
                            build_on_miss: !cache_only && !staged,
                            cache_only,
                        });
                        publication = result.publication;
                        cached = result.result.hit;
                        if cache_only && result.result.status == Status::NotFound {
                            return retry_after_loading(session.revision);
                        }
                        result.result.status
                    }
                }
            }; // Session/source reborrows are gone; capture checked actual exit.
            drop(op); // SDK lender/reader/storage/backend/breaker operation is gone.
            if capture != Status::Ok {
                return PageWork::failed(capture);
            }
            let work = self.publish_source(reader, revision);
            if cached && work.result != Completion::Published { reader.trace_cache_miss(CacheMissReason::Capture); }
            drop(publication);
            if cached && work.result == Completion::Published { reader.trace_route(ReaderOpenRoute::CacheHit); }
            if matches!(request, PageRequest::Page(..) | PageRequest::Anchor(..)) && work.result == Completion::Published {
                reader.trace_route(ReaderOpenRoute::PageReady);
            }
            work
        })();
        work
    }

    fn repaint_revision(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        revision: &mut ImportedRevision<'_>,
    ) -> Status {
        revision.begin_immutable_read();
        let status = self
            .panel
            .with_frame(|frame| reader.repaint(revision, frame))
            .unwrap_or_else(|e| e);
        // Exit runs even when repaint or frame acquisition failed.
        let fresh = revision.finish_immutable_read();
        if !fresh && status == Status::Ok {
            reject_changed_receipt(reader, revision);
            return Status::Stale;
        }
        status
    }

    fn publish_source<S: BookSource + ?Sized>(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        source: &mut S,
    ) -> PageWork {
        match self.panel.with_frame(|frame| reader.publish(source, frame)) {
            Ok(status) => completed(status),
            Err(error) => {
                OperationPhased::new(reader, self.storage, self.panel, self.backend, self.breaker)
                    .discard_candidate(error);
                PageWork::failed(error)
            }
        }
    }

    fn execute_raw(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        source: &mut dyn BookSource,
        digest: &Digest,
        request: PageRequest,
    ) -> PageWork {
        let mut publication = ActionScope::inactive();
        let status = match request {
            PageRequest::Open => {
                return completed(reader.open_indexed(
                    source, self.cache.store, digest,
                    self.storage,
                    self.panel,
                    self.backend,
                ));
            }
            PageRequest::Menu(focus) => {
                return prepared(reader.prepare_chapter_menu_indexed(
                    source, self.cache.store, digest,
                    focus,
                    self.storage,
                    self.panel,
                    self.backend,
                ));
            }
            PageRequest::Chapter(spine, toc) => reader.capture_chapter_indexed(
                source, self.cache.store, digest,
                spine,
                toc,
                self.storage,
                self.panel,
                self.backend,
                self.breaker,
            ),
            PageRequest::Page(spine, target) | PageRequest::Anchor(spine, target) => {
                reader.trace_route(ReaderOpenRoute::CacheMiss);
                publication = reader.action_scope(ActionStage::OrdinaryFallback);
                let mode = if matches!(request, PageRequest::Anchor(..)) {
                    newt_phased_sink::Target::Anchor
                } else {
                    newt_phased_sink::Target::Page
                };
                reader.capture_indexed(
                    source, self.cache.store, digest,
                    spine,
                    target,
                    mode,
                    self.storage,
                    self.panel,
                    self.backend,
                    self.breaker,
                )
            }
        };
        if status != Status::Ok {
            return PageWork::failed(status);
        }
        let work = self.publish_source(reader, source);
        drop(publication);
        if matches!(request, PageRequest::Page(..) | PageRequest::Anchor(..)) && work.result == Completion::Published {
            reader.trace_route(ReaderOpenRoute::PageReady);
        }
        work
    }

    fn execute_reader(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        source: &mut ReaderSource<'_>,
        request: PageRequest,
    ) -> PageWork {
        if matches!(source.mode, ReaderSourceMode::Closed | ReaderSourceMode::MustReload(_)) {
            return PageWork::failed(Status::NotFound);
        }
        let shown = Shown::of(reader);
        begin_receipt_action(&mut source.revision, request);
        let work = self.execute_reader_body(reader, source, request);
        finish_reader_action(reader, source, request, shown, work)
    }

    fn execute_reader_body(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        source: &mut ReaderSource<'_>,
        request: PageRequest,
    ) -> PageWork {
        let ReaderSource {
            revision,
            mode,
            digest,
        } = source;
        if matches!(mode, ReaderSourceMode::Closed | ReaderSourceMode::MustReload(_)) {
            return PageWork::failed(Status::NotFound);
        }
        let mut work = match mode {
            ReaderSourceMode::Revision => {
                self.execute_revision_body(reader, revision, request, true, self.cache_only)
            }
            ReaderSourceMode::Strict if request == PageRequest::Open => {
                self.open_strict(reader, revision, mode, *digest)
            }
            ReaderSourceMode::Strict
                if self.cache_only
                    && matches!(request, PageRequest::Page(..) | PageRequest::Anchor(..)) =>
            {
                PageWork::VERIFICATION_PENDING
            }
            ReaderSourceMode::Strict => revision
                .with_held_source(|raw| self.execute_raw(reader, raw, digest, request))
                .unwrap_or_else(|| PageWork::failed(Status::IoError)),
            ReaderSourceMode::MustReload(_) | ReaderSourceMode::Closed => unreachable!(),
        };
        if matches!(mode, ReaderSourceMode::Revision)
            && request == PageRequest::Open
            && work.result == Completion::Failed(Status::IoError)
        {
            let expected = match self.open {
                PhysicalOpen::Loaded { digest, .. } | PhysicalOpen::Selected { digest, .. } => {
                    digest
                }
                PhysicalOpen::Prepared | PhysicalOpen::Import => *digest,
            };
            *digest = expected;
            work = self.open_strict(reader, revision, mode, expected);
        }
        work
    }

    fn open_strict(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        revision: &mut ImportedRevision<'_>,
        mode: &mut ReaderSourceMode,
        expected: Digest,
    ) -> PageWork {
        reader.trace_route(ReaderOpenRoute::StrictFallback);
        let status = revision.prepare_strict_source(expected, &|| true);
        if status != Status::Ok {
            return PageWork::failed(status);
        }
        *mode = ReaderSourceMode::Strict;
        reader.trace_route(ReaderOpenRoute::StrictAdmission);
        revision
            .with_held_source(|raw| self.execute_raw(reader, raw, &expected, PageRequest::Open))
            .unwrap_or_else(|| PageWork::failed(Status::IoError))
    }
}

/// A cache-only probe that didn't finish retries once Loading shows, unless a
/// cancel interrupted it: that work is cancelled, not owed (#199).
fn retry_after_loading(revision: &mut ImportedRevision<'_>) -> PageWork {
    if revision.cancelled() { PageWork::CANCELLED } else { PageWork::VERIFICATION_PENDING }
}

impl<B: ParserBackend + ?Sized, L: ReaderPanelLoan, C: ReadScope + CachePresence, S: PhysicalSource>
    OwnerPages<S> for PhysicalOwner<'_, '_, '_, B, L, C>
{
    fn execute(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        source: &mut S,
        request: PageRequest,
    ) -> PageWork {
        let work = source.with_physical_source(|mut source| {
            // Open selects its source again itself.
            if request != PageRequest::Open {
                if let Err(work) = self.reload_source(reader, &mut source) {
                    return work;
                }
            }
            match source {
                PhysicalSourceRef::Revision(revision) => {
                    self.execute_revision(reader, revision, request, false)
                }
                PhysicalSourceRef::Reader(source) => self.execute_reader(reader, source, request),
                PhysicalSourceRef::Strict(source) => self.execute_strict(reader, source, request),
                PhysicalSourceRef::Pending(source) => self.execute_pending(reader, source, request),
            }
        });
        if work.result == Completion::Cancelled { self.cache.store.close_after_cancel(); }
        work
    }
    fn repaint(&mut self, reader: &mut PhasedReader<'_, '_>, source: &mut S)
        -> Result<Status, Cancelled> {
        let status = source.with_physical_source(|mut source| {
            if let Err(work) = self.reload_source(reader, &mut source) {
                return work.settle();
            }
            match source {
                PhysicalSourceRef::Revision(revision) => Ok(self.repaint_revision(reader, revision)),
                PhysicalSourceRef::Reader(source) => self.repaint_reader(reader, source),
                PhysicalSourceRef::Strict(source) => self.repaint_strict(reader, source),
                PhysicalSourceRef::Pending(source) => self.repaint_pending(reader, source),
            }
        });
        if status.is_err() { self.cache.store.close_after_cancel(); }
        status
    }
}

impl<B: ParserBackend + ?Sized, L: ReaderPanelLoan, C: ReadScope + CachePresence>
    PhysicalOwner<'_, '_, '_, B, L, C>
{
    /// Every action and repaint passes here before it touches its source, so
    /// nothing reads a source that a cancel closed until it reloads: a receipt
    /// reloads, a strict view hashes again or probes its new descriptor (#199).
    /// Both paths read the result the same way; a changed book closes the reader.
    fn reload_source(&mut self, reader: &mut PhasedReader<'_, '_>,
        source: &mut PhysicalSourceRef<'_, '_, '_>) -> Result<(), PageWork> {
        let reloaded = match source {
            PhysicalSourceRef::Strict(strict) => {
                match self.panel.with_parse(|scratch| strict.reload(reader, scratch)) {
                    Ok(reloaded) => reloaded,
                    Err(status) => return Err(PageWork::failed(status)),
                }
            }
            PhysicalSourceRef::Reader(source) => source.reload(),
            PhysicalSourceRef::Pending(pending) => pending.source.reload(),
            PhysicalSourceRef::Revision(_) => Ok(()),
        };
        reloaded.map_err(|interrupted| {
            if interrupted == Interrupted::Changed {
                reader.close();
            }
            PageWork::interrupted(interrupted)
        })
    }
}

impl<B: ParserBackend + ?Sized, L: ReaderPanelLoan, C: ReadScope + CachePresence>
    PhysicalOwner<'_, '_, '_, B, L, C>
{
    fn repaint_reader(&mut self, reader: &mut PhasedReader<'_, '_>,
        source: &mut ReaderSource<'_>) -> Result<Status, Cancelled> {
        let strict = match source.mode {
            ReaderSourceMode::Revision => false,
            ReaderSourceMode::Strict => true,
            ReaderSourceMode::MustReload(_) | ReaderSourceMode::Closed => return Ok(Status::NotFound),
        };
        source.revision.begin_immutable_read();
        let status = if strict {
            source.revision.with_held_source(|raw| {
                self.panel.with_frame(|frame| reader.repaint(raw, frame)).unwrap_or_else(|e| e)
            }).unwrap_or(Status::IoError)
        } else {
            self.panel.with_frame(|frame| reader.repaint(&mut source.revision, frame))
                .unwrap_or_else(|e| e)
        };
        // Exit runs even when repaint or frame acquisition failed.
        let fresh = source.revision.finish_immutable_read();
        // A cancel refuses the freshness read and the repaint's own reads. Like
        // an action's, it says nothing about the book: the source closes for a
        // reload and nothing latches (#199). Unsupported (no retained plan) read
        // nothing, so the owner's page action runs and meets the cancel itself.
        let refused = !fresh || !matches!(status, Status::Ok | Status::Unsupported);
        if refused && reader.opened() && source.revision.cancelled() {
            source.close_after_cancel();
            return Err(Cancelled);
        }
        if !fresh && status == Status::Ok {
            reject_changed_receipt(reader, &mut source.revision);
            source.mode = ReaderSourceMode::Closed;
            return Ok(Status::Stale);
        }
        Ok(status)
    }
}

struct PhysicalMenu<
    'a,
    'sel,
    'plan,
    'op,
    'bank,
    'cache,
    B: ParserBackend + ?Sized,
    L,
    C,
    S: PhysicalSource,
> {
    reader: &'a mut PhasedReader<'sel, 'plan>,
    revision: &'a mut S,
    resources: &'a mut PhysicalOwner<'op, 'bank, 'cache, B, L, C>,
}
impl<
    'sel,
    'plan,
    B: ParserBackend + ?Sized,
    L: ReaderPanelLoan,
    C: ReadScope + CachePresence,
    S: PhysicalSource,
> MenuOperation for PhysicalMenu<'_, 'sel, 'plan, '_, '_, '_, B, L, C, S>
{
    fn menu(&self) -> newt_phased_reader::ChapterMenuView<'_> {
        self.reader.chapter_menu()
    }
    fn shown(&self, owner: &ReaderOwnerState) -> newt_core::tracker::ShownPage {
        owner.position(self.reader).shown
    }
    fn prepare(&mut self, owner: &mut ReaderOwnerState, focus: u32) -> MenuPreparation {
        // A cancelled list load says nothing about the book, so nothing latches:
        // the reader keeps its page and the next action reloads (#199).
        let status = perform(self.reader, self.revision, PageRequest::Menu(focus), self.resources);
        if let Ok(status) = status {
            owner.menu_result(status);
        }
        MenuPreparation { status, valid: status == Ok(Status::Ok) && self.menu().info().valid }
    }
}
impl<B: ParserBackend + ?Sized, L: ReaderPanelLoan, C: ReadScope + CachePresence>
    PhysicalOwner<'_, '_, '_, B, L, C>
{
    pub fn prepare_ui_paint<S: PhysicalSource>(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        revision: &mut S,
        owner: &mut ReaderOwnerState,
        ui: &mut ReaderUiState,
        pass: ReaderPaintPass,
    ) -> PreparedReaderPaint {
        crate::compose::prepare_reader_ui_paint_with(
            &mut PhysicalMenu {
                reader,
                revision,
                resources: self,
            },
            owner,
            ui,
            pass,
        )
    }
    pub fn repaint_ui<S: PhysicalSource>(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        revision: &mut S,
        owner: &mut ExternalOwner,
        prepared: PreparedReaderPaint,
        now: u32,
    ) -> ReaderPaintWork {
        self.repaint_ui_drawn(reader, revision, owner, prepared, now)
            .unwrap_or(ReaderPaintWork::Cancelled)
    }
    fn repaint_ui_drawn<S: PhysicalSource>(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        revision: &mut S,
        owner: &mut ExternalOwner,
        prepared: PreparedReaderPaint,
        now: u32,
    ) -> Result<ReaderPaintWork, Cancelled> {
        let restore_pixels = !prepared.paint;
        let work = crate::compose::repaint_reader_ui_with(
            reader,
            &mut owner.state,
            prepared,
            &mut owner.tracker,
            now,
            |r| self.repaint(r, revision),
        ).drawn()?;
        if restore_pixels {
            // Retrying menu preparation used/cleared native storage. Recreate
            // the validated FIRST page even if retry latched a source failure.
            let status = self
                .panel
                .with_frame(|frame| reader.restore_retained_frame(frame))
                .unwrap_or_else(|e| e);
            if status != Status::Ok {
                owner.state.note_failure(status);
                owner.tracker.pause(now);
                return Ok(ReaderPaintWork::Complete { ready: false });
            }
        }
        Ok(work)
    }
    pub fn finish_ui_paint<S: PhysicalSource>(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        revision: &mut S,
        owner: &mut ExternalOwner,
        completion: ReaderPaintCompletion,
        now: u32,
    ) -> ReaderPaintWork {
        super::finish_owner_ui_paint_with(owner, reader, revision, completion, now, self)
    }
    /// Bounded original first-paint/retry sequence, using its private continuations.
    pub fn paint_ui<S: PhysicalSource>(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        revision: &mut S,
        owner: &mut ExternalOwner,
        ui: &mut ReaderUiState,
        now: u32,
    ) -> PhysicalPaint {
        let mut pass = ReaderPaintPass::First;
        loop {
            let prepared = self.prepare_ui_paint(reader, revision, &mut owner.state, ui, pass);
            let mut work = self.repaint_ui(reader, revision, owner, prepared, now);
            if let ReaderPaintWork::OwnerPaint { completion, .. } = work {
                work = self.finish_ui_paint(reader, revision, owner, completion, now);
            }
            match work {
                ReaderPaintWork::Complete { ready } => return PhysicalPaint(Painted::Drawn { ready }),
                ReaderPaintWork::Cancelled => return PhysicalPaint(Painted::Cancelled),
                ReaderPaintWork::RetryChapterMenu(retry) => {
                    pass = ReaderPaintPass::RetryAfterReadyPage(retry)
                }
                ReaderPaintWork::OwnerPaint { .. } => unreachable!("owner fallback completed"),
            }
        }
    }
    /// Call immediately after paint_ui, before any new parse loan. SDK inversion
    /// happens ONCE, after page AND chrome/overlay. Caller refreshes after return.
    pub fn compose_native(
        &mut self,
        reader: &PhasedReader<'_, '_>,
        owner: &ExternalOwner,
        ui: &ReaderUiState,
        settings: ViewSettings,
        info: &DeviceInfo<'_>,
        progress: &Progress,
        painted: PhysicalPaint,
        book: Option<&BookStatsView>,
    ) -> Result<Composed, Status> {
        let Painted::Drawn { ready } = painted.0 else { return Ok(Composed::Cancelled) };
        self.panel.with_frame(|frame| {
            compose_reader_ui(
                reader,
                &owner.state,
                ui,
                settings,
                info,
                progress,
                ready,
                frame,
                owner.chapter_failure(),
                owner.last_chapter_failure(),
                book,
            );
            for byte in frame {
                *byte = !*byte;
            }
            Composed::Drawn
        })
    }
}
