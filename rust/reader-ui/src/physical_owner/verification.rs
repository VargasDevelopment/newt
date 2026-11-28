// SPDX-License-Identifier: MIT
//! Required short verification at the existing physical raw-fallback boundary.
use super::*;
use crate::physical_source::{PendingReaderSource, SourceOperations, StrictOperations};
use newt_phased_reader::imported_revision::session::Reader;

/// The one actual presence query just succeeded for this operation's selection.
/// Stage consumes that result without reopening the same native cache name.
struct StoredChapter<'op, 'cache, C> {
    cache: &'op mut PhasedChapterCache<'cache, C>,
    digest: &'op Digest,
    spine: u32,
}
impl<B: ParserBackend + ?Sized, L: ReaderPanelLoan, C: ReadScope + CachePresence>
    Cache<OperationPhased<'_, '_, '_, '_, B, L>> for StoredChapter<'_, '_, C>
{
    fn stage_original_verified(&mut self, reader: &mut OperationPhased<'_, '_, '_, '_, B, L>,
        revision: &mut ImportedRevision<'_>, spine: u32, plain: bool) -> Option<Result<(), Status>> {
        self.cache.stage_original_verified(reader, revision, spine, plain)
    }
    fn strict_open(&mut self, reader: &mut OperationPhased<'_, '_, '_, '_, B, L>, source: &mut dyn BookSource,
        digest: &Digest) -> Status {
        self.cache.strict_open(reader, source, digest)
    }
    fn stored(&mut self, digest: &Digest, spine: u32,
        reader: &mut OperationPhased<'_, '_, '_, '_, B, L>) -> bool {
        let _action = reader.reader.action_scope(ActionStage::CacheQuery);
        digest == self.digest && spine == self.spine
    }
    fn build(&mut self, source: &mut dyn BookSource, digest: &Digest, spine: u32,
        reader: &mut OperationPhased<'_, '_, '_, '_, B, L>)
        -> newt_phased_reader::imported_revision::session::Build {
        self.cache.build(source, digest, spine, reader)
    }
}

impl<B: ParserBackend + ?Sized, L: ReaderPanelLoan, C: ReadScope + CachePresence>
    PhysicalOwner<'_, '_, '_, B, L, C>
{
    pub(super) fn execute_strict(&mut self, reader: &mut PhasedReader<'_, '_>,
        source: &mut StrictOperations<'_>, request: PageRequest) -> PageWork {
        let work = (|| {
            if request == PageRequest::Open {
                if matches!(self.open, PhysicalOpen::Loaded { digest, .. }
                    | PhysicalOpen::Selected { digest, .. } if digest != source.expected()) {
                    return PageWork::failed(Status::Stale);
                }
                let authorized = match source.take_open_authorization() {
                    Some(authorized) => Ok(authorized),
                    None => self.panel.with_parse(|scratch| source.authorize_open(scratch)),
                };
                match authorized {
                    Ok(true) => {}
                    Ok(false) => return PageWork::failed(Status::IoError),
                    Err(error) => return PageWork::failed(error),
                }
            } else if !source.source_verified() {
                return PageWork::failed(Status::Stale);
            } else if self.cache_only
                && matches!(request, PageRequest::Page(..) | PageRequest::Anchor(..))
            {
                return PageWork::VERIFICATION_PENDING;
            }
            let digest = source.expected();
            self.execute_raw(reader, source, &digest, request)
        })();
        if request == PageRequest::Open && work.result != Completion::Published {
            reader.close();
        }
        work
    }

    pub(super) fn repaint_strict(&mut self, reader: &mut PhasedReader<'_, '_>,
        source: &mut StrictOperations<'_>) -> Result<Status, Cancelled> {
        if !source.source_verified() { return Ok(Status::Stale); }
        // The no-session route keeps main's behavior: a cancel is a status.
        Ok(self.panel.with_frame(|frame| reader.repaint(source, frame)).unwrap_or_else(|e| e))
    }

    pub(super) fn execute_pending(&mut self, reader: &mut PhasedReader<'_, '_>,
        source: &mut PendingReaderSource<'_, '_>, request: PageRequest) -> PageWork {
        if source.verified { return self.execute_reader(reader, source.source, request); }
        if matches!(source.source.mode, ReaderSourceMode::Closed | ReaderSourceMode::MustReload(_)) {
            return PageWork::failed(Status::NotFound);
        }
        let shown = Shown::of(reader);
        begin_receipt_action(&mut source.source.revision, request);
        let cached = match request {
            PageRequest::Open => self.open_pending(reader, source),
            PageRequest::Page(spine, _) | PageRequest::Anchor(spine, _)
                if spine == source.selected_spine && reader.opened()
                    && reader.info().spine == spine
                    && matches!(source.source.mode, ReaderSourceMode::Revision) => {
                self.execute_revision_body(reader, &mut source.source.revision, request, true, true)
            }
            _ => PageWork::VERIFICATION_PENDING,
        };
        let work = if cached.result == Completion::VerificationPending {
            if self.cache_only
                && matches!(request, PageRequest::Page(..) | PageRequest::Anchor(..))
            {
                cached
            } else {
                match self.finish_pending(source) {
                    Ok(true) => self.execute_reader_body(reader, source.source, request),
                    // An open has no shown page to keep: its retry stays pending.
                    Ok(false) | Err(Completion::Cancelled) if request == PageRequest::Open => cached,
                    Ok(false) => cached,
                    Err(result) => PageWork { result },
                }
            }
        } else { cached };
        finish_reader_action(reader, source.source, request, shown, work)
    }

    /// True when verified. A cancel stays its own outcome, never a missing
    /// page or a failure (#199).
    fn finish_pending(&mut self, source: &mut PendingReaderSource<'_, '_>) -> Result<bool, Completion> {
        let allowed = source.allowed;
        match self.panel.with_parse(|scratch| source.finish_verification(scratch, allowed)) {
            Ok(false) if !allowed() => Err(Completion::Cancelled),
            Ok(verified) => Ok(verified),
            Err(error) => Err(Completion::Failed(error)),
        }
    }

    fn open_pending(&mut self, reader: &mut PhasedReader<'_, '_>,
        source: &mut PendingReaderSource<'_, '_>) -> PageWork {
        let revision = &mut source.source.revision;
        let expected = match self.open {
            PhysicalOpen::Selected { digest, spine } | PhysicalOpen::Loaded { digest, spine }
                if spine == source.selected_spine => digest,
            _ => return PageWork::VERIFICATION_PENDING,
        };
        if !matches!(source.source.mode, ReaderSourceMode::Revision)
            || revision.provisional() || !revision.good() || *revision.digest() != expected {
            return PageWork::VERIFICATION_PENDING;
        }
        let mut op = OperationPhased::new(reader, self.storage, self.panel, self.backend,
            self.breaker);
        Reader::close(&mut op);
        op.reader.trace_route(newt_core::reader_work_profile::ReaderOpenRoute::WarmAttempt);
        if !self.cache.stored(&expected, source.selected_spine, &mut op) {
            op.reader.trace_route(newt_core::reader_work_profile::ReaderOpenRoute::WarmMiss);
            return PageWork::VERIFICATION_PENDING;
        }
        let mut cache = StoredChapter { cache: &mut self.cache, digest: &expected,
            spine: source.selected_spine };
        let mut session = Session { revision, reader: &mut op, cache: &mut cache };
        let status = session.stage_owned(source.selected_spine);
        if status == Status::Ok {
            session.reader.commit(source.selected_spine);
            session.reader.reader.trace_route(newt_core::reader_work_profile::ReaderOpenRoute::WarmHit);
            completed(status)
        } else if session.revision.good() {
            session.reader.reader.trace_route(newt_core::reader_work_profile::ReaderOpenRoute::WarmMiss);
            // Keep the held source: only the required full-D gate may authorize
            // the existing reimport/strict fallback after a warm miss.
            PageWork::VERIFICATION_PENDING
        } else {
            session.reader.reader.trace_route(newt_core::reader_work_profile::ReaderOpenRoute::WarmMiss);
            PageWork::failed(Status::Stale)
        }
    }

    pub(super) fn repaint_pending(&mut self, reader: &mut PhasedReader<'_, '_>,
        source: &mut PendingReaderSource<'_, '_>) -> Result<Status, Cancelled> {
        // Repaint only consumes the retained plan. A changed chapter/strict
        // source requires the same actual finish before any frame publication.
        if !source.verified && (reader.info().spine != source.selected_spine
            || !matches!(source.source.mode, ReaderSourceMode::Revision)) {
            match self.finish_pending(source) {
                Ok(true) => {}
                Ok(false) => return Ok(Status::NotFound),
                Err(Completion::Cancelled) => {
                    source.source.close_after_cancel();
                    return Err(Cancelled);
                }
                Err(result) => return PageWork { result }.settle(),
            }
        }
        self.repaint_reader(reader, source.source)
    }
}
