//! MIT. Operation adapters hold owners/scalars, never pre-created bank views.
use super::*;
use crate::imported_revision::{Digest, ImportedRevision, session};
use crate::phase_storage::{BuildLoan, INDEX_BYTES, ReadScope, Storage};
use newt_core::{chapter_layout::types::LineBreak, preferences::ViewSettings,
    admission::{self, ActionScope}, reader_action_profile::{ActionStage, CacheMissReason},
    reader_work_profile::ReaderOpenRoute};

#[cfg(test)]
#[path = "phase_session_cases.rs"]
mod tests;

#[path = "phase_session/operation.rs"]
mod operation;

pub struct OperationPhased<'op, 'sel, 'plan, 'bank, B: ParserBackend + ?Sized, L> {
    pub reader: &'op mut PhasedReader<'sel, 'plan>,
    pub storage: &'op mut Storage<'bank>,
    pub lender: &'op mut L,
    pub backend: &'op mut B,
    pub breaker: &'op mut dyn LineBreak,
    staged: Option<(u32, u64, bool, bool)>,
}
impl<B: ParserBackend + ?Sized, L: BuildLoan> session::Reader
    for OperationPhased<'_, '_, '_, '_, B, L>
{
    fn trace(&self, event: newt_core::admission::WorkEvent) {
        #[cfg(feature = "dev-usb")]
        if let Some(hook) = self.reader.on_yield { hook.observe(event); }
        #[cfg(not(feature = "dev-usb"))]
        let _ = event;
    }
    fn close(&mut self) {
        self.staged = None;
        self.reader.close();
        self.storage.discard_styles();
    }
    fn strict_open(&mut self, source: &mut dyn BookSource) -> Status {
        session::Reader::close(self);
        self.reader
            .open_phased(source, self.storage, self.lender, self.backend)
    }
    fn spine_count(&self) -> u32 {
        self.reader.selection().spines
    }
    fn stage(
        &mut self,
        _source: &mut dyn BookSource,
        _spine: u32,
        _plain: bool,
    ) -> Result<Entry, Status> {
        // The legacy external-lease API cannot compose the SDK owner safely.
        // Session::stage_owned is the supported route for this adapter.
        Err(Status::Unsupported)
    }
    fn stage_original_verified(
        &mut self,
        revision: &mut ImportedRevision<'_>,
        spine: u32,
        plain: bool,
    ) -> Option<Result<(), Status>> {
        Some(self.stage_verified_physical(revision, spine, plain))
    }
    fn discard_owned_stage(&mut self) {
        self.staged = None;
    }
    fn commit(&mut self, spine: u32) {
        let Some((staged, bytes, undeclared, plain)) = self.staged.take() else {
            return;
        };
        if staged != spine {
            return;
        }
        self.reader.active ^= 1;
        self.reader.source_bytes = bytes;
        self.reader.opened = true;
        self.reader.info.spine = spine;
        self.reader.proofs.selection = true;
        self.reader.styles_ready = !plain && self.storage.styles_ready(spine);
        self.reader.undeclared_styles = undeclared;
    }
}

pub struct PhasedChapterCache<'a, C> {
    pub store: &'a mut C,
    pub hash: &'a mut dyn crate::hash_provider::CacheHash,
    pub settings: ViewSettings,
}
impl<B: ParserBackend + ?Sized, L: BuildLoan, C: ReadScope + CachePresence>
    session::Cache<OperationPhased<'_, '_, '_, '_, B, L>> for PhasedChapterCache<'_, C>
{
    fn stored(
        &mut self,
        digest: &Digest,
        spine: u32,
        r: &mut OperationPhased<'_, '_, '_, '_, B, L>,
    ) -> bool {
        let _action = ActionScope::new(r.reader.on_yield, ActionStage::CacheQuery);
        if r.reader.plan.is_none()
            || !session_adapter::reader_settings_match(r.reader, self.settings)
        {
            return false;
        }
        let opening = !r.reader.has_page();
        session_adapter::cached_for_reader(r.reader, self.settings, digest, spine, self.hash, &mut |name| {
            self.store.present(name, opening)
        })
    }
    fn build(
        &mut self,
        source: &mut dyn BookSource,
        digest: &Digest,
        spine: u32,
        r: &mut OperationPhased<'_, '_, '_, '_, B, L>,
    ) -> session::Build {
        if r.staged.is_some()
            || !session_adapter::reader_settings_match(r.reader, self.settings)
        {
            return session::Build::Failed(Status::Unsupported);
        }
        if let Err(status) = CacheBuild::physical_ready(r.reader, spine) {
            return session::Build::Failed(status);
        }
        let (name, generation) = match session_adapter::identity(self.settings, digest, spine, self.hash, r.reader.selection().language) {
            Ok(v) => v,
            Err(e) => return session::Build::Failed(e),
        };
        let result = CacheBuild { name: &name, generation }.build_phased(
            r.reader, source, spine, self.store, r.storage, r.lender, r.backend, r.breaker,
        );
        if result.verified_empty { session::Build::Empty }
        else if result.status == Status::Ok { session::Build::Ready }
        else { session::Build::Failed(result.status) }
    }
}
impl<C: ReadScope + CachePresence> PhasedChapterCache<'_, C> {
    /// Capture only. SDK loan/query finish, then source guard, then retained
    /// plan copy. Caller checks revision exit before borrowing native frame and
    /// calling publish. A failed checked exit must discard_candidate.
    pub fn capture<B: ParserBackend + ?Sized, L: BuildLoan>(
        &mut self, source: &mut dyn BookSource, digest: &Digest, request: CacheRequest,
        r: &mut OperationPhased<'_, '_, '_, '_, B, L>,
    ) -> ShowResult {
        let trace = r.reader.on_yield;
        let CapturedPage { result, publication } = self.capture_traced(source, digest, request, r);
        drop(publication);
        if result.hit { admission::open_route(trace, ReaderOpenRoute::CacheHit); }
        result
    }
    pub fn capture_traced<'trace, B: ParserBackend + ?Sized, L: BuildLoan>(
        &mut self,
        source: &mut dyn BookSource,
        digest: &Digest,
        request: CacheRequest,
        r: &mut OperationPhased<'_, 'trace, '_, '_, B, L>,
    ) -> CapturedPage<'trace> {
        if r.reader.pending.is_some() {
            return CapturedPage::failed(Status::Unsupported);
        }
        let trace = r.reader.on_yield;
        let ready = r.reader.opened
            && r.reader.plan.is_some()
            && (request.staged || request.spine == r.reader.info.spine)
            && session_adapter::reader_settings_match(r.reader, self.settings);
        if ready {
            let guard = r.reader.validate(source);
            if guard != Status::Ok {
                return CapturedPage::failed(guard);
            }
            let language = if request.staged {
                r.reader.staged_selection().language
            } else {
                r.reader.selection().language
            };
            if let Ok((mut base, mut generation)) =
                session_adapter::identity(self.settings, digest, request.spine, self.hash, language)
            {
                let certified = cache_show::segment_of(r.reader, request.spine, request.target);
                if !request.anchor {
                    if let Some(segment) = certified {
                        let mut name = cache_show::expanded(&base);
                        if newt_core::cache_names::segment::segment_name(
                            segment,
                            &mut name,
                            &mut |b, d| self.hash.hash(b, d),
                        ) {
                            admission::open_route(trace, ReaderOpenRoute::CacheProbe);
                            let publication = ActionScope::new(trace, ActionStage::CacheQuery);
                            let hit = self.read_entry(source, &name[..69], generation, request, None, r) == Status::Ok;
                            if hit {
                                return CapturedPage { result: ShowResult { status: Status::Ok, hit: true }, publication };
                            }
                            drop(publication);
                            cache_show::restore_identity(
                                self.settings,
                                digest,
                                request.spine,
                                self.hash,
                                &mut base,
                                &mut generation,
                                language,
                            );
                        }
                    }
                }
                admission::open_route(trace, ReaderOpenRoute::CacheProbe);
                let publication = ActionScope::new(trace, ActionStage::CacheQuery);
                let mut status = self.read_entry(source, &base, generation, request, None, r);
                if status == Status::Ok {
                    return CapturedPage { result: ShowResult { status, hit: true }, publication };
                }
                drop(publication);
                admission::open_route(trace, ReaderOpenRoute::CacheMiss);
                if status != Status::Ok && request.anchor {
                    admission::open_route(trace, ReaderOpenRoute::CacheProbe);
                    let publication = ActionScope::new(trace, ActionStage::CacheQuery);
                    let mut name = cache_show::expanded(&base);
                    if newt_core::cache_names::segment::map_name(&mut name, &mut |b, d| {
                        self.hash.hash(b, d)
                    }) && self.store.present(&name[..69], !r.reader.has_page)
                    {
                        if let Some(located) =
                            self.locate(source, &name[..69], generation, request.target, r)
                        {
                            let identified = cache_show::restore_identity(
                                    self.settings,
                                    digest,
                                    request.spine,
                                    self.hash,
                                    &mut base,
                                    &mut generation,
                                    language,
                                );
                            name = cache_show::expanded(&base);
                            if identified
                                && newt_core::cache_names::segment::segment_name(
                                    located.segment,
                                    &mut name,
                                    &mut |b, d| self.hash.hash(b, d),
                                )
                            {
                                status = self.read_entry(
                                    source,
                                    &name[..69],
                                    generation,
                                    request,
                                    Some(located),
                                    r,
                                );
                            }
                        }
                    }
                    if status == Status::Ok {
                        return CapturedPage { result: ShowResult { status, hit: true }, publication };
                    }
                    drop(publication);
                }
            } else {
                admission::open_route(trace, ReaderOpenRoute::CacheMiss);
            }
        } else {
            admission::open_route(trace, ReaderOpenRoute::CacheMiss);
        }
        if request.cache_only {
            return CapturedPage::failed(Status::NotFound);
        }
        // Original capture remains the failed/missing-cache fallback. Cache
        // construction is an explicit Session build operation.
        let _action = ActionScope::new(trace, ActionStage::OrdinaryFallback);
        admission::open_route(trace, ReaderOpenRoute::OrdinaryPage);
        r.staged = None;
        let status = r.reader.capture_phased(
            source,
            request.spine,
            request.target,
            if request.anchor {
                Target::Anchor
            } else {
                Target::Page
            },
            r.storage,
            r.lender,
            r.backend,
            r.breaker,
        );
        CapturedPage { result: ShowResult { status, hit: false }, publication: _action }
    }
    fn read_entry<B: ParserBackend + ?Sized, L: BuildLoan>(
        &mut self,
        source: &mut dyn BookSource,
        name: &[u8],
        generation: u32,
        request: CacheRequest,
        located: Option<newt_core::cache_names::segment::SegmentAnchor>,
        r: &mut OperationPhased<'_, '_, '_, '_, B, L>,
    ) -> Status {
        let trace = r.reader.on_yield;
        let miss = cache_show::Miss::default();
        let bytes = r.reader.source_bytes;
        let certified = cache_show::segment_of(r.reader, request.spine, request.target);
        // Whole chapters and segments use the same checked opening scope.
        let opening = !r.reader.has_page();
        let reader = &mut r.reader;
        let staged = &mut r.staged;
        let result = r.storage.cache_page(
            r.lender,
            self.store,
            name,
            opening,
            generation,
            request,
            located,
            certified,
            source,
            bytes,
            &miss,
            |source, page, pages, chars| {
                if request.staged && !staged.is_some_and(|v| v.0 == request.spine) {
                    return Status::Unsupported;
                }
                if !request.staged {
                    let (left, right) = reader.selections.split_at_mut(1);
                    if reader.active == 0 {
                        right[0].copy_from(&left[0]);
                    } else {
                        left[0].copy_from(&right[0]);
                    }
                }
                let status = prepare_cached(reader, source, page, request.spine, pages, chars);
                if status == Status::Ok && request.staged {
                    reader.pending_undeclared = staged.take().map(|v| v.2);
                }
                status
            },
        );
        let status = match result {
            Ok(status) => {
                if status != Status::Ok { miss.set(CacheMissReason::Capture); }
                status
            }
            Err(e) => e,
        };
        miss.publish(trace);
        status
    }

}

fn prepare_cached(
    reader: &mut PhasedReader<'_, '_>,
    source: &mut dyn BookSource,
    page: newt_core::chapter_layout::types::Page<'_>,
    spine: u32,
    pages: u32,
    chars: u32,
) -> Status {
    use newt_core::chapter_layout::types::Sink;
    reader.malformed_utf8 = false;
    reader.empty_chapter = false;
    if !reader.opened || reader.plan.is_none() {
        return reader.failed(Status::NotFound);
    }
    let status = reader.validate(source);
    if status != Status::Ok {
        return reader.failed(status);
    }
    reader.proofs.plan = false;
    let mut metrics = ReaderAtlas;
    let mut sink = match SelectingSink::new(
        page.index,
        Target::Page,
        reader.landscape,
        &mut metrics,
        &ReaderAtlas,
        Output::Plan(reader.plan.as_deref_mut().unwrap()),
        None,
    ) {
        Ok(sink) => sink,
        Err(_) => return reader.failed(Status::Unsupported),
    };
    sink.page(page);
    let error = if sink.state.exceeded || !sink.state.found {
        Some(Status::Stale)
    } else if sink.state.overflow {
        Some(Status::OutOfMemory)
    } else if sink.state.malformed {
        Some(Status::ParseError)
    } else {
        None
    };
    let state = sink.state;
    drop(sink);
    if let Some(error) = error {
        return reader.failed(error);
    }
    let status = reader.validate(source);
    if status != Status::Ok {
        return reader.failed(status);
    }
    reader.pending = Some((spine, pages, chars, state));
    reader.pending_cached = true;
    Status::Ok
}

impl<'trace, B: ParserBackend + ?Sized, L: BuildLoan, C: ReadScope + CachePresence>
    session::Session<'_, '_, OperationPhased<'_, 'trace, '_, '_, B, L>, PhasedChapterCache<'_, C>>
{
    /// Actual revision Session owns the post-return check. Native receiptAction
    /// scope integration remains a separate firmware adapter obligation.
    pub fn capture_page_owned(&mut self, request: CacheRequest) -> ShowResult {
        let CapturedPage { result, publication } = self.capture_page_owned_traced(request);
        drop(publication);
        #[cfg(feature = "dev-usb")]
        if result.hit { session::Reader::trace(self.reader, newt_core::admission::WorkEvent::new(
            newt_core::admission::WorkKind::OpenRoute, ReaderOpenRoute::CacheHit as i32)); }
        result
    }
    pub fn capture_page_owned_traced(&mut self, request: CacheRequest) -> CapturedPage<'trace> {
        let before = session::stamp(self.revision);
        let digest = *self.revision.digest();
        let result = self
            .cache
            .capture_traced(self.revision, &digest, request, self.reader);
        if before != session::stamp(self.revision)
            || (result.result.status == Status::Ok && !self.revision.good())
        {
            self.reader.discard_candidate(Status::Stale);
            return CapturedPage::failed(Status::Stale);
        }
        result
    }
}

impl<B: ParserBackend + ?Sized, L: BuildLoan> OperationPhased<'_, '_, '_, '_, B, L> {
    fn stage_verified_physical(
        &mut self,
        revision: &mut ImportedRevision<'_>,
        spine: u32,
        plain: bool,
    ) -> Result<(), Status> {
        // Range verification needs the actual ImportedRevision capability, not
        // a downcast or a second source alias captured beside the same source.
        self.staged = None;
        if self.reader.pending.is_some() {
            return Err(Status::Unsupported);
        }
        let bytes = if self.reader.opened {
            self.reader.source_bytes
        } else {
            revision.size()
        };
        let status = validate(revision, bytes);
        if status != Status::Ok {
            return Err(status);
        }
        let r = &mut self.reader;
        let backend = &mut self.backend;
        let undeclared = self.storage.with_loan(self.lender, |phase| {
            let undeclared = r.select_physical(revision, spine, phase, *backend, plain)?;
            let entry = r.staged_selection().entry;
            admission::open_route(r.on_yield, ReaderOpenRoute::CanonicalSelected);
            {
                let _action = ActionScope::new(r.on_yield, ActionStage::ChapterVerify);
                let (at, count) = session::chapter_range(revision, &entry)?;
                if !revision.verify_range(at, count) { return Err(Status::Stale); }
            }
            Ok(undeclared)
        })?;
        let status = validate(revision, bytes);
        if status != Status::Ok {
            return Err(status);
        }
        self.staged = Some((spine, bytes, undeclared, plain));
        Ok(())
    }
}

impl<B: ParserBackend + ?Sized, L: BuildLoan, C: ReadScope + CachePresence>
    session::Session<'_, '_, OperationPhased<'_, '_, '_, '_, B, L>, PhasedChapterCache<'_, C>>
{
    pub fn prepare_menu_owned(&mut self, focus: u32) -> Status {
        let before = session::stamp(self.revision);
        let r = &mut self.reader;
        let status = r.reader.prepare_chapter_menu_phased(self.revision, focus,
            r.storage, r.lender, r.backend);
        if before != session::stamp(self.revision) || (status == Status::Ok && !self.revision.good()) {
            let active = r.reader.active;
            r.reader.selections[active ^ 1].navigation.invalidate_menu();
            return Status::Stale;
        }
        status
    }
    pub fn capture_chapter_owned(&mut self, spine: u32, toc: u16) -> Status {
        if self.revision.provisional() { return Status::Unsupported; }
        let before = session::stamp(self.revision);
        let r = &mut self.reader;
        let status = r.reader.capture_chapter_phased(self.revision, spine, toc,
            r.storage, r.lender, r.backend, r.breaker);
        if before != session::stamp(self.revision) || (status == Status::Ok && !self.revision.good()) {
            // Pure resolution failure preserves the previous plan. Only a
            // successful candidate or changed authority requires revocation.
            r.discard_candidate(Status::Stale);
            return Status::Stale;
        }
        status
    }
}

#[path = "producer_admission.rs"]
mod producer_admission;
