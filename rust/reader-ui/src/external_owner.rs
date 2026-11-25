//! MIT. External-book reader owner policy.
//! Source-only preparation; no source, parser, Action or frame is retained.
use crate::*;
use newt_core::{
    progress::{Locator, Progress},
    tracker::{ReadingDirection, ReadingTracker},
    xml_session::ParserBackend,
    zip_container::Status,
    zip_window::source::BookSource,
};
use newt_phased_reader::{OperationReader, PhasedReader};

/// Pinned initialProgress (src/sample_reader.cpp:26-31): the saved progress
/// of this book, else a fresh start of it. Open and its warm spine share it.
pub use newt_core::progress::initial_progress;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestoreFailure {
    None,
    Unavailable,
    InvalidCheckpoint,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageRequest {
    Open,
    Menu(u32),
    Page(u32, u32),
    Anchor(u32, u32),
    Chapter(u32, u16),
}
impl PageRequest {
    pub fn spine(self) -> u32 {
        match self {
            Self::Open | Self::Menu(_) => 0,
            Self::Page(s, _) | Self::Anchor(s, _) | Self::Chapter(s, _) => s,
        }
    }
}
/// Capture completion is separate from publication. A cache hit with disjoint
/// frame backing is already published; it must never call publish a second time.
#[must_use]
pub struct PageWork {
    result: Completion,
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum Completion {
    Captured,
    Published,
    Failed(Status),
    VerificationPending,
}
impl PageWork {
    const CAPTURED: Self = Self {
        result: Completion::Captured,
    };
    const PUBLISHED: Self = Self {
        result: Completion::Published,
    };
    const VERIFICATION_PENDING: Self = Self {
        result: Completion::VerificationPending,
    };
    fn failed(status: Status) -> Self {
        Self {
            result: Completion::Failed(status),
        }
    }
}

/// The UI and owner share exactly this state. Tracker and the bounded failure
/// ring are policy data, not another readiness/status authority. Caller owns
/// PhasedReader's selections/plan, all backing, and allocation failure handling.
pub struct ExternalOwner {
    pub state: ReaderOwnerState,
    pub tracker: ReadingTracker,
    pub restore_failure: RestoreFailure,
    failures: [Option<ReaderChapterFailure>; 8],
    next_failure: usize,
    current: Option<ReaderChapterFailure>,
    failure_shown: bool,
    failed_malformed: bool,
}
impl Default for ExternalOwner {
    fn default() -> Self {
        Self {
            state: ReaderOwnerState::from_open(false, Status::NotFound, u32::MAX),
            tracker: ReadingTracker::new(Progress::default()),
            restore_failure: RestoreFailure::Unavailable,
            failures: [None; 8],
            next_failure: 0,
            current: None,
            failure_shown: false,
            failed_malformed: false,
        }
    }
}
impl ExternalOwner {
    pub fn chapter_failure(&self) -> Option<&ReaderChapterFailure> {
        self.current
            .as_ref()
            .filter(|_| self.failure_shown && self.state.status() != Status::Ok)
    }
    pub fn malformed_utf8(&self, reader: &PhasedReader<'_, '_>) -> bool {
        if self.state.has_progress() {
            reader.malformed_utf8
        } else {
            self.failed_malformed
        }
    }
    pub fn last_chapter_failure(&self) -> Option<&ReaderChapterFailure> {
        self.current.as_ref()
    }
    fn known(&self, spine: u32) -> Option<ReaderChapterFailure> {
        self.failures
            .iter()
            .flatten()
            .find(|f| f.spine == spine)
            .copied()
    }
    fn status(&mut self, status: Status) {
        self.state
            .navigation_completed(status, self.state.readable_spines());
    }
    fn note(&mut self, reader: &mut PhasedReader<'_, '_>, source: &mut dyn BookSource, spine: u32) {
        let failure = self.known(spine).unwrap_or(ReaderChapterFailure {
            spine,
            status: self.state.status(),
            invalid_text: reader.malformed_utf8,
        });
        self.current = Some(failure);
        self.failure_shown = self.state.chapter_notice()
            && reader.has_page()
            && reader.source_available(source);
        if self.failure_shown && self.known(spine).is_none() {
            self.failures[self.next_failure] = Some(failure);
            self.next_failure = (self.next_failure + 1) % 8;
        }
    }
    pub fn pause(&mut self, now: u32) {
        self.tracker.pause(now);
    }
    pub fn leave(&mut self, now: u32) {
        self.tracker.leave(now);
    }
    pub fn resume(&mut self, reader: &PhasedReader<'_, '_>, now: u32) {
        if self.state.opened() {
            self.tracker.enter(now, self.state.position(reader).shown);
        }
    }
    pub fn close(&mut self, reader: &mut PhasedReader<'_, '_>) {
        reader.close();
        *self = Self::default();
        self.restore_failure = RestoreFailure::None;
    }
    /// Disjoint-frame compatibility wrappers. Policy is shared with physical calls.
    #[allow(clippy::too_many_arguments)]
    pub fn open<S: BookSource, F>(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        source: &mut S,
        identity: u64,
        saved: Progress,
        policy: RestorePolicy,
        now: u32,
        frame: &mut Frame,
        page: F,
    ) -> bool
    where
        F: FnMut(&mut PhasedReader<'_, '_>, &mut S, PageRequest, &mut Frame) -> PageWork,
    {
        self.open_with(reader, source, identity, saved, policy, now, &mut DisjointPages { frame, page })
    }
    #[allow(clippy::too_many_arguments)]
    pub fn turn<S: BookSource, F>(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        source: &mut S,
        action: Action,
        now: u32,
        frame: &mut Frame,
        page: F,
    ) -> bool
    where
        F: FnMut(&mut PhasedReader<'_, '_>, &mut S, PageRequest, &mut Frame) -> PageWork,
    {
        self.turn_with(reader, source, action, now, &mut DisjointPages { frame, page })
    }
    #[allow(clippy::too_many_arguments)]
    pub fn jump<S: BookSource, F>(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        source: &mut S,
        chapter: u32,
        now: u32,
        frame: &mut Frame,
        page: F,
    ) -> ChapterJumpOutcome
    where
        F: FnMut(&mut PhasedReader<'_, '_>, &mut S, PageRequest, &mut Frame) -> PageWork,
    {
        self.jump_with(reader, source, chapter, now, &mut DisjointPages { frame, page })
    }
    #[allow(clippy::too_many_arguments)]
    pub fn paint<S: BookSource, F>(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        source: &mut S,
        frame: &mut Frame,
        now: u32,
        page: F,
    ) -> bool
    where
        F: FnMut(&mut PhasedReader<'_, '_>, &mut S, PageRequest, &mut Frame) -> PageWork,
    {
        self.paint_with(reader, source, now, &mut DisjointPages { frame, page })
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn open_with<S: BookSource, P: OwnerPages<S>>(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        source: &mut S,
        identity: u64,
        saved: Progress,
        policy: RestorePolicy,
        now: u32,
        pages: &mut P,
    ) -> bool {
        self.close(reader);
        self.restore_failure = RestoreFailure::Unavailable;
        let fallback = policy.allows_initial_fallback();
        if !fallback && saved.book != identity {
            self.restore_failure = RestoreFailure::InvalidCheckpoint;
            return false;
        }
        if identity == 0 {
            self.status(Status::Unsupported);
            return false;
        }
        let initial = initial_progress(saved, identity);
        self.tracker = ReadingTracker::new(initial);
        let result = perform(reader, source, PageRequest::Open, pages);
        if result != Status::Ok {
            self.reject_open(reader, result);
            return false;
        }
        self.state = ReaderOwnerState::from_open(true, Status::Ok, self.state.readable_spines());
        let mut locator = initial.locator;
        if locator.spine >= reader.selection().spines {
            if !fallback {
                self.restore_failure = RestoreFailure::InvalidCheckpoint;
                self.reject_open(reader, Status::NotFound);
                return false;
            }
            locator = Locator::default();
        }
        let (result, _, _) = if fallback {
            self.locate(
                reader,
                source,
                locator.spine,
                locator.character,
                false,
                pages,
            )
        } else {
            (
                perform(
                    reader,
                    source,
                    PageRequest::Anchor(locator.spine, locator.character),
                    pages,
                ),
                false,
                locator.spine,
            )
        };
        if result != Status::Ok {
            self.reject_open(reader, result);
            return false;
        }
        let p = reader.info();
        if !fallback
            && (p.spine != locator.spine
                || locator.character >= p.total_chars
                || (policy.requires_exact_page() && p.char_start != locator.character))
        {
            self.restore_failure = RestoreFailure::InvalidCheckpoint;
            self.reject_open(reader, Status::NotFound);
            return false;
        }
        self.restore_failure = RestoreFailure::None;
        self.resume(reader, now);
        true
    }
    fn reject_open(&mut self, reader: &mut PhasedReader<'_, '_>, result: Status) {
        self.failed_malformed = reader.malformed_utf8;
        reader.close();
        self.state = ReaderOwnerState::from_open(false, result, self.state.readable_spines());
    }
    #[allow(clippy::too_many_arguments)]
    fn locate<S: BookSource, P: OwnerPages<S>>(
        &self,
        reader: &mut PhasedReader<'_, '_>,
        source: &mut S,
        mut spine: u32,
        mut anchor: u32,
        backward: bool,
        pages: &mut P,
    ) -> (Status, bool, u32) {
        let mut skipped = 0;
        loop {
            if let Some(f) = self.known(spine) {
                return (f.status, false, spine);
            }
            let status = perform(
                reader,
                source,
                PageRequest::Anchor(spine, anchor),
                pages,
            );
            if status != Status::NotFound || !reader.empty_chapter {
                return (status, false, spine);
            }
            if if backward {
                spine == 0
            } else {
                spine.saturating_add(1) >= reader.selection().spines
            } {
                return (status, true, spine);
            }
            if skipped == 8 {
                return (status, false, spine);
            }
            skipped += 1;
            if backward {
                spine -= 1;
                anchor = u32::MAX;
            } else {
                spine += 1;
                anchor = 0;
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn turn_with<S: BookSource, P: OwnerPages<S>>(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        source: &mut S,
        action: Action,
        now: u32,
        pages: &mut P,
    ) -> bool {
        if !matches!(action, Action::Previous | Action::Next) {
            return false;
        }
        let mut route = PendingPages { inner: pages, pending: false };
        let next = action == Action::Next;
        let failed = self.chapter_failure().copied();
        if failed.is_none() && !self.state.opened() {
            return false;
        }
        let old = reader.info();
        let mut spine = failed.map_or(old.spine, |f| f.spine);
        let (result, boundary, attempted) = if failed.is_none()
            && (if next {
                old.page + 1 < old.page_count
            } else {
                old.page > 0
            }) {
            let target = if next { old.page + 1 } else { old.page - 1 };
            (
                perform(
                    reader,
                    source,
                    PageRequest::Page(spine, target),
                    &mut route,
                ),
                false,
                spine,
            )
        } else {
            if if next {
                spine.saturating_add(1) >= reader.selection().spines
            } else {
                spine == 0
            } {
                return false;
            }
            spine = if next { spine + 1 } else { spine - 1 };
            self.locate(
                reader,
                source,
                spine,
                if next { 0 } else { u32::MAX },
                !next,
                &mut route,
            )
        };
        if boundary {
            if !next || failed.is_some() || self.state.readable_spines() <= old.spine + 1 {
                return false;
            }
            self.state.navigation_completed(Status::Ok, old.spine + 1);
            self.status(Status::Ok);
            self.tracker.navigate(
                now,
                self.state.position(reader).shown,
                ReadingDirection::Forward,
            );
            return true;
        }
        if result == Status::NotFound && route.pending && failed.is_none() {
            self.status(Status::Ok);
            return false;
        }
        self.status(result);
        self.failure_shown = false;
        if result != Status::Ok {
            self.tracker.pause(now);
            self.note(reader, source, attempted);
            return failed.is_some();
        }
        if failed.is_some() {
            self.tracker.pause(now);
            self.resume(reader, now);
        } else {
            self.tracker.navigate(
                now,
                self.state.position(reader).shown,
                if next {
                    ReadingDirection::Forward
                } else {
                    ReadingDirection::Backward
                },
            );
        }
        true
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn jump_with<S: BookSource, P: OwnerPages<S>>(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        source: &mut S,
        chapter: u32,
        now: u32,
        pages: &mut P,
    ) -> ChapterJumpOutcome {
        if !self.state.opened() {
            return ChapterJumpOutcome::Failed;
        }
        if !reader.chapter_menu().row(chapter).valid {
            let menu_status = perform(reader, source, PageRequest::Menu(chapter), pages);
            self.state.menu_result(menu_status);
            if menu_status != Status::Ok {
                return ChapterJumpOutcome::Unresolved;
            }
        }
        let menu = reader.chapter_menu();
        let row = menu.row(chapter);
        if !row.valid || row.spine >= reader.selection().spines {
            return ChapterJumpOutcome::Unresolved;
        }
        let spine = row.spine;
        let toc = if row.fragment {
            row.toc
        } else {
            newt_reader_navigation::NO_TOC
        };
        let previous = self.state.status();
        let status = if let Some(f) = self.known(spine) {
            f.status
        } else {
            perform(
                reader,
                source,
                PageRequest::Chapter(spine, toc),
                pages,
            )
        };
        self.status(status);
        if status != Status::Ok {
            if status != Status::NotFound {
                self.note(reader, source, spine);
            }
            if reader.opened() {
                self.status(previous);
            }
            return if status == Status::NotFound && reader.opened() {
                ChapterJumpOutcome::Unresolved
            } else {
                ChapterJumpOutcome::Failed
            };
        }
        self.failure_shown = false;
        self.tracker.pause(now);
        self.tracker.relocate(self.state.position(reader).shown);
        ChapterJumpOutcome::Moved
    }
    pub(crate) fn paint_with<S: BookSource, P: OwnerPages<S>>(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        source: &mut S,
        now: u32,
        pages: &mut P,
    ) -> bool {
        if !self.state.opened() {
            return false;
        }
        let p = reader.info();
        let mut status = pages.repaint(reader, source);
        if status == Status::Unsupported {
            status = perform(
                reader,
                source,
                PageRequest::Page(p.spine, p.page),
                pages,
            );
        }
        self.status(status);
        if status != Status::Ok {
            self.tracker.pause(now);
        }
        status == Status::Ok
    }
}
// Private route: external callers cannot implement an owner-success authority.
pub(crate) trait OwnerPages<S: BookSource> {
    fn execute(&mut self, reader: &mut PhasedReader<'_, '_>, source: &mut S,
        request: PageRequest) -> PageWork;
    fn repaint(&mut self, reader: &mut PhasedReader<'_, '_>, source: &mut S) -> Status;
}
struct DisjointPages<'a, F> { frame: &'a mut Frame, page: F }
impl<S: BookSource, F> OwnerPages<S> for DisjointPages<'_, F>
where F: FnMut(&mut PhasedReader<'_, '_>, &mut S, PageRequest, &mut Frame) -> PageWork {
    fn execute(&mut self, reader: &mut PhasedReader<'_, '_>, source: &mut S,
        request: PageRequest) -> PageWork {
        let work = (self.page)(reader, source, request, self.frame);
        if work.result == Completion::Captured {
            completed(reader.publish(source, self.frame))
        } else { work }
    }
    fn repaint(&mut self, reader: &mut PhasedReader<'_, '_>, source: &mut S) -> Status {
        reader.repaint(source, self.frame)
    }
}
struct PendingPages<'a, P> { inner: &'a mut P, pending: bool }
impl<S: BookSource, P: OwnerPages<S>> OwnerPages<S> for PendingPages<'_, P> {
    fn execute(&mut self, reader: &mut PhasedReader<'_, '_>, source: &mut S,
        request: PageRequest) -> PageWork {
        let work = self.inner.execute(reader, source, request);
        self.pending |= work.result == Completion::VerificationPending;
        work
    }
    fn repaint(&mut self, reader: &mut PhasedReader<'_, '_>, source: &mut S) -> Status {
        self.inner.repaint(reader, source)
    }
}
fn completed(status: Status) -> PageWork {
    if status == Status::Ok { PageWork::PUBLISHED } else { PageWork::failed(status) }
}
fn perform<S: BookSource, P: OwnerPages<S>>(
    reader: &mut PhasedReader<'_, '_>, source: &mut S, request: PageRequest, pages: &mut P,
) -> Status {
    match pages.execute(reader, source, request).result {
        Completion::Published => Status::Ok,
        Completion::Failed(status) => status,
        Completion::VerificationPending => Status::NotFound,
        // Captured is consumed by the concrete route before it reaches policy.
        Completion::Captured => unreachable!("unpublished owner work"),
    }
}
/// Actual direct external route. OperationReader drops before returning to
/// perform; parse and frame may share backing only via the caller's safe lender.
pub fn capture_direct<B: ParserBackend + ?Sized>(
    op: OperationReader<'_, '_, '_, '_, '_, '_, B>,
    source: &mut dyn BookSource,
    request: PageRequest,
) -> PageWork {
    if request == PageRequest::Open {
        let status = open_direct(op, source);
        return if status == Status::Ok {
            PageWork::PUBLISHED
        } else {
            PageWork::failed(status)
        };
    }
    let status = match request {
        PageRequest::Open | PageRequest::Menu(_) => Status::Unsupported,
        PageRequest::Chapter(spine, toc) => op.reader.capture_chapter(
            source,
            spine,
            toc,
            op.metadata,
            op.parse,
            op.resources,
            op.toc,
            op.style,
            op.sources,
            op.slot,
            op.layout,
            Some(op.lease),
        ),
        PageRequest::Page(spine, target) | PageRequest::Anchor(spine, target) => op.reader.capture(
            source,
            spine,
            target,
            if matches!(request, PageRequest::Page(..)) {
                newt_phased_sink::Target::Page
            } else {
                newt_phased_sink::Target::Anchor
            },
            op.metadata,
            op.parse,
            op.resources,
            op.toc,
            op.style,
            op.sources,
            op.slot,
            op.layout,
            Some(op.lease),
        ),
    };
    drop(op);
    if status == Status::Ok {
        PageWork::CAPTURED
    } else {
        PageWork::failed(status)
    }
}
pub fn open_direct<B: ParserBackend + ?Sized>(
    mut op: OperationReader<'_, '_, '_, '_, '_, '_, B>,
    source: &mut dyn BookSource,
) -> Status {
    use newt_phased_reader::imported_revision::session::Reader;
    op.strict_open(source)
}

/// Prepared revision open uses the actual Session admission/promotion path.
/// Revision load/begin_import and its storage/SHA ownership remain caller work.
pub fn open_verified<
    B: ParserBackend + ?Sized,
    C: newt_core::page_cache::CacheStorage + newt_phased_reader::CachePresence,
>(
    revision: &mut newt_phased_reader::imported_revision::ImportedRevision<'_>,
    op: &mut OperationReader<'_, '_, '_, '_, '_, '_, B>,
    cache: &mut newt_phased_reader::OperationCache<'_, '_, C>,
    lent: Option<&mut [u8]>,
) -> Status {
    let mut session = newt_phased_reader::imported_revision::session::Session {
        revision,
        reader: op,
        cache,
    };
    match lent {
        Some(bytes) => session.finish_prepared_open_lent(bytes),
        None => session.finish_prepared_open(),
    }
}
/// Complete actual UI work only after handle_reader_ui consumed its operation.
/// This explicit continuation avoids retaining/reborrowing OperationReader's
/// Drop-bearing fields across frame reuse.
impl ExternalOwner {
    pub fn complete_ui<S: BookSource, F>(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        source: &mut S,
        ui: &mut ReaderUiState,
        result: ReaderUiResult,
        now: u32,
        frame: &mut Frame,
        page: F,
    ) -> ReaderUiResult
    where
        F: FnMut(&mut PhasedReader<'_, '_>, &mut S, PageRequest, &mut Frame) -> PageWork,
    {
        self.complete_ui_with(reader, source, ui, result, now,
            &mut DisjointPages { frame, page })
    }
    pub(crate) fn complete_ui_with<S: BookSource, P: OwnerPages<S>>(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        source: &mut S,
        ui: &mut ReaderUiState,
        mut result: ReaderUiResult,
        now: u32,
        pages: &mut P,
    ) -> ReaderUiResult {
        match result.work {
            ReaderUiWork::Turn(action) => {
                let recovering = self.chapter_failure().is_some();
                let changed = self.turn_with(reader, source, action, now, pages);
                result.work =
                    ReaderUiWork::Intent(if changed || (!recovering && !self.state.opened()) {
                        ReaderUiIntent::Repaint
                    } else {
                        ReaderUiIntent::None
                    });
            }
            ReaderUiWork::Jump(chapter) => {
                let outcome = self.jump_with(reader, source, chapter, now, pages);
                if outcome == ChapterJumpOutcome::Moved {
                    self.resume(reader, now);
                }
                result.work = ReaderUiWork::Intent(settle_chapter_jump(ui, outcome));
            }
            ReaderUiWork::Intent(_) => {}
        }
        result
    }
}

/// Completes the UI's private typed continuation using actual owner repaint /
/// resource-backed fallback. No caller may forge a successful paint completion.
#[allow(clippy::too_many_arguments)]
pub fn finish_owner_ui_paint<F, S: BookSource>(
    owner: &mut ExternalOwner,
    reader: &mut PhasedReader<'_, '_>,
    source: &mut S,
    completion: ReaderPaintCompletion,
    frame: &mut Frame,
    now: u32,
    page: F,
) -> ReaderPaintWork
where
    F: FnMut(&mut PhasedReader<'_, '_>, &mut S, PageRequest, &mut Frame) -> PageWork,
{
    finish_owner_ui_paint_with(owner, reader, source, completion, now,
        &mut DisjointPages { frame, page })
}
pub(crate) fn finish_owner_ui_paint_with<S: BookSource, P: OwnerPages<S>>(
    owner: &mut ExternalOwner, reader: &mut PhasedReader<'_, '_>, source: &mut S,
    completion: ReaderPaintCompletion, now: u32, pages: &mut P,
) -> ReaderPaintWork {
    owner.paint_with(reader, source, now, pages);
    let status = owner.state.status();
    finish_reader_ui_paint(
        reader,
        &mut owner.state,
        completion,
        status,
        &mut owner.tracker,
        now,
    )
}

impl ExternalOwner {
    /// Caller invokes only after the settings commit is confirmed, checkpoints
    /// first, then reopens the semantic anchor. Failed/unconfirmed commits do
    /// not enter this method; draft UI continues using the existing dispatch.
    pub fn settings_committed(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        settings: ViewSettings,
    ) {
        self.close(reader);
        reader.set_closed_layout(settings.page_layout());
    }
    /// Session has no retained operation, but the revision still owns source
    /// authority. Reset that authority explicitly before releasing its backing.
    pub fn close_verified(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        revision: &mut newt_phased_reader::imported_revision::ImportedRevision<'_>,
    ) {
        self.close(reader);
        revision.reset();
    }
}

/// Complete direct operation dispatch, including the on-demand menu preparation
/// used by jump. Optional mapping denial uses the canonical generic rows.
pub fn execute_direct<B: ParserBackend + ?Sized>(
    op: OperationReader<'_, '_, '_, '_, '_, '_, B>,
    source: &mut dyn BookSource,
    request: PageRequest,
    directory: &mut [u8; 512],
    mapping: &mut [u32],
) -> PageWork {
    if let PageRequest::Menu(focus) = request {
        let status = op.reader.prepare_chapter_menu(
            source,
            focus,
            op.metadata,
            op.parse,
            op.resources,
            op.toc,
            directory,
            mapping,
            Some(op.lease),
        );
        drop(op);
        if status == Status::Ok {
            PageWork::PUBLISHED
        } else {
            PageWork::failed(status)
        }
    } else {
        capture_direct(op, source, request)
    }
}

/// Invoke actual Session admission; callers cannot turn a status into success.
#[allow(clippy::too_many_arguments)]
pub fn import_verified<
    'source,
    B: ParserBackend + ?Sized,
    C: newt_core::page_cache::CacheStorage + newt_phased_reader::CachePresence,
>(
    mut op: OperationReader<'_, '_, '_, '_, '_, '_, B>,
    revision: &mut newt_phased_reader::imported_revision::ImportedRevision<'source>,
    source: &'source mut impl BookSource,
    cache: &mut newt_phased_reader::OperationCache<'_, '_, C>,
    mode: VerifiedOpen,
    lease: &mut impl newt_phased_reader::imported_revision::session::ParseLease,
) -> PageWork {
    use newt_phased_reader::imported_revision::session::Session;
    let mut session = Session {
        revision,
        reader: &mut op,
        cache,
    };
    let status = match mode {
        VerifiedOpen::Cold => session.import(source),
        VerifiedOpen::Prepared => {
            if session.revision.begin_import(source) {
                session.finish_prepared_open()
            } else {
                Status::IoError
            }
        }
        VerifiedOpen::Warm { digest, spine } => session.warm_open(source, digest, spine, lease),
    };
    if status == Status::Ok {
        PageWork::PUBLISHED
    } else {
        PageWork::failed(status)
    }
}
#[derive(Clone, Copy)]
pub enum VerifiedOpen {
    Cold,
    Prepared,
    Warm {
        digest: newt_phased_reader::imported_revision::Digest,
        spine: u32,
    },
}

#[path = "physical_owner.rs"]
mod physical;
pub use physical::*;
