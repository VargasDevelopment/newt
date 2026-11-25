// SPDX-License-Identifier: MIT
//! One book's reader-owner operations inside HomeRuntime's mounted reader
//! scope: the source file, receipt and Pages owners, the PhasedReader and
//! ExternalOwner over the scope's sole panel. Fixture-only
//! cold entry closes its descriptors; reader_image lends persistent
//! source/session states to the same operation bodies, returning every short
//! panel/parser/native loan before Home or blocking display publication.
#[path = "reader_inputs.rs"]
mod inputs;
pub use inputs::{BookMemory, BookOpen, BookProviders};
#[path = "reader_sleep.rs"]
pub mod sleep;
#[path = "reader_checkpoint.rs"]
pub mod checkpoint;
#[path = "reader_statistics_panel.rs"]
pub(crate) mod panel_statistics;
#[path = "reader_book_metadata.rs"]
pub(crate) mod metadata;
#[path = "reader_book_import.rs"]
mod import;
pub(crate) use panel_statistics::stats_book;
#[path = "reading_stats_runtime.rs"]
mod statistics;
use crate::home_startup::{CardPort, preferences::ReaderPreferences};
use crate::nvs::{self, NvsBlobBackend};
use crate::reader_pages::ReaderPages;
use crate::reading_state_medium::MountedStateMedium;
use crate::stats_clock::read_panel_stats_session_clock;
pub use checkpoint::{CheckpointRestore, ReaderCheckpoint};
use newt_c3_board::ReaderPanel;
use newt_core::chapter_layout::types::LineBreak;
use newt_core::progress::Progress;
use newt_core::zip_container::Status;
use newt_core::{
    reading_stats::{BookFacts, BookFinish, book_finish},
    reading_stats_once::Sha256,
    reading_stats_service::{StatsResult, stored_book_facts},
    time_zone::TimeZone,
    tracker::{DayMarkState, ReadingStatsMarks},
};
use newt_native_io::UiIo;
use newt_native_storage::reading_state::{ReadingStateRouter, StateFacts};
use newt_native_storage::{Authority, Directory, Native};
use newt_phased_reader::imported_revision::{Digest, RevisionHasher};
use newt_phased_reader::phase_storage::{BANK_BYTES, Storage};
#[cfg(feature = "checkpoint-fixture")]
use newt_phased_reader::phase_storage::BuildLoan;
use newt_phased_reader::{PhasedChapterCache, PhasedReader};
pub use newt_c3_board::reader_memory::Runtime as BookRuntime;
use newt_reader_ui::{
    Action, DeviceInfo, ExternalOwner, PhysicalOpen, PhysicalOwner, ReaderFinishRow,
    ReaderUiIntent, ReaderUiNotice, ReaderUiState, ReaderUiWork, RestorePolicy, ViewSettings,
};
#[cfg(feature = "checkpoint-fixture")]
use {crate::sd_revision::SdRevisionStorage, newt_core::zip_window::source::BookSource,
    newt_native_storage::{chapter_cache::{ChapterCacheStorage, ChapterCacheState},
        sd_source::SdSourceState, source_file::FileBookSource, RouteState},
    newt_phased_reader::imported_revision::ImportedRevision, newt_reader_ui::initial_progress};
use newt_target_expat::Expat;
pub use statistics::ReadingStatsRuntime;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StatsCondition {
    #[default]
    Unknown,
    Seeded,
    Unseeded,
    Unsupported,
    Unavailable,
}

/// Firmware's NEWT_READER_CACHE_ROOT.
pub const CACHE_ROOT: &str = "/newt-sd/Newt/Cache";

pub struct BookRequest<'a> {
    /// Only the whole-book fixture opens a path; short operations hold a source.
    #[cfg(feature = "checkpoint-fixture")]
    pub path: &'a str,
    /// The catalog filename; its SHA-256 names the cache route (cacheRouteKey).
    pub filename: &'a str,
    pub open: BookOpen,
    pub identity: u64,
    pub saved: Progress,
    pub policy: RestorePolicy,
    pub info: &'a DeviceInfo<'a>,
    pub cancel: &'a dyn Authority,
    pub now: fn() -> u32,
    /// Already-published, call-scoped Home presentation.
    pub sleep: Option<&'a sleep::Scene<'a>>,
    /// The published choice of this mounted scope's Home clock.
    pub zone: TimeZone,
}

/// Only the actual clock choice and cancellation are observed by statistics;
/// exit/checkpoint work does not require a fabricated DeviceInfo or paint.
pub(crate) trait StatsRequest {
    fn zone(&self) -> TimeZone;
    fn cancelled(&self) -> bool;
    fn action_boundary(&self, stage: u8);
}
impl StatsRequest for BookRequest<'_> {
    fn zone(&self) -> TimeZone { self.zone }
    fn cancelled(&self) -> bool { self.cancel.cancelled() }
    fn action_boundary(&self, stage: u8) { self.cancel.action_boundary(stage); }
}
pub(crate) struct ExitRequest<'a> {
    pub(crate) zone: TimeZone,
    pub(crate) cancel: &'a dyn Authority,
}
impl StatsRequest for ExitRequest<'_> {
    fn zone(&self) -> TimeZone { self.zone }
    fn cancelled(&self) -> bool { self.cancel.cancelled() }
    fn action_boundary(&self, stage: u8) { self.cancel.action_boundary(stage); }
}

/// The existing request lends its authority only until this operation returns.
/// No hook or clock is retained by a checkpoint or by the reader.
struct StatsActionScope<'a> {
    #[cfg(feature = "dev-usb")]
    request: &'a dyn StatsRequest,
    #[cfg(not(feature = "dev-usb"))]
    borrow: core::marker::PhantomData<&'a ()>,
}
impl<'a> StatsActionScope<'a> {
    fn new(request: &'a impl StatsRequest, stage: crate::reader_action::ActionStage) -> Self {
        #[cfg(feature = "dev-usb")]
        {
            request.action_boundary(stage as u8 | crate::reader_action::ActionStage::Push as u8);
            Self { request }
        }
        #[cfg(not(feature = "dev-usb"))]
        { let _ = (request, stage); Self { borrow: core::marker::PhantomData } }
    }
}
impl Drop for StatsActionScope<'_> {
    fn drop(&mut self) {
        #[cfg(feature = "dev-usb")]
        self.request.action_boundary(crate::reader_action::ActionStage::Restore as u8);
    }
}

/// The caller's turn between owner operations. No owner operation runs while
/// it holds these: the panel to refresh, read-only reader state, and the
/// Pages owner, UiIo and card for State work on MountedStateMedium.
pub struct Between<'b, 'io, 'm, 'p, 'scope, 'sdk, 'route, P: CardPort> {
    pub panel: &'b mut ReaderPanel<'scope, 'sdk>,
    pub reader: &'b PhasedReader<'m, 'p>,
    pub owner: &'b ExternalOwner,
    pub ui: &'b ReaderUiState,
    pub pages: &'b mut ReaderPages<'route, P::Port<'io>>,
    pub io: &'io UiIo,
    pub card: &'b mut P,
    /// The frame composed since the caller's last turn, if any; refresh it.
    pub composed: Option<Result<(), Status>>,
    pub statistics: BookStatistics,
}

/// Each idle command asserts the caller's quiet/input admission. The same
/// cancellation authority is checked again before idle statistics effects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BookStep {
    Action(Action),
    Idle { checkpoint: bool, admitted: bool },
    Save,
    /// Settings reflow checkpoints the anchor locator and marks the reading
    /// destination as an anchor for the proposed settings; reopening runs
    /// after this loan ends.
    ReflowCheckpoint {
        anchor: newt_core::progress::Locator,
        settings: ViewSettings,
    },
    /// Mark as finished / not finished after verification and any owed
    /// selection/progress commit have settled.
    MarkFinished(newt_core::reading_stats::StatsMark),
    Sleep(newt_device_ui::HomePlane),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BookStatistics {
    pub lifecycle: ReadingStatsRuntime,
    pub marks: ReadingStatsMarks,
    pub finish: ReaderFinishRow,
    pub notice: ReaderUiNotice,
    pub checkpoint: checkpoint::CheckpointResult,
    pub accepted_open_owed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BookFailure {
    /// The filename could not be keyed.
    Route,
    /// The book could not be opened or bound.
    Source,
    /// Import, or receipt selection (its load, source verification,
    /// reimport or path checks), failed.
    Revision,
    /// The owner's open failed with this status.
    Open(Status),
}

#[derive(Clone, Copy, Debug)]
pub struct BookExit {
    /// The intent that left the reader; None when the caller stopped.
    pub intent: Option<ReaderUiIntent>,
    /// The clock read for the dispatched action, before its physical effects.
    pub action_at: u32,
    /// Completed idle duty renewing the one shared settle clock.
    pub idle_quiet_from: Option<u32>,
    /// The digest the book opened under, even if a later checked exit
    /// revoked the receipt (which zeroes the revision's own).
    pub digest: Digest,
    pub progress: Progress,
    pub statistics: BookStatistics,
    /// Frozen readerPreferenceUnavailable: a failed verification is not an
    /// assertion that the durable record stayed unchanged.
    pub preferences_unavailable: bool,
}

/// The sole book entry uses the mounted Home scope's State resources.
/// Each turn briefly reborrows NVS/facts so State work cannot overlap parsing.
/// A cold component caller supplies a fresh Committed checkpoint owner.
#[cfg(feature = "checkpoint-fixture")]
#[allow(clippy::too_many_arguments)]
pub fn with_book_checkpoint<'io, 'm, 'p, 'scope, 'sdk, N, P: CardPort, F>(
    panel: &mut ReaderPanel<'scope, 'sdk>,
    io: &'io UiIo,
    card: &mut P,
    nvs: &mut N,
    facts: &mut StateFacts,
    preferences: &mut ReaderPreferences,
    request: &BookRequest<'_>,
    providers: BookProviders<'m>,
    memory: BookMemory<'m, 'p>,
    checkpoint: &mut ReaderCheckpoint,
    next: F,
    pause: &mut ProducerPauseFn<'_, 'io, N, P>,
) -> Result<BookExit, BookFailure>
where
    N: nvs::Native,
    P::Port<'io>: Directory + newt_native_storage::page_entries::PageEntries,
    F: FnMut(
        Between<'_, 'io, 'm, 'p, 'scope, 'sdk, '_, P>,
        &mut N,
        &mut StateFacts,
    ) -> Option<BookStep>,
{
    let mut state = MountedCheckpoint {
        checkpoint,
        nvs,
        facts,
    };
    let mut key = [0; 64];
    let key = route_key(request.filename, &mut *providers.identity, &mut key)
        .ok_or(BookFailure::Route)?;
    let route_state = RouteState::new(CACHE_ROOT, key);
    let route = route_state.lend(request.cancel, request.now);
    let mut source_native = card.port(io);
    let mut receipt_native = card.port(io);
    let mut pages_native = card.port(io);
    let mut file_state = SdSourceState::new();
    let mut file = file_state.lend(&mut source_native);
    let mut receipt_state = crate::sd_revision::SdRevisionState::new(&mut receipt_native, &route);
    let mut receipt = SdRevisionStorage::lend(&mut receipt_state, &mut receipt_native, &route)
        .map_err(|_| BookFailure::Route)?;
    let mut pages_state = ChapterCacheState::new();
    let mut pages = ReaderPages(ChapterCacheStorage::lend(&mut pages_state, &mut pages_native, &route)
        .map_err(|_| BookFailure::Route)?);
    let mut source = FileBookSource::new(Some(request.cancel));
    let result = if file.open(request.path) && source.bind(&mut file) {
        read_book(
            panel,
            io,
            card,
            request,
            providers,
            memory,
            &mut source,
            &mut receipt,
            &mut pages,
            preferences,
            next,
            &mut state,
            pause,
        )
    } else {
        Err(BookFailure::Source)
    };
    // Pinned close(): read_book dropped the revision; then the receipt and
    // Pages descriptors, then the outer owner's source loan and file.
    receipt.close();
    let mut access = pages.0.access();
    access.close();
    access.forget_absent();
    source.release();
    file.close();
    result
}

/// The revision borrows the source and receipt, so it ends here, before
/// the outer entry closes them.
#[cfg(feature = "checkpoint-fixture")]
#[allow(clippy::too_many_arguments)]
fn read_book<'io, 'm, 'p, 'scope, 'sdk, 'route, P: CardPort, F, N: nvs::Native>(
    panel: &mut ReaderPanel<'scope, 'sdk>,
    io: &'io UiIo,
    card: &mut P,
    request: &BookRequest<'_>,
    providers: BookProviders<'m>,
    memory: BookMemory<'m, 'p>,
    source: &mut dyn BookSource,
    receipt: &mut SdRevisionStorage<'route, P::Port<'io>>,
    pages: &mut ReaderPages<'route, P::Port<'io>>,
    preferences: &mut ReaderPreferences,
    next: F,
    checkpoint: &mut MountedCheckpoint<'_, N>,
    pause: &mut ProducerPauseFn<'_, 'io, N, P>,
) -> Result<BookExit, BookFailure>
where
    P::Port<'io>: Directory + newt_native_storage::page_entries::PageEntries,
    F: FnMut(
        Between<'_, 'io, 'm, 'p, 'scope, 'sdk, '_, P>,
        &mut N,
        &mut StateFacts,
    ) -> Option<BookStep>,
{
    let BookProviders {
        whole,
        block,
        identity,
        breaker,
        stats_sha256,
        on_yield,
        producer,
    } = providers;
    let BookMemory {
        bank,
        runtime,
        receipt: scratch,
        producer_ledger,
    } = memory;
    let mut revision = ImportedRevision::compact(receipt, whole, block, scratch);
    let mut saved = request.saved;
    let mut book_identity = request.identity;
    let open = match request.open {
        BookOpen::Import => {
            if revision.begin_import(source) {
                if let Some(restored) = checkpoint.restore(io, card, pages, revision.digest()) {
                    saved = restored;
                    book_identity = restored.book;
                }
                Ok(PhysicalOpen::Prepared)
            } else {
                Err(BookFailure::Revision)
            }
        }
        BookOpen::Receipt(expected) => {
            let allowed = || !request.cancel.cancelled();
            // Pinned openExternal: the session warm-opens initialProgress's
            // spine. On any miss Session strictly reimports the source it
            // holds, then the owner normalizes the locator (openRevision).
            match revision.select_source(source, &mut bank[..], &allowed) {
                None => Err(BookFailure::Revision),
                // prepareRetainedSource: the restored checkpoint's digest must
                // name the selected source exactly, or the open is Stale.
                Some((_, digest)) if digest != expected => Err(BookFailure::Open(Status::Stale)),
                Some((imported, _)) => {
                    if let Some(restored) = checkpoint.restore(io, card, pages, &expected) {
                        saved = restored;
                        book_identity = restored.book;
                    }
                    let initial = initial_progress(saved, book_identity);
                    Ok(if imported {
                        PhysicalOpen::Prepared
                    } else {
                        PhysicalOpen::Selected {
                            digest: expected,
                            spine: initial.locator.spine,
                        }
                    })
                }
            }
        }
    };
    let open = match open {
        Ok(open) => open,
        Err(failure) => {
            // Also aborts a reimport's provisional receipt.
            revision.reset();
            return Err(failure);
        }
    };
    let revision = newt_reader_ui::ReaderSource::new(revision);
    let mut revision = revision;
    let mut storage = Storage::new(bank);
    runtime.reader.on_yield = on_yield;
    let mut resources = Resources {
        panel, pages, storage: &mut storage, parser: Expat::default(), breaker,
        hash: identity, settings: preferences.settings(), open, recovery: None,
    };
    let digest = revision.digest();
    let mut statistics = ReadingStatsRuntime::default();
    let mut accepted_open = false;
    let mut recovery = None;
    // Fixture-only: the imported revision owns this source loan; BookSource
    // cannot release its descriptor. Missing indexes that spill are refused;
    // product opens use SelectedSource::with_index_source/open_releasing.
    let indexed = newt_phased_reader::index_build::open(&mut revision, &digest, resources.storage,
        resources.panel, &mut resources.parser, resources.pages, on_yield,
        newt_phased_reader::index_build::Verification::Verified).map_err(BookFailure::Open)?;
    let result = run::drive(io, card, request, &mut runtime.reader, &mut runtime.owner,
        &mut runtime.ui, &mut revision, &mut resources,
        preferences, checkpoint, stats_sha256, &mut statistics, &mut accepted_open,
        &mut recovery, book_identity, saved, digest, &producer, Some(producer_ledger),
        Some(&indexed), true, run::Route::WholeLoop, true, None, None, false, None, next, pause);
    revision.reset();
    result
}

#[path = "reader_book_resources.rs"]
mod resources;
pub(crate) use resources::Resources;
#[path = "reader_pending_recovery.rs"]
mod pending_recovery;
pub(crate) use pending_recovery::PendingRecovery;
#[path = "reader_book_visit.rs"]
mod visit;
pub(crate) use visit::VisitParts;

/// cacheRouteKey: the lowercase hex SHA-256 of the catalog filename.
pub(crate) fn route_key<'k>(
    filename: &str,
    hash: &mut dyn newt_phased_reader::hash_provider::CacheHash,
    key: &'k mut [u8; 64],
) -> Option<&'k str> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut digest = [0; 32];
    if !hash.hash(filename.as_bytes(), &mut digest) {
        return None;
    }
    for (pair, byte) in key.chunks_exact_mut(2).zip(digest) {
        pair[0] = HEX[usize::from(byte >> 4)];
        pair[1] = HEX[usize::from(byte & 15)];
    }
    core::str::from_utf8(key).ok()
}

/// State loans are short: a caller may use the same NVS/facts during a turn.
pub(crate) struct MountedCheckpoint<'a, N> {
    checkpoint: &'a mut ReaderCheckpoint,
    nvs: &'a mut N,
    facts: &'a mut StateFacts,
}
impl<'a, N> MountedCheckpoint<'a, N> {
    pub(crate) fn new(checkpoint: &'a mut ReaderCheckpoint, nvs: &'a mut N, facts: &'a mut StateFacts) -> Self {
        Self { checkpoint, nvs, facts }
    }
}

fn checkpoint_verified(checkpoint: &ReaderCheckpoint) -> bool {
    let result = checkpoint.result();
    result.progress == checkpoint::Outcome::Verified
        && result.destination != checkpoint::Outcome::Unconfirmed
}

fn observe(
    lifecycle: &ReadingStatsRuntime,
    owner: &ExternalOwner,
    ui: &ReaderUiState,
    checkpoint: &ReaderCheckpoint,
    accepted_open_owed: bool,
) -> BookStatistics {
    BookStatistics {
        lifecycle: *lifecycle,
        marks: *owner.tracker.stats_marks(),
        finish: ui.finish,
        notice: ui.notice,
        checkpoint: checkpoint.result(),
        accepted_open_owed,
    }
}

enum StatsOperation {
    RestoreFinish,
    Lease { idle: bool },
    Fold,
    Idle,
    Mark { opening: bool, mark: newt_core::reading_stats::StatsMark },
}

impl<N: nvs::Native> MountedCheckpoint<'_, N> {
    fn publish_open<'io, P: CardPort>(
        &mut self,
        io: &'io UiIo,
        card: &mut P,
        resources: &mut Resources<'_, '_, '_, '_, '_, P::Port<'io>>,
        reader: &PhasedReader<'_, '_>,
        owner: &mut ExternalOwner,
        ui: &mut ReaderUiState,
        digest: &Digest,
        source_verified: bool,
        request: &BookRequest<'_>,
        stats_sha256: Sha256<'_>,
        runtime: &mut ReadingStatsRuntime,
        opened_at: u32,
        accepted_save: bool,
        session_present: bool,
    ) -> (bool, Option<u32>)
    where
        P::Port<'io>: Directory,
    {
        self.stats(
            io,
            card,
            resources,
            reader,
            owner,
            ui,
            digest,
            source_verified,
            request,
            stats_sha256,
            runtime,
            opened_at,
            StatsOperation::RestoreFinish,
        );
        // publishReaderOpen anchors once, even if the first checkpoint cannot
        // yet be acknowledged. The RTC read itself touches no State storage.
        if owner.tracker.stats_marks().day.state == DayMarkState::Empty {
            let at = (request.now)();
            let mut utc = 0;
            read_panel_stats_session_clock(resources.panel, request.zone, Some(&mut utc));
            owner
                .tracker
                .stats_marks_mut()
                .day
                .anchor(utc, at, request.zone);
        }
        let mut saved_at = None;
        if accepted_save && source_verified {
            // The lease sees the loaded acknowledgement before the accepted-open
            // checkpoint changes it. An accepted open does not close live time.
            self.stats(
                io,
                card,
                resources,
                reader,
                owner,
                ui,
                digest,
                source_verified,
                request,
                stats_sha256,
                runtime,
                opened_at,
                StatsOperation::Lease { idle: false },
            );
            let mut peers = newt_core::progress_store::PeerLocators::default();
            let at = (request.now)();
            saved_at = Some(at);
            let candidate = *owner.tracker.snapshot(at);
            self.save_admitted_core(
                io,
                card,
                resources,
                digest,
                candidate,
                false,
                true,
                owner.state.opened() && source_verified,
                Some(&mut peers),
                request,
                None,
            );
            if self.checkpoint.result().progress == checkpoint::Outcome::Verified {
                if let Some(saved) = self.checkpoint.acknowledged_for(digest) {
                    metadata::publish(resources.pages, reader, digest, &saved, &peers,
                        source_verified, false, session_present, false, false, &|| false);
                }
                self.destination(io, card, resources.pages, digest, resources.settings,
                    true, false, source_verified, request);
            }
            PendingRecovery::complete_checkpoint(&mut resources.recovery, self.checkpoint,
                candidate, resources.settings, digest);
        }
        if checkpoint_verified(self.checkpoint) && self.checkpoint.notice().is_none() {
            resources.recovery = None;
        }
        if !accepted_save {
            // A recovery open does not invent an accepted-open save debt.
            // acceptOpen retires its pending page only when uncertainty cleared.
            if self.checkpoint.notice().is_none() { resources.recovery = None; }
            (false, saved_at)
        } else { (!source_verified || !checkpoint_verified(self.checkpoint), saved_at) }
    }

    /// Every operation constructs a short State loan after parser/frame loans
    /// have ended; MountedStateMedium releases Pages before each transaction.
    #[allow(clippy::too_many_arguments)]
    fn stats<'io, P: CardPort>(
        &mut self,
        io: &'io UiIo,
        card: &mut P,
        resources: &mut Resources<'_, '_, '_, '_, '_, P::Port<'io>>,
        reader: &PhasedReader<'_, '_>,
        owner: &mut ExternalOwner,
        ui: &mut ReaderUiState,
        digest: &Digest,
        source_verified: bool,
        request: &impl StatsRequest,
        sha256: Sha256<'_>,
        runtime: &mut ReadingStatsRuntime,
        now: u32,
        operation: StatsOperation,
    ) -> bool
    where
        P::Port<'io>: Directory,
    {
        let book =
            panel_statistics::stats_book(self.checkpoint, digest, source_verified, owner, reader);
        let ready = owner.state.opened() && !request.cancelled();
        let mut access = resources.pages.0.access();
        let mut medium = MountedStateMedium::new(io, Some(&mut access));
        let mut native = card.port(io);
        let nvs = NvsBlobBackend::new(&mut *self.nvs);
        let mut store = ReadingStateRouter::new(nvs, &mut *self.facts, &mut medium, &mut native);
        let mut clock = || {
            let mut utc = 0;
            let clock =
                read_panel_stats_session_clock(resources.panel, request.zone(), Some(&mut utc));
            (clock, utc)
        };
        let mut cancelled = || request.cancelled();
        match operation {
            StatsOperation::RestoreFinish => {
                // Frozen noteHomeFinished reads the selected book's committed
                // finish at every open, while its file check may still be
                // running (a fast open): a read, never a write (#319).
                let mut facts = BookFacts::default();
                let finish = if stored_book_facts(&mut store, digest, &mut facts)
                    == StatsResult::Done
                {
                    book_finish(&facts)
                } else {
                    BookFinish::No
                };
                statistics::show_finish(ui, finish);
                true
            }
            StatsOperation::Lease { idle } => runtime.lease(
                &mut store,
                book.as_ref(),
                owner.tracker.stats_marks_mut(),
                ui,
                now,
                request.zone(),
                clock,
                if idle { Some(&mut cancelled) } else { None },
                sha256,
            ),
            StatsOperation::Fold => runtime.fold(
                &mut store,
                book.as_ref(),
                owner.tracker.stats_marks_mut(),
                ui,
                now,
                request.zone(),
                clock,
                None,
                sha256,
            ),
            StatsOperation::Idle => runtime.idle(
                &mut store,
                book.as_ref(),
                owner.tracker.stats_marks_mut(),
                ui,
                now,
                request.zone(),
                ready,
                self.checkpoint.page_unconfirmed(),
                clock,
                Some(&mut cancelled),
                sha256,
            ),
            StatsOperation::Mark { opening, mark } => {
                runtime.mark(
                    &mut store,
                    book.as_ref(),
                    owner.tracker.stats_marks_mut(),
                    ui,
                    now,
                    request.zone(),
                    opening,
                    mark,
                    self.checkpoint.page_unconfirmed(),
                    &mut clock,
                    sha256,
                );
                true
            }
        }
    }

    #[cfg(feature = "checkpoint-fixture")]
    fn restore<'io, P: CardPort>(
        &mut self,
        io: &'io UiIo,
        card: &mut P,
        pages: &mut ReaderPages<'_, P::Port<'io>>,
        digest: &[u8; 32],
    ) -> Option<Progress> {
        let mut access = pages.0.access();
        let mut medium = MountedStateMedium::new(io, Some(&mut access));
        let mut native = card.port(io);
        let nvs = NvsBlobBackend::new(&mut *self.nvs);
        let mut store = ReadingStateRouter::new(nvs, &mut *self.facts, &mut medium, &mut native);
        Some(self.checkpoint.restore(&mut store, digest))
    }
    #[cfg(feature = "checkpoint-fixture")]
    fn save<'io, P: CardPort>(
        &mut self,
        io: &'io UiIo,
        card: &mut P,
        resources: &mut Resources<'_, '_, '_, '_, '_, P::Port<'io>>,
        digest: &[u8; 32],
        candidate: Progress,
        reading: bool,
        accepted_open: bool,
        ready: bool, request: &impl StatsRequest,
    ) {
        self.save_with_peers(io, card, resources, digest, candidate, reading,
            accepted_open, ready, None, request);
    }

    pub(crate) fn save_with_peers<'io, P: CardPort>(
        &mut self,
        io: &'io UiIo,
        card: &mut P,
        resources: &mut Resources<'_, '_, '_, '_, '_, P::Port<'io>>,
        digest: &[u8; 32],
        candidate: Progress,
        reading: bool,
        accepted_open: bool,
        ready: bool,
        peers: Option<&mut newt_core::progress_store::PeerLocators>,
        request: &impl StatsRequest,
    ) {
        self.save_admitted_core(io, card, resources, digest, candidate, reading,
            accepted_open, ready, peers, request, None);
        PendingRecovery::complete_checkpoint(&mut resources.recovery, self.checkpoint,
            candidate, resources.settings, digest);
    }

    fn save_admitted_core<'io, P: CardPort>(
        &mut self, io: &'io UiIo, card: &mut P,
        resources: &mut Resources<'_, '_, '_, '_, '_, P::Port<'io>>,
        digest: &[u8; 32], candidate: Progress, reading: bool,
        accepted_open: bool, ready: bool,
        peers: Option<&mut newt_core::progress_store::PeerLocators>,
        request: &impl StatsRequest,
        mut admission: Option<&mut newt_core::durable::Admission<'_>>,
    ) {
        let _action = StatsActionScope::new(request, crate::reader_action::ActionStage::ProgressPublish);
        if !ready {
            self.checkpoint.refuse();
        } else {
            let mut access = resources.pages.0.access();
            let mut medium = MountedStateMedium::new(io, Some(&mut access));
            let mut native = card.port(io);
            let nvs = NvsBlobBackend::new(&mut *self.nvs);
            let mut store = ReadingStateRouter::new(nvs, &mut *self.facts, &mut medium, &mut native);
            self.checkpoint.save_admitted(&mut store, digest, candidate,
                resources.settings, reading, accepted_open, peers, admission.as_deref_mut());
        }
        if admission.is_some_and(|admission| admission.deferred) { return; }
    }

    #[allow(clippy::too_many_arguments)]
    fn destination<'io, P: CardPort>(
        &mut self,
        io: &'io UiIo,
        card: &mut P,
        pages: &mut ReaderPages<'_, P::Port<'io>>,
        digest: &Digest,
        settings: ViewSettings,
        reading: bool,
        anchor: bool,
        source_verified: bool, request: &impl StatsRequest,
    ) {
        let _action = StatsActionScope::new(request, crate::reader_action::ActionStage::DESTINATION);
        let mut access = pages.0.access();
        let mut medium = MountedStateMedium::new(io, Some(&mut access));
        let mut native = card.port(io);
        let nvs = NvsBlobBackend::new(&mut *self.nvs);
        let mut store = ReadingStateRouter::new(nvs, &mut *self.facts, &mut medium, &mut native);
        self.checkpoint
            .publish_destination(&mut store, digest, settings, reading, anchor, source_verified);
    }
}

#[path = "reader_producer.rs"]
mod producer;
pub use producer::{ProducerCallbacks, ProducerEntry, ProducerPause, ProducerPauseFn};
#[cfg(all(feature = "rust-app", feature = "dev-usb"))]
pub use producer::ProducerEvent;

#[cfg(feature = "checkpoint-fixture")]
#[path = "reader_book_preferences.rs"]
mod preferences;
#[path = "reader_book_run.rs"]
pub(crate) mod run;

#[path = "reader_book_exit.rs"]
mod exit;
