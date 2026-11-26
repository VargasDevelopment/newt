// SPDX-License-Identifier: MIT
//! End-to-end host proof of the current-chapter page-cache fill policy and
//! cold-boot wake cache hit over the real firmware driver, phased reader,
//! native ChapterCacheStorage, Expat, libunibreak and host FAT card model.
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

use core::cell::Cell;
use core::ffi::CStr;
use home_startup::CardPort;
use newt_core::{
    cache_names::{namespace::PAGE_ENTRY_GRANT, segment::{ChapterSegment, PAGES}},
    preferences::Orientation,
    producer_policy::Stop,
    producer_visit::{Admission, Position, ProducerVisit},
    progress::{Locator, Progress},
    selection_authority::flag,
    zip_container::Status,
    zip_window::source::BookSource,
};
use newt_phased_reader::{
    CacheRequest, OperationPhased, PageInfo, PageLayout, PhasedChapterCache,
    PhasedReader, Selection,
    imported_revision::{Digest, ImportedRevision, RevisionHasher, RevisionStorage},
    phase_storage::{BANK_BYTES, Storage},
};
use newt_phased_sink::{FRAME_BYTES, Plan, RUNS, Run, TEXT, Target};
use newt_reader_ui::{
    DeviceInfo, ExternalOwner, PhysicalOpen, ReaderSource, ReaderUiState, RestorePolicy,
    StrictSource, ViewSettings,
};
use reader_book::{
    BookRequest, BookStep, CheckpointRestore, MountedCheckpoint, ProducerCallbacks,
    ProducerEntry, ProducerPause, ReaderCheckpoint, ReadingStatsRuntime, Resources, VisitParts,
};

unsafe extern "C" {
    fn SHA256(data: *const u8, length: usize, out: *mut u8) -> *mut u8;
}

fn sha256(bytes: &[u8], out: &mut [u8; 32]) -> bool {
    // SAFETY: host libcrypto reads `bytes` and writes 32 bytes into `out`.
    !unsafe { SHA256(bytes.as_ptr(), bytes.len(), out.as_mut_ptr()) }.is_null()
}

#[derive(Default)]
struct Hasher {
    buf: Vec<u8>,
}

impl RevisionHasher for Hasher {
    fn begin(&mut self) -> bool {
        self.buf.clear();
        true
    }
    fn update(&mut self, input: &[u8]) -> bool {
        self.buf.extend_from_slice(input);
        true
    }
    fn finish(&mut self, output: &mut Digest) -> bool {
        sha256(&self.buf, output)
    }
    fn hash(&mut self, input: &[u8], output: &mut Digest) -> bool {
        sha256(input, output)
    }
}

struct BytesSource(Vec<u8>);
impl BookSource for BytesSource {
    fn size(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let Ok(at) = usize::try_from(at) else { return -1; };
        let Some(bytes) = self.0.get(at..) else { return -1; };
        let count = bytes.len().min(out.len());
        out[..count].copy_from_slice(&bytes[..count]);
        count as i32
    }
}

struct Never;
impl newt_native_storage::Authority for Never {
    fn cancelled(&self) -> bool {
        false
    }
}

struct Flash;
impl nvs::Native for Flash {
    fn storage_ready(&mut self) -> bool {
        true
    }
    fn open(&mut self, _: &CStr, _: bool, _: &mut u32) -> core::result::Result<(), nvs::NativeError> {
        Err(nvs::NativeError::NOT_FOUND)
    }
    fn close(&mut self, _: u32) {}
    fn get_blob(
        &mut self, _: u32, _: &CStr, _: Option<&mut [u8]>, _: &mut usize,
    ) -> core::result::Result<(), nvs::NativeError> {
        Err(nvs::NativeError::NOT_FOUND)
    }
    fn set_blob(&mut self, _: u32, _: &CStr, _: &[u8]) -> core::result::Result<(), nvs::NativeError> {
        panic!("unexpected NVS write")
    }
    fn commit(&mut self, _: u32) -> core::result::Result<(), nvs::NativeError> {
        panic!("unexpected NVS commit")
    }
    fn erase_key(&mut self, _: u32, _: &CStr) -> core::result::Result<(), nvs::NativeError> {
        panic!("unexpected NVS erase")
    }
    fn readback_durable(&self) -> bool {
        true
    }
}

struct StaleOnFinish<'a, R> {
    inner: &'a mut R,
    stale: &'a Cell<bool>,
}

impl<R: RevisionStorage> RevisionStorage for StaleOnFinish<'_, R> {
    fn begin_immutable_read(&mut self) { self.inner.begin_immutable_read(); }
    fn finish_immutable_read(&mut self) -> bool {
        self.inner.finish_immutable_read() && !self.stale.get()
    }
    fn begin_deferred_path_checks(&mut self) { self.inner.begin_deferred_path_checks(); }
    fn finish_deferred_path_checks(&mut self) -> bool { self.inner.finish_deferred_path_checks() }
    fn load(&mut self, bytes: &mut u32) -> bool { self.inner.load(bytes) }
    fn begin(&mut self, bytes: u32) -> bool { self.inner.begin(bytes) }
    fn read(&mut self, offset: u32, output: &mut [u8]) -> bool { self.inner.read(offset, output) }
    fn write(&mut self, offset: u32, input: &[u8]) -> bool { self.inner.write(offset, input) }
    fn seal(&mut self) -> bool { self.inner.seal() }
    fn abort(&mut self) { self.inner.abort(); }
    fn generation(&self) -> u64 { self.inner.generation() }
    fn stable(&mut self, generation: u64) -> bool { self.inner.stable(generation) }
    fn begin_read(&mut self, generation: u64) -> u64 { self.inner.begin_read(generation) }
    fn stable_read(&mut self, generation: u64, token: u64) -> bool {
        self.inner.stable_read(generation, token)
    }
    fn read_within(&mut self, generation: u64, token: u64, offset: u32, output: &mut [u8]) -> bool {
        self.inner.read_within(generation, token, offset, output)
    }
    fn end_read(&mut self, generation: u64, token: u64) -> bool {
        self.inner.end_read(generation, token)
    }
    fn cancel_read(&mut self, generation: u64, token: u64) {
        self.inner.cancel_read(generation, token);
    }
}

fn clock_now() -> u32 {
    10_000
}

fn portrait() -> ViewSettings {
    ViewSettings {
        orientation: Orientation::Portrait,
        ..ViewSettings::default()
    }
}

fn page_files(card: &card::Card) -> Vec<String> {
    card.0
        .borrow()
        .model
        .files
        .keys()
        .filter(|path| path.contains("/Pages/") && path.ends_with(".fibp"))
        .cloned()
        .collect()
}

/// Probe whether segment `index` of certified chapter `(spine, info)` is stored
/// durably on `card` via the real `PhasedChapterCache` cache-only page lookup
/// (no segment map required when `reader.has_page()` is certified).
fn segment_cached(card: &card::Card, bytes: &[u8], digest: &Digest, info: PageInfo, index: u32) -> bool {
    let mut probe_card = card.clone();
    let mut tokens = [0u8; 2];
    // SAFETY: host Board fake treats these two stack bytes as disjoint tokens.
    let (mut board, pending, _) = unsafe {
        newt_c3_board::Board::acquire_x3_optional_sd(
            (&mut tokens[0] as *mut u8).cast(),
            (&mut tokens[1] as *mut u8).cast(),
        )
    }
    .unwrap();
    drop(pending);
    let mut hit = false;
    board
        .with_reader_io(|panel, io| {
            let mut raw = BytesSource(bytes.to_vec());
            let mut bank = [0; BANK_BYTES];
            let mut storage = Storage::new(&mut bank);
            let mut parser = newt_target_expat::Expat::default();
            let mut breaker = linebreak::NativeBreak;
            let route_state = newt_native_storage::RouteState::new(
                reader_book::CACHE_ROOT,
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            );
            let route = route_state.lend(&Never, clock_now);
            let mut pages_native = probe_card.port(io);
            let mut pages_state = newt_native_storage::chapter_cache::ChapterCacheState::new();
            let mut pages = reader_pages::ReaderPages(
                newt_native_storage::chapter_cache::ChapterCacheStorage::lend(
                    &mut pages_state, &mut pages_native, &route,
                )
                .unwrap(),
            );
            let mut selections = [Selection::default(), Selection::default()];
            let mut runs = [Run::default(); RUNS];
            let mut text = [0; TEXT];
            let mut plan = Plan::new(&mut runs, &mut text);
            let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), PageLayout::PORTRAIT);
            let mut frame = [0; FRAME_BYTES];
            assert_eq!(reader.open_phased(&mut raw, &mut storage, panel, &mut parser), Status::Ok);
            // Establish certified totals for `info.spine` on page 0 so cache_show
            // queries the target segment directly by page index.
            assert_eq!(
                reader.capture_phased(
                    &mut raw, info.spine, 0, Target::Page, &mut storage, panel, &mut parser,
                    &mut breaker,
                ),
                Status::Ok
            );
            assert_eq!(reader.publish(&mut raw, &mut frame), Status::Ok);
            assert_eq!(reader.info().page_count, info.page_count);
            assert_eq!(reader.info().total_chars, info.total_chars);
            let mut hash = Hasher::default();
            let mut cache = PhasedChapterCache {
                store: &mut pages,
                hash: &mut hash,
                settings: portrait(),
            };
            let mut op = OperationPhased::new(&mut reader, &mut storage, panel, &mut parser, &mut breaker);
            let shown = cache.capture(
                &mut raw,
                digest,
                CacheRequest {
                    spine: info.spine,
                    target: index * PAGES,
                    anchor: false,
                    staged: false,
                    build_on_miss: false,
                    cache_only: true,
                },
                &mut op,
            );
            hit = shown.status == Status::Ok && shown.hit;
            pages.0.access().close();
        })
        .unwrap();
    hit
}

fn inspect_visit(
    ledger: &mut [u8; 256],
    view: u16,
    flags: u16,
    pos: Option<Position>,
    spine: u32,
) -> (u32, u8, u8, Admission, bool) {
    let admission = match (flags & flag::PAGE_CACHE_MASK) >> 9 {
        1 => Admission::Denied,
        2 => Admission::Spent,
        3 => Admission::Granted,
        _ => Admission::Pending,
    };
    let visited = flags & flag::PAGE_CACHE_VISIT != 0;
    let mut visit = ProducerVisit::from_parts(ledger, view, admission, visited);
    let target = visit.target(pos);
    let tries = visit.tries(pos, spine);
    let prefetch_tries = ((view & 0x3000) >> 12) as u8;
    let current_admission = visit.admission();
    let pending = visit.work_pending(pos);
    (target, tries, prefetch_tries, current_admission, pending)
}

fn measure_page(bytes: &[u8], spine: u32, page_index: u32) -> (PageInfo, Vec<u8>) {
    let mut tokens = [0u8; 2];
    // SAFETY: host Board fake treats these two stack bytes as disjoint tokens.
    let (mut board, pending, _) = unsafe {
        newt_c3_board::Board::acquire_x3_optional_sd(
            (&mut tokens[0] as *mut u8).cast(),
            (&mut tokens[1] as *mut u8).cast(),
        )
    }
    .unwrap();
    drop(pending);
    let mut out = None;
    board
        .with_reader_io(|panel, _| {
            let mut raw = BytesSource(bytes.to_vec());
            let mut bank = [0; BANK_BYTES];
            let mut storage = Storage::new(&mut bank);
            let mut parser = newt_target_expat::Expat::default();
            let mut breaker = linebreak::NativeBreak;
            let mut selections = [Selection::default(), Selection::default()];
            let mut runs = [Run::default(); RUNS];
            let mut text = [0; TEXT];
            let mut plan = Plan::new(&mut runs, &mut text);
            let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), PageLayout::PORTRAIT);
            let mut frame = [0; FRAME_BYTES];
            assert_eq!(reader.open_phased(&mut raw, &mut storage, panel, &mut parser), Status::Ok);
            assert_eq!(
                reader.capture_phased(
                    &mut raw, spine, page_index, Target::Page, &mut storage, panel, &mut parser,
                    &mut breaker,
                ),
                Status::Ok
            );
            assert_eq!(reader.publish(&mut raw, &mut frame), Status::Ok);
            out = Some((reader.info(), reader.page_plan().unwrap().text().to_vec()));
        })
        .unwrap();
    out.unwrap()
}

fn verify_cold_cache_hit(
    card: &card::Card,
    bytes: &[u8],
    digest: &Digest,
    spine: u32,
    expected_char: u32,
    expected_page_index: u32,
    expected_text: &[u8],
) {
    let mut cold = card.cold_bytes();
    let mut tokens = [0u8; 2];
    // SAFETY: host Board fake treats these two stack bytes as disjoint tokens.
    let (mut board, pending, _) = unsafe {
        newt_c3_board::Board::acquire_x3_optional_sd(
            (&mut tokens[0] as *mut u8).cast(),
            (&mut tokens[1] as *mut u8).cast(),
        )
    }
    .unwrap();
    drop(pending);
    board
        .with_reader_io(|panel, io| {
            let mut raw = BytesSource(bytes.to_vec());
            let mut bank = [0; BANK_BYTES];
            let mut storage = Storage::new(&mut bank);
            let mut parser = newt_target_expat::Expat::default();
            let mut breaker = linebreak::NativeBreak;
            let route_state = newt_native_storage::RouteState::new(
                reader_book::CACHE_ROOT,
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            );
            let route = route_state.lend(&Never, clock_now);
            let mut pages_native = cold.port(io);
            let mut pages_state = newt_native_storage::chapter_cache::ChapterCacheState::new();
            let mut pages = reader_pages::ReaderPages(
                newt_native_storage::chapter_cache::ChapterCacheStorage::lend(
                    &mut pages_state, &mut pages_native, &route,
                )
                .unwrap(),
            );
            let mut selections = [Selection::default(), Selection::default()];
            let mut runs = [Run::default(); RUNS];
            let mut text = [0; TEXT];
            let mut plan = Plan::new(&mut runs, &mut text);
            let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), PageLayout::PORTRAIT);
            let mut frame = [0; FRAME_BYTES];
            let mut receipt_native = cold.port(io);
            let mut receipt_state = sd_revision::SdRevisionState::new(&mut receipt_native, &route);
            let mut sd_receipt = sd_revision::SdRevisionStorage::lend(
                &mut receipt_state, &mut receipt_native, &route,
            )
            .unwrap();
            let mut whole_hasher = Hasher::default();
            let mut block_hasher = Hasher::default();
            let mut scratch = [0u8; 256];
            let mut imported = ImportedRevision::compact(
                &mut sd_receipt, &mut whole_hasher, &mut block_hasher, &mut scratch,
            );
            assert!(imported.begin_import(&mut raw), "cold wake begin_import must succeed");
            let _indexed = newt_phased_reader::index_build::open(
                &mut imported, digest, &mut storage, panel, &mut parser, &mut pages, None,
                newt_phased_reader::index_build::Verification::Verified,
            )
            .unwrap();
            let mut hash = Hasher::default();
            let mut cache = PhasedChapterCache {
                store: &mut pages,
                hash: &mut hash,
                settings: portrait(),
            };
            let mut op = OperationPhased::new(&mut reader, &mut storage, panel, &mut parser, &mut breaker);
            let result = {
                let mut session = newt_phased_reader::imported_revision::session::Session {
                    revision: &mut imported,
                    reader: &mut op,
                    cache: &mut cache,
                };
                assert_eq!(session.finish_prepared_open(), Status::Ok);
                let staged = if spine != session.reader.reader.info().spine {
                    assert_eq!(session.stage_owned(spine), Status::Ok, "cold wake stage_owned must succeed");
                    true
                } else {
                    false
                };
                let shown = session.capture_page_owned_traced(CacheRequest {
                    spine,
                    target: expected_char,
                    anchor: true,
                    staged,
                    build_on_miss: false,
                    cache_only: true,
                });
                let result = shown.result;
                drop(shown.publication);
                result
            };
            drop(op);
            assert_eq!(
                result.status, Status::Ok,
                "cold wake cache_only lookup must succeed (spine={spine} page={expected_page_index} char={expected_char})"
            );
            assert!(result.hit, "cold wake cache lookup must hit published cache");
            assert_eq!(reader.publish(&mut imported, &mut frame), Status::Ok);
            assert_eq!(reader.info().char_start, expected_char, "cold wake char_start must match");
            assert_eq!(reader.info().page, expected_page_index, "cold wake page index must match");
            assert_eq!(
                reader.page_plan().unwrap().text(),
                expected_text,
                "cold wake page text must match"
            );
            pages.0.access().close();
            drop(imported);
            sd_receipt.close();
        })
        .unwrap();
}

struct IdleOutcome {
    quiet_from: Option<u32>,
    target: u32,
    tries: u8,
    target_tries: u8,
    admission: Admission,
    pending: bool,
}

#[allow(clippy::too_many_arguments)]
fn with_reader_session<F>(
    bytes: &[u8],
    spine: u32,
    character: u32,
    stale_flag: &Cell<bool>,
    run_steps: F,
) -> card::Card
where
    F: FnOnce(
        &mut dyn FnMut(
            &ProducerCallbacks<'_>,
            &mut dyn FnMut(&dyn Fn() -> bool) -> (Stop, bool),
            &mut bool,
        ) -> IdleOutcome,
        &card::Card,
        PageInfo,
    ),
{
    with_reader_session_seeded(bytes, spine, character, stale_flag, |_| {}, run_steps)
}

#[allow(clippy::too_many_arguments)]
fn with_reader_session_seeded<S, F>(
    bytes: &[u8],
    spine: u32,
    character: u32,
    stale_flag: &Cell<bool>,
    seed: S,
    run_steps: F,
) -> card::Card
where
    S: FnOnce(&card::Card),
    F: FnOnce(
        &mut dyn FnMut(
            &ProducerCallbacks<'_>,
            &mut dyn FnMut(&dyn Fn() -> bool) -> (Stop, bool),
            &mut bool,
        ) -> IdleOutcome,
        &card::Card,
        PageInfo,
    ),
{
    let mut tokens = [0u8; 2];
    // SAFETY: host Board fake treats these two stack bytes as disjoint tokens.
    let (mut board, pending, _) = unsafe {
        newt_c3_board::Board::acquire_x3_optional_sd(
            (&mut tokens[0] as *mut u8).cast(),
            (&mut tokens[1] as *mut u8).cast(),
        )
    }
    .unwrap();
    drop(pending);
    let mut digest = [0u8; 32];
    assert!(sha256(bytes, &mut digest));
    let book = u64::from_le_bytes(digest[..8].try_into().unwrap());
    let mut result = None;
    board
        .with_reader_io(|panel, io| {
            let mut card = card::Card::default();
            seed(&card);
            let route_state = newt_native_storage::RouteState::new(
                reader_book::CACHE_ROOT,
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            );
            let route = route_state.lend(&Never, clock_now);
            let mut receipt_native = card.port(io);
            let mut receipt_state = sd_revision::SdRevisionState::new(&mut receipt_native, &route);
            let mut sd_receipt = sd_revision::SdRevisionStorage::lend(
                &mut receipt_state, &mut receipt_native, &route,
            )
            .unwrap();
            let mut receipt = StaleOnFinish {
                inner: &mut sd_receipt,
                stale: stale_flag,
            };
            let mut whole_hasher = Hasher::default();
            let mut block_hasher = Hasher::default();
            let mut scratch = [0u8; 256];
            let mut raw = BytesSource(bytes.to_vec());
            let mut imported = ImportedRevision::compact(
                &mut receipt, &mut whole_hasher, &mut block_hasher, &mut scratch,
            );
            assert!(imported.begin_import(&mut raw), "begin_import must succeed");
            let mut source = ReaderSource::new(imported);

            let mut pages_native = card.port(io);
            let mut pages_state = newt_native_storage::chapter_cache::ChapterCacheState::new();
            let mut pages = reader_pages::ReaderPages(
                newt_native_storage::chapter_cache::ChapterCacheStorage::lend(
                    &mut pages_state, &mut pages_native, &route,
                )
                .unwrap(),
            );
            let mut bank = [0; BANK_BYTES];
            let mut storage = Storage::new(&mut bank);
            let mut selections = [Selection::default(), Selection::default()];
            let mut runs = [Run::default(); RUNS];
            let mut text = [0; TEXT];
            let mut plan = Plan::new(&mut runs, &mut text);
            let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), PageLayout::PORTRAIT);
            let mut owner = ExternalOwner::default();
            let mut ui = ReaderUiState::default();
            let mut cache_hash = Hasher::default();
            let mut resources = Resources {
                panel,
                pages: &mut pages,
                storage: &mut storage,
                parser: newt_target_expat::Expat::default(),
                breaker: &mut linebreak::NativeBreak,
                hash: &mut cache_hash,
                settings: portrait(),
                open: PhysicalOpen::Prepared,
                recovery: None,
            };
            let indexed = newt_phased_reader::index_build::open(
                &mut source, &digest, resources.storage, resources.panel, &mut resources.parser,
                resources.pages, None, newt_phased_reader::index_build::Verification::Verified,
            )
            .unwrap();
            let device_info = DeviceInfo::default();
            let saved = Progress {
                book,
                locator: Locator { spine, character },
                ..Progress::default()
            };
            let request = BookRequest {
                filename: "synthetic.epub",
                open: reader_book::BookOpen::Import,
                identity: book,
                saved,
                policy: RestorePolicy::Validated,
                info: &device_info,
                cancel: &Never,
                now: clock_now,
                sleep: None,
                zone: newt_core::time_zone::TimeZone::Utc,
            };
            let mut prefs = home_startup::preferences::ReaderPreferences::default();
            let mut checkpoint = ReaderCheckpoint::new(CheckpointRestore::Latest);
            let mut flash = Flash;
            let mut facts = newt_native_storage::reading_state::StateFacts::default();
            let mut mounted = MountedCheckpoint::new(&mut checkpoint, &mut flash, &mut facts);
            let mut statistics = ReadingStatsRuntime::default();
            let mut accepted = false;
            let mut recovery = None;
            let mut ledger = [0u8; 256];
            let mut visit_view = u16::MAX;
            let mut visit_flags = 0u16;
            let mut index_trim_owed = true;

            // Open the book at the requested spine + character locator.
            let blocked_entry = |_| ProducerEntry::Blocked;
            let blocked_cb = ProducerCallbacks {
                entry: &blocked_entry,
                stop: &|_| Stop::None,
                pause_request: &|_, _| Some(Stop::None),
                yield_now: None,
                wait: None,
            };
            let mut noop_pause = |_: ProducerPause<'_, '_, '_, '_, Flash, card::Card>,
                                  _: &dyn Fn() -> bool| (Stop::None, false);
            reader_book::run::drive(
                io, &mut card, &request, &mut reader, &mut owner, &mut ui, &mut source,
                &mut resources, &mut prefs, &mut mounted, &sha256, &mut statistics,
                &mut accepted, &mut recovery, book, saved, digest, &blocked_cb,
                Some(&mut ledger), Some(&indexed), false, reader_book::run::Route::Short,
                false, Some(VisitParts { view: &mut visit_view, flags: &mut visit_flags }),
                None, false, None, &mut index_trim_owed, |_, _, _| None, &mut noop_pause,
            )
            .unwrap();
            let info = reader.info();
            let spines = reader.selection().spines;
            let pos = Some(Position { spine, spines, verified: true });
            let card_obs = card.clone();

            {
                let mut idle_step = |callbacks: &ProducerCallbacks<'_>,
                                     on_duty: &mut dyn FnMut(&dyn Fn() -> bool) -> (Stop, bool),
                                     trim_owed: &mut bool| {
                    let mut step = Some(BookStep::Idle {
                        checkpoint: false,
                        admitted: true,
                    });
                    let mut pause = |_: ProducerPause<'_, '_, '_, '_, Flash, card::Card>,
                                     cancelled: &dyn Fn() -> bool| on_duty(cancelled);
                    let exit = reader_book::run::drive(
                        io, &mut card, &request, &mut reader, &mut owner, &mut ui, &mut source,
                        &mut resources, &mut prefs, &mut mounted, &sha256, &mut statistics,
                        &mut accepted, &mut recovery, book, saved, digest, callbacks,
                        Some(&mut ledger), None, false, reader_book::run::Route::Short, false,
                        Some(VisitParts { view: &mut visit_view, flags: &mut visit_flags }),
                        None, false, None, trim_owed, |_, _, _| step.take(), &mut pause,
                    )
                    .unwrap();
                    let (target, tries, target_tries, admission, pending) =
                        inspect_visit(&mut ledger, visit_view, visit_flags, pos, spine);
                    IdleOutcome {
                        quiet_from: exit.idle_quiet_from,
                        target,
                        tries,
                        target_tries,
                        admission,
                        pending,
                    }
                };
                run_steps(&mut idle_step, &card_obs, info);
            }
            resources.pages.0.access().close();
            drop(source);
            sd_receipt.close();
            result = Some(card);
        })
        .unwrap();
    result.unwrap()
}

fn target_first_leftover_progress_and_cold_boot_hit(bytes: &[u8]) {
    let (measured, expected_text) = measure_page(bytes, 0, 290);
    assert!(measured.page_count >= 4 * PAGES - 64, "long chapter needs 4 segments");
    assert_eq!(ChapterSegment::count(measured.page_count), 4, "expected 4 segments");
    assert_eq!(measured.page / PAGES, 2, "target must sit in segment 2");
    let mut digest = [0u8; 32];
    assert!(sha256(bytes, &mut digest));
    let stale = Cell::new(false);

    let card = with_reader_session(bytes, 0, measured.char_start, &stale, |idle, card, info| {
        assert_eq!(info.page, measured.page);
        let ready = |_| ProducerEntry::Ready;
        let callbacks = ProducerCallbacks {
            entry: &ready,
            stop: &|_| Stop::None,
            pause_request: &|_, _| Some(Stop::None),
            yield_now: None,
            wait: None,
        };
        let mut duty = |_: &dyn Fn() -> bool| (Stop::None, false);
        let mut trim_owed = true;

        // Attempt 1: seals target segment 2 and segment 3, leaving segments 0 and 1
        // skipped after the 2-pass cap -> Progress (never Unsupported).
        let out1 = idle(&callbacks, &mut duty, &mut trim_owed);
        assert!(segment_cached(card, bytes, &digest, info, 2), "first attempt must seal target segment 2");
        assert!(segment_cached(card, bytes, &digest, info, 3), "first attempt must seal forward segment 3");
        assert!(!segment_cached(card, bytes, &digest, info, 0), "leftover segment 0 waits for next attempt");
        assert!(!segment_cached(card, bytes, &digest, info, 1), "leftover segment 1 waits for next attempt");
        assert_eq!(page_files(card).len(), 2, "only segments 2 and 3 exist before attempt 2 (no map yet)");

        assert_ne!(
            out1.admission, Admission::Denied,
            "leftover segments after sealing must return Progress, not deny admission"
        );
        assert_eq!(out1.target, 0, "Progress must keep target on current spine");
        assert_eq!(out1.tries, 0, "Progress must not spend a try");
        assert_eq!(
            out1.admission, Admission::Pending,
            "Progress must re-arm admission Pending"
        );
        assert!(out1.pending, "Progress must leave current spine pending");
        assert_eq!(out1.quiet_from, Some(clock_now()), "Progress must renew quiet window");
        assert!(trim_owed, "idle index trim must wait behind Progress");

        // Attempt 2: seals remaining segments 0 and 1, then publishes the map -> Done.
        let out2 = idle(&callbacks, &mut duty, &mut trim_owed);
        assert!(segment_cached(card, bytes, &digest, info, 0), "second attempt must seal segment 0");
        assert!(segment_cached(card, bytes, &digest, info, 1), "second attempt must seal segment 1");
        assert_eq!(page_files(card).len(), 5, "all 4 segments plus the segment map exist after Done");
        assert_eq!(out2.target, 1, "Done must advance visit target to prefetch spine 1");
        assert_eq!(out2.tries, 3, "Done closes current-spine try count");
        assert_ne!(out2.admission, Admission::Denied);
        assert_eq!(out2.quiet_from, None, "Done retains quiet");
        assert!(trim_owed, "idle index trim waits behind Done pass that parsed");

        // An idle pass where the producer does no work finally runs idle index trim.
        let blocked = |_| ProducerEntry::Blocked;
        let blocked_cb = ProducerCallbacks {
            entry: &blocked,
            stop: &|_| Stop::None,
            pause_request: &|_, _| Some(Stop::None),
            yield_now: None,
            wait: None,
        };
        let out3 = idle(&blocked_cb, &mut duty, &mut trim_owed);
        assert_eq!(out3.quiet_from, None);
        assert!(!trim_owed, "idle index trim runs once producer does no work");
    });

    // Test 10: discard all RAM/RTC/cache-presence state; cold-boot anchor lookup
    // hits across all 4 segments including both sides of every 128-page seam
    // (127/255/383 are intentional CacheMissReason::Edge pages for anchor lookup).
    for check_page in [0, 126, 128, 254, 256, 290, 382, 384, measured.page_count - 1] {
        let (p_info, p_text) = measure_page(bytes, 0, check_page);
        verify_cold_cache_hit(
            &card, bytes, &digest, 0, p_info.char_start, p_info.page, &p_text,
        );
    }

    // Partial eviction check: delete segment 0 while keeping segments 1, 2, 3 and
    // the segment map intact; a fresh session at page 290 rebuilds only segment 0.
    let seg0_path = {
        let mut found = None;
        for path in page_files(&card) {
            let probe = card.cold_bytes();
            probe.0.borrow_mut().model.files.remove(&path);
            if !segment_cached(&probe, bytes, &digest, measured, 0)
                && segment_cached(&probe, bytes, &digest, measured, 1)
                && segment_cached(&probe, bytes, &digest, measured, 2)
                && segment_cached(&probe, bytes, &digest, measured, 3)
            {
                found = Some(path);
                break;
            }
        }
        found.expect("must identify segment 0 file on completed card")
    };
    let evicted = card.cold_bytes();
    evicted.0.borrow_mut().model.files.remove(&seg0_path);
    assert_eq!(page_files(&evicted).len(), 4, "3 segments + existing map remain after evicting seg 0");
    let rebuilt = with_reader_session_seeded(
        bytes,
        0,
        measured.char_start,
        &stale,
        |dest| {
            let src = evicted.0.borrow();
            let mut dst = dest.0.borrow_mut();
            dst.model.dirs = src.model.dirs.clone();
            dst.model.files = src.model.files.clone();
        },
        |idle, card, info| {
            let ready = |_| ProducerEntry::Ready;
            let callbacks = ProducerCallbacks {
                entry: &ready,
                stop: &|_| Stop::None,
                pause_request: &|_, _| Some(Stop::None),
                yield_now: None,
                wait: None,
            };
            let mut duty = |_: &dyn Fn() -> bool| (Stop::None, false);
            let mut trim_owed = true;
            let out = idle(&callbacks, &mut duty, &mut trim_owed);
            assert_eq!(out.target, 1, "partial-eviction rebuild must complete chapter in one attempt");
            assert_eq!(out.quiet_from, None);
            assert_eq!(page_files(card).len(), 5);
            assert!(segment_cached(card, bytes, &digest, info, 0), "evicted segment 0 rebuilt");
        },
    );
    let (p0_info, p0_text) = measure_page(bytes, 0, 0);
    verify_cold_cache_hit(&rebuilt, bytes, &digest, 0, p0_info.char_start, p0_info.page, &p0_text);
    verify_cold_cache_hit(&rebuilt, bytes, &digest, 0, measured.char_start, measured.page, &expected_text);
    println!("PASS target_first_leftover_progress_and_cold_boot_hit");
}

fn target_first_interrupt_after_first_segment(bytes: &[u8]) {
    let (measured, expected_text) = measure_page(bytes, 0, 290);
    let mut digest = [0u8; 32];
    assert!(sha256(bytes, &mut digest));
    let stale = Cell::new(false);

    let card = with_reader_session(bytes, 0, measured.char_start, &stale, |idle, card, info| {
        let ready = |_| ProducerEntry::Ready;
        // Fire Stop::Interrupted via `stop` on the very first poll after the
        // canonical segment file appears (hitting port.rs post-promote poll)
        // while keeping the receipt fresh.
        let stop_after_one = |_: u32| {
            if !page_files(card).is_empty() {
                Stop::Interrupted
            } else {
                Stop::None
            }
        };
        let callbacks = ProducerCallbacks {
            entry: &ready,
            stop: &stop_after_one,
            pause_request: &|_, _| Some(Stop::None),
            yield_now: None,
            wait: None,
        };
        let mut duty = |_: &dyn Fn() -> bool| (Stop::None, false);
        let mut trim_owed = true;

        let out = idle(&callbacks, &mut duty, &mut trim_owed);
        let files = page_files(card);
        assert_eq!(files.len(), 1, "interrupted after first segment seals exactly one file");
        assert!(
            segment_cached(card, bytes, &digest, info, 2),
            "first sealed segment must be target segment k_reader (got files {files:?})"
        );
        assert!(!segment_cached(card, bytes, &digest, info, 0), "segment 0 must not seal before segment 2");
        assert_eq!(out.target, 0, "interrupted Progress must keep target on spine 0");
        assert_eq!(out.tries, 0, "interrupted Progress must not spend a try");
        assert_eq!(out.admission, Admission::Pending, "interrupted Progress must re-arm admission Pending");
        assert!(out.pending);
        assert_eq!(out.quiet_from, Some(clock_now()), "interrupted Progress must renew quiet window");
        assert!(trim_owed, "idle index trim must wait behind interrupted Progress");

        // Finish remaining segments cleanly without interruption.
        let clean = ProducerCallbacks {
            entry: &ready,
            stop: &|_| Stop::None,
            pause_request: &|_, _| Some(Stop::None),
            yield_now: None,
            wait: None,
        };
        let mut last = out;
        for _ in 0..4 {
            if last.target == 1 { break; }
            last = idle(&clean, &mut duty, &mut trim_owed);
        }
        assert_eq!(last.target, 1, "chapter completes after interrupted Progress");
        assert_eq!(page_files(card).len(), 5, "all 4 segments and map exist after completion");
    });
    verify_cold_cache_hit(
        &card, bytes, &digest, 0, measured.char_start, measured.page, &expected_text,
    );
    println!("PASS target_first_interrupt_after_first_segment");
}

fn short_chapter_done_advances_to_prefetch(bytes: &[u8]) {
    let (measured, expected_text) = measure_page(bytes, 1, 5);
    assert!(measured.page_count <= PAGES, "spine 1 must be a short non-segmented chapter");
    let mut digest = [0u8; 32];
    assert!(sha256(bytes, &mut digest));
    let stale = Cell::new(false);

    let card = with_reader_session(bytes, 1, measured.char_start, &stale, |idle, card, _info| {
        let ready = |_| ProducerEntry::Ready;
        let callbacks = ProducerCallbacks {
            entry: &ready,
            stop: &|_| Stop::None,
            pause_request: &|_, _| Some(Stop::None),
            yield_now: None,
            wait: None,
        };
        let mut duty = |_: &dyn Fn() -> bool| (Stop::None, false);
        let mut trim_owed = true;
        let out = idle(&callbacks, &mut duty, &mut trim_owed);
        assert_eq!(out.quiet_from, None, "short chapter Done retains quiet");
        assert_eq!(page_files(card).len(), 1, "short chapter seals one whole cache file");
        assert_eq!(out.target, 2, "short chapter Done advances target to spine + 1");
        assert_eq!(out.tries, 3);
        assert_ne!(out.admission, Admission::Denied);
        assert!(out.pending);
    });
    verify_cold_cache_hit(
        &card, bytes, &digest, 1, measured.char_start, measured.page, &expected_text,
    );
    println!("PASS short_chapter_done_advances_to_prefetch");
}

fn three_interrupted_no_progress_attempts_deny(bytes: &[u8]) {
    let (measured, _) = measure_page(bytes, 0, 290);
    let stale = Cell::new(false);
    with_reader_session(bytes, 0, measured.char_start, &stale, |idle, card, _info| {
        let ready = |_| ProducerEntry::Ready;
        let interrupted_cb = ProducerCallbacks {
            entry: &ready,
            stop: &|_| Stop::Interrupted,
            pause_request: &|_, _| Some(Stop::None),
            yield_now: None,
            wait: None,
        };
        let mut duty = |_: &dyn Fn() -> bool| (Stop::None, false);
        let mut trim_owed = true;

        for expected_tries in 1u8..=3 {
            let out = idle(&interrupted_cb, &mut duty, &mut trim_owed);
            assert!(page_files(card).is_empty(), "interrupted NoProgress seals nothing");
            assert_eq!(out.quiet_from, Some(clock_now()), "interrupted NoProgress renews quiet");
            assert_eq!(out.target, 0);
            assert_eq!(out.tries, expected_tries, "each interrupted NoProgress spends one try");
            assert_eq!(out.pending, expected_tries < 3, "third interrupted NoProgress denies spine");
        }

        // A 4th attempt with no interruption must not run because spine 0 is denied.
        let clean_cb = ProducerCallbacks {
            entry: &ready,
            stop: &|_| Stop::None,
            pause_request: &|_, _| Some(Stop::None),
            yield_now: None,
            wait: None,
        };
        let out = idle(&clean_cb, &mut duty, &mut trim_owed);
        assert_eq!(out.quiet_from, None);
        assert!(page_files(card).is_empty(), "denied spine must not produce on 4th attempt");
    });
    println!("PASS three_interrupted_no_progress_attempts_deny");
}

fn stale_plus_input_denies_immediately(bytes: &[u8]) {
    let (measured, _) = measure_page(bytes, 0, 290);
    let stale = Cell::new(false);
    with_reader_session(bytes, 0, measured.char_start, &stale, |idle, card, _info| {
        let ready = |_| ProducerEntry::Ready;
        // Once target segment 2 seals, both user input (Stop::Interrupted) and
        // source staleness (!fresh -> Status::Stale) fire in the same attempt.
        let stop = |_: u32| {
            if !page_files(card).is_empty() {
                stale.set(true);
                Stop::Interrupted
            } else {
                Stop::None
            }
        };
        let callbacks = ProducerCallbacks {
            entry: &ready,
            stop: &stop,
            pause_request: &|_, _| Some(Stop::None),
            yield_now: None,
            wait: None,
        };
        let mut duty = |_: &dyn Fn() -> bool| (Stop::None, false);
        let mut trim_owed = true;

        let out = idle(&callbacks, &mut duty, &mut trim_owed);
        assert!(stale.get(), "precondition: segment 2 sealed and triggered stale+input");
        assert_eq!(
            out.admission, Admission::Denied,
            "Stale plus input must deny admission immediately"
        );
        assert_eq!(out.tries, 3, "Stale plus input must spend all tries immediately");
        assert!(!out.pending, "Stale plus input must leave spine non-pending");
        assert_eq!(out.target, 0);
        assert_eq!(out.quiet_from, None, "terminal Stale plus input must not renew quiet");
    });
    println!("PASS stale_plus_input_denies_immediately");
}

fn terminal_duty_plus_input_denies_immediately(bytes: &[u8]) {
    let (measured, _) = measure_page(bytes, 0, 290);
    let stale = Cell::new(false);
    with_reader_session(bytes, 0, measured.char_start, &stale, |idle, card, _info| {
        let ready = |_| ProducerEntry::Ready;
        let input_arrived = Cell::new(false);
        let duty_ran = Cell::new(false);
        let stop = |_: u32| {
            if input_arrived.get() { Stop::Interrupted } else { Stop::None }
        };
        // Trigger service_duty right after segment 2 seals.
        let pause_request = |_: u32, _: u8| {
            if !page_files(card).is_empty() { None } else { Some(Stop::None) }
        };
        let callbacks = ProducerCallbacks {
            entry: &ready,
            stop: &stop,
            pause_request: &pause_request,
            yield_now: None,
            wait: None,
        };
        let mut duty = |cancelled: &dyn Fn() -> bool| {
            duty_ran.set(true);
            // Latch Stop::Interrupted inside duty, then return Stop::Terminal.
            input_arrived.set(true);
            assert!(cancelled(), "precondition: concurrent input latched inside duty");
            (Stop::Terminal, false)
        };
        let mut trim_owed = true;

        let out = idle(&callbacks, &mut duty, &mut trim_owed);
        assert!(duty_ran.get(), "precondition: duty callback executed after segment 2 sealed");
        assert_eq!(
            out.admission, Admission::Denied,
            "terminal duty failure must deny admission even after sealing a segment and input"
        );
        assert_eq!(out.tries, 3);
        assert!(!out.pending);
        assert_eq!(out.target, 0);
        assert_eq!(out.quiet_from, None, "terminal duty failure must not renew quiet");
    });
    println!("PASS terminal_duty_plus_input_denies_immediately");
}

fn terminal_stop_without_progress_denies_immediately(bytes: &[u8]) {
    let (measured, _) = measure_page(bytes, 0, 290);
    let stale = Cell::new(false);
    with_reader_session(bytes, 0, measured.char_start, &stale, |idle, card, _info| {
        let ready = |_| ProducerEntry::Ready;
        let callbacks = ProducerCallbacks {
            entry: &ready,
            stop: &|_| Stop::Terminal,
            pause_request: &|_, _| Some(Stop::None),
            yield_now: None,
            wait: None,
        };
        let mut duty = |_: &dyn Fn() -> bool| (Stop::None, false);
        let mut trim_owed = true;

        let out = idle(&callbacks, &mut duty, &mut trim_owed);
        assert!(page_files(card).is_empty());
        assert_eq!(out.admission, Admission::Denied, "terminal stop denies immediately");
        assert_eq!(out.tries, 3);
        assert!(!out.pending);
        assert_eq!(out.quiet_from, None);
    });
    println!("PASS terminal_stop_without_progress_denies_immediately");
}

fn suspend_and_resume_input_stays_interruptible(bytes: &[u8]) {
    let (measured, _) = measure_page(bytes, 0, 290);
    let stale = Cell::new(false);

    // Subcase A: input first observed inside suspend_producer_write after seg 2 seals.
    with_reader_session(bytes, 0, measured.char_start, &stale, |idle, card, _info| {
        let ready = |_| ProducerEntry::Ready;
        let pause_armed = Cell::new(false);
        let duty_ran = Cell::new(false);
        let stop = |_: u32| {
            if pause_armed.get() { Stop::Interrupted } else { Stop::None }
        };
        let pause_request = |_: u32, _: u8| {
            let writing_next = card.0.borrow().handles.values().any(|p| p.contains("/Pages/"));
            if !pause_armed.get() && page_files(card).len() == 1 && writing_next {
                pause_armed.set(true);
                None
            } else {
                Some(Stop::None)
            }
        };
        let callbacks = ProducerCallbacks {
            entry: &ready,
            stop: &stop,
            pause_request: &pause_request,
            yield_now: None,
            wait: None,
        };
        let mut duty = |_: &dyn Fn() -> bool| {
            duty_ran.set(true);
            (Stop::None, false)
        };
        let mut trim_owed = true;
        let out = idle(&callbacks, &mut duty, &mut trim_owed);
        assert!(pause_armed.get(), "precondition: pause triggered while writing next segment");
        assert!(!duty_ran.get(), "precondition: input observed inside suspend_producer_write before duty");
        assert_eq!(
            out.admission, Admission::Pending,
            "input observed inside suspend_producer_write must stay interruptible"
        );
        assert_eq!(out.tries, 0, "interrupted Progress after seg 2 must not spend a try");
        assert_eq!(out.quiet_from, Some(clock_now()));
    });

    // Subcase B: input first observed inside resume_producer_write after seg 2 seals.
    with_reader_session(bytes, 0, measured.char_start, &stale, |idle, card, _info| {
        let ready = |_| ProducerEntry::Ready;
        let duty_done = Cell::new(false);
        let post_duty_polls = Cell::new(0u32);
        let stop = |_: u32| {
            if duty_done.get() {
                let n = post_duty_polls.get() + 1;
                post_duty_polls.set(n);
                // Poll 1 is the line-272 `cancelled()` check after duty returns;
                // Poll 2 is inside `resume_producer_write` (`port.rs:942`).
                if n >= 2 { Stop::Interrupted } else { Stop::None }
            } else {
                Stop::None
            }
        };
        let pause_request = |_: u32, _: u8| {
            let writing_next = card.0.borrow().handles.values().any(|p| p.contains("/Pages/"));
            if !duty_done.get() && page_files(card).len() == 1 && writing_next {
                None
            } else {
                Some(Stop::None)
            }
        };
        let callbacks = ProducerCallbacks {
            entry: &ready,
            stop: &stop,
            pause_request: &pause_request,
            yield_now: None,
            wait: None,
        };
        let mut duty = |_: &dyn Fn() -> bool| {
            duty_done.set(true);
            (Stop::None, false)
        };
        let mut trim_owed = true;
        let out = idle(&callbacks, &mut duty, &mut trim_owed);
        assert!(duty_done.get(), "precondition: duty completed cleanly before resume");
        assert!(post_duty_polls.get() >= 2, "precondition: input observed inside resume_producer_write");
        assert_eq!(
            out.admission, Admission::Pending,
            "input observed inside resume_producer_write must stay interruptible"
        );
        assert_eq!(out.tries, 0, "interrupted Progress on resume must not spend a try");
        assert_eq!(out.quiet_from, Some(clock_now()));
    });
    println!("PASS suspend_and_resume_input_stays_interruptible");
}

fn cleanup_unlink_fault_during_interrupt_denies_immediately(bytes: &[u8]) {
    let (measured, _) = measure_page(bytes, 0, 290);
    let stale = Cell::new(false);
    with_reader_session(bytes, 0, measured.char_start, &stale, |idle, card, _info| {
        let ready = |_| ProducerEntry::Ready;
        let temp_path = std::cell::RefCell::new(None::<String>);
        let stop = |_: u32| {
            if temp_path.borrow().is_some() {
                return Stop::Interrupted;
            }
            let writing_temp = card
                .0
                .borrow()
                .handles
                .values()
                .find(|p| p.contains("/Pages/") && p.ends_with(".tmp"))
                .cloned();
            if page_files(card).len() == 1 {
                if let Some(path) = writing_temp {
                    *temp_path.borrow_mut() = Some(path);
                    card.0.borrow_mut().model.faults.push(model::Fault::new(
                        model::Op::Unlink, 1, 1, model::Effect::Errno("EIO"), 0,
                    ));
                    return Stop::Interrupted;
                }
            }
            Stop::None
        };
        let callbacks = ProducerCallbacks {
            entry: &ready,
            stop: &stop,
            pause_request: &|_, _| Some(Stop::None),
            yield_now: None,
            wait: None,
        };
        let mut duty = |_: &dyn Fn() -> bool| (Stop::None, false);
        let mut trim_owed = true;

        let out = idle(&callbacks, &mut duty, &mut trim_owed);
        let path = temp_path.borrow().clone().expect("precondition: segment 3 opened provisional .tmp file");
        let unlink_failed = card.0.borrow().model.trace.iter().any(|line| {
            line.starts_with("unlink ") && line.contains(&path) && line.contains("EIO")
        });
        assert!(unlink_failed, "precondition: model recorded failing unlink EIO for {path}");
        assert_eq!(
            out.admission, Admission::Denied,
            "cleanup unlink I/O fault during interrupt must deny admission immediately"
        );
        assert_eq!(out.tries, 3);
        assert!(!out.pending);
        assert_eq!(out.quiet_from, None, "cleanup unlink I/O fault must not renew quiet");
    });
    println!("PASS cleanup_unlink_fault_during_interrupt_denies_immediately");
}

fn detached_finalization_terminal_duty_denies_visit(bytes: &[u8]) {
    let (measured, _) = measure_page(bytes, 0, 290);
    let stale = Cell::new(false);
    with_reader_session(bytes, 0, measured.char_start, &stale, |idle, card, _info| {
        let ready = |_| ProducerEntry::Ready;
        let duty_ran = Cell::new(false);
        // Suppress mid-stream pauses while sealing segments 2 and 3; once both
        // are sealed (clean 2-pass leftover Progress), allow only the detached
        // finalization duty check after producer unwind to run and fail terminally.
        let pause_request = |_: u32, _: u8| {
            if page_files(card).len() >= 2 { None } else { Some(Stop::None) }
        };
        let callbacks = ProducerCallbacks {
            entry: &ready,
            stop: &|_| Stop::None,
            pause_request: &pause_request,
            yield_now: None,
            wait: None,
        };
        let mut duty = |_: &dyn Fn() -> bool| {
            duty_ran.set(true);
            (Stop::Terminal, false)
        };
        let mut trim_owed = true;

        let out = idle(&callbacks, &mut duty, &mut trim_owed);
        assert!(duty_ran.get(), "precondition: detached finalization duty executed");
        assert_eq!(page_files(card).len(), 2, "precondition: clean leftover Progress sealed 2 segments");
        assert_eq!(
            out.admission, Admission::Denied,
            "terminal detached finalization duty must deny visit"
        );
        assert_eq!(out.tries, 3);
        assert!(!out.pending);
        assert_eq!(out.quiet_from, None, "terminal detached duty must not renew quiet");
    });
    println!("PASS detached_finalization_terminal_duty_denies_visit");
}

/// #194: with all 4 segments sealed and the segment map missing, the boundary
/// checks each segment with `exists()`, which keeps a validated read of the
/// last one while the stream lays out the chapter for the map. A duty that
/// comes due while that read is held must not end the fill Terminal.
fn duty_due_while_read_held_completes(bytes: &[u8]) {
    let (measured, _) = measure_page(bytes, 0, 0);
    let mut digest = [0u8; 32];
    assert!(sha256(bytes, &mut digest));
    let stale = Cell::new(false);
    let full = with_reader_session(bytes, 0, measured.char_start, &stale, |idle, card, _info| {
        let ready = |_| ProducerEntry::Ready;
        let callbacks = ProducerCallbacks {
            entry: &ready,
            stop: &|_| Stop::None,
            pause_request: &|_, _| Some(Stop::None),
            yield_now: None,
            wait: None,
        };
        let mut duty = |_: &dyn Fn() -> bool| (Stop::None, false);
        idle(&callbacks, &mut duty, &mut true);
        assert_eq!(page_files(card).len(), 5, "precondition: 4 segments plus the segment map");
    });
    let map_path = page_files(&full).into_iter().find(|path| {
        let probe = full.cold_bytes();
        probe.0.borrow_mut().model.files.remove(path);
        (0..4).all(|index| segment_cached(&probe, bytes, &digest, measured, index))
    }).expect("must identify the segment map file");
    let unmapped = full.cold_bytes();
    unmapped.0.borrow_mut().model.files.remove(&map_path);

    with_reader_session_seeded(bytes, 0, measured.char_start, &stale, |dest| {
        let src = unmapped.0.borrow();
        let mut dst = dest.0.borrow_mut();
        dst.model.dirs = src.model.dirs.clone();
        dst.model.files = src.model.files.clone();
    }, |idle, card, info| {
        // A sealed page read the producer holds, with no writer open.
        let held = || {
            let state = card.0.borrow();
            let pages: Vec<_> = state.handles.values().filter(|p| p.contains("/Pages/")).collect();
            !pages.is_empty() && pages.iter().all(|p| p.ends_with(".fibp"))
        };
        let held_path = core::cell::RefCell::new(None::<String>);
        let pause_request = |_: u32, _: u8| {
            if held_path.borrow().is_none() && held() {
                let state = card.0.borrow();
                *held_path.borrow_mut() = state.handles.values().find(|p| p.contains("/Pages/")).cloned();
                None // The Progress duty comes due now.
            } else {
                Some(Stop::None)
            }
        };
        let ready = |_| ProducerEntry::Ready;
        let callbacks = ProducerCallbacks {
            entry: &ready,
            stop: &|_| Stop::None,
            pause_request: &pause_request,
            yield_now: None,
            wait: None,
        };
        let read_in_duty = Cell::new(None::<bool>);
        let trace_at_duty = Cell::new(0);
        let mut duty = |_: &dyn Fn() -> bool| {
            read_in_duty.set(Some(held()));
            trace_at_duty.set(card.0.borrow().model.trace.len());
            (Stop::None, false)
        };
        let out = idle(&callbacks, &mut duty, &mut true);
        let path = held_path.borrow().clone().expect("precondition: a duty came due while a sealed page read was held");
        assert_ne!(out.admission, Admission::Denied, "a duty due while a read is held must not end the fill");
        assert_eq!(read_in_duty.get(), Some(false), "the pause must close the held read before the duty");
        assert!(card.0.borrow().model.files.contains_key(&map_path), "the segment map is published after the duty");
        assert_eq!(page_files(card).len(), 5, "no other page file appears");
        assert_eq!(out.target, 1, "Done must advance visit target to prefetch spine 1");

        // Closing the held read is safe: no op used a closed descriptor, the
        // next attempt's lookup on this port opens that file again, and
        // every segment still reads back.
        let quiet = ProducerCallbacks {
            entry: &ready,
            stop: &|_| Stop::None,
            pause_request: &|_, _| Some(Stop::None),
            yield_now: None,
            wait: None,
        };
        idle(&quiet, &mut |_| (Stop::None, false), &mut true);
        let state = card.0.borrow();
        let trace = &state.model.trace;
        assert!(!trace.iter().any(|line| line.ends_with("EBADF")), "no op may use a closed descriptor");
        let reopen = format!("open {path} rn = ");
        assert!(
            trace[trace_at_duty.get()..].iter()
                .any(|line| line.strip_prefix(&reopen).is_some_and(|fd| fd.parse::<i32>().is_ok())),
            "the closed read must reopen on the next lookup"
        );
        drop(state);
        for index in 0..4 {
            assert!(segment_cached(card, bytes, &digest, info, index), "segment {index} must stay readable");
        }
    });
    println!("PASS duty_due_while_read_held_completes");
}

fn crowded_pages_dir_fills_across_four_entry_grants(bytes: &[u8]) {
    // Open spine 3 (5 segments of prose) at page 5 so all 5 segments are
    // reachable within two passes, guaranteeing attempt 1 hits the 4-entry grant cap.
    let (measured, _) = measure_page(bytes, 3, 5);
    assert_eq!(ChapterSegment::count(measured.page_count), 5, "spine 3 must have 5 segments");
    assert_eq!(measured.page / PAGES, 0, "target sits in segment 0");
    let mut digest = [0u8; 32];
    assert!(sha256(bytes, &mut digest));
    let stale = Cell::new(false);
    let seeded_count = usize::from(PAGE_ENTRY_GRANT) - 4;
    let route_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let pages_dir = format!("{}/Pages", reader_book::CACHE_ROOT);

    let card = with_reader_session_seeded(
        bytes,
        3,
        measured.char_start,
        &stale,
        |card| {
            for i in 0..seeded_count {
                card.seed(&format!("{pages_dir}/{route_key}-{i:064x}.fibp"), vec![b'x'; 64]);
            }
        },
        |idle, card, info| {
            let ready = |_| ProducerEntry::Ready;
            let callbacks = ProducerCallbacks {
                entry: &ready,
                stop: &|_| Stop::None,
                pause_request: &|_, _| Some(Stop::None),
                yield_now: None,
                wait: None,
            };
            let mut duty = |_: &dyn Fn() -> bool| (Stop::None, false);
            let mut trim_owed = true;

            assert_eq!(page_files(card).len(), seeded_count, "precondition: Pages dir has PAGE_ENTRY_GRANT - 4 files");

            // Attempt 1: directory has PAGE_ENTRY_GRANT - 4 files, so census
            // grants exactly 4 entries. Producer must seal 4 segments and return
            // clean Progress before trying to open the 5th file.
            let out1 = idle(&callbacks, &mut duty, &mut trim_owed);
            assert_eq!(
                out1.admission, Admission::Pending,
                "crowded 4-entry grant must return Progress before opening 5th file"
            );
            assert_eq!(out1.target, 3, "Progress keeps target on spine 3");
            assert_eq!(out1.tries, 0, "Progress under grant cap spends 0 tries");
            assert!(out1.pending);
            assert_eq!(out1.quiet_from, Some(clock_now()), "Progress renews quiet");
            let sealed_count = (0..5u32)
                .filter(|&k| segment_cached(card, bytes, &digest, info, k))
                .count();
            assert_eq!(sealed_count, 4, "attempt 1 must seal all 4 granted segments");
            assert_eq!(
                page_files(card).len(),
                usize::from(PAGE_ENTRY_GRANT),
                "4 granted files fill Pages dir to PAGE_ENTRY_GRANT"
            );

            // Attempt 2: census evicts unrelated seeded files while preserving
            // all 4 newly sealed segments, then seals the 5th segment + map -> Done.
            let out2 = idle(&callbacks, &mut duty, &mut trim_owed);
            assert_eq!(out2.target, 4, "crowded directory must complete chapter on attempt 2");
            assert_eq!(out2.tries, 3);
            assert_eq!(out2.quiet_from, None);
            for k in 0..5u32 {
                assert!(segment_cached(card, bytes, &digest, info, k), "segment {k} present after Done");
            }
            assert!(
                page_files(card).len() <= usize::from(PAGE_ENTRY_GRANT),
                "unrelated files were evicted to stay within PAGE_ENTRY_GRANT"
            );
        },
    );
    for check_page in [0, 128, 256, 384, 512] {
        let (p_info, p_text) = measure_page(bytes, 3, check_page);
        verify_cold_cache_hit(&card, bytes, &digest, 3, p_info.char_start, p_info.page, &p_text);
    }
    println!("PASS crowded_pages_dir_fills_across_four_entry_grants");
}

fn final_pause_request_terminal_denies_visit(bytes: &[u8]) {
    let (measured, _) = measure_page(bytes, 0, 290);
    let stale = Cell::new(false);

    // Subcase A: clean leftover Progress followed by direct Some(Stop::Terminal)
    // at the final pause_request check (e.g. 120 s wall limit).
    with_reader_session(bytes, 0, measured.char_start, &stale, |idle, card, _info| {
        let ready = |_| ProducerEntry::Ready;
        let final_terminal_fired = Cell::new(false);
        let pause_request = |_: u32, _: u8| {
            if page_files(card).len() >= 2 {
                final_terminal_fired.set(true);
                Some(Stop::Terminal)
            } else {
                Some(Stop::None)
            }
        };
        let callbacks = ProducerCallbacks {
            entry: &ready,
            stop: &|_| Stop::None,
            pause_request: &pause_request,
            yield_now: None,
            wait: None,
        };
        let mut duty = |_: &dyn Fn() -> bool| (Stop::None, false);
        let mut trim_owed = true;

        let out = idle(&callbacks, &mut duty, &mut trim_owed);
        assert!(final_terminal_fired.get(), "precondition: final pause_request returned Some(Stop::Terminal)");
        assert_eq!(page_files(card).len(), 2, "precondition: clean leftover Progress sealed 2 segments");
        assert_eq!(
            out.admission, Admission::Denied,
            "direct Some(Stop::Terminal) from final pause_request must deny visit"
        );
        assert_eq!(out.tries, 3);
        assert!(!out.pending);
        assert_eq!(out.quiet_from, None, "terminal final pause_request must not renew quiet");
    });

    // Subcase B: interrupted in-producer duty sets deadline_failed = true after
    // sealing segment 2, then final pause_request hits Some(Stop::Terminal)
    // during unwind. Must still deny without repeating detached duty I/O.
    with_reader_session(bytes, 0, measured.char_start, &stale, |idle, card, _info| {
        let ready = |_| ProducerEntry::Ready;
        let duty_calls = Cell::new(0u32);
        let final_terminal_fired = Cell::new(false);
        let pause_request = |_: u32, _: u8| {
            if duty_calls.get() == 0 {
                let writing_next = card.0.borrow().handles.values().any(|p| p.contains("/Pages/") && p.ends_with(".tmp"));
                if page_files(card).len() == 1 && writing_next {
                    None
                } else {
                    Some(Stop::None)
                }
            } else {
                final_terminal_fired.set(true);
                Some(Stop::Terminal)
            }
        };
        let callbacks = ProducerCallbacks {
            entry: &ready,
            stop: &|_| Stop::None,
            pause_request: &pause_request,
            yield_now: None,
            wait: None,
        };
        let mut duty = |_: &dyn Fn() -> bool| {
            duty_calls.set(duty_calls.get() + 1);
            (Stop::Interrupted, false)
        };
        let mut trim_owed = true;

        let out = idle(&callbacks, &mut duty, &mut trim_owed);
        assert_eq!(duty_calls.get(), 1, "detached duty must not repeat after deadline_failed");
        assert_eq!(
            out.admission, Admission::Denied,
            "terminal final pause_request after interrupted duty must deny visit"
        );
        assert!(final_terminal_fired.get(), "precondition: read-only final pause_request sampled after interrupted duty");
        assert_eq!(out.tries, 3);
        assert!(!out.pending);
        assert_eq!(out.quiet_from, None);
    });
    println!("PASS final_pause_request_terminal_denies_visit");
}

fn post_rename_interrupt_on_third_try_completes_whole_and_map_chapters(bytes: &[u8]) {
    let mut digest = [0u8; 32];
    assert!(sha256(bytes, &mut digest));
    let stale = Cell::new(false);

    // Subcase A: whole-only short chapter (spine 1). Spend 2 interrupted tries,
    // then fire Stop::Interrupted immediately after the canonical file appears
    // on try #3 (port.rs post-promote poll) with a fresh receipt -> must Done!
    let (short_info, short_text) = measure_page(bytes, 1, 5);
    assert!(short_info.page_count <= PAGES);
    let short_card = with_reader_session(bytes, 1, short_info.char_start, &stale, |idle, card, _info| {
        let ready = |_| ProducerEntry::Ready;
        let always_interrupt = ProducerCallbacks {
            entry: &ready,
            stop: &|_| Stop::Interrupted,
            pause_request: &|_, _| Some(Stop::None),
            yield_now: None,
            wait: None,
        };
        let mut duty = |_: &dyn Fn() -> bool| (Stop::None, false);
        let mut trim_owed = true;
        for expected in 1u8..=2 {
            let out = idle(&always_interrupt, &mut duty, &mut trim_owed);
            assert_eq!(out.tries, expected);
            assert_eq!(out.admission, Admission::Pending);
        }

        let stop_after_publish = |_: u32| {
            if !page_files(card).is_empty() { Stop::Interrupted } else { Stop::None }
        };
        let third_cb = ProducerCallbacks {
            entry: &ready,
            stop: &stop_after_publish,
            pause_request: &|_, _| Some(Stop::None),
            yield_now: None,
            wait: None,
        };
        let out3 = idle(&third_cb, &mut duty, &mut trim_owed);
        assert_eq!(page_files(card).len(), 1, "whole chapter file published before post-rename interrupt");
        assert_eq!(out3.target, 2, "post-rename interrupt on published whole chapter must complete and advance to prefetch");
        assert_ne!(out3.admission, Admission::Denied, "published whole chapter must not deny on try 3");
        assert_eq!(out3.quiet_from, None, "Done retains quiet");
    });
    verify_cold_cache_hit(
        &short_card, bytes, &digest, 1, short_info.char_start, short_info.page, &short_text,
    );

    // Subcase B: map-only segmented chapter (spine 0). First build all 4 segments
    // + map cleanly, remove ONLY the map file, then start a fresh session, spend
    // 2 interrupted tries, and fire Stop::Interrupted immediately after the map's
    // canonical file appears on try #3 -> must recognize Done and advance!
    let (long_info, long_text) = measure_page(bytes, 0, 290);
    let full_card = with_reader_session(bytes, 0, long_info.char_start, &stale, |idle, card, _info| {
        let ready = |_| ProducerEntry::Ready;
        let clean = ProducerCallbacks {
            entry: &ready,
            stop: &|_| Stop::None,
            pause_request: &|_, _| Some(Stop::None),
            yield_now: None,
            wait: None,
        };
        let mut duty = |_: &dyn Fn() -> bool| (Stop::None, false);
        let mut trim_owed = true;
        let _ = idle(&clean, &mut duty, &mut trim_owed);
        let out2 = idle(&clean, &mut duty, &mut trim_owed);
        assert_eq!(out2.target, 1);
        assert_eq!(page_files(card).len(), 5);
    });
    // Identify the map file (removing it leaves all 4 segments intact).
    let map_path = {
        let mut found = None;
        for path in page_files(&full_card) {
            let probe = full_card.cold_bytes();
            probe.0.borrow_mut().model.files.remove(&path);
            if (0..4u32).all(|k| segment_cached(&probe, bytes, &digest, long_info, k)) {
                found = Some(path);
                break;
            }
        }
        found.expect("must identify segment map file on completed card")
    };
    let no_map = full_card.cold_bytes();
    no_map.0.borrow_mut().model.files.remove(&map_path);
    assert_eq!(page_files(&no_map).len(), 4, "all 4 segments present, map absent");

    let map_card = with_reader_session_seeded(
        bytes,
        0,
        long_info.char_start,
        &stale,
        |dest| {
            let src = no_map.0.borrow();
            let mut dst = dest.0.borrow_mut();
            dst.model.dirs = src.model.dirs.clone();
            dst.model.files = src.model.files.clone();
        },
        |idle, card, _info| {
            let ready = |_| ProducerEntry::Ready;
            let always_interrupt = ProducerCallbacks {
                entry: &ready,
                stop: &|_| Stop::Interrupted,
                pause_request: &|_, _| Some(Stop::None),
                yield_now: None,
                wait: None,
            };
            let mut duty = |_: &dyn Fn() -> bool| (Stop::None, false);
            let mut trim_owed = true;
            for expected in 1u8..=2 {
                let out = idle(&always_interrupt, &mut duty, &mut trim_owed);
                assert_eq!(out.tries, expected);
                assert_eq!(out.admission, Admission::Pending);
            }
            let stop_after_map = |_: u32| {
                if page_files(card).len() >= 5 { Stop::Interrupted } else { Stop::None }
            };
            let third_cb = ProducerCallbacks {
                entry: &ready,
                stop: &stop_after_map,
                pause_request: &|_, _| Some(Stop::None),
                yield_now: None,
                wait: None,
            };
            let out3 = idle(&third_cb, &mut duty, &mut trim_owed);
            assert_eq!(page_files(card).len(), 5, "map file published before post-rename interrupt");
            assert_eq!(
                out3.target, 1,
                "post-rename interrupt on published segment map must complete and advance to prefetch"
            );
            assert_ne!(out3.admission, Admission::Denied, "published map must not deny on try 3");
            assert_eq!(out3.quiet_from, None, "Done retains quiet");
        },
    );
    verify_cold_cache_hit(
        &map_card, bytes, &digest, 0, long_info.char_start, long_info.page, &long_text,
    );
    println!("PASS post_rename_interrupt_on_third_try_completes_whole_and_map_chapters");
}

fn prefetch_cleanup_unlink_fault_preserves_legacy_interrupted_behavior(bytes: &[u8]) {
    let (short_info, _) = measure_page(bytes, 1, 5);
    assert!(short_info.page_count <= PAGES);
    let stale = Cell::new(false);
    with_reader_session(bytes, 1, short_info.char_start, &stale, |idle, card, _info| {
        let ready = |_| ProducerEntry::Ready;
        let clean = ProducerCallbacks {
            entry: &ready,
            stop: &|_| Stop::None,
            pause_request: &|_, _| Some(Stop::None),
            yield_now: None,
            wait: None,
        };
        let mut duty = |_: &dyn Fn() -> bool| (Stop::None, false);
        let mut trim_owed = true;

        // First complete current short chapter (spine 1) -> target advances to prefetch spine 2.
        let out1 = idle(&clean, &mut duty, &mut trim_owed);
        assert_eq!(out1.target, 2);
        assert_eq!(page_files(card).len(), 1);

        // Now run prefetch for spine 2: once spine 2 opens its provisional /Pages/
        // `.tmp` file, inject Stop::Interrupted + failing unlink during cleanup.
        // Prefetch must preserve legacy interrupted behavior (1 try, Pending, quiet renewed).
        let temp_path = std::cell::RefCell::new(None::<String>);
        let stop = |_: u32| {
            if temp_path.borrow().is_some() { return Stop::Interrupted; }
            let writing_temp = card
                .0
                .borrow()
                .handles
                .values()
                .find(|p| p.contains("/Pages/") && p.ends_with(".tmp"))
                .cloned();
            if let Some(path) = writing_temp {
                *temp_path.borrow_mut() = Some(path);
                card.0.borrow_mut().model.faults.push(model::Fault::new(
                    model::Op::Unlink, 1, 1, model::Effect::Errno("EIO"), 0,
                ));
                Stop::Interrupted
            } else {
                Stop::None
            }
        };
        let prefetch_cb = ProducerCallbacks {
            entry: &ready,
            stop: &stop,
            pause_request: &|_, _| Some(Stop::None),
            yield_now: None,
            wait: None,
        };
        let out2 = idle(&prefetch_cb, &mut duty, &mut trim_owed);
        let path = temp_path.borrow().clone().expect("precondition: prefetch opened provisional .tmp file");
        let unlink_failed = card.0.borrow().model.trace.iter().any(|line| {
            line.starts_with("unlink ") && line.contains(&path) && line.contains("EIO")
        });
        assert!(unlink_failed, "precondition: model recorded failing unlink EIO for prefetch temp {path}");
        assert_eq!(out2.target, 2, "prefetch stays on target spine 2");
        assert_eq!(out2.target_tries, 1, "interrupted prefetch spends 1 try on target spine");
        assert_eq!(
            out2.admission, Admission::Pending,
            "prefetch cleanup fault after interruption preserves legacy Pending admission"
        );
        assert_eq!(out2.quiet_from, Some(clock_now()), "prefetch renews quiet window");
    });
    println!("PASS prefetch_cleanup_unlink_fault_preserves_legacy_interrupted_behavior");
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("synthetic epub path");
    let filter = match args.next().as_deref() {
        Some("--case") => Some(args.next().expect("case name")),
        None => None,
        Some(other) => panic!("unexpected argument: {other}"),
    };
    let bytes = std::fs::read(path).unwrap();
    let cases: [(&str, fn(&[u8])); 15] = [
        ("target_first_leftover_progress_and_cold_boot_hit", target_first_leftover_progress_and_cold_boot_hit),
        ("target_first_interrupt_after_first_segment", target_first_interrupt_after_first_segment),
        ("short_chapter_done_advances_to_prefetch", short_chapter_done_advances_to_prefetch),
        ("three_interrupted_no_progress_attempts_deny", three_interrupted_no_progress_attempts_deny),
        ("stale_plus_input_denies_immediately", stale_plus_input_denies_immediately),
        ("terminal_duty_plus_input_denies_immediately", terminal_duty_plus_input_denies_immediately),
        ("terminal_stop_without_progress_denies_immediately", terminal_stop_without_progress_denies_immediately),
        ("suspend_and_resume_input_stays_interruptible", suspend_and_resume_input_stays_interruptible),
        ("cleanup_unlink_fault_during_interrupt_denies_immediately", cleanup_unlink_fault_during_interrupt_denies_immediately),
        ("detached_finalization_terminal_duty_denies_visit", detached_finalization_terminal_duty_denies_visit),
        ("duty_due_while_read_held_completes", duty_due_while_read_held_completes),
        ("crowded_pages_dir_fills_across_four_entry_grants", crowded_pages_dir_fills_across_four_entry_grants),
        ("final_pause_request_terminal_denies_visit", final_pause_request_terminal_denies_visit),
        ("post_rename_interrupt_on_third_try_completes_whole_and_map_chapters", post_rename_interrupt_on_third_try_completes_whole_and_map_chapters),
        ("prefetch_cleanup_unlink_fault_preserves_legacy_interrupted_behavior", prefetch_cleanup_unlink_fault_preserves_legacy_interrupted_behavior),
    ];
    let mut ran = 0usize;
    for (name, case) in cases {
        if filter.as_deref().is_none_or(|wanted| wanted == name) {
            case(&bytes);
            ran += 1;
        }
    }
    assert!(ran > 0, "no case matched filter {filter:?}");
    println!("PASS producer fill policy ({ran} cases)");
}
