// SPDX-License-Identifier: MIT
//! Actual firmware drive, parser and canvas; only Board/NVS/card are host ports.
extern crate alloc;
pub use newt_native_storage::{Directory, Error, Kind, Native, Open, Result, Stat};
#[path = "../../firmware/board_clock.rs"] pub mod board_clock;
#[path = "../../firmware/home_startup.rs"] pub mod home_startup;
#[path = "../../firmware/nvs/nvs.rs"] pub mod nvs;
#[path = "../../firmware/reader_book.rs"] pub mod reader_book;
#[path = "../../firmware/reader_metadata.rs"] pub mod reader_metadata;
#[path = "../../firmware/reader_pages.rs"] pub mod reader_pages;
#[path = "../../firmware/reader_restore_policy.rs"] mod reader_restore_policy;
#[path = "../../firmware/retained_runtime/record.rs"] pub mod retained_record;
mod retained_runtime { pub use crate::retained_record as record; }
#[path = "../../firmware/reading_state_medium.rs"] pub mod reading_state_medium;
#[path = "../../firmware/sd_revision.rs"] pub mod sd_revision;
#[path = "../../firmware/stats_clock.rs"] pub mod stats_clock;
#[path = "../../firmware/reader_exit_metadata.rs"] mod reader_exit_metadata;
mod reader_action { pub use newt_core::reader_action_profile::ActionStage; }
#[path = "../reader_checkpoint/card.rs"] mod card;
#[path = "../sd_reading_state/model.rs"] mod model;
#[path = "../phased_reader/physical/linebreak.rs"] mod linebreak;
#[path = "upgrade.rs"] mod upgrade;
#[path = "boot_policy.rs"] mod boot_policy;
#[path = "boot_resume.rs"] mod boot_resume;
#[path = "picture_restore.rs"] mod picture_restore;
#[path = "cache_only_cancel.rs"] mod cache_only_cancel;

use core::ffi::CStr;
use newt_core::{progress::Progress, producer_policy::Stop, zip_window::source::BookSource};
use newt_phased_reader::{PageLayout, PhasedReader, Selection, phase_storage::{BANK_BYTES, Storage}};
use newt_phased_sink::{Plan, Run, RUNS, TEXT};
use newt_reader_ui::{Action, DeviceInfo, ExternalOwner, PhysicalOpen, ReaderPanel, ReaderUiIntent,
    ReaderUiState, RestorePolicy, StrictSource, ViewSettings};
use reader_book::{BookRequest, BookStep, CheckpointRestore, MountedCheckpoint, ProducerCallbacks,
    ReaderCheckpoint, ReadingStatsRuntime, Resources};

unsafe extern "C" { fn SHA256(data: *const u8, length: usize, out: *mut u8) -> *mut u8; }
fn hash(bytes: &[u8], out: &mut [u8; 32]) -> bool {
    // SAFETY: host libcrypto consumes this slice and fills exactly 32 bytes.
    !unsafe { SHA256(bytes.as_ptr(), bytes.len(), out.as_mut_ptr()) }.is_null()
}
struct Hash;
impl newt_phased_reader::hash_provider::CacheHash for Hash {
    fn hash(&mut self, bytes: &[u8], out: &mut [u8; 32]) -> bool { hash(bytes, out) }
}
struct Source(Vec<u8>);
impl BookSource for Source {
    fn size(&self) -> u64 { self.0.len() as u64 }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let Ok(at) = usize::try_from(at) else { return -1; };
        let Some(bytes) = self.0.get(at..) else { return -1; };
        let count = bytes.len().min(out.len());
        out[..count].copy_from_slice(&bytes[..count]); count as i32
    }
}
struct Never;
impl newt_native_storage::Authority for Never { fn cancelled(&self) -> bool { false } }
struct Flash;
impl nvs::Native for Flash {
    fn storage_ready(&mut self) -> bool { true }
    fn open(&mut self, _: &CStr, _: bool, _: &mut u32) -> core::result::Result<(), nvs::NativeError> { Err(nvs::NativeError::NOT_FOUND) }
    fn close(&mut self, _: u32) {}
    fn get_blob(&mut self, _: u32, _: &CStr, _: Option<&mut [u8]>, _: &mut usize) -> core::result::Result<(), nvs::NativeError> { Err(nvs::NativeError::NOT_FOUND) }
    fn set_blob(&mut self, _: u32, _: &CStr, _: &[u8]) -> core::result::Result<(), nvs::NativeError> { panic!("unexpected NVS write") }
    fn commit(&mut self, _: u32) -> core::result::Result<(), nvs::NativeError> { panic!("unexpected NVS commit") }
    fn erase_key(&mut self, _: u32, _: &CStr) -> core::result::Result<(), nvs::NativeError> { panic!("unexpected NVS erase") }
    fn readback_durable(&self) -> bool { true }
}
fn now() -> u32 { 1000 }

fn scenario(bytes: Vec<u8>, finished: bool) {
    let mut tokens = [0u8; 2];
    // SAFETY: only the existing host Board fake sees these disjoint tokens.
    let (mut board, pending, _) = unsafe { newt_c3_board::Board::acquire_x3_optional_sd(
        (&mut tokens[0] as *mut u8).cast(), (&mut tokens[1] as *mut u8).cast()) }.unwrap();
    drop(pending);
    board.with_reader_io(|panel, io| {
        let mut card = card::Card::default();
        let mut native = card.port(io);
        let route_state = newt_native_storage::RouteState::new(reader_book::CACHE_ROOT,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef");
        let route = route_state.lend(&Never, now);
        let mut page_state = newt_native_storage::chapter_cache::ChapterCacheState::new();
        let mut pages = reader_pages::ReaderPages(newt_native_storage::chapter_cache::ChapterCacheStorage::lend(
            &mut page_state, &mut native, &route).unwrap());
        let mut bank = [0; BANK_BYTES];
        let mut selections = [Selection::default(), Selection::default()];
        let mut runs = [Run::default(); RUNS]; let mut text = [0; TEXT];
        let mut plan = Plan::new(&mut runs, &mut text);
        let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), PageLayout::PORTRAIT);
        let mut owner = ExternalOwner::default(); let mut ui = ReaderUiState::default();
        let settings = ViewSettings::default();
        let mut resources = Resources { panel, pages: &mut pages, storage: &mut Storage::new(&mut bank),
            parser: newt_target_expat::Expat::default(), breaker: &mut linebreak::NativeBreak,
            hash: &mut Hash, settings, open: PhysicalOpen::Import, cache_only: false, recovery: None };
        let mut raw = Source(bytes); let mut digest = [0; 32]; assert!(hash(&raw.0, &mut digest));
        let mut authorize = |raw: &mut dyn BookSource, expected: &[u8;32], _: &mut [u8], allowed: &dyn Fn() -> bool| {
            let mut bytes = vec![0; raw.size() as usize]; let mut actual = [0;32];
            allowed() && raw.read_at(0, &mut bytes) == bytes.len() as i32 && hash(&bytes, &mut actual) && actual == *expected
        };
        let mut source = StrictSource::new(&mut raw, &digest).with_reopen_authorizer(&mut authorize, &|| true);
        let book = u64::from_le_bytes(digest[..8].try_into().unwrap());
        let info = DeviceInfo::default(); let saved = Progress { book, ..Progress::default() };
        let request = BookRequest {
            #[cfg(feature = "checkpoint-fixture")]
            path: "/newt-sd/Books/generated.epub", filename: "generated.epub",
            open: reader_book::BookOpen::Receipt(digest), identity: book, saved, policy: RestorePolicy::ExactPage,
            info: &info, cancel: &Never, now, sleep: None, zone: newt_core::time_zone::TimeZone::Utc };
        let mut prefs = home_startup::preferences::ReaderPreferences::default();
        let mut checkpoint = ReaderCheckpoint::new(CheckpointRestore::Latest);
        let mut flash = Flash; let mut facts = newt_native_storage::reading_state::StateFacts::default();
        if finished {
            let selected = newt_core::sd_resume::SdResumeRecord::new(c"generated.epub", digest).unwrap();
            let mut access = resources.pages.0.access();
            let mut medium = reading_state_medium::MountedStateMedium::new(io, Some(&mut access));
            let mut state_native = card.port(io);
            let mut store = newt_native_storage::reading_state::ReadingStateRouter::new(
                nvs::NvsBlobBackend::new(&mut flash), &mut facts, &mut medium, &mut state_native);
            assert_eq!(newt_core::selected_book::SelectedBookStore::new(&mut store).save(selected),
                newt_core::durable::Commit::Saved);
            checkpoint.selection_committed(&mut store, &digest, false, false);
        }
        let mut mounted = MountedCheckpoint::new(&mut checkpoint, &mut flash, &mut facts);
        let mut statistics = ReadingStatsRuntime::default(); let mut accepted = false; let mut recovery = None;
        let producer_entry = |_| reader_book::ProducerEntry::Blocked;
        let callbacks = ProducerCallbacks { entry: &producer_entry, stop: &|_| Stop::Terminal,
            pause_request: &|_, _| Some(Stop::Terminal), yield_now: None, wait: None };
        let mut pause = |_: reader_book::ProducerPause<'_, '_, '_, '_, Flash, card::Card>,
            _: &dyn Fn() -> bool| (Stop::Terminal, false);
        let indexed = newt_phased_reader::index_build::open(&mut source, &digest, resources.storage,
            resources.panel, &mut resources.parser, resources.pages, None,
            newt_phased_reader::index_build::Verification::Verified).unwrap();
        let mut run = |ui: &mut ReaderUiState, initialize, paint_first, step: Option<BookStep>| {
            let mut step = step; let mut compositions = 0;
            let exit = reader_book::run::drive(io, &mut card, &request, &mut reader, &mut owner, ui,
                &mut source, &mut resources, &mut prefs, &mut mounted, &hash, &mut statistics,
                &mut accepted, &mut recovery, book, saved, digest, &callbacks, None, if initialize { Some(&indexed) } else { None },
                paint_first, reader_book::run::Route::Short, false, None, None, false, None, &mut false,
                |turn, _, _| {
                    if let Some(composed) = turn.composed { assert!(composed.is_ok()); compositions += 1; }
                    step.take()
                }, &mut pause).unwrap();
            (exit, compositions)
        };
        let (opened, composed) = run(&mut ui, true, false, None);
        assert_eq!(opened.intent, None); assert_eq!(composed, 0, "short open leaves composition to image loop");
        let (_, composed) = run(&mut ui, false, true, None); assert_eq!(composed, 1);
        if finished {
            ui.panel = ReaderPanel::Menu; ui.selected = 4;
            let (exit, composed) = run(&mut ui, false, false, Some(BookStep::Action(Action::Activate)));
            assert_eq!(composed, 0, "short action must not compose before image paint");
            assert_eq!(exit.intent, Some(ReaderUiIntent::MarkFinished),
                "short mark-finished returns explicit intent before mark settles");
            // Mirror reader_image_action::finish_pending_mark_image: settle owed
            // progress commit before issuing the explicit BookStep::MarkFinished.
            let requested_mark = if ui.finish == newt_reader_ui::ReaderFinishRow::Undo {
                newt_core::reading_stats::StatsMark::NotFinished
            } else {
                newt_core::reading_stats::StatsMark::Finished
            };
            let (saved, composed) = run(&mut ui, false, false, Some(BookStep::Save));
            assert_eq!(composed, 0, "short save before mark-finished must not compose");
            assert_eq!(saved.intent, None, "short save settles without taking over intent");
            assert_eq!(saved.statistics.checkpoint.progress, reader_book::checkpoint::Outcome::Verified,
                "owed progress commit must verify before mark-finished");
            assert_ne!(saved.statistics.checkpoint.destination, reader_book::checkpoint::Outcome::Unconfirmed,
                "owed destination commit must not stay unconfirmed before mark-finished");
            let (marked, composed) = run(&mut ui, false, false, Some(BookStep::MarkFinished(requested_mark)));
            assert_eq!(composed, 0, "short mark-finished step must not compose before image paint");
            assert_eq!(marked.intent, Some(ReaderUiIntent::Repaint),
                "settled mark-finished returns Repaint");
            assert_eq!(ui.finish, newt_reader_ui::ReaderFinishRow::Undo,
                "mark-finished updated finish row");
            assert_eq!(ui.notice, newt_reader_ui::ReaderUiNotice::None,
                "mark-finished settled without error notice");
        } else {
            let (exit, composed) = run(&mut ui, false, false, Some(BookStep::Action(Action::Next)));
            assert_eq!(composed, 0, "short action must not compose before image paint");
            assert_eq!(exit.intent, Some(ReaderUiIntent::Repaint));
        }
        let (_, composed) = run(&mut ui, false, true, None); assert_eq!(composed, 1, "one actual image composition");
    }).unwrap();
}
use home_startup::CardPort;
/// Wake diagnostics report a retained book's orientation through
/// `SleepRecord::wake_orientation`, which unpacks the whole word: clockwise
/// sets the layout-class bit too, so bit 0 alone would report counterclockwise.
fn retained_clockwise() {
    use newt_core::{app::{AppState, Screen}, preferences::Orientation};
    use retained_runtime::record::{Recovery, make_record};
    let state = AppState { screen: Screen::EpubReading, ..AppState::default() };
    let progress = Progress { book: 7, ..Progress::default() };
    for orientation in [Orientation::Portrait, Orientation::LandscapeCcw, Orientation::LandscapeCw] {
        let settings = ViewSettings { orientation, ..ViewSettings::default() };
        for sd_source in [false, true] {
            let record = make_record(&state, Some(&progress), settings, true, Recovery::Ordinary, sd_source);
            assert_eq!(record.reader_view(), Some(settings), "retained {orientation:?} evidence");
            assert_eq!(record.wake_orientation(None), Some(orientation),
                "wake diagnostics report retained {orientation:?}");
            let other = if orientation == Orientation::Portrait { Orientation::LandscapeCw } else { Orientation::Portrait };
            let durable = ViewSettings { orientation: other, ..settings };
            assert_eq!(record.wake_orientation(Some(durable)), Some(other),
                "wake diagnostics prefer the durable target");
        }
    }
}
fn main() {
    let path = std::env::args().nth(1).expect("generated public fixture");
    if path == "--restore-policy" {
        retained_clockwise();
        boot_policy::check();
        return;
    }
    if path == "--cache-only-cancel" {
        cache_only_cancel::check();
        return;
    }
    let bytes = std::fs::read(path).unwrap();
    if std::env::args().any(|arg| arg == "--boot-resume") {
        boot_resume::check(bytes);
        return;
    }
    if std::env::args().any(|arg| arg == "--picture-restore") {
        picture_restore::check(bytes);
        return;
    }
    if std::env::args().any(|arg| arg == "--cache-upgrade") {
        upgrade::resume(bytes);
        return;
    }
    scenario(bytes.clone(), false); scenario(bytes, true);
    println!("PASS actual short open/action/finished plus image paint");
}
