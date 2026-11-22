// SPDX-License-Identifier: MIT
//! Persistent selection/source state with reader backing admitted per open.
//! Physical helpers borrow app_image's one display receipt; no view retains
//! the Board, a parser, a backend or a policy callback between operations.
use core::ffi::CStr;
use newt_c3_board::reader_memory::{ReaderMemory, View};
use newt_core::{clock_format::ClockFormat, durable::Load,
    progress::Progress, sd_resume::SdResumeRecord,
    selection_authority::SelectionAuthority};
use newt_device_ui::HomeData;
use newt_sdk_heap::Fixed;
use crate::reader_book::{ReaderCheckpoint, CheckpointRestore, ReadingStatsRuntime};
use crate::retained_runtime::wake::WakeEvidence;
use newt_core::{reader_scheduler::YieldCadence, save_notice::ReadingSaveNotice};
use newt_core::{reading_stats::{BookFacts, BookFinish, book_finish},
    reading_stats_service::{StatsResult, stored_book_facts}, reading_stats_store::StatsBackend};
use crate::reader_selected_source as selected;
#[path = "reader_image_session.rs"]
mod session;
#[path = "reader_image_memory.rs"]
mod memory;
use memory::{ReaderBacking, ReadingView};
#[path = "reader_image_sample.rs"]
#[cfg(feature = "embedded-sample")]
mod sample;
#[path = "reader_image_sample_action.rs"]
#[cfg(feature = "embedded-sample")]
mod sample_action;
#[path = "reader_image_sample_paint.rs"]
#[cfg(feature = "embedded-sample")]
mod sample_paint;
#[path = "reader_image_boot.rs"]
mod boot;
#[path = "reader_image_exit.rs"]
mod exit;
#[path = "reader_image_checkpoint.rs"]
mod checkpoint;
#[path = "reader_image_metadata.rs"]
mod metadata;
#[path = "reader_image_home_stats.rs"]
mod home_stats;
#[path = "reader_image_hash.rs"]
mod crossink_hash;
#[path = "reader_image_producer.rs"]
mod producer;
#[path = "reader_image_operation.rs"]
mod operation;
#[path = "reader_image_idle_save.rs"]
mod idle_save;
#[path = "reader_image_drive.rs"]
mod drive;
#[path = "reader_image_hints.rs"]
mod hints;
#[path = "reader_image_verification.rs"]
mod verification;
#[path = "reader_image_changed.rs"]
mod changed;
#[path = "reader_image_action.rs"]
mod action;
#[path = "reader_image_preferences.rs"]
mod preferences;
#[path = "reader_image_profile.rs"]
mod profile;
#[path = "reader_image_wake.rs"]
mod wake;
#[path = "reader_image_picture.rs"]
mod picture;
#[path = "reader_image_picture_native.rs"]
mod picture_native;
#[path = "reader_image_picture_work.rs"]
mod picture_work;
#[path = "reader_image_open.rs"]
mod open;
#[path = "reader_image_work.rs"]
mod work;
#[path = "reader_image_paint.rs"]
mod paint;
#[path = "reader_image_purge.rs"]
mod purge;
#[path = "reader_cover_idle.rs"]
pub mod cover_idle;
#[path = "cover_resume.rs"]
mod cover_resume;
#[path = "reader_cover_producer.rs"]
mod cover_producer;
#[path = "reader_image_app.rs"]
mod app;
#[path = "reader_image_picture_slot.rs"]
mod picture_slot;

pub(crate) type MemoryLoan<'m, 'p> = newt_c3_board::reader_memory::Loan<'m, 'p>;

// SAFETY: existing Board bridge with the same no-argument scalar timer ABI.
// Calls occur only under the acquired, live Home SDK owner.
unsafe extern "C" { fn nb_millis() -> u32; }
fn millis() -> u32 {
    // SAFETY: image operations are called only while Home's SDK lease lives.
    unsafe { nb_millis() }
}

/// Frozen SdLibrary::sameBook: two current stats and two existing hint records,
/// one at a time. A miss never falls back to hashing either book's contents.
pub fn same_books(io: &newt_native_io::UiIo, kept: &[u8], copy: &[u8]) -> bool {
    let mut digests = [[0; 32]; 2];
    for (name, digest) in [kept, copy].into_iter().zip(&mut digests) {
        if !verified_name_digest(io, name, digest) { return false; }
    }
    digests[0] == digests[1]
}

fn verified_name_digest(io: &newt_native_io::UiIo, name: &[u8], digest: &mut [u8; 32]) -> bool {
    use newt_native_storage::{Authority, Kind, Native, ReaderSourceHints,
        ReaderSourceHintsState, RouteState, SourceChangeHints};
    // Frozen cacheRoute(...,{}) supplies no cancellation hook for sameBook.
    struct LibraryScan;
    impl Authority for LibraryScan { fn cancelled(&self) -> bool { false } }
    let Some(filename) = newt_core::library_name::name_bytes(name)
        .and_then(|name| core::str::from_utf8(name).ok()) else { return false; };
    let mut path = [0; b"/newt-sd/Newt/Books".len() + 2 + newt_core::library_name::FILENAME_BYTES];
    if !newt_library_current::library_book_path(Some(b"/newt-sd/Newt/Books\0"), Some(name), Some(&mut path)) {
        return false;
    }
    let Some(end) = path.iter().position(|&byte| byte == 0) else { return false; };
    let Ok(path) = core::str::from_utf8(&path[..end]) else { return false; };
    let mut native = io.port();
    let Ok(stat) = native.stat(path, false) else { return false; };
    if stat.kind != Kind::Regular || stat.size < 0 { return false; }
    let hints = SourceChangeHints { bytes: stat.size as u64, modified: stat.seconds };
    let mut key = [0; 64];
    let mut hash = crate::reader_hash::PackageHasher::from_live_sdk();
    let Some(key) = crate::reader_book::route_key(filename, &mut hash, &mut key) else { return false; };
    let state = RouteState::new(crate::reader_book::CACHE_ROOT, key);
    let route = state.lend(&LibraryScan, millis);
    let mut state = ReaderSourceHintsState::new(&mut native, &route);
    let loaded = match ReaderSourceHints::lend(&mut state, &mut native, &route) {
        Ok(mut record) => record.verified_source_digest(hints, digest),
        Err(_) => false,
    };
    let closed = state.close(&mut native, &route);
    loaded && closed.is_ok_and(|outcome| outcome.clean)
}

/// The four reader allocations exist only while an admitted reader owns them.
/// Nothing in this owner borrows the Board, panel, native backend or authority.
pub struct Runtime {
    memory: ReaderBacking,
    // Frozen main's readerUi survives SampleReader::close and failed admission.
    reader_ui: newt_reader_ui::ReaderUiState,
    pub(crate) selection: SelectionAuthority,
    pub(crate) saved: Progress,
    title: &'static mut [u8; 256],
    home: &'static mut HomeData<'static>,
    picture: &'static mut newt_core::resume_picture::Mark,
    source: Option<Fixed<selected::SelectedSource>>,
    session: Option<Fixed<session::Session>>,
    checkpoint: ReaderCheckpoint,
    statistics: ReadingStatsRuntime,
    recovery: Option<crate::reader_book::PendingRecovery>,
    saves: ReadingSaveNotice,
    cadence: YieldCadence,
    wake: WakeEvidence,
    cover_generation: u32,
    reading_refresh: newt_core::refresh::ReadingRefresh,
    standalone_status: newt_core::zip_container::Status,
    restore_failure: newt_reader_ui::RestoreFailure,
    progress_ready: bool,
    exit_metadata: Option<Fixed<crate::reader_exit_metadata::ExitMetadata>>,
    settings: newt_reader_ui::ViewSettings,
    last_progress: core::cell::Cell<u32>,
    idle: cover_idle::CoverIdleState,
}

/// The app owns scalar selection/recovery state; each reader open admits its
/// existing four allocations, and close releases them before returning Home.
pub fn with_runtime<R>(title: &'static mut core::mem::MaybeUninit<[u8; 256]>,
    home: &'static mut core::mem::MaybeUninit<HomeData<'static>>,
    picture: &'static mut core::mem::MaybeUninit<newt_core::resume_picture::Mark>,
    work: impl FnOnce(&mut Runtime) -> R,
) -> R {
    work(&mut Runtime::new(title, home, picture))
}

impl Runtime {
    fn new(title: &'static mut core::mem::MaybeUninit<[u8; 256]>,
        home: &'static mut core::mem::MaybeUninit<HomeData<'static>>,
        picture: &'static mut core::mem::MaybeUninit<newt_core::resume_picture::Mark>,
    ) -> Self {
        Self { memory: ReaderBacking::Closed, reader_ui: newt_reader_ui::ReaderUiState::default(),
            selection: SelectionAuthority::new(), saved: Progress::default(),
            title: crate::reading_storage::initialize(title, || [0; 256]),
            home: crate::reading_storage::initialize(home, || HomeData { book_title: None,
                reading_available: false, ..HomeData::default() }), source: None, session: None,
            picture: crate::reading_storage::initialize(picture, newt_core::resume_picture::Mark::new),
            checkpoint: ReaderCheckpoint::new(CheckpointRestore::Latest),
            statistics: ReadingStatsRuntime::default(), recovery: None,
            saves: ReadingSaveNotice::default(), cadence: YieldCadence::new(), wake: WakeEvidence::default(),
            cover_generation: 0, reading_refresh: newt_core::refresh::ReadingRefresh::default(),
            standalone_status: newt_core::zip_container::Status::NotFound,
            restore_failure: newt_reader_ui::RestoreFailure::None, progress_ready: false,
            exit_metadata: None, settings: newt_reader_ui::ViewSettings::default(),
            last_progress: core::cell::Cell::new(0), idle: cover_idle::CoverIdleState::new() }
    }
    pub fn book(&self) -> Option<View<'_>> {
        self.memory.external().map(|memory| {
            let mut view = memory.view();
            view.ui = self.reader_ui;
            view
        })
    }
    fn unselected_book_id(&self) -> u64 {
        #[cfg(feature = "embedded-sample")]
        { crate::sample_epub::IDENTITY }
        #[cfg(not(feature = "embedded-sample"))]
        { 0 }
    }
    fn reading(&self) -> Option<ReadingView<'_>> { self.memory.reading(self.standalone_status) }
    fn opened(&self) -> bool { self.reading().is_some_and(|view| view.opened) }
    fn has_progress(&self) -> bool { self.reading().is_some_and(|view| view.has_progress) }
    pub fn progress(&self) -> &Progress { &self.saved }
    pub fn selection(&self) -> &SelectionAuthority { &self.selection }
    pub(crate) fn cover_generation(&self) -> u32 { self.cover_generation }
    pub(crate) fn progress_ready(&self) -> bool { self.progress_ready }
    pub(super) fn note_cover_publication(generation: &mut u32) {
        *generation = generation.wrapping_add(1);
    }
    fn note_home_cover_changed(&mut self) {
        Self::note_cover_publication(&mut self.cover_generation);
    }
    pub(crate) fn status(&self) -> newt_core::zip_container::Status {
        self.book().map_or(self.standalone_status, |book| book.owner.state.status())
    }

    /// Boot identity is resume evidence only; open still verifies the source.
    /// Call once on this fresh epoch before native selection/session creation.
    pub fn restore_boot_identity(&mut self, identity: &SdResumeRecord) -> bool {
        self.selection.restore_identity(identity, &mut || {}, &mut || self.title.fill(0))
    }

    /// The caller supplies Home's actual durable router after RTC arbitration.
    pub fn load_progress<B: StatsBackend>(&mut self, store: &mut B, trace: crate::reader_action::Trace<'_>) -> Load<()> {
        let _action = trace.scope(crate::reader_action::ActionStage::ProgressLoad);
        let loaded = self.selection.load_progress(store, &mut self.saved);
        self.progress_ready = matches!(loaded, Load::Loaded(()) | Load::Empty);
        self.home.reading_available = self.progress_ready;
        // Frozen loadReaderProgress: each load, boot's too, sets the notice.
        self.saves.set_progress_unavailable(!self.progress_ready);
        self.home.progress_unavailable = true;
        self.home.percent = -1;
        self.home.clear_chapter();
        // Frozen noteHomeFinished (#319): the card's finish fact, never RAM's,
        // so a cold boot shows a finished book at 100%. A damaged record
        // reads finished (Unknown), as frozen; no record or no ledger, not.
        let mut facts = BookFacts::default();
        self.home.set_finished(self.selection.active()
            && stored_book_facts(store, self.selection.digest(), &mut facts) == StatsResult::Done
            && book_finish(&facts) != BookFinish::No);
        loaded
    }

    pub fn with_home_data<R>(&self, clock: Option<&CStr>, format: ClockFormat,
        device_description: &[u8], work: impl FnOnce(&HomeData<'_>) -> R,
    ) -> R {
        let title = &self.title[..self.title.iter().position(|&b| b == 0).unwrap_or(self.title.len())];
        let mut data = HomeData {
            clock,
            book_title: if !self.selection.active() { Some(newt_stage_home::SAMPLE_TITLE) }
                else if title.is_empty() { Some(b"Your SD book") } else { Some(title) },
            chapter_title: self.home.chapter_title,
            percent: self.home.percent,
            reading_available: self.home.reading_available,
            progress_unavailable: self.home.progress_unavailable,
            fixture_cover: false,
            flags: self.home.flags,
            device_description,
        };
        data.set_clock_format(format);
        work(&data)
    }
}
