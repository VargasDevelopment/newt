// SPDX-License-Identifier: MIT
//! End-to-end host proof of verified resume-point restore over real Expat,
//! libunibreak, PhasedReader, Producer, ChapterCacheStorage, VerifiedStore,
//! PageCacheReader and host FAT card model.
extern crate alloc;

pub use newt_native_storage::{Directory, Error, Kind, Native, Open, Result, Stat};

pub mod home_startup {
    pub trait CardPort {
        type Port<'ui>: crate::Native;
        fn port<'ui>(&mut self, io: &'ui newt_native_io::UiIo) -> Self::Port<'ui>;
    }
}

#[path = "../reader_checkpoint/card.rs"]
mod card;
#[path = "../sd_reading_state/model.rs"]
mod model;
#[path = "../phased_reader/physical/linebreak.rs"]
mod linebreak;

use crate::home_startup::CardPort;
use core::alloc::{GlobalAlloc, Layout as AllocLayout};
use core::cell::Cell;
use core::sync::atomic::{AtomicUsize, Ordering};
use newt_core::{
    cache_names::segment::{ChapterSegment, PAGES, map_name, segment_name},
    chapter_layout::types::{Image, Link, Run},
    page_cache::{
        Anchor, CacheStorage, Entry, MAX_PAGE_BYTES, PageCacheReader, PageMemory, ReaderMemory,
        VERSION_PAGES, VERSION_POINT,
    },
    page_cache_access::VerifiedAccess,
    page_envelope::{self, BLOCK_BYTES, Query, RawRead, VerifiedStore},
    preferences::{Orientation, ViewSettings},
    producer_policy::{Cancellation, Hooks, ProducerReceipt, Stop},
    producer_visit::{Admission, CurrentOutcome, LEDGER_BYTES, Position, ProducerVisit},
    resume_point::{HEADER_BYTES, Layout, MAX_POINT_BYTES, Section, TRAILER_BYTES},
    zip_container::Status,
    zip_window::{arena::Arena, source::BookSource},
};
use newt_native_storage::{
    Authority, RouteState,
    chapter_cache::{ChapterCacheAccess, ChapterCacheState, ChapterCacheStorage},
};
use newt_phased_reader::{
    PageInfo, PageLayout, PhasedReader, Selection,
    hash_provider::CacheHash,
    imported_revision::Digest,
    phase_storage::{BANK_BYTES, BuildLoan, Storage},
    producer::Route,
};
use newt_phased_sink::{FRAME_BYTES, Plan, RUNS, TEXT, Target};
use std::collections::BTreeMap;
use std::path::Path;

struct CountingAlloc;
static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
// SAFETY: delegates every allocation and deallocation directly to `std::alloc::System`.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: AllocLayout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        unsafe { std::alloc::System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: AllocLayout) {
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: AllocLayout, new_size: usize) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        unsafe { std::alloc::System.realloc(ptr, layout, new_size) }
    }
}
#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

unsafe extern "C" {
    fn SHA256(data: *const u8, length: usize, out: *mut u8) -> *mut u8;
}

fn sha256(bytes: &[u8], out: &mut [u8; 32]) -> bool {
    // SAFETY: host libcrypto reads `bytes` and writes 32 bytes into `out`.
    !unsafe { SHA256(bytes.as_ptr(), bytes.len(), out.as_mut_ptr()) }.is_null()
}

#[derive(Default)]
struct Hasher {
    base_id: Option<[u8; 69]>,
}
impl CacheHash for Hasher {
    fn hash(&mut self, input: &[u8], output: &mut Digest) -> bool {
        if !sha256(input, output) {
            return false;
        }
        if input.len() == 99 && self.base_id.is_none() {
            let mut name = [0u8; 69];
            for (i, b) in output.iter().enumerate() {
                name[2 * i] = b"0123456789abcdef"[(b >> 4) as usize];
                name[2 * i + 1] = b"0123456789abcdef"[(b & 15) as usize];
            }
            name[64..69].copy_from_slice(b".fibp");
            self.base_id = Some(name);
        }
        true
    }
}

struct CountingSource {
    bytes: Vec<u8>,
    read_bytes: u64,
    receipt_live: bool,
    stale_after_reads: Option<u64>,
    read_calls: u64,
    shared_reads: std::rc::Rc<Cell<u64>>,
    fake_size: Option<u64>,
}
impl CountingSource {
    fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            read_bytes: 0,
            receipt_live: true,
            stale_after_reads: None,
            read_calls: 0,
            shared_reads: std::rc::Rc::new(Cell::new(0)),
            fake_size: None,
        }
    }
}
impl BookSource for CountingSource {
    fn size(&self) -> u64 {
        self.fake_size.unwrap_or(self.bytes.len() as u64)
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        self.read_calls += 1;
        if self.stale_after_reads.is_some_and(|n| self.read_calls > n) {
            return -1;
        }
        let Ok(at) = usize::try_from(at) else {
            return -1;
        };
        let Some(bytes) = self.bytes.get(at..) else {
            return -1;
        };
        let count = bytes.len().min(out.len());
        out[..count].copy_from_slice(&bytes[..count]);
        self.read_bytes += count as u64;
        self.shared_reads.set(self.read_bytes);
        count as i32
    }
}
impl ProducerReceipt for CountingSource {
    fn producer_receipt_live(&mut self) -> bool {
        self.receipt_live
    }
}

struct Never;
impl Authority for Never {
    fn cancelled(&self) -> bool {
        false
    }
}

#[repr(align(16))]
struct FrameLoan([u8; FRAME_BYTES]);
impl Default for FrameLoan {
    fn default() -> Self {
        Self([0xa5; FRAME_BYTES])
    }
}
impl BuildLoan for FrameLoan {
    fn with_parse<R>(
        &mut self,
        operation: impl for<'a> FnOnce(&'a mut [u8]) -> R,
    ) -> core::result::Result<R, Status> {
        let res = operation(&mut self.0);
        self.0.fill(0xff);
        Ok(res)
    }
}

fn clock_now() -> u32 {
    10_000
}

fn settings_for(landscape: bool) -> ViewSettings {
    ViewSettings {
        orientation: if landscape { Orientation::LandscapeCw } else { Orientation::Portrait },
        ..ViewSettings::default()
    }
}

fn layout_for(landscape: bool) -> PageLayout {
    if landscape { PageLayout::LANDSCAPE } else { PageLayout::PORTRAIT }
}

fn find_path(files: &BTreeMap<String, Vec<u8>>, name: &[u8; 69]) -> Option<String> {
    let suffix = core::str::from_utf8(name).unwrap();
    files.keys().find(|k| k.ends_with(suffix)).cloned()
}

fn find_bytes<'a>(files: &'a BTreeMap<String, Vec<u8>>, name: &[u8; 69]) -> Option<&'a Vec<u8>> {
    let suffix = core::str::from_utf8(name).unwrap();
    files.iter().find(|(k, _)| k.ends_with(suffix)).map(|(_, v)| v)
}

/// Rebuild a valid block-envelope around `payload` for `name`.
fn wrap_envelope(name: &[u8; 69], payload: &[u8]) -> Vec<u8> {
    let route = Query::new(Some(name)).route;
    let mut dir = Vec::new();
    for (block, bytes) in payload.chunks(BLOCK_BYTES as usize).enumerate() {
        dir.extend_from_slice(&page_envelope::block_crc(&route, block as u32, bytes).to_le_bytes());
    }
    let dir_crc = page_envelope::crc(u32::MAX, &dir) ^ u32::MAX;
    let whole_crc = page_envelope::crc(u32::MAX, payload) ^ u32::MAX;
    let mut manifest = [0u8; 64];
    page_envelope::manifest(&mut manifest, payload.len() as u32, &route, dir_crc, whole_crc);
    [payload, &dir, &manifest].concat()
}

/// Extract verified payload from a sealed envelope file.
fn unwrap_envelope(name: &[u8; 69], env: &[u8]) -> Vec<u8> {
    struct Raw(Vec<u8>);
    impl RawRead for Raw {
        fn read(&mut self, _: &Query<'_>, off: u32, out: &mut [u8]) -> bool {
            let at = off as usize;
            self.0.get(at..at + out.len()).map(|s| out.copy_from_slice(s)).is_some()
        }
    }
    let mut raw = Raw(env.to_vec());
    let (mut dir_buf, mut win_buf) = ([0u8; 4160], [0u8; 4096]);
    let mut vs = VerifiedStore::new(Some(name), &mut dir_buf, &mut win_buf);
    vs.query.extent = env.len() as u32;
    vs.query.bound = true;
    assert!(vs.attach(&mut raw), "envelope must attach cleanly");
    let mut out = vec![0u8; vs.query.payload as usize];
    assert_eq!(vs.read_at(&mut raw, Some(name), 0, &mut out), out.len() as i32);
    out
}

/// Replace the `FIRP` point section inside a valid v4 segment envelope, updating
/// the v4 footer (`index`, `anchor_off`, `point_len`), index offsets, FIRP CRC-32,
/// and outer block envelope so the resulting file has 100% valid envelope + FIRP CRCs.
fn rewrite_segment_point(
    name: &[u8; 69],
    env: &[u8],
    mutate_point: impl FnOnce(&mut Vec<u8>),
) -> Vec<u8> {
    let payload = unwrap_envelope(name, env);
    assert_eq!(&payload[..4], b"FIBP");
    assert_eq!(u16::from_le_bytes([payload[4], payload[5]]), VERSION_POINT);
    let ftr_at = payload.len() - 32;
    let u32_at = |buf: &[u8], at: usize| u32::from_le_bytes(buf[at..at + 4].try_into().unwrap());
    let index = u32_at(&payload, ftr_at) as usize;
    let count = u32_at(&payload, ftr_at + 4) as usize;
    let anchor_off = u32_at(&payload, ftr_at + 8) as usize;
    let anchors = u32_at(&payload, ftr_at + 12) as usize;
    let total = u32_at(&payload, ftr_at + 16);
    let pt_off = u32_at(&payload, ftr_at + 20) as usize;
    let pt_len = u32_at(&payload, ftr_at + 24) as usize;
    assert!(pt_len >= HEADER_BYTES + TRAILER_BYTES as usize);

    let mut point = payload[pt_off..pt_off + pt_len].to_vec();
    mutate_point(&mut point);
    // Recompute valid FIRP CRC-32 over header + sections.
    let body_len = point.len() - TRAILER_BYTES as usize;
    let crc = page_envelope::crc(u32::MAX, &point[..body_len]) ^ u32::MAX;
    point[body_len..].copy_from_slice(&crc.to_le_bytes());

    let delta = point.len() as isize - pt_len as isize;
    let mut rebuilt = Vec::new();
    rebuilt.extend_from_slice(&payload[..pt_off]);
    rebuilt.extend_from_slice(&point);
    rebuilt.extend_from_slice(&payload[pt_off + pt_len..index]);
    let new_index = (index as isize + delta) as u32;
    for i in 0..count {
        let e_at = index + i * 8;
        let off = u32_at(&payload, e_at);
        let chars = u32_at(&payload, e_at + 4);
        let adj_off = if (off as usize) > pt_off {
            (off as isize + delta) as u32
        } else {
            off
        };
        rebuilt.extend_from_slice(&adj_off.to_le_bytes());
        rebuilt.extend_from_slice(&chars.to_le_bytes());
    }
    let new_anchor_off = (anchor_off as isize + delta) as u32;
    rebuilt.extend_from_slice(&payload[anchor_off..anchor_off + anchors * 8]);
    let mut ftr = [0u8; 32];
    ftr[0..4].copy_from_slice(&new_index.to_le_bytes());
    ftr[4..8].copy_from_slice(&(count as u32).to_le_bytes());
    ftr[8..12].copy_from_slice(&new_anchor_off.to_le_bytes());
    ftr[12..16].copy_from_slice(&(anchors as u32).to_le_bytes());
    ftr[16..20].copy_from_slice(&total.to_le_bytes());
    ftr[20..24].copy_from_slice(&(pt_off as u32).to_le_bytes());
    ftr[24..28].copy_from_slice(&(point.len() as u32).to_le_bytes());
    ftr[28..32].copy_from_slice(b"FIBX");
    rebuilt.extend_from_slice(&ftr);
    wrap_envelope(name, &rebuilt)
}

/// Replace one section inside a FIRP blob and update its 36-byte header length.
fn replace_firp_section(point: &mut Vec<u8>, section: Section, replacement: &[u8]) {
    let hdr: &[u8; HEADER_BYTES] = point[..HEADER_BYTES].try_into().unwrap();
    let layout = Layout::parse(hdr, point.len() as u32).expect("valid source FIRP");
    let (off, len) = layout.range(section);
    let mut next = Vec::new();
    next.extend_from_slice(&point[..off as usize]);
    next.extend_from_slice(replacement);
    next.extend_from_slice(&point[(off + len) as usize..]);
    let len_at = 8 + 4 * (section as usize);
    next[len_at..len_at + 4].copy_from_slice(&(replacement.len() as u32).to_le_bytes());
    *point = next;
}

/// Extract `(point_offset, point_length)` from a valid v4 segment envelope.
fn segment_point_range(name: &[u8; 69], env: &[u8]) -> (u32, u32) {
    let payload = unwrap_envelope(name, env);
    let ftr_at = payload.len() - 32;
    let off = u32::from_le_bytes(payload[ftr_at + 20..ftr_at + 24].try_into().unwrap());
    let len = u32::from_le_bytes(payload[ftr_at + 24..ftr_at + 28].try_into().unwrap());
    (off, len)
}

#[derive(Default, Clone)]
struct StoreFault {
    flip_on_env_read: Option<usize>,
    fail_on_env_read: Option<usize>,
    /// Track Pass A -> Pass B transition when `(point_offset, point_length)` is known.
    track_point: Option<(u32, u32)>,
    /// When set alongside `track_point`, mutates this real FAT card file once Pass B starts.
    replace_card_path_on_pass_b: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DutyPhase {
    PassA,
    PassB,
    Discard,
    MapCollect,
    MapCollectAlways,
}

struct CountingBreak {
    inner: linebreak::NativeBreak,
    calls: u64,
}
impl CountingBreak {
    fn new() -> Self {
        Self { inner: linebreak::NativeBreak, calls: 0 }
    }
}
impl newt_core::chapter_layout::types::LineBreak for CountingBreak {
    fn breaks(&mut self, text: &[u8], language: Option<&core::ffi::CStr>, out: &mut [u8]) {
        self.calls += 1;
        self.inner.breaks(text, language, out);
    }
}

struct SpyStore<'a, S: CacheStorage> {
    inner: &'a mut S,
    fault: StoreFault,
    card_for_replace: Option<card::Card>,
    source_reads: std::rc::Rc<Cell<u64>>,
    source_reads_at_env_finish: u64,
    source_reads_at_write_begin: u64,
    env_begins: usize,
    env_reads: usize,
    env_finishes: usize,
    write_begins: usize,
    saw_point_tail: bool,
    in_pass_b: bool,
    replaced_card: bool,
    /// When set, triggers `Stop` via `stop_cell` on the N-th `read_envelope_raw`.
    stop_on_env_read: Option<(usize, Stop)>,
    stop_cell: &'a Cell<Stop>,
}
impl<'a, S: CacheStorage> SpyStore<'a, S> {
    fn new(
        inner: &'a mut S,
        fault: StoreFault,
        card_for_replace: Option<card::Card>,
        source_reads: std::rc::Rc<Cell<u64>>,
        stop_cell: &'a Cell<Stop>,
    ) -> Self {
        Self {
            inner,
            fault,
            card_for_replace,
            source_reads,
            source_reads_at_env_finish: 0,
            source_reads_at_write_begin: 0,
            env_begins: 0,
            env_reads: 0,
            env_finishes: 0,
            write_begins: 0,
            saw_point_tail: false,
            in_pass_b: false,
            replaced_card: false,
            stop_on_env_read: None,
            stop_cell,
        }
    }
}
impl<S: CacheStorage> CacheStorage for SpyStore<'_, S> {
    fn exists(&mut self, name: &[u8]) -> bool {
        self.inner.exists(name)
    }
    fn remove(&mut self, name: &[u8]) -> bool {
        self.inner.remove(name)
    }
    fn file_size(&mut self, name: &[u8]) -> i64 {
        self.inner.file_size(name)
    }
    fn read_at(&mut self, name: &[u8], offset: u32, out: &mut [u8]) -> i32 {
        self.inner.read_at(name, offset, out)
    }
    fn begin_write(&mut self, name: &[u8]) -> bool {
        self.write_begins += 1;
        if self.source_reads_at_write_begin == 0 {
            self.source_reads_at_write_begin = self.source_reads.get();
        }
        self.inner.begin_write(name)
    }
    fn write(&mut self, bytes: &[u8]) -> bool {
        self.inner.write(bytes)
    }
    fn end_write(&mut self) -> bool {
        self.inner.end_write()
    }
    fn end_write_checked(&mut self, receipt_live: &mut dyn FnMut() -> bool) -> bool {
        self.inner.end_write_checked(receipt_live)
    }
    fn read_back_at(&mut self, offset: u32, out: &mut [u8]) -> i32 {
        self.inner.read_back_at(offset, out)
    }
    fn producer_can_begin(&self) -> bool {
        self.inner.producer_can_begin()
    }
    fn producer_remaining_credit(&self) -> Option<u64> {
        self.inner.producer_remaining_credit()
    }
    fn last_write_published(&self) -> bool {
        self.inner.last_write_published()
    }
    fn supports_envelope_read(&self) -> bool {
        self.inner.supports_envelope_read()
    }
    fn envelope_read_suspended(&self) -> bool {
        self.inner.envelope_read_suspended()
    }
    fn begin_envelope_read(&mut self, name: &[u8], query: &mut Query<'_>) -> bool {
        self.env_begins += 1;
        self.saw_point_tail = false;
        self.in_pass_b = false;
        self.inner.begin_envelope_read(name, query)
    }
    fn read_envelope_raw(&mut self, query: &Query<'_>, offset: u32, out: &mut [u8]) -> bool {
        self.env_reads += 1;
        if let Some((pt_off, pt_len)) = self.fault.track_point {
            let pt_end = pt_off.saturating_add(pt_len);
            if self.saw_point_tail && offset == pt_off {
                self.in_pass_b = true;
            }
            if offset < pt_end && offset.saturating_add(out.len() as u32) >= pt_end {
                self.saw_point_tail = true;
            }
        }
        if let Some((at, reason)) = self.stop_on_env_read {
            if self.env_reads == at {
                self.stop_cell.set(reason);
                return false;
            }
        }
        if self.fault.fail_on_env_read == Some(self.env_reads) {
            return false;
        }
        let ok = self.inner.read_envelope_raw(query, offset, out);
        if ok && self.fault.flip_on_env_read == Some(self.env_reads) && !out.is_empty() {
            out[0] ^= 0x01;
        }
        ok
    }
    fn finish_envelope_read(&mut self, query: &mut Query<'_>) -> bool {
        self.env_finishes += 1;
        self.source_reads_at_env_finish = self.source_reads.get();
        if self.in_pass_b && !self.replaced_card {
            if let (Some(path), Some(c)) = (
                self.fault.replace_card_path_on_pass_b.as_deref(),
                self.card_for_replace.as_ref(),
            ) {
                // Replace the actual on-card file right after Pass B finishes reading
                // so only the real `FilePort::fresh_optional` check at finish can catch it!
                c.seed(path, vec![0x42; 64]);
                self.replaced_card = true;
            }
        }
        self.inner.finish_envelope_read(query)
    }
}

struct RunOpts {
    spine: u32,
    start_page: u32,
    landscape: bool,
    capture_points: bool,
    restore_points: bool,
    unsettled_style: bool,
    entries_per_attempt: u8,
    max_attempts: usize,
    guarded: bool,
    receipt_live: bool,
    stale_source_size: Option<u64>,
    fault: StoreFault,
    stop_on_env_read: Option<(usize, Stop)>,
    stop_on_pause_call: Option<(usize, Stop)>,
    duty_on_phase: Option<DutyPhase>,
}
impl Default for RunOpts {
    fn default() -> Self {
        Self {
            spine: 0,
            start_page: 0,
            landscape: false,
            capture_points: true,
            restore_points: true,
            unsettled_style: false,
            entries_per_attempt: 16,
            max_attempts: 4,
            guarded: false,
            receipt_live: true,
            stale_source_size: None,
            fault: StoreFault::default(),
            stop_on_env_read: None,
            stop_on_pause_call: None,
            duty_on_phase: None,
        }
    }
}

struct RunOutcome {
    outcome: CurrentOutcome,
    info: PageInfo,
    generation: u32,
    digest: Digest,
    seg_names: Vec<[u8; 69]>,
    map_name: [u8; 69],
    files: BTreeMap<String, Vec<u8>>,
    env_begins: usize,
    env_reads: usize,
    env_finishes: usize,
    write_begins: usize,
    book_read_bytes: u64,
    break_calls: u64,
    interrupted: bool,
    terminal: bool,
    most_open: usize,
    duty_ran: usize,
    in_pass_b_seen: bool,
    replaced_card: bool,
    source_reads_at_duty: u64,
    source_reads_at_write_begin: u64,
    state_with_pages: bool,
}

fn run_session(epub: &[u8], seed_files: &BTreeMap<String, Vec<u8>>, opts: &RunOpts) -> RunOutcome {
    let mut digest = [0u8; 32];
    assert!(sha256(epub, &mut digest));
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

    let mut card = card::Card::default();
    {
        let mut st = card.0.borrow_mut();
        for dir in ["/Newt", "/Newt/Cache", "/Newt/Cache/Pages", "/Newt/Cache/Meta", "/Newt/State"] {
            st.model.dirs.insert(dir.to_string());
        }
        st.enforce_reader_limit = true;
        // Reserve 2 real handles for Book + Receipt so cache + state MUST fit in <= 1 (total <= 3)!
        st.model.files.insert(
            "/book.epub".to_string(),
            model::File { durable: vec![1], readable: vec![1] },
        );
        st.model.files.insert(
            "/Newt/State/receipt.bin".to_string(),
            model::File { durable: vec![2], readable: vec![2] },
        );
    }
    for (path, bytes) in seed_files {
        card.seed(path, bytes.clone());
    }

    let mut out = None;
    board
        .with_reader_io(|panel, io| {
            let mut raw = CountingSource::new(epub.to_vec());
            let mut bank = [0x5a; BANK_BYTES];
            let mut storage = Storage::new(&mut bank);
            let mut parser = newt_target_expat::Expat::default();
            let mut breaker = CountingBreak::new();
            let mut selections = [Selection::default(), Selection::default()];
            let mut runs = [newt_phased_sink::Run::default(); RUNS];
            let mut text = [0u8; TEXT];
            let mut plan = Plan::new(&mut runs, &mut text);
            let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), layout_for(opts.landscape));
            let mut frame = [0u8; FRAME_BYTES];
            assert_eq!(reader.open_phased(&mut raw, &mut storage, panel, &mut parser), Status::Ok);
            assert_eq!(
                reader.capture_phased(
                    &mut raw, opts.spine, opts.start_page, Target::Page, &mut storage, panel,
                    &mut parser, &mut breaker,
                ),
                Status::Ok
            );
            assert_eq!(reader.publish(&mut raw, &mut frame), Status::Ok);
            let info = reader.info();
            let settings = settings_for(opts.landscape);

            // Reset counters after foreground capture so we measure ONLY producer work!
            raw.read_bytes = 0;
            raw.read_calls = 0;
            raw.shared_reads.set(0);
            breaker.calls = 0;
            raw.receipt_live = opts.receipt_live;
            raw.fake_size = opts.stale_source_size;

            // Open the 2 held device files (Book + Receipt) on the same FAT card model.
            let mut held_port = card.port(io);
            let fd_book = held_port.open("/book.epub", Open::SourceReadOnly).unwrap();
            let fd_rcpt = held_port.open("/Newt/State/receipt.bin", Open::SourceReadOnly).unwrap();

            let route_state = RouteState::new(
                "/Newt/Cache",
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            );
            let route_loan = route_state.lend(&Never, clock_now);
            let mut native = card.port(io);
            let mut cache_state = ChapterCacheState::new();
            let mut cache = ChapterCacheStorage::lend(&mut cache_state, &mut native, &route_loan).unwrap();

            let mut frame_loan = FrameLoan::default();
            let mut hash = Hasher::default();
            let mut generation = 0u32;
            let policy = newt_native_storage::ProducerPolicy::new(&Never);

            let mut last_outcome = CurrentOutcome::NoProgress(Status::Unsupported);
            let mut env_begins = 0usize;
            let mut env_reads = 0usize;
            let mut env_finishes = 0usize;
            let mut write_begins = 0usize;
            let mut last_interrupted = false;
            let mut last_terminal = false;
            let mut duty_ran = 0usize;
            let mut in_pass_b_seen = false;
            let mut replaced_card = false;
            let mut source_reads_at_duty = 0u64;
            let mut source_reads_at_write_begin = 0u64;
            let mut state_with_pages = false;
            let source_reads_ref = raw.shared_reads.clone();
            if opts.unsettled_style {
                // Reset `storage` so `phase.ready(spine)` is false, forcing
                // `produce_current` through `settle_current_selected` first.
                storage = Storage::new(&mut bank);
            }

            for _ in 0..opts.max_attempts {
                let stop_cell = Cell::new(Stop::None);
                let cancel = Cancellation::new(Hooks {
                    context: &stop_cell,
                    stop: Some(|c: &Cell<Stop>| c.get()),
                    yield_now: None,
                    pause: None,
                });
                let pause_calls = Cell::new(0usize);
                let step = cache
                    .produce(&policy, |access| {
                        access.set_producer_allowance(127, opts.entries_per_attempt);
                        let mut spy = SpyStore::new(
                            access,
                            opts.fault.clone(),
                            Some(card.clone()),
                            source_reads_ref.clone(),
                            &stop_cell,
                        );
                        spy.stop_on_env_read = opts.stop_on_env_read;
                        let mut route = Route { settings, digest: &digest, info, hash: &mut hash };

                        let card_for_duty = card.clone();
                        let reads_for_duty = source_reads_ref.clone();
                        let mut pause_cb =
                            |store: &mut SpyStore<'_, ChapterCacheAccess<'_, '_, card::Port<'_>>>,
                             _: &Cancellation<'_, Cell<Stop>>|
                             -> Stop {
                                let call = pause_calls.get() + 1;
                                pause_calls.set(call);
                                if let Some((at, reason)) = opts.stop_on_pause_call {
                                    if call == at {
                                        return reason;
                                    }
                                }
                                let fire_duty = match opts.duty_on_phase {
                                    Some(DutyPhase::PassA) => {
                                        duty_ran == 0
                                            && store.env_begins == 1
                                            && store.env_reads >= 2
                                            && !store.in_pass_b
                                            && store.env_finishes == 0
                                    }
                                    Some(DutyPhase::PassB) => {
                                        duty_ran == 0
                                            && store.in_pass_b
                                            && store.env_finishes == 0
                                    }
                                    Some(DutyPhase::Discard) => {
                                        // Fire only AFTER `discard_to` has already pulled at least
                                        // one chunk past `finish_envelope_read`, never at the
                                        // pre-discard `verify_session_alive` poll!
                                        duty_ran == 0
                                            && store.env_finishes >= 1
                                            && store.write_begins == 0
                                            && reads_for_duty.get() > store.source_reads_at_env_finish
                                    }
                                    Some(DutyPhase::MapCollect) => {
                                        duty_ran == 0
                                            && store.env_begins == 1
                                            && store.env_reads >= 1
                                            && store.env_finishes == 0
                                    }
                                    Some(DutyPhase::MapCollectAlways) => {
                                        // Fire once inside every single verified read lease in `collect_missing_starts`.
                                        store.env_begins > duty_ran
                                            && store.env_finishes < store.env_begins
                                            && store.env_reads >= 1
                                    }
                                    None => false,
                                };
                                if fire_duty {
                                    source_reads_at_duty = reads_for_duty.get();
                                    assert!(store.inner.suspend_producer_write());
                                    let mut state_port = card_for_duty.clone().port(io);
                                    let pages_open = card_for_duty
                                        .0
                                        .borrow()
                                        .handles
                                        .values()
                                        .any(|p| p.contains("/Pages/"));
                                    state_with_pages |= pages_open;
                                    // Opening State while Book+Receipt are open tests the exact <= 3 ceiling!
                                    let fd = state_port
                                        .open("/Newt/State/duty.tmp", Open::CreateTruncateWriteOnly)
                                        .unwrap();
                                    let _ = state_port.write(fd, b"duty-ok").unwrap();
                                    state_port.close(fd).unwrap();
                                    assert!(store.inner.resume_producer_write());
                                    duty_ran += 1;
                                }
                                Stop::None
                            };

                        let r = storage
                            .with_producer(
                                &mut frame_loan,
                                &cancel,
                                &mut spy,
                                &mut raw,
                                info.page_count,
                                Some(&mut pause_cb),
                                |producer| {
                                    let mut run_body = |producer: &mut newt_phased_reader::phase_storage::producer::Producer<'_, '_, '_, _, _, _>| {
                                        let res = if opts.capture_points && opts.restore_points {
                                            producer.produce_current(
                                                &mut reader, &mut parser, &mut breaker, &mut route,
                                            )
                                        } else {
                                            producer.state.set_language(reader.book_language());
                                            producer.with_buffered(|phase, mut attempt| {
                                                if attempt.state.count != 0 {
                                                    attempt.writer.segmented = true;
                                                    attempt.writer.capture_points = opts.capture_points;
                                                    attempt.writer.restore_points = opts.restore_points;
                                                    attempt.writer.mapping = true;
                                                    Ok(attempt.produce_segments_and_map_outcome(
                                                        &reader, phase, &mut parser, &mut breaker,
                                                        &mut route,
                                                    ))
                                                } else {
                                                    Ok(CurrentOutcome::NoProgress(Status::Unsupported))
                                                }
                                            })?
                                        };
                                        generation = producer.state.generation;
                                        Ok(res)
                                    };
                                    if opts.guarded {
                                        producer.with_source_guard(epub.len() as u64, run_body)
                                    } else {
                                        run_body(producer)
                                    }
                                },
                            )
                            .unwrap();
                        env_begins += spy.env_begins;
                        env_reads += spy.env_reads;
                        env_finishes += spy.env_finishes;
                        write_begins += spy.write_begins;
                        in_pass_b_seen |= spy.in_pass_b;
                        replaced_card |= spy.replaced_card;
                        if source_reads_at_write_begin == 0 {
                            source_reads_at_write_begin = spy.source_reads_at_write_begin;
                        }
                        r
                    })
                    .unwrap();
                last_outcome = step;
                last_interrupted = cancel.interrupted();
                last_terminal = cancel.terminal();
                if last_outcome != CurrentOutcome::Progress {
                    break;
                }
            }

            held_port.close(fd_rcpt).unwrap();
            held_port.close(fd_book).unwrap();

            let base_id = hash.base_id.expect("producer must hash chapter identity");
            let seg_count = ChapterSegment::count(info.page_count);
            assert!(seg_count >= 2, "test chapter must be segmented");
            let mut seg_names = Vec::new();
            for index in 0..seg_count {
                let mut name = [0u8; 128];
                name[..69].copy_from_slice(&base_id);
                let seg = ChapterSegment {
                    index,
                    pages: info.page_count,
                    chars: info.total_chars,
                };
                assert!(segment_name(seg, &mut name, &mut |b, d| sha256(b, d)));
                seg_names.push(name[..69].try_into().unwrap());
            }
            let mut mname = [0u8; 128];
            mname[..69].copy_from_slice(&base_id);
            assert!(map_name(&mut mname, &mut |b, d| sha256(b, d)));
            let map_name: [u8; 69] = mname[..69].try_into().unwrap();

            let st = card.0.borrow();
            let files: BTreeMap<String, Vec<u8>> = st
                .model
                .files
                .iter()
                .filter(|(k, _)| k.contains("/Pages/") || k.contains("/Meta/"))
                .map(|(k, v)| (k.clone(), v.durable.clone()))
                .collect();

            out = Some(RunOutcome {
                outcome: last_outcome,
                info,
                generation,
                digest,
                seg_names,
                map_name,
                files,
                env_begins,
                env_reads,
                env_finishes,
                write_begins,
                book_read_bytes: raw.read_bytes,
                break_calls: breaker.calls,
                interrupted: last_interrupted,
                terminal: last_terminal,
                most_open: st.most_open,
                duty_ran,
                in_pass_b_seen,
                replaced_card,
                source_reads_at_duty,
                source_reads_at_write_begin,
                state_with_pages,
            });
        })
        .unwrap();
    out.unwrap()
}

fn verify_segment_readable(name: &[u8; 69], file_bytes: &[u8], generation: u32) -> (u16, u32, u32) {
    struct Raw(Vec<u8>);
    impl RawRead for Raw {
        fn read(&mut self, _: &Query<'_>, off: u32, out: &mut [u8]) -> bool {
            let at = off as usize;
            self.0.get(at..at + out.len()).map(|s| out.copy_from_slice(s)).is_some()
        }
    }
    let mut raw = Raw(file_bytes.to_vec());
    let (mut dir_buf, mut win_buf) = ([0u8; 4160], [0u8; 4096]);
    let mut vs = VerifiedStore::new(Some(name), &mut dir_buf, &mut win_buf);
    vs.query.extent = file_bytes.len() as u32;
    vs.query.bound = true;
    assert!(vs.attach(&mut raw));
    let mut access = VerifiedAccess { store: &mut vs, source: &mut raw };
    let mut hdr = [0u8; 12];
    assert_eq!(access.read_at(name, 0, &mut hdr), 12);
    let version = u16::from_le_bytes([hdr[4], hdr[5]]);
    let mut entries = [Entry::default(); 256];
    let mut anchors = [Anchor::default(); 192];
    let mut rm = ReaderMemory { pages: &mut entries, anchors: &mut anchors };
    let mut name_buf = [0u8; 128];
    let mut names = Arena::new(&mut name_buf);
    let mut reader = PageCacheReader::default();
    assert_eq!(reader.open(&mut access, name, generation, &mut names, &mut rm), Status::Ok);
    // Decode first and last page in the segment to confirm exact page records.
    for idx in [0, reader.page_count() - 1] {
        let mut sb = [0u8; 32 * 1024];
        let mut sa = Arena::new(&mut sb);
        let mut r = [Run::default(); 304];
        let mut im = [Image::default(); 8];
        let mut lk = [Link::default(); 16];
        let mut pm = PageMemory { runs: &mut r, images: &mut im, links: &mut lk };
        let _ = reader.read_page(&mut access, &rm, &names, idx, &mut sa, &mut pm).unwrap();
    }
    (version, reader.page_count(), entries[0].chars)
}

fn case_restore_equals_byte0_oracle(fixtures: &Path) {
    for (fname, spine, landscape) in [
        ("stored.epub", 0, false),
        ("stored.epub", 0, true),
        ("stored.epub", 2, false), // replay-rich + multi-window + active link + nested blocks
        ("deflated.epub", 0, false),
        ("utf8bom.epub", 0, false), // UTF-8 BOM + non-ASCII <café>
    ] {
        let epub = std::fs::read(fixtures.join(fname)).unwrap();
        // Build seg 0..N one segment per attempt on identical starting cards:
        // compare `restore_points = true` vs `restore_points = false` (byte-0 oracle)
        // at EVERY step from the exact same starting card!
        let mut card_files = BTreeMap::new();
        let mut total_env_begins = 0usize;
        let mut seg_names = Vec::new();
        let mut map_name = [0u8; 69];

        for step in 0..8usize {
            let start_page = (step as u32) * PAGES;
            let restored = run_session(
                &epub,
                &card_files,
                &RunOpts {
                    spine,
                    start_page: if seg_names.is_empty() {
                        0
                    } else {
                        start_page.min((seg_names.len() as u32 - 1) * PAGES)
                    },
                    landscape,
                    capture_points: true,
                    restore_points: true,
                    entries_per_attempt: 1,
                    max_attempts: 1,
                    ..RunOpts::default()
                },
            );
            let oracle = run_session(
                &epub,
                &card_files,
                &RunOpts {
                    spine,
                    start_page: if seg_names.is_empty() {
                        0
                    } else {
                        start_page.min((seg_names.len() as u32 - 1) * PAGES)
                    },
                    landscape,
                    capture_points: true,
                    restore_points: false,
                    entries_per_attempt: 1,
                    max_attempts: 1,
                    ..RunOpts::default()
                },
            );
            if seg_names.is_empty() {
                seg_names = oracle.seg_names.clone();
                map_name = oracle.map_name;
                assert!(seg_names.len() >= 3, "{fname} spine {spine} must have >= 3 segments");
            }
            assert_eq!(
                restored.outcome, oracle.outcome,
                "{fname} spine {spine} step {step} outcome must match byte-0 oracle"
            );
            total_env_begins += restored.env_begins;
            if step >= 1 && step < seg_names.len() {
                assert!(
                    restored.env_begins >= 1 && restored.env_finishes >= 1,
                    "{fname} spine {spine} seg {step} must open and close predecessor envelope"
                );
                // Prove restore ACTUALLY skipped prefix layout (cannot pass via byte-0 fallback!)
                // on EVERY stored, deflated, landscape, replay-rich, and UTF-8 BOM segment.
                // All fixtures have >= 3 segments, so a byte-0 fallback lays out >= 2/3 of
                // `oracle` while a real restore lays out ~1/3 (`< 1/2`).
                assert!(
                    restored.break_calls * 2 < oracle.break_calls,
                    "{fname} spine {spine} (landscape={landscape}) seg {step} must skip prefix layout via restore (break_calls={} vs oracle={})",
                    restored.break_calls,
                    oracle.break_calls
                );
                if fname != "deflated.epub" {
                    assert!(
                        restored.book_read_bytes * 2 < oracle.book_read_bytes,
                        "{fname} spine {spine} (landscape={landscape}) seg {step} stored seek must skip prefix bytes ({} vs oracle {})",
                        restored.book_read_bytes,
                        oracle.book_read_bytes
                    );
                }
            }
            // Exact byte-for-byte parity on EVERY file written at this step (pages, anchors,
            // links, child FIRP point, and final NTSM map)!
            assert_eq!(
                restored.files, oracle.files,
                "restored step {step} of {fname} spine {spine} (landscape={landscape}) must be byte-identical to byte-0 oracle"
            );
            card_files = restored.files;
            if restored.outcome == CurrentOutcome::Done {
                break;
            }
            assert_eq!(restored.outcome, CurrentOutcome::Progress);
        }
        assert!(total_env_begins >= seg_names.len() - 1);
        assert!(find_bytes(&card_files, &map_name).is_some(), "final NTSM map must exist");
    }
    println!("PASS restore_equals_byte0_oracle_stored_and_deflated");
}

fn case_d3_target_first_and_decreasing_candidates(fixtures: &Path) {
    let epub = std::fs::read(fixtures.join("five_seg.epub")).unwrap();
    // Seed seg 0 and seg 1 via two single-entry passes so each is a real v4 segment.
    let s0 = run_session(
        &epub,
        &BTreeMap::new(),
        &RunOpts { entries_per_attempt: 1, max_attempts: 1, ..RunOpts::default() },
    );
    assert!(s0.seg_names.len() >= 5, "fixture must have >= 5 segments");
    let s1 = run_session(
        &epub,
        &s0.files,
        &RunOpts { start_page: PAGES, entries_per_attempt: 1, max_attempts: 1, ..RunOpts::default() },
    );
    let seed = s1.files;

    // 1. Seed seg 0 & seg 1 only; holes at 2, 3, 4; target = 4 (`start_page = 4 * 128`), entries = 1.
    let res = run_session(
        &epub,
        &seed,
        &RunOpts {
            start_page: 4 * PAGES,
            capture_points: true,
            restore_points: true,
            entries_per_attempt: 1,
            max_attempts: 1,
            ..RunOpts::default()
        },
    );
    assert_eq!(res.outcome, CurrentOutcome::Progress);
    assert_eq!(res.write_begins, 1, "must seal only target seg 4 under 1-entry grant");
    assert!(find_bytes(&res.files, &s0.seg_names[4]).is_some());
    assert!(find_bytes(&res.files, &s0.seg_names[2]).is_none(), "must skip hole 2");
    assert!(find_bytes(&res.files, &s0.seg_names[3]).is_none(), "must skip hole 3");
    // Compare seg 4 against byte-0 oracle with seg 0..3 present (so seg 4 is open at target_first).
    let mut seed0123 = seed.clone();
    for seg_i in [2usize, 3] {
        let step = run_session(
            &epub,
            &seed0123,
            &RunOpts {
                start_page: (seg_i as u32) * PAGES,
                restore_points: false,
                entries_per_attempt: 1,
                max_attempts: 1,
                ..RunOpts::default()
            },
        );
        seed0123 = step.files;
    }
    let oracle4 = run_session(
        &epub,
        &seed0123,
        &RunOpts {
            start_page: 4 * PAGES,
            restore_points: false,
            entries_per_attempt: 1,
            max_attempts: 1,
            ..RunOpts::default()
        },
    );
    assert_eq!(
        find_bytes(&res.files, &s0.seg_names[4]),
        find_bytes(&oracle4.files, &s0.seg_names[4]),
        "target seg 4 restored from seg 1 across holes 2..3 must match oracle"
    );
    assert!(
        res.break_calls < oracle4.break_calls * 3 / 4
            && res.book_read_bytes < oracle4.book_read_bytes * 3 / 4,
        "target seg 4 must restore from seg 1 instead of byte 0 (breaks {} vs {}, bytes {} vs {})",
        res.break_calls,
        oracle4.break_calls,
        res.book_read_bytes,
        oracle4.book_read_bytes
    );

    // 2. Corrupt seg 1's FIRP point; seg 0 remains valid. Target = 2, entries = 1.
    //    Strictly decreasing search rejects cand 1 locally and restores from cand 0!
    let mut seed2 = seed.clone();
    let p1 = find_path(&seed2, &s0.seg_names[1]).unwrap();
    let bad1 = rewrite_segment_point(&s0.seg_names[1], &seed2[&p1], |pt| {
        pt[0] ^= 0xff; // corrupt FIRP magic (outer CRC still valid)
    });
    seed2.insert(p1, bad1);
    let res2 = run_session(
        &epub,
        &seed2,
        &RunOpts {
            start_page: 2 * PAGES,
            capture_points: true,
            restore_points: true,
            entries_per_attempt: 1,
            max_attempts: 1,
            ..RunOpts::default()
        },
    );
    let oracle2 = run_session(
        &epub,
        &seed,
        &RunOpts {
            start_page: 2 * PAGES,
            capture_points: true,
            restore_points: false,
            entries_per_attempt: 1,
            max_attempts: 1,
            ..RunOpts::default()
        },
    );
    assert_eq!(res2.outcome, CurrentOutcome::Progress);
    assert!(!res2.terminal);
    assert_eq!(res2.env_begins, 2, "must probe cand 1 (reject) then cand 0 (restore)");
    assert_eq!(res2.write_begins, 1);
    assert!(
        res2.break_calls < oracle2.break_calls * 3 / 4
            && res2.book_read_bytes < oracle2.book_read_bytes * 3 / 4,
        "fallback to cand 0 must restore from seg 0 instead of byte 0 (breaks {} vs {}, bytes {} vs {})",
        res2.break_calls,
        oracle2.break_calls,
        res2.book_read_bytes,
        oracle2.book_read_bytes
    );
    assert_eq!(
        find_bytes(&res2.files, &s0.seg_names[2]),
        find_bytes(&oracle2.files, &s0.seg_names[2]),
        "fallback to cand 0 must seal byte-identical seg 2"
    );
    println!("PASS d3_target_first_and_strictly_decreasing_candidates");
}

fn case_peak_open_files_with_duty(fixtures: &Path) {
    // Use deflated spine 2 (replay-rich, multi-window FIRP > 2 KiB + Mode::Reinflate)
    // so Pass A, Pass B, and Reinflate discard each have observed phase boundaries.
    let epub = std::fs::read(fixtures.join("deflated.epub")).unwrap();
    let s0 = run_session(
        &epub,
        &BTreeMap::new(),
        &RunOpts { spine: 2, entries_per_attempt: 1, max_attempts: 1, ..RunOpts::default() },
    );
    let seed0 = s0.files;
    let p0 = find_path(&seed0, &s0.seg_names[0]).unwrap();
    let pt_range = segment_point_range(&s0.seg_names[0], &seed0[&p0]);
    let oracle1 = run_session(
        &epub,
        &seed0,
        &RunOpts {
            spine: 2,
            start_page: PAGES,
            restore_points: false,
            entries_per_attempt: 1,
            max_attempts: 1,
            ..RunOpts::default()
        },
    );
    let clean = run_session(
        &epub,
        &seed0,
        &RunOpts {
            spine: 2,
            start_page: PAGES,
            capture_points: true,
            restore_points: true,
            entries_per_attempt: 1,
            max_attempts: 1,
            fault: StoreFault { track_point: Some(pt_range), ..StoreFault::default() },
            ..RunOpts::default()
        },
    );
    assert!(clean.in_pass_b_seen && clean.break_calls * 2 < oracle1.break_calls);

    // Inject due duty from observed phase boundaries: Pass A, Pass B, and Reinflate discard.
    for (phase_name, phase) in [
        ("Pass A", DutyPhase::PassA),
        ("Pass B", DutyPhase::PassB),
        ("Reinflate discard", DutyPhase::Discard),
    ] {
        let res = run_session(
            &epub,
            &seed0,
            &RunOpts {
                spine: 2,
                start_page: PAGES,
                capture_points: true,
                restore_points: true,
                entries_per_attempt: 1,
                max_attempts: 1,
                fault: StoreFault { track_point: Some(pt_range), ..StoreFault::default() },
                duty_on_phase: Some(phase),
                ..RunOpts::default()
            },
        );
        assert_eq!(res.outcome, CurrentOutcome::Progress, "{phase_name}: must still seal target");
        assert_eq!(res.duty_ran, 1, "{phase_name}: duty must have actually fired once");
        assert!(res.most_open <= 3, "{phase_name}: peak open files {} exceeded 3", res.most_open);
        assert!(!res.state_with_pages, "{phase_name}: State must never open while Pages is open");
        assert_eq!(
            find_bytes(&res.files, &s0.seg_names[1]),
            find_bytes(&oracle1.files, &s0.seg_names[1]),
            "{phase_name}: sealed seg 1 must match oracle"
        );
        match phase {
            DutyPhase::PassA => {
                assert!(!res.in_pass_b_seen, "Pass A duty must fire before Pass B starts");
                assert!(
                    res.break_calls >= clean.break_calls + clean.break_calls / 2,
                    "Pass A duty must reject closed lease and fall back to byte 0 (breaks {} vs clean {})",
                    res.break_calls,
                    clean.break_calls
                );
            }
            DutyPhase::PassB => {
                assert!(res.in_pass_b_seen, "Pass B duty must fire inside Pass B");
                assert!(
                    res.break_calls >= clean.break_calls + clean.break_calls / 2,
                    "Pass B duty must reject closed lease and fall back to byte 0 (breaks {} vs clean {})",
                    res.break_calls,
                    clean.break_calls
                );
            }
            DutyPhase::Discard => {
                assert!(res.in_pass_b_seen, "Discard duty must occur after Pass B completes");
                assert!(
                    res.source_reads_at_duty > 0
                        && res.source_reads_at_write_begin > res.source_reads_at_duty,
                    "Discard duty must fire inside discard_to loop between source reads (at_duty={}, at_write_begin={})",
                    res.source_reads_at_duty,
                    res.source_reads_at_write_begin
                );
                // File 3 is already closed before positioning, so duty during
                // discard must keep the loaded point and complete restore!
                assert_eq!(
                    res.break_calls, clean.break_calls,
                    "Discard duty after file-3 close must complete restore without falling back to byte 0 (breaks {} vs clean {})",
                    res.break_calls,
                    clean.break_calls
                );
            }
            DutyPhase::MapCollect | DutyPhase::MapCollectAlways => unreachable!(),
        }
    }
    println!("PASS peak_open_files_at_most_three_with_duty_during_pass_a_pass_b_and_discard");
}

fn case_pass_b_tamper_and_replacement(fixtures: &Path) {
    let epub = std::fs::read(fixtures.join("stored.epub")).unwrap();
    let s0 = run_session(
        &epub,
        &BTreeMap::new(),
        &RunOpts {
            spine: 2, // multi-window FIRP (> 2 KiB)
            entries_per_attempt: 1,
            max_attempts: 1,
            ..RunOpts::default()
        },
    );
    let seed0 = s0.files;
    let p0 = find_path(&seed0, &s0.seg_names[0]).unwrap();
    let pt_range = segment_point_range(&s0.seg_names[0], &seed0[&p0]);
    let oracle1 = run_session(
        &epub,
        &seed0,
        &RunOpts {
            spine: 2,
            start_page: PAGES,
            restore_points: false,
            entries_per_attempt: 1,
            max_attempts: 1,
            ..RunOpts::default()
        },
    );

    // Measure total env_reads in a clean restore of seg 1 so we pinpoint exact Pass B reads!
    let clean = run_session(
        &epub,
        &seed0,
        &RunOpts {
            spine: 2,
            start_page: PAGES,
            entries_per_attempt: 1,
            max_attempts: 1,
            fault: StoreFault { track_point: Some(pt_range), ..StoreFault::default() },
            ..RunOpts::default()
        },
    );
    assert!(clean.in_pass_b_seen, "clean restore must execute Pass B");
    assert!(clean.env_reads >= 6, "multi-window Pass A + Pass B must do >= 6 env reads");
    let pass_b_read = clean.env_reads;

    // 1. Flip 1 byte on the final Pass B read (inside Replay section!).
    let flipped = run_session(
        &epub,
        &seed0,
        &RunOpts {
            spine: 2,
            start_page: PAGES,
            entries_per_attempt: 1,
            max_attempts: 1,
            fault: StoreFault {
                flip_on_env_read: Some(pass_b_read),
                track_point: Some(pt_range),
                ..StoreFault::default()
            },
            ..RunOpts::default()
        },
    );
    assert_eq!(flipped.outcome, CurrentOutcome::Progress);
    assert!(flipped.in_pass_b_seen, "flip fault must fire during Pass B");
    assert!(!flipped.terminal, "Pass B tamper must reject locally, never latch terminal");
    assert_eq!(
        find_bytes(&flipped.files, &s0.seg_names[1]),
        find_bytes(&oracle1.files, &s0.seg_names[1]),
        "Pass B CRC must reject tampered replay byte and seal exact oracle segment"
    );
    assert!(
        flipped.book_read_bytes >= clean.book_read_bytes + clean.book_read_bytes / 2
            && flipped.break_calls >= clean.break_calls + clean.break_calls / 2,
        "Pass B CRC must reject tampered replay byte and trigger byte-0 fallback (bytes {} vs {}, breaks {} vs {})",
        flipped.book_read_bytes,
        clean.book_read_bytes,
        flipped.break_calls,
        clean.break_calls
    );

    // 2. Short/failed read in Pass B -> local reject + byte-0 fallback.
    let short_read = run_session(
        &epub,
        &seed0,
        &RunOpts {
            spine: 2,
            start_page: PAGES,
            entries_per_attempt: 1,
            max_attempts: 1,
            fault: StoreFault {
                fail_on_env_read: Some(pass_b_read),
                track_point: Some(pt_range),
                ..StoreFault::default()
            },
            ..RunOpts::default()
        },
    );
    assert_eq!(short_read.outcome, CurrentOutcome::Progress);
    assert!(short_read.in_pass_b_seen);
    assert!(!short_read.terminal);
    assert!(
        short_read.book_read_bytes >= clean.book_read_bytes + clean.book_read_bytes / 2
            && short_read.break_calls >= clean.break_calls + clean.break_calls / 2,
        "Pass B short read must trigger byte-0 fallback"
    );
    assert_eq!(
        find_bytes(&short_read.files, &s0.seg_names[1]),
        find_bytes(&oracle1.files, &s0.seg_names[1])
    );

    // 3. Real FAT card file replacement right before finish_envelope_read -> real
    //    `FilePort::fresh_optional` rejects the replaced lease -> local reject + byte-0 fallback!
    let replaced = run_session(
        &epub,
        &seed0,
        &RunOpts {
            spine: 2,
            start_page: PAGES,
            entries_per_attempt: 1,
            max_attempts: 1,
            fault: StoreFault {
                track_point: Some(pt_range),
                replace_card_path_on_pass_b: Some(p0),
                ..StoreFault::default()
            },
            ..RunOpts::default()
        },
    );
    assert_eq!(replaced.outcome, CurrentOutcome::Progress);
    assert!(replaced.in_pass_b_seen && replaced.replaced_card, "must replace real on-card file after Pass B");
    assert!(!replaced.terminal);
    assert!(
        replaced.book_read_bytes >= clean.book_read_bytes + clean.book_read_bytes / 2
            && replaced.break_calls >= clean.break_calls + clean.break_calls / 2,
        "real on-card replacement at finish_envelope_read must reject locally and trigger byte-0 fallback (bytes {} vs {}, breaks {} vs {})",
        replaced.book_read_bytes,
        clean.book_read_bytes,
        replaced.break_calls,
        clean.break_calls
    );
    assert_eq!(
        find_bytes(&replaced.files, &s0.seg_names[1]),
        find_bytes(&oracle1.files, &s0.seg_names[1])
    );
    println!("PASS pass_b_tamper_short_read_and_file_replacement_detected");
}

fn case_adversarial_valid_crc_points(fixtures: &Path) {
    let epub = std::fs::read(fixtures.join("stored.epub")).unwrap();
    let s0 = run_session(
        &epub,
        &BTreeMap::new(),
        &RunOpts {
            spine: 0,
            entries_per_attempt: 1,
            max_attempts: 1,
            ..RunOpts::default()
        },
    );
    let p0 = find_path(&s0.files, &s0.seg_names[0]).unwrap();
    let seg0_good = &s0.files[&p0];
    let oracle = run_session(
        &epub,
        &s0.files,
        &RunOpts {
            spine: 0,
            start_page: PAGES,
            capture_points: true,
            restore_points: false,
            entries_per_attempt: 1,
            max_attempts: 1,
            ..RunOpts::default()
        },
    );

    let clean = run_session(
        &epub,
        &BTreeMap::from([(p0.clone(), seg0_good.clone())]),
        &RunOpts {
            spine: 0,
            start_page: PAGES,
            entries_per_attempt: 1,
            max_attempts: 1,
            ..RunOpts::default()
        },
    );
    let clean_book_bytes = clean.book_read_bytes;

    // Build a dense forged Replay that forces > 6 page flushes to cross edge=128
    // inside any open inline/block element if ReplaySink::page fails to reject `next >= edge`!
    let edge_crossing_replay: Vec<u8> = "H<hr/>".repeat(200).into_bytes();

    type Mutator<'a> = (&'a str, Box<dyn Fn(&mut Vec<u8>)>);
    let mutators: Vec<Mutator<'_>> = vec![
        ("engine depth = i32::MAX", Box::new(|pt| {
            let hdr: &[u8; HEADER_BYTES] = pt[..HEADER_BYTES].try_into().unwrap();
            let layout = Layout::parse(hdr, pt.len() as u32).unwrap();
            let (off, _) = layout.range(Section::Engine);
            let depth_at = off as usize + 4 + 3 * 4;
            pt[depth_at..depth_at + 4].copy_from_slice(&(i32::MAX as u32).to_le_bytes());
        })),
        ("engine depth mismatches Names stack", Box::new(|pt| {
            let hdr: &[u8; HEADER_BYTES] = pt[..HEADER_BYTES].try_into().unwrap();
            let layout = Layout::parse(hdr, pt.len() as u32).unwrap();
            let (off, _) = layout.range(Section::Engine);
            let depth_at = off as usize + 4 + 3 * 4;
            let cur = u32::from_le_bytes(pt[depth_at..depth_at + 4].try_into().unwrap());
            pt[depth_at..depth_at + 4].copy_from_slice(&(cur.saturating_sub(1)).to_le_bytes());
        })),
        ("engine text lone UTF-8 continuation bytes", Box::new(|pt| {
            let hdr: &[u8; HEADER_BYTES] = pt[..HEADER_BYTES].try_into().unwrap();
            let layout = Layout::parse(hdr, pt.len() as u32).unwrap();
            let (eoff, elen) = layout.range(Section::Engine);
            let eng = &pt[eoff as usize..(eoff + elen) as usize];
            let old_len = u16::from_le_bytes(eng[36..38].try_into().unwrap()) as usize;
            let old_spans = u16::from_le_bytes(eng[38..40].try_into().unwrap()) as usize;
            // Without UTF-8 validation, 39 lone 0x80 bytes expand 1->2 B in
            // emit_segment past its n + n/2 + 4 reservation and panic!
            let mut rebuilt = Vec::new();
            rebuilt.extend_from_slice(&eng[..81]);
            rebuilt[36..38].copy_from_slice(&40u16.to_le_bytes());
            rebuilt[38..40].copy_from_slice(&1u16.to_le_bytes());
            rebuilt.push(b'A');
            rebuilt.extend_from_slice(&[0x80u8; 39]);
            // Span { start: 0, size: 100, margin: 0, flags: 0, link: 0 }
            rebuilt.extend_from_slice(&[0, 0, 100, 0, 0, 0, 0, 0]);
            rebuilt.extend_from_slice(&eng[81 + old_len + old_spans * 8..]);
            replace_firp_section(pt, Section::Engine, &rebuilt);
        })),
        ("binding pages_at == edge", Box::new(|pt| {
            let hdr: &[u8; HEADER_BYTES] = pt[..HEADER_BYTES].try_into().unwrap();
            let layout = Layout::parse(hdr, pt.len() as u32).unwrap();
            let (off, _) = layout.range(Section::Binding);
            let pages_at = off as usize + 4 + 32 + 9 * 4;
            pt[pages_at..pages_at + 4].copy_from_slice(&PAGES.to_le_bytes());
        })),
        ("binding + engine pages_at disagrees with point_owner", Box::new(|pt| {
            let hdr: &[u8; HEADER_BYTES] = pt[..HEADER_BYTES].try_into().unwrap();
            let layout = Layout::parse(hdr, pt.len() as u32).unwrap();
            let (boff, _) = layout.range(Section::Binding);
            let (eoff, _) = layout.range(Section::Engine);
            let pages_at = boff as usize + 4 + 32 + 9 * 4;
            let cur = u32::from_le_bytes(pt[pages_at..pages_at + 4].try_into().unwrap());
            let shifted = if cur > PAGES - 4 { cur - 1 } else { cur + 1 };
            pt[pages_at..pages_at + 4].copy_from_slice(&shifted.to_le_bytes());
            let eng_pages = eoff as usize + 4;
            pt[eng_pages..eng_pages + 4].copy_from_slice(&shifted.to_le_bytes());
        })),
        ("engine pending_margin above u16::MAX", Box::new(|pt| {
            let hdr: &[u8; HEADER_BYTES] = pt[..HEADER_BYTES].try_into().unwrap();
            let layout = Layout::parse(hdr, pt.len() as u32).unwrap();
            let (eoff, _) = layout.range(Section::Engine);
            let pm_at = eoff as usize + 4 + 6 * 4;
            pt[pm_at..pm_at + 4].copy_from_slice(&(u32::from(u16::MAX) + 1).to_le_bytes());
        })),
        ("entity filter source_done with unread raw tail", Box::new(|pt| {
            let hdr: &[u8; HEADER_BYTES] = pt[..HEADER_BYTES].try_into().unwrap();
            let layout = Layout::parse(hdr, pt.len() as u32).unwrap();
            let (boff, _) = layout.range(Section::Binding);
            let (soff, _) = layout.range(Section::Source);
            let (xoff, _) = layout.range(Section::Stream);
            let member_uncomp = u32::from_le_bytes(
                pt[boff as usize + 4 + 32 + 8 * 4..boff as usize + 4 + 32 + 9 * 4]
                    .try_into()
                    .unwrap(),
            );
            pt[soff as usize + 4..soff as usize + 8].copy_from_slice(&member_uncomp.to_le_bytes());
            let mut stream = Vec::new();
            stream.extend_from_slice(&pt[xoff as usize..xoff as usize + 12]); // XST1 + consumed
            stream.extend_from_slice(&member_uncomp.to_le_bytes()); // produced == member_uncomp
            stream.push(1); // has_filter = true
            stream.extend_from_slice(b"FLT1");
            stream.extend_from_slice(&[0, 0, 0, 0]); // entity/pending empty
            stream.extend_from_slice(&1u16.to_le_bytes()); // raw_unread = 1
            stream.push(b'x'); // 1 unread raw byte
            stream.push(1); // source_done = 1 (impossible with raw_unread > 0)
            replace_firp_section(pt, Section::Stream, &stream);
        })),
        ("entity filter source_done with in_entity", Box::new(|pt| {
            let hdr: &[u8; HEADER_BYTES] = pt[..HEADER_BYTES].try_into().unwrap();
            let layout = Layout::parse(hdr, pt.len() as u32).unwrap();
            let (boff, _) = layout.range(Section::Binding);
            let (soff, _) = layout.range(Section::Source);
            let (xoff, _) = layout.range(Section::Stream);
            let member_uncomp = u32::from_le_bytes(
                pt[boff as usize + 4 + 32 + 8 * 4..boff as usize + 4 + 32 + 9 * 4]
                    .try_into()
                    .unwrap(),
            );
            pt[soff as usize + 4..soff as usize + 8].copy_from_slice(&member_uncomp.to_le_bytes());
            let mut stream = Vec::new();
            stream.extend_from_slice(&pt[xoff as usize..xoff as usize + 12]); // XST1 + consumed
            stream.extend_from_slice(&member_uncomp.to_le_bytes()); // produced == member_uncomp
            stream.push(1); // has_filter = true
            stream.extend_from_slice(b"FLT1");
            stream.extend_from_slice(&[2, 1, 0, 0]); // entity_len=2, in_entity=1, pending empty
            stream.extend_from_slice(&0u16.to_le_bytes()); // raw_unread = 0
            stream.extend_from_slice(b"nb"); // partial entity name
            stream.push(1); // source_done = 1 (impossible with in_entity == 1)
            replace_firp_section(pt, Section::Stream, &stream);
        })),
        ("binding cut > fed", Box::new(|pt| {
            let hdr: &[u8; HEADER_BYTES] = pt[..HEADER_BYTES].try_into().unwrap();
            let layout = Layout::parse(hdr, pt.len() as u32).unwrap();
            let (off, _) = layout.range(Section::Binding);
            let cut_at = off as usize + 4 + 32 + 12 * 4;
            pt[cut_at..cut_at + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        })),
        ("stream produced != source consumed", Box::new(|pt| {
            let hdr: &[u8; HEADER_BYTES] = pt[..HEADER_BYTES].try_into().unwrap();
            let layout = Layout::parse(hdr, pt.len() as u32).unwrap();
            let (off, _) = layout.range(Section::Source);
            let src_at = off as usize + 4;
            let v = u32::from_le_bytes(pt[src_at..src_at + 4].try_into().unwrap());
            pt[src_at..src_at + 4].copy_from_slice(&(v.wrapping_sub(1)).to_le_bytes());
        })),
        ("forged prolog with element", Box::new(|pt| {
            replace_firp_section(pt, Section::Prolog, b"<html>");
        })),
        ("forged prolog with latin1 xml decl", Box::new(|pt| {
            replace_firp_section(pt, Section::Prolog, b"<?xml version='1.0' encoding='ISO-8859-1'?>");
        })),
        ("forged names with tag injection", Box::new(|pt| {
            replace_firp_section(pt, Section::Names, b"\x01\x04a><b");
        })),
        ("forged replay causing Expat ParseError", Box::new(|pt| {
            let bad = b"<<<bad";
            let hdr: &[u8; HEADER_BYTES] = pt[..HEADER_BYTES].try_into().unwrap();
            let layout = Layout::parse(hdr, pt.len() as u32).unwrap();
            let (boff, _) = layout.range(Section::Binding);
            let cut = u64::from_le_bytes(pt[boff as usize + 84..boff as usize + 92].try_into().unwrap());
            let new_fed = cut + bad.len() as u64;
            pt[boff as usize + 92..boff as usize + 100].copy_from_slice(&new_fed.to_le_bytes());
            let (xoff, _) = layout.range(Section::Stream);
            pt[xoff as usize + 4..xoff as usize + 12].copy_from_slice(&new_fed.to_le_bytes());
            replace_firp_section(pt, Section::Replay, bad);
        })),
        ("forged replay closing root element", Box::new(|pt| {
            let close = b"</p></body></html>";
            let hdr: &[u8; HEADER_BYTES] = pt[..HEADER_BYTES].try_into().unwrap();
            let layout = Layout::parse(hdr, pt.len() as u32).unwrap();
            let (boff, _) = layout.range(Section::Binding);
            let cut = u64::from_le_bytes(pt[boff as usize + 84..boff as usize + 92].try_into().unwrap());
            let new_fed = cut + close.len() as u64;
            pt[boff as usize + 92..boff as usize + 100].copy_from_slice(&new_fed.to_le_bytes());
            let (xoff, _) = layout.range(Section::Stream);
            pt[xoff as usize + 4..xoff as usize + 12].copy_from_slice(&new_fed.to_le_bytes());
            replace_firp_section(pt, Section::Replay, close);
        })),
        ("forged replay crossing edge", Box::new(move |pt| {
            let hdr: &[u8; HEADER_BYTES] = pt[..HEADER_BYTES].try_into().unwrap();
            let layout = Layout::parse(hdr, pt.len() as u32).unwrap();
            let (boff, _) = layout.range(Section::Binding);
            let cut = u64::from_le_bytes(pt[boff as usize + 84..boff as usize + 92].try_into().unwrap());
            let new_fed = cut + edge_crossing_replay.len() as u64;
            pt[boff as usize + 92..boff as usize + 100].copy_from_slice(&new_fed.to_le_bytes());
            let (xoff, _) = layout.range(Section::Stream);
            pt[xoff as usize + 4..xoff as usize + 12].copy_from_slice(&new_fed.to_le_bytes());
            replace_firp_section(pt, Section::Replay, &edge_crossing_replay);
        })),
        ("extra trailing byte in Engine section", Box::new(|pt| {
            let hdr: &[u8; HEADER_BYTES] = pt[..HEADER_BYTES].try_into().unwrap();
            let layout = Layout::parse(hdr, pt.len() as u32).unwrap();
            let (off, len) = layout.range(Section::Engine);
            let mut eng = pt[off as usize..(off + len) as usize].to_vec();
            eng.push(0);
            replace_firp_section(pt, Section::Engine, &eng);
        })),
    ];

    for (label, mutate) in &mutators {
        let forged_env = rewrite_segment_point(&oracle.seg_names[0], seg0_good, |pt| mutate(pt));
        let mut seed = BTreeMap::new();
        seed.insert(p0.clone(), forged_env);
        let res = run_session(
            &epub,
            &seed,
            &RunOpts {
                spine: 0,
                start_page: PAGES,
                entries_per_attempt: 1,
                max_attempts: 1,
                ..RunOpts::default()
            },
        );
        assert_eq!(
            res.outcome,
            CurrentOutcome::Progress,
            "adversarial valid-CRC point ({label}) must fall back cleanly and seal seg 1 under 1-entry grant"
        );
        assert!(!res.terminal, "adversarial point ({label}) must not latch terminal");
        assert_eq!(
            res.write_begins, 1,
            "adversarial point ({label}) must not burn the 1-entry grant before fallback"
        );
        assert!(
            res.book_read_bytes >= clean_book_bytes + clean_book_bytes / 2
                && res.break_calls >= clean.break_calls + clean.break_calls / 2,
            "malformed point ({label}) must reject locally and fall back to byte 0 (bytes {} vs clean {}, breaks {} vs clean {})",
            res.book_read_bytes,
            clean_book_bytes,
            res.break_calls,
            clean.break_calls
        );
        assert_eq!(
            find_bytes(&res.files, &oracle.seg_names[1]),
            find_bytes(&oracle.files, &oracle.seg_names[1]),
            "adversarial point ({label}) fallback must seal byte-identical seg 1"
        );
    }

    // Oversized point range (`length > MAX_POINT_BYTES`) inside a valid outer block
    // envelope must be rejected in Pass A BEFORE streaming the index or point range!
    {
        let payload = unwrap_envelope(&oracle.seg_names[0], seg0_good);
        let ftr_at = payload.len() - 32;
        let u32_at = |b: &[u8], at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
        let index = u32_at(&payload, ftr_at) as usize;
        let count = u32_at(&payload, ftr_at + 4) as usize;
        let anchor_off = u32_at(&payload, ftr_at + 8) as usize;
        let anchors = u32_at(&payload, ftr_at + 12) as usize;
        let total = u32_at(&payload, ftr_at + 16);
        let pt_off = u32_at(&payload, ftr_at + 20) as usize;
        let pt_len = u32_at(&payload, ftr_at + 24) as usize;
        let huge_len = MAX_POINT_BYTES as usize + 4096;
        let delta = huge_len as isize - pt_len as isize;
        let mut rebuilt = Vec::new();
        rebuilt.extend_from_slice(&payload[..pt_off]);
        rebuilt.resize(pt_off + huge_len, 0u8);
        rebuilt.extend_from_slice(&payload[pt_off + pt_len..index]);
        let new_index = (index as isize + delta) as u32;
        for i in 0..count {
            let e_at = index + i * 8;
            let off = u32_at(&payload, e_at);
            let chars = u32_at(&payload, e_at + 4);
            let adj_off = if (off as usize) > pt_off {
                (off as isize + delta) as u32
            } else {
                off
            };
            rebuilt.extend_from_slice(&adj_off.to_le_bytes());
            rebuilt.extend_from_slice(&chars.to_le_bytes());
        }
        let new_anchor_off = (anchor_off as isize + delta) as u32;
        rebuilt.extend_from_slice(&payload[anchor_off..anchor_off + anchors * 8]);
        let mut ftr = [0u8; 32];
        ftr[0..4].copy_from_slice(&new_index.to_le_bytes());
        ftr[4..8].copy_from_slice(&(count as u32).to_le_bytes());
        ftr[8..12].copy_from_slice(&new_anchor_off.to_le_bytes());
        ftr[12..16].copy_from_slice(&(anchors as u32).to_le_bytes());
        ftr[16..20].copy_from_slice(&total.to_le_bytes());
        ftr[20..24].copy_from_slice(&(pt_off as u32).to_le_bytes());
        ftr[24..28].copy_from_slice(&(huge_len as u32).to_le_bytes());
        ftr[28..32].copy_from_slice(b"FIBX");
        rebuilt.extend_from_slice(&ftr);
        let huge_env = wrap_envelope(&oracle.seg_names[0], &rebuilt);

        let mut seed = BTreeMap::new();
        seed.insert(p0.clone(), huge_env);
        let res = run_session(
            &epub,
            &seed,
            &RunOpts {
                spine: 0,
                start_page: PAGES,
                entries_per_attempt: 1,
                max_attempts: 1,
                ..RunOpts::default()
            },
        );
        assert_eq!(res.outcome, CurrentOutcome::Progress);
        assert!(!res.terminal);
        assert!(
            res.env_reads <= 14,
            "oversized point range must be rejected in Pass A before scanning range (saw {} env_reads)",
            res.env_reads
        );
        assert_eq!(
            find_bytes(&res.files, &oracle.seg_names[1]),
            find_bytes(&oracle.files, &oracle.seg_names[1]),
        );
    }
    println!("PASS adversarial_valid_crc_structured_points_rejected_without_panic_or_terminal");
}

fn case_real_cancel_duty_terminal_and_stale_receipt(fixtures: &Path) {
    let epub = std::fs::read(fixtures.join("deflated.epub")).unwrap();
    let oracle = run_session(
        &epub,
        &BTreeMap::new(),
        &RunOpts {
            capture_points: true,
            restore_points: false,
            entries_per_attempt: 16,
            max_attempts: 2,
            ..RunOpts::default()
        },
    );
    let mut seed = BTreeMap::new();
    for i in [0usize, 1] {
        let p = find_path(&oracle.files, &oracle.seg_names[i]).unwrap();
        seed.insert(p.clone(), oracle.files[&p].clone());
    }

    // 1. Real Stop::Interrupted during Pass A read of cand 1 -> must NOT try cand 0 or byte 0!
    let cancel_a = run_session(
        &epub,
        &seed,
        &RunOpts {
            start_page: 2 * PAGES,
            entries_per_attempt: 1,
            max_attempts: 1,
            stop_on_env_read: Some((2, Stop::Interrupted)),
            ..RunOpts::default()
        },
    );
    assert_eq!(cancel_a.outcome, CurrentOutcome::NoProgress(Status::IoError));
    assert!(cancel_a.interrupted && !cancel_a.terminal, "interruption during Pass A must stay interrupted");
    assert_eq!(cancel_a.env_begins, 1, "interruption during Pass A must not try older candidate");
    assert_eq!(cancel_a.write_begins, 0, "interruption during Pass A must not fall back to byte 0");

    // 2. Duty returning Stop::Terminal during Pass A -> terminal, NO fallback!
    let duty_term = run_session(
        &epub,
        &seed,
        &RunOpts {
            start_page: 2 * PAGES,
            entries_per_attempt: 1,
            max_attempts: 1,
            stop_on_pause_call: Some((3, Stop::Terminal)),
            ..RunOpts::default()
        },
    );
    assert!(matches!(duty_term.outcome, CurrentOutcome::NoProgress(_)));
    assert!(duty_term.terminal, "terminal duty failure must stay terminal");
    assert_eq!(duty_term.env_begins, 1, "terminal duty failure must not try older candidate");
    assert_eq!(duty_term.write_begins, 0, "terminal duty failure must not fall back to byte 0");

    // 3. Stale ProducerReceipt (guarded=true, receipt_live=false) -> Stale + terminal, NO fallback!
    let stale_rcpt = run_session(
        &epub,
        &seed,
        &RunOpts {
            start_page: 2 * PAGES,
            entries_per_attempt: 1,
            max_attempts: 1,
            guarded: true,
            receipt_live: false,
            ..RunOpts::default()
        },
    );
    assert_eq!(
        stale_rcpt.outcome,
        CurrentOutcome::NoProgress(Status::Stale),
        "stale_rcpt must fail with Stale before opening target"
    );
    assert!(stale_rcpt.terminal);
    assert_eq!(stale_rcpt.write_begins, 0, "stale_rcpt must never open target temp");

    // 4. Stale BookSource size -> stops immediately without fallback!
    let stale_src = run_session(
        &epub,
        &seed,
        &RunOpts {
            start_page: 2 * PAGES,
            entries_per_attempt: 1,
            max_attempts: 1,
            stale_source_size: Some(123),
            ..RunOpts::default()
        },
    );
    assert!(matches!(stale_src.outcome, CurrentOutcome::NoProgress(_)));
    assert_eq!(stale_src.write_begins, 0);

    // 5. Real Stop::Interrupted during Reinflate discard -> interrupted, NOT terminal, NO byte-0 fallback!
    let cancel_disc = run_session(
        &epub,
        &seed,
        &RunOpts {
            start_page: 2 * PAGES,
            entries_per_attempt: 1,
            max_attempts: 1,
            stop_on_pause_call: Some((35, Stop::Interrupted)),
            ..RunOpts::default()
        },
    );
    assert_eq!(cancel_disc.outcome, CurrentOutcome::NoProgress(Status::IoError));
    assert!(cancel_disc.interrupted && !cancel_disc.terminal);
    assert_eq!(cancel_disc.write_begins, 0);
    println!("PASS real_cancellation_duty_terminal_and_stale_receipt_never_fall_back");
}

fn case_damaged_deflate_tail(fixtures: &Path) {
    let epub = std::fs::read(fixtures.join("deflated.epub")).unwrap();
    let oracle = run_session(
        &epub,
        &BTreeMap::new(),
        &RunOpts {
            capture_points: true,
            restore_points: false,
            entries_per_attempt: 16,
            max_attempts: 2,
            ..RunOpts::default()
        },
    );
    let mut seed0 = BTreeMap::new();
    let p0 = find_path(&oracle.files, &oracle.seg_names[0]).unwrap();
    seed0.insert(p0.clone(), oracle.files[&p0].clone());

    // Corrupt a compressed byte near the end of ch0.xhtml inside the ZIP (after seg 0's cut).
    // Locate ch0.xhtml compressed data via local header signature in `epub`.
    let mut damaged = epub.clone();
    let marker = b"OEBPS/ch0.xhtml";
    let pos = damaged
        .windows(marker.len())
        .position(|w| w == marker)
        .expect("ch0 local header");
    let lfh = pos - 30;
    let comp_len = u32::from_le_bytes(damaged[lfh + 18..lfh + 22].try_into().unwrap()) as usize;
    let name_len = u16::from_le_bytes(damaged[lfh + 26..lfh + 28].try_into().unwrap()) as usize;
    let extra_len = u16::from_le_bytes(damaged[lfh + 28..lfh + 30].try_into().unwrap()) as usize;
    let data_start = lfh + 30 + name_len + extra_len;
    let flip_at = data_start + (comp_len * 9) / 10;
    for b in &mut damaged[flip_at..flip_at + 16] {
        *b ^= 0xa5;
    }

    let mut seed01 = seed0.clone();
    let p1 = find_path(&oracle.files, &oracle.seg_names[1]).unwrap();
    seed01.insert(p1.clone(), oracle.files[&p1].clone());
    let seg2_suffix = core::str::from_utf8(&oracle.seg_names[2]).unwrap().to_string();
    let _ = damaged;

    // Directly test a source that flips compressed tail bytes after foreground publish:
    let mut digest = [0u8; 32];
    assert!(sha256(&epub, &mut digest));
    let mut tokens = [0u8; 2];
    let (mut board, pending, _) = unsafe {
        newt_c3_board::Board::acquire_x3_optional_sd(
            (&mut tokens[0] as *mut u8).cast(),
            (&mut tokens[1] as *mut u8).cast(),
        )
    }
    .unwrap();
    drop(pending);

    let mut run_with_damaged_tail = |restore_points: bool| -> (CurrentOutcome, bool, bool, usize, usize, u64) {
        let mut card = card::Card::default();
        for (k, v) in &seed01 {
            card.seed(k, v.clone());
        }
        let mut res = None;
        board
            .with_reader_io(|panel, io| {
                let mut raw = CountingSource::new(epub.clone());
                let mut bank = [0; BANK_BYTES];
                let mut storage = Storage::new(&mut bank);
                let mut parser = newt_target_expat::Expat::default();
                let mut breaker = CountingBreak::new();
                let mut selections = [Selection::default(), Selection::default()];
                let mut runs = [newt_phased_sink::Run::default(); RUNS];
                let mut text = [0u8; TEXT];
                let mut plan = Plan::new(&mut runs, &mut text);
                let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), PageLayout::PORTRAIT);
                let mut frame = [0u8; FRAME_BYTES];
                assert_eq!(reader.open_phased(&mut raw, &mut storage, panel, &mut parser), Status::Ok);
                assert_eq!(
                    reader.capture_phased(
                        &mut raw, 0, 2 * PAGES, Target::Page, &mut storage, panel, &mut parser,
                        &mut breaker,
                    ),
                    Status::Ok
                );
                assert_eq!(reader.publish(&mut raw, &mut frame), Status::Ok);
                let info = reader.info();
                breaker.calls = 0;
                // Now corrupt the final 10% of ch0's compressed stream before producer runs!
                for b in &mut raw.bytes[flip_at..flip_at + 16] {
                    *b ^= 0xa5;
                }

                let route_state = RouteState::new(
                    "/Newt/Cache",
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                );
                let route_loan = route_state.lend(&Never, clock_now);
                let mut native = card.port(io);
                let mut cache_state = ChapterCacheState::new();
                let mut cache = ChapterCacheStorage::lend(&mut cache_state, &mut native, &route_loan).unwrap();
                let stop_cell = Cell::new(Stop::None);
                let cancel = Cancellation::new(Hooks {
                    context: &stop_cell,
                    stop: Some(|c: &Cell<Stop>| c.get()),
                    yield_now: None,
                    pause: None,
                });
                let mut frame_loan = FrameLoan::default();
                let mut hash = Hasher::default();
                let policy = newt_native_storage::ProducerPolicy::new(&Never);
                let mut env_begins = 0usize;
                let mut env_finishes = 0usize;
                let out = cache
                    .produce(&policy, |access| {
                        access.set_producer_allowance(127, 4);
                        let mut spy = SpyStore::new(
                            access,
                            StoreFault::default(),
                            None,
                            raw.shared_reads.clone(),
                            &stop_cell,
                        );
                        let mut route = Route { settings: settings_for(false), digest: &digest, info, hash: &mut hash };
                        let r = storage
                            .with_producer(
                                &mut frame_loan,
                                &cancel,
                                &mut spy,
                                &mut raw,
                                info.page_count,
                                None,
                                |producer| {
                                    producer.state.set_language(reader.book_language());
                                    producer.with_buffered(|phase, mut attempt| {
                                        attempt.writer.segmented = true;
                                        attempt.writer.capture_points = true;
                                        attempt.writer.restore_points = restore_points;
                                        attempt.writer.mapping = true;
                                        Ok(attempt.produce_segments_and_map_outcome(
                                            &reader, phase, &mut parser, &mut breaker, &mut route,
                                        ))
                                    })
                                },
                            )
                            .unwrap();
                        env_begins = spy.env_begins;
                        env_finishes = spy.env_finishes;
                        r
                    })
                    .unwrap();
                let has_seg2 = card
                    .0
                    .borrow()
                    .model
                    .files
                    .keys()
                    .any(|k| k.ends_with(&seg2_suffix));
                res = Some((out, cancel.terminal(), has_seg2, env_begins, env_finishes, breaker.calls));
            })
            .unwrap();
        res.unwrap()
    };

    let (o_ord, t_ord, pub_ord, _, _, ord_breaks) = run_with_damaged_tail(false);
    let (o_res, t_res, pub_res, res_begins, res_finishes, res_breaks) = run_with_damaged_tail(true);
    assert!(matches!(o_ord, CurrentOutcome::NoProgress(_)) && t_ord && !pub_ord);
    assert!(matches!(o_res, CurrentOutcome::NoProgress(_)) && t_res && !pub_res);
    assert!(
        res_begins >= 1 && res_finishes >= 1 && res_breaks * 2 < ord_breaks,
        "damaged deflate tail must hit failure after real restore rather than byte-0 fallback (begins={res_begins}, finishes={res_finishes}, breaks={res_breaks} vs ord={ord_breaks})"
    );
    assert_eq!(o_res, o_ord, "damaged deflate tail under restore must match ordinary failure");
    println!("PASS damaged_deflate_tail_matches_ordinary_failure");
}

fn case_missing_map_and_corrupt_prefix_withholding(fixtures: &Path) {
    let epub = std::fs::read(fixtures.join("stored.epub")).unwrap();
    let oracle = run_session(
        &epub,
        &BTreeMap::new(),
        &RunOpts {
            spine: 0,
            capture_points: true,
            restore_points: false,
            entries_per_attempt: 16,
            max_attempts: 2,
            ..RunOpts::default()
        },
    );
    assert_eq!(oracle.outcome, CurrentOutcome::Done);

    // 1. Interrupted fill (entries=1 on seg 0 only) must NEVER publish a partial/zeroed NTSM map!
    let partial = run_session(
        &epub,
        &BTreeMap::new(),
        &RunOpts {
            spine: 0,
            entries_per_attempt: 1,
            max_attempts: 1,
            ..RunOpts::default()
        },
    );
    assert_eq!(partial.outcome, CurrentOutcome::Progress);
    assert!(find_bytes(&partial.files, &oracle.map_name).is_none(), "interrupted fill must not publish NTSM map");

    // 2. Seed v4 prefix segments 0..n-1 with NO map; run restore for the last segment.
    let last = oracle.seg_names.len() - 1;
    let mut prefix = BTreeMap::new();
    for i in 0..last {
        let p = find_path(&oracle.files, &oracle.seg_names[i]).unwrap();
        prefix.insert(p.clone(), oracle.files[&p].clone());
    }
    let filled = run_session(
        &epub,
        &prefix,
        &RunOpts {
            spine: 0,
            start_page: (last as u32) * PAGES,
            entries_per_attempt: 4,
            max_attempts: 1,
            ..RunOpts::default()
        },
    );
    assert_eq!(
        filled.outcome,
        CurrentOutcome::Done,
        "verified prefix-start collection must complete map publication"
    );
    assert_eq!(
        find_bytes(&filled.files, &oracle.map_name),
        find_bytes(&oracle.files, &oracle.map_name),
        "verified prefix-start collection must produce byte-identical NTSM map"
    );

    // 3. Corrupt seg 0's internal FIBP generation inside a VALID envelope CRC so `exists()` passes
    //    while `collect_missing_starts` rejects seg 0. Last segment IS sealed (`sealed == 1`),
    //    so map publication MUST be withheld (`Progress`, NO map file on card, never zeroed slots!).
    let p0 = find_path(&oracle.files, &oracle.seg_names[0]).unwrap();
    let mut bad_payload = unwrap_envelope(&oracle.seg_names[0], &oracle.files[&p0]);
    bad_payload[8] ^= 0xff; // wrong generation inside valid block envelope
    let mut corrupt_prefix = prefix.clone();
    corrupt_prefix.insert(p0, wrap_envelope(&oracle.seg_names[0], &bad_payload));
    let withheld = run_session(
        &epub,
        &corrupt_prefix,
        &RunOpts {
            spine: 0,
            start_page: (last as u32) * PAGES,
            entries_per_attempt: 4,
            max_attempts: 1,
            ..RunOpts::default()
        },
    );
    assert_eq!(
        withheld.outcome,
        CurrentOutcome::Progress,
        "corrupt prefix segment must withhold map publication as Progress"
    );
    assert!(
        find_bytes(&withheld.files, &oracle.map_name).is_none(),
        "corrupt prefix segment must never publish NTSM map with unverified zero slots"
    );

    // 4. Re-running with ALL segments now present (`sealed == 0`) while seg 0 is STILL corrupt
    //    must NEVER fall back to byte-0 counting to publish a map over the corrupt segment!
    let withheld_all = run_session(
        &epub,
        &withheld.files,
        &RunOpts {
            spine: 0,
            start_page: (last as u32) * PAGES,
            entries_per_attempt: 4,
            max_attempts: 1,
            ..RunOpts::default()
        },
    );
    assert_eq!(
        withheld_all.outcome,
        CurrentOutcome::NoProgress(Status::Stale),
        "all-present corrupt on-card segment must fail closed as NoProgress(Stale)"
    );
    assert!(withheld_all.terminal, "all-present corrupt on-card segment must latch terminal");
    assert!(
        find_bytes(&withheld_all.files, &oracle.map_name).is_none(),
        "all-present corrupt on-card segment must NEVER publish NTSM map via byte-0 fallback"
    );

    // 5. Corrupt middle segment (seg 2) on a 5-segment chapter while sealing target seg 4
    //    by restoring from seg 1 (which streams past seg 2's page range with skip=true!).
    //    Skipped pages MUST NOT mark slot 2 certified; collect_missing_starts MUST reject
    //    corrupt on-card seg 2 and withhold the NTSM map!
    let epub5 = std::fs::read(fixtures.join("five_seg.epub")).unwrap();
    let oracle5 = run_session(
        &epub5,
        &BTreeMap::new(),
        &RunOpts {
            spine: 0,
            capture_points: true,
            restore_points: false,
            entries_per_attempt: 16,
            max_attempts: 2,
            ..RunOpts::default()
        },
    );
    assert_eq!(oracle5.outcome, CurrentOutcome::Done);
    assert!(oracle5.seg_names.len() >= 5);
    let last5 = oracle5.seg_names.len() - 1;
    let mut mid_corrupt = BTreeMap::new();
    for i in 0..last5 {
        let p = find_path(&oracle5.files, &oracle5.seg_names[i]).unwrap();
        let bytes = if i == 2 {
            let mut pay = unwrap_envelope(&oracle5.seg_names[i], &oracle5.files[&p]);
            pay[8] ^= 0xff; // wrong FIBP generation inside valid outer envelope
            wrap_envelope(&oracle5.seg_names[i], &pay)
        } else {
            oracle5.files[&p].clone()
        };
        mid_corrupt.insert(p, bytes);
    }
    // Also remove any v4 point from seg 3 so restore for seg 4 restores from seg 1
    // (streaming past seg 2's pages 256..384 with skip=true before opening/writing seg 4).
    for i in 3..last5 {
        let p = find_path(&mid_corrupt, &oracle5.seg_names[i]).unwrap();
        let stripped = rewrite_segment_point(&oracle5.seg_names[i], &mid_corrupt[&p], |pt| {
            pt[0] ^= 0xff; // reject seg i as restore candidate so cand 1 is used
        });
        mid_corrupt.insert(p, stripped);
    }
    let withheld_mid = run_session(
        &epub5,
        &mid_corrupt,
        &RunOpts {
            spine: 0,
            start_page: (last5 as u32) * PAGES,
            entries_per_attempt: 4,
            max_attempts: 1,
            ..RunOpts::default()
        },
    );
    assert_eq!(
        withheld_mid.outcome,
        CurrentOutcome::Progress,
        "skipped corrupt middle segment must still be verified and withhold map as Progress"
    );
    assert!(
        find_bytes(&withheld_mid.files, &oracle5.map_name).is_none(),
        "skipped corrupt middle segment must NEVER be certified by skipped on_page calls"
    );

    // 6. Unsettled style triggers preparatory settlement before segment restore;
    //    settlement's counting sink must NOT certify on-card segments it never read!
    let withheld_unsettled = run_session(
        &epub,
        &corrupt_prefix,
        &RunOpts {
            spine: 0,
            start_page: (last as u32) * PAGES,
            unsettled_style: true,
            entries_per_attempt: 4,
            max_attempts: 1,
            ..RunOpts::default()
        },
    );
    assert_eq!(
        withheld_unsettled.outcome,
        CurrentOutcome::Progress,
        "unsettled style with corrupt prefix must withhold map as Progress"
    );
    assert!(
        find_bytes(&withheld_unsettled.files, &oracle.map_name).is_none(),
        "unsettled style with corrupt prefix must never publish NTSM map"
    );

    // 7. All segments already exist and only the NTSM map is missing (`sealed == 0`):
    //    successful duty firing inside `collect_missing_starts` closes the Pages lease
    //    for <= 3 FDs; map collection must reopen and retry that slot and publish the oracle map!
    let mut all_segs_no_map = BTreeMap::new();
    for seg_name in &oracle.seg_names {
        let p = find_path(&oracle.files, seg_name).unwrap();
        all_segs_no_map.insert(p.clone(), oracle.files[&p].clone());
    }
    let map_only_duty = run_session(
        &epub,
        &all_segs_no_map,
        &RunOpts {
            spine: 0,
            start_page: (last as u32) * PAGES,
            entries_per_attempt: 4,
            max_attempts: 1,
            duty_on_phase: Some(DutyPhase::MapCollect),
            ..RunOpts::default()
        },
    );
    assert_eq!(
        map_only_duty.outcome,
        CurrentOutcome::Done,
        "map-only collection must survive duty suspension and finish Done"
    );
    assert_eq!(map_only_duty.duty_ran, 1, "duty must have fired inside map collection");
    assert!(map_only_duty.most_open <= 3, "peak open files {} exceeded 3", map_only_duty.most_open);
    assert!(!map_only_duty.state_with_pages);
    assert!(!map_only_duty.terminal);
    assert_eq!(
        find_bytes(&map_only_duty.files, &oracle.map_name),
        find_bytes(&oracle.files, &oracle.map_name),
        "map-only collection after duty must publish exact oracle NTSM map"
    );

    // 7b. Exhausting all 8 duty suspensions on map-only (`sealed == 0`) must stay
    //     retryable (`interrupted && !terminal`), keep `ProducerVisit` admittable
    //     (`Admission::Pending`, never `Denied`), and succeed cleanly on the next attempt!
    {
        let mut ledger = [0u8; LEDGER_BYTES];
        let mut visit = ProducerVisit::new(&mut ledger);
        let pos = Some(Position { spine: 0, spines: 3, verified: true });
        assert!(visit.pending(pos, true));
        visit.granted();

        let exhausted = run_session(
            &epub,
            &all_segs_no_map,
            &RunOpts {
                spine: 0,
                start_page: (last as u32) * PAGES,
                entries_per_attempt: 4,
                max_attempts: 1,
                duty_on_phase: Some(DutyPhase::MapCollectAlways),
                ..RunOpts::default()
            },
        );
        assert_eq!(exhausted.duty_ran, 8, "all 8 map-collection retries must fire duty");
        assert_eq!(exhausted.outcome, CurrentOutcome::NoProgress(Status::IoError));
        assert!(
            exhausted.interrupted && !exhausted.terminal,
            "exhausted map-only duty suspensions must stay interrupted, never terminal"
        );
        assert!(exhausted.most_open <= 3 && !exhausted.state_with_pages);
        assert!(find_bytes(&exhausted.files, &oracle.map_name).is_none());

        let quiet = visit.finish_current(
            pos,
            0,
            true,
            exhausted.outcome,
            !exhausted.terminal && exhausted.interrupted,
            || 10_000,
        );
        assert_eq!(quiet, Some(10_000));
        assert_eq!(
            visit.admission(),
            Admission::Pending,
            "exhausted map-only duty suspensions must NOT deny visit admission"
        );
        assert!(visit.work_pending(pos));

        visit.granted();
        let recovered = run_session(
            &epub,
            &exhausted.files,
            &RunOpts {
                spine: 0,
                start_page: (last as u32) * PAGES,
                entries_per_attempt: 4,
                max_attempts: 1,
                ..RunOpts::default()
            },
        );
        assert_eq!(recovered.outcome, CurrentOutcome::Done);
        assert_eq!(
            find_bytes(&recovered.files, &oracle.map_name),
            find_bytes(&oracle.files, &oracle.map_name),
            "subsequent map-only attempt after 8 duty suspensions must publish oracle map"
        );
        let _ = visit.finish_current(pos, 0, true, recovered.outcome, false, || 11_000);
    }

    // 8. Overlapping later v4 index gap (`entries[2].offset == entries[0].offset`) inside
    //    a valid block envelope: `PageCacheReader` rejects page 0 as `Stale`, so
    //    `collect_missing_starts` must use the exact same v4 extent rule and withhold the map!
    {
        let p0 = find_path(&oracle.files, &oracle.seg_names[0]).unwrap();
        let mut pay = unwrap_envelope(&oracle.seg_names[0], &oracle.files[&p0]);
        let ftr_at = pay.len() - 32;
        let index = u32::from_le_bytes(pay[ftr_at..ftr_at + 4].try_into().unwrap()) as usize;
        let off0: [u8; 4] = pay[index..index + 4].try_into().unwrap();
        // Overwrite entry 2's offset with entry 0's offset so gap 2 overlaps gap 0.
        pay[index + 16..index + 20].copy_from_slice(&off0);
        let overlap_env = wrap_envelope(&oracle.seg_names[0], &pay);

        // Confirm PageCacheReader itself rejects page 0 of this segment as Stale.
        {
            struct Raw(Vec<u8>);
            impl RawRead for Raw {
                fn read(&mut self, _: &Query<'_>, off: u32, out: &mut [u8]) -> bool {
                    let at = off as usize;
                    self.0.get(at..at + out.len()).map(|s| out.copy_from_slice(s)).is_some()
                }
            }
            let mut raw = Raw(overlap_env.clone());
            let (mut dir_buf, mut win_buf) = ([0u8; 4160], [0u8; 4096]);
            let mut vs = VerifiedStore::new(Some(&oracle.seg_names[0]), &mut dir_buf, &mut win_buf);
            vs.query.extent = overlap_env.len() as u32;
            vs.query.bound = true;
            assert!(vs.attach(&mut raw));
            let mut access = VerifiedAccess { store: &mut vs, source: &mut raw };
            let mut entries = [Entry::default(); 256];
            let mut anchors = [Anchor::default(); 192];
            let mut rm = ReaderMemory { pages: &mut entries, anchors: &mut anchors };
            let mut name_buf = [0u8; 128];
            let mut names = Arena::new(&mut name_buf);
            let mut reader = PageCacheReader::default();
            assert_eq!(reader.open(&mut access, &oracle.seg_names[0], oracle.generation, &mut names, &mut rm), Status::Ok);
            let mut sb = [0u8; 32 * 1024];
            let mut sa = Arena::new(&mut sb);
            let mut r = [Run::default(); 304];
            let mut im = [Image::default(); 8];
            let mut lk = [Link::default(); 16];
            let mut pm = PageMemory { runs: &mut r, images: &mut im, links: &mut lk };
            assert_eq!(
                reader.read_page(&mut access, &rm, &names, 0, &mut sa, &mut pm).err(),
                Some(Status::Stale),
                "PageCacheReader must reject overlapping v4 page 0 as Stale"
            );
        }

        let mut overlap_prefix = prefix.clone();
        overlap_prefix.insert(p0, overlap_env);
        let withheld_overlap = run_session(
            &epub,
            &overlap_prefix,
            &RunOpts {
                spine: 0,
                start_page: (last as u32) * PAGES,
                entries_per_attempt: 4,
                max_attempts: 1,
                ..RunOpts::default()
            },
        );
        assert_eq!(
            withheld_overlap.outcome,
            CurrentOutcome::Progress,
            "v4 segment with overlapping later gap must withhold map as Progress"
        );
        assert!(
            find_bytes(&withheld_overlap.files, &oracle.map_name).is_none(),
            "v4 segment with overlapping later gap must not be certified for NTSM map"
        );
    }

    // 9. Non-overlapping page-0 extent exceeding `MAX_PAGE_BYTES` (128 KiB) inside a
    //    valid outer block envelope: `PageCacheReader` rejects page 0 as `Stale`, so
    //    `collect_missing_starts` must enforce the same 128 KiB bound and withhold the map!
    {
        let p0 = find_path(&oracle.files, &oracle.seg_names[0]).unwrap();
        let pay = unwrap_envelope(&oracle.seg_names[0], &oracle.files[&p0]);
        let ftr_at = pay.len() - 32;
        let u32_at = |b: &[u8], at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
        let index = u32_at(&pay, ftr_at) as usize;
        let count = u32_at(&pay, ftr_at + 4) as usize;
        let anchor_off = u32_at(&pay, ftr_at + 8) as usize;
        let anchors = u32_at(&pay, ftr_at + 12) as usize;
        let total = u32_at(&pay, ftr_at + 16);
        let pt_off = u32_at(&pay, ftr_at + 20);
        let pt_len = u32_at(&pay, ftr_at + 24);
        assert!(count >= 2);
        let off0 = u32_at(&pay, index) as usize;
        let off1 = u32_at(&pay, index + 8) as usize;
        let pad = (MAX_PAGE_BYTES as usize + 64).saturating_sub(off1 - off0);
        let mut big = Vec::with_capacity(pay.len() + pad);
        big.extend_from_slice(&pay[..off1]);
        big.resize(off1 + pad, 0u8);
        big.extend_from_slice(&pay[off1..index]);
        for i in 0..count {
            let e_at = index + i * 8;
            let off = u32_at(&pay, e_at);
            let chars = u32_at(&pay, e_at + 4);
            let adj = if i == 0 { off } else { off + pad as u32 };
            big.extend_from_slice(&adj.to_le_bytes());
            big.extend_from_slice(&chars.to_le_bytes());
        }
        big.extend_from_slice(&pay[anchor_off..anchor_off + anchors * 8]);
        let mut ftr = [0u8; 32];
        ftr[0..4].copy_from_slice(&(index as u32 + pad as u32).to_le_bytes());
        ftr[4..8].copy_from_slice(&(count as u32).to_le_bytes());
        ftr[8..12].copy_from_slice(&(anchor_off as u32 + pad as u32).to_le_bytes());
        ftr[12..16].copy_from_slice(&(anchors as u32).to_le_bytes());
        ftr[16..20].copy_from_slice(&total.to_le_bytes());
        ftr[20..24].copy_from_slice(&(pt_off + pad as u32).to_le_bytes());
        ftr[24..28].copy_from_slice(&pt_len.to_le_bytes());
        ftr[28..32].copy_from_slice(b"FIBX");
        big.extend_from_slice(&ftr);
        let big_env = wrap_envelope(&oracle.seg_names[0], &big);

        // Confirm PageCacheReader rejects page 0 as Stale.
        {
            struct Raw(Vec<u8>);
            impl RawRead for Raw {
                fn read(&mut self, _: &Query<'_>, off: u32, out: &mut [u8]) -> bool {
                    let at = off as usize;
                    self.0.get(at..at + out.len()).map(|s| out.copy_from_slice(s)).is_some()
                }
            }
            let mut raw = Raw(big_env.clone());
            let (mut dir_buf, mut win_buf) = ([0u8; 4160], [0u8; 4096]);
            let mut vs = VerifiedStore::new(Some(&oracle.seg_names[0]), &mut dir_buf, &mut win_buf);
            vs.query.extent = big_env.len() as u32;
            vs.query.bound = true;
            assert!(vs.attach(&mut raw));
            let mut access = VerifiedAccess { store: &mut vs, source: &mut raw };
            let mut entries = [Entry::default(); 256];
            let mut anchors_arr = [Anchor::default(); 192];
            let mut rm = ReaderMemory { pages: &mut entries, anchors: &mut anchors_arr };
            let mut name_buf = [0u8; 128];
            let mut names = Arena::new(&mut name_buf);
            let mut reader = PageCacheReader::default();
            assert_eq!(
                reader.open(&mut access, &oracle.seg_names[0], oracle.generation, &mut names, &mut rm),
                Status::Ok
            );
            let mut sb = [0u8; 32 * 1024];
            let mut sa = Arena::new(&mut sb);
            let mut r = [Run::default(); 304];
            let mut im = [Image::default(); 8];
            let mut lk = [Link::default(); 16];
            let mut pm = PageMemory { runs: &mut r, images: &mut im, links: &mut lk };
            assert_eq!(
                reader.read_page(&mut access, &rm, &names, 0, &mut sa, &mut pm).err(),
                Some(Status::Stale),
                "PageCacheReader must reject > 128 KiB page 0 extent as Stale"
            );
        }

        let mut big_prefix = prefix.clone();
        big_prefix.insert(p0, big_env);
        let withheld_big = run_session(
            &epub,
            &big_prefix,
            &RunOpts {
                spine: 0,
                start_page: (last as u32) * PAGES,
                entries_per_attempt: 4,
                max_attempts: 1,
                ..RunOpts::default()
            },
        );
        assert_eq!(
            withheld_big.outcome,
            CurrentOutcome::Progress,
            "page-0 extent above MAX_PAGE_BYTES must withhold map as Progress"
        );
        assert!(
            find_bytes(&withheld_big.files, &oracle.map_name).is_none(),
            "page-0 extent above MAX_PAGE_BYTES must not certify NTSM map"
        );
    }

    // 10. Truncated 10-byte page-0 extent declaring 1 run (`nr = 1`) AND v4 trailing
    //     garbage byte in page 0, both inside valid block envelopes: `PageCacheReader`
    //     rejects page 0 as `Stale`, so `collect_missing_starts` must reject both and
    //     withhold the `NTSM` map!
    for (label, pad_tail) in [
        ("truncated 10-byte page 0 declaring 1 run", false),
        ("v4 page 0 with 1 trailing garbage byte", true),
    ] {
        let p0 = find_path(&oracle.files, &oracle.seg_names[0]).unwrap();
        let pay = unwrap_envelope(&oracle.seg_names[0], &oracle.files[&p0]);
        let ftr_at = pay.len() - 32;
        let u32_at = |b: &[u8], at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
        let index = u32_at(&pay, ftr_at) as usize;
        let count = u32_at(&pay, ftr_at + 4) as usize;
        let anchor_off = u32_at(&pay, ftr_at + 8) as usize;
        let anchors = u32_at(&pay, ftr_at + 12) as usize;
        let total = u32_at(&pay, ftr_at + 16);
        let pt_off = u32_at(&pay, ftr_at + 20);
        let pt_len = u32_at(&pay, ftr_at + 24);
        let off0 = u32_at(&pay, index) as usize;
        let chars0 = u32_at(&pay, index + 4);
        let off1 = u32_at(&pay, index + 8) as usize;

        let mut new_page0 = if pad_tail {
            let mut v = pay[off0..off1].to_vec();
            v.push(0xaa); // 1 extra trailing byte past valid v4 page record
            v
        } else {
            let mut v = vec![0u8; 10];
            v[0..4].copy_from_slice(&chars0.to_le_bytes());
            v[4..6].copy_from_slice(&1u16.to_le_bytes()); // nr = 1 with 0 run bytes
            v
        };
        let delta = new_page0.len() as isize - (off1 - off0) as isize;
        let mut rebuilt = Vec::new();
        rebuilt.extend_from_slice(&pay[..off0]);
        rebuilt.append(&mut new_page0);
        rebuilt.extend_from_slice(&pay[off1..index]);
        for i in 0..count {
            let e_at = index + i * 8;
            let off = u32_at(&pay, e_at);
            let chars = u32_at(&pay, e_at + 4);
            let adj = if i == 0 { off } else { (off as isize + delta) as u32 };
            rebuilt.extend_from_slice(&adj.to_le_bytes());
            rebuilt.extend_from_slice(&chars.to_le_bytes());
        }
        rebuilt.extend_from_slice(&pay[anchor_off..anchor_off + anchors * 8]);
        let mut ftr = [0u8; 32];
        ftr[0..4].copy_from_slice(&((index as isize + delta) as u32).to_le_bytes());
        ftr[4..8].copy_from_slice(&(count as u32).to_le_bytes());
        ftr[8..12].copy_from_slice(&((anchor_off as isize + delta) as u32).to_le_bytes());
        ftr[12..16].copy_from_slice(&(anchors as u32).to_le_bytes());
        ftr[16..20].copy_from_slice(&total.to_le_bytes());
        ftr[20..24].copy_from_slice(&((pt_off as isize + delta) as u32).to_le_bytes());
        ftr[24..28].copy_from_slice(&pt_len.to_le_bytes());
        ftr[28..32].copy_from_slice(b"FIBX");
        rebuilt.extend_from_slice(&ftr);
        let bad_env = wrap_envelope(&oracle.seg_names[0], &rebuilt);

        // Confirm PageCacheReader rejects page 0 as Stale.
        {
            struct Raw(Vec<u8>);
            impl RawRead for Raw {
                fn read(&mut self, _: &Query<'_>, off: u32, out: &mut [u8]) -> bool {
                    let at = off as usize;
                    self.0.get(at..at + out.len()).map(|s| out.copy_from_slice(s)).is_some()
                }
            }
            let mut raw = Raw(bad_env.clone());
            let (mut dir_buf, mut win_buf) = ([0u8; 4160], [0u8; 4096]);
            let mut vs = VerifiedStore::new(Some(&oracle.seg_names[0]), &mut dir_buf, &mut win_buf);
            vs.query.extent = bad_env.len() as u32;
            vs.query.bound = true;
            assert!(vs.attach(&mut raw));
            let mut access = VerifiedAccess { store: &mut vs, source: &mut raw };
            let mut entries = [Entry::default(); 256];
            let mut anchors_arr = [Anchor::default(); 192];
            let mut rm = ReaderMemory { pages: &mut entries, anchors: &mut anchors_arr };
            let mut name_buf = [0u8; 128];
            let mut names = Arena::new(&mut name_buf);
            let mut reader = PageCacheReader::default();
            assert_eq!(
                reader.open(&mut access, &oracle.seg_names[0], oracle.generation, &mut names, &mut rm),
                Status::Ok
            );
            let mut sb = [0u8; 32 * 1024];
            let mut sa = Arena::new(&mut sb);
            let mut r = [Run::default(); 304];
            let mut im = [Image::default(); 8];
            let mut lk = [Link::default(); 16];
            let mut pm = PageMemory { runs: &mut r, images: &mut im, links: &mut lk };
            assert_eq!(
                reader.read_page(&mut access, &rm, &names, 0, &mut sa, &mut pm).err(),
                Some(Status::Stale),
                "PageCacheReader must reject {label} as Stale"
            );
        }

        let mut bad_prefix = prefix.clone();
        bad_prefix.insert(p0, bad_env);
        let withheld_rec = run_session(
            &epub,
            &bad_prefix,
            &RunOpts {
                spine: 0,
                start_page: (last as u32) * PAGES,
                entries_per_attempt: 4,
                max_attempts: 1,
                ..RunOpts::default()
            },
        );
        assert_eq!(
            withheld_rec.outcome,
            CurrentOutcome::Progress,
            "{label} must withhold map as Progress"
        );
        assert!(
            find_bytes(&withheld_rec.files, &oracle.map_name).is_none(),
            "{label} must not certify NTSM map"
        );
    }
    println!("PASS missing_map_verified_prefix_starts_and_corrupt_prefix_withholding");
}

fn case_zero_heap_alloc_on_missing_and_backup_envelope_reads(fixtures: &Path) {
    let epub = std::fs::read(fixtures.join("stored.epub")).unwrap();
    let s0 = run_session(
        &epub,
        &BTreeMap::new(),
        &RunOpts { spine: 0, entries_per_attempt: 1, max_attempts: 1, ..RunOpts::default() },
    );
    let p0 = find_path(&s0.files, &s0.seg_names[0]).unwrap();
    let seg0_bytes = s0.files[&p0].clone();
    let seg0_name = s0.seg_names[0];
    let missing_name = s0.seg_names[1];
    let bak_path = format!("{p0}.bak");

    /// Zero-allocation in-memory `Native` store holding one optional canonical
    /// file and one optional `.bak` file so `ALLOC_COUNT` isolates `ChapterCacheAccess` + `FilePort`.
    struct StaticMemFs<'a> {
        canonical_path: &'a str,
        canonical_present: &'a Cell<bool>,
        bak_path: &'a str,
        bak_bytes: &'a [u8],
        open_fd: Option<(i32, usize)>,
        nonce: u32,
    }
    impl StaticMemFs<'_> {
        fn stat_of(&self, len: usize, inode: u64) -> Stat {
            Stat {
                kind: Kind::Regular,
                size: len as i64,
                inode,
                device: 1,
                seconds: 1000,
                nanos: 0,
            }
        }
    }
    impl Native for StaticMemFs<'_> {
        fn open(&mut self, path: &str, _mode: Open) -> Result<i32> {
            if self.open_fd.is_some() {
                return Err(Error::Other);
            }
            if path == self.canonical_path && self.canonical_present.get() {
                self.open_fd = Some((1, 0));
                Ok(1)
            } else if path == self.bak_path {
                self.open_fd = Some((2, 0));
                Ok(2)
            } else {
                Err(Error::Missing)
            }
        }
        fn close(&mut self, fd: i32) -> Result<()> {
            if self.open_fd.is_some_and(|(cur, _)| cur == fd) {
                self.open_fd = None;
                Ok(())
            } else {
                Err(Error::Other)
            }
        }
        fn read(&mut self, fd: i32, out: &mut [u8]) -> Result<usize> {
            let Some((cur, ref mut pos)) = self.open_fd else {
                return Err(Error::Other);
            };
            if cur != fd {
                return Err(Error::Other);
            }
            let data = if fd == 2 { self.bak_bytes } else { &self.bak_bytes[..64] };
            let avail = data.len().saturating_sub(*pos);
            let n = avail.min(out.len());
            out[..n].copy_from_slice(&data[*pos..*pos + n]);
            *pos += n;
            Ok(n)
        }
        fn write(&mut self, _: i32, _: &[u8]) -> Result<usize> {
            Err(Error::Other)
        }
        fn seek(&mut self, fd: i32, offset: u32) -> Result<u64> {
            let Some((cur, ref mut pos)) = self.open_fd else {
                return Err(Error::Other);
            };
            if cur != fd {
                return Err(Error::Other);
            }
            *pos = offset as usize;
            Ok(u64::from(offset))
        }
        fn truncate(&mut self, _: i32, _: u32) -> Result<()> {
            Err(Error::Other)
        }
        fn sync(&mut self, _: i32) -> Result<()> {
            Ok(())
        }
        fn fstat(&mut self, fd: i32) -> Result<Stat> {
            if self.open_fd.is_some_and(|(cur, _)| cur == fd) {
                if fd == 2 {
                    Ok(self.stat_of(self.bak_bytes.len(), 20))
                } else {
                    Ok(self.stat_of(64, 10))
                }
            } else {
                Err(Error::Other)
            }
        }
        fn stat(&mut self, path: &str, _no_follow: bool) -> Result<Stat> {
            if path == self.canonical_path && self.canonical_present.get() {
                Ok(self.stat_of(64, 10))
            } else if path == self.bak_path {
                Ok(self.stat_of(self.bak_bytes.len(), 20))
            } else {
                Err(Error::Missing)
            }
        }
        fn mkdir(&mut self, _: &str) -> Result<()> {
            Err(Error::Other)
        }
        fn unlink(&mut self, _: &str) -> Result<()> {
            Err(Error::Other)
        }
        fn rename(&mut self, _: &str, _: &str) -> Result<()> {
            Err(Error::Other)
        }
        fn next_nonce(&mut self) -> u32 {
            self.nonce = self.nonce.wrapping_add(1);
            self.nonce
        }
    }

    let route_state = RouteState::new(
        "/Newt/Cache",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    );
    let route_loan = route_state.lend(&Never, clock_now);
    let canonical_present = Cell::new(false);
    let mut native = StaticMemFs {
        canonical_path: &p0,
        canonical_present: &canonical_present,
        bak_path: &bak_path,
        bak_bytes: &seg0_bytes,
        open_fd: None,
        nonce: 0,
    };
    let mut cache_state = ChapterCacheState::new();
    let mut cache = ChapterCacheStorage::lend(&mut cache_state, &mut native, &route_loan).unwrap();
    let policy = newt_native_storage::ProducerPolicy::new(&Never);

    cache
        .produce(&policy, |access| {
            // 1. Missing canonical + missing `.bak`: ZERO heap allocations.
            let mut q_miss = Query::new(Some(&missing_name));
            let a0 = ALLOC_COUNT.load(Ordering::Relaxed);
            let opened_miss = access.begin_envelope_read(&missing_name, &mut q_miss);
            let a1 = ALLOC_COUNT.load(Ordering::Relaxed);
            assert!(!opened_miss, "missing segment must fail begin_envelope_read");
            assert_eq!(a1, a0, "missing-canonical envelope open must not allocate heap (delta={})", a1 - a0);

            // 2. Missing canonical + valid `.bak`: open, raw read, and checked finish with ZERO heap allocations.
            let mut q_bak = Query::new(Some(&seg0_name));
            let mut hdr = [0u8; 12];
            let b0 = ALLOC_COUNT.load(Ordering::Relaxed);
            let opened_bak = access.begin_envelope_read(&seg0_name, &mut q_bak);
            let read_ok = opened_bak && access.read_envelope_raw(&q_bak, 0, &mut hdr);
            let finished_bak = read_ok && access.finish_envelope_read(&mut q_bak);
            let b1 = ALLOC_COUNT.load(Ordering::Relaxed);
            assert!(opened_bak && read_ok && finished_bak, "backup-only envelope open/read/finish must succeed");
            assert_eq!(&hdr[..4], b"FIBP");
            assert_eq!(b1, b0, "backup-only envelope read must not allocate heap (delta={})", b1 - b0);

            // 3. Stale lease teardown must invalidate the old query WITHOUT closing a newer read!
            let mut q_old = Query::new(Some(&seg0_name));
            assert!(access.begin_envelope_read(&seg0_name, &mut q_old));
            let mut q_new = Query::new(Some(&seg0_name));
            assert!(access.begin_envelope_read(&seg0_name, &mut q_new));
            assert!(
                !access.finish_envelope_read(&mut q_old),
                "stale query finish must return false"
            );
            assert!(!q_old.active, "stale query must be invalidated on exit");
            let mut hdr2 = [0u8; 12];
            assert!(
                access.read_envelope_raw(&q_new, 0, &mut hdr2) && &hdr2[..4] == b"FIBP",
                "stale query teardown must not disturb newer read lease"
            );
            assert!(
                access.finish_envelope_read(&mut q_new),
                "newer read lease must still finish cleanly after stale query teardown"
            );

            // 4. Backup-only freshness rejects when the canonical file appears before finish!
            let mut q_race = Query::new(Some(&seg0_name));
            assert!(access.begin_envelope_read(&seg0_name, &mut q_race));
            canonical_present.set(true);
            assert!(
                !access.finish_envelope_read(&mut q_race),
                "backup-only freshness must reject when canonical file appears"
            );
        })
        .unwrap();

    // 5. Stale read lease teardown must NOT close or unlink a newer active writer temp!
    {
        let mut tokens = [0u8; 2];
        let (mut board, pending, _) = unsafe {
            newt_c3_board::Board::acquire_x3_optional_sd(
                (&mut tokens[0] as *mut u8).cast(),
                (&mut tokens[1] as *mut u8).cast(),
            )
        }
        .unwrap();
        drop(pending);
        let card = card::Card::default();
        card.seed(&p0, seg0_bytes);
        board
            .with_reader_io(|_, io| {
                let mut port = card.clone().port(io);
                let mut st = ChapterCacheState::new();
                let mut c = ChapterCacheStorage::lend(&mut st, &mut port, &route_loan).unwrap();
                c.produce(&policy, |access| {
                    access.set_producer_allowance(127, 4);
                    let mut q_stale = Query::new(Some(&seg0_name));
                    assert!(access.begin_envelope_read(&seg0_name, &mut q_stale));
                    // Open a writer temp on the same port (replaces read lease).
                    assert!(access.begin_write(&missing_name));
                    // Finishing the stale read query must return false AND leave the writer live!
                    assert!(!access.finish_envelope_read(&mut q_stale));
                    assert!(!q_stale.active);
                    assert!(access.write(b"payload-for-live-writer"));
                    assert!(access.end_write());
                })
                .unwrap();
            })
            .unwrap();
    }
    println!("PASS zero_heap_alloc_on_missing_and_backup_envelope_reads");
}

fn case_upgrade_v3_and_v4_fixtures(fixtures: &Path) {
    let epub = std::fs::read(fixtures.join("stored.epub")).unwrap();
    let v071 = run_session(
        &epub,
        &BTreeMap::new(),
        &RunOpts {
            spine: 0,
            capture_points: false,
            restore_points: false,
            entries_per_attempt: 16,
            max_attempts: 2,
            ..RunOpts::default()
        },
    );
    assert_eq!(v071.outcome, CurrentOutcome::Done);
    let last = v071.seg_names.len() - 1;

    // 1. Upgrade from v0.7.1 v3 prefix (seg 0..last-1 on card, last + map missing):
    let mut v3_seed = BTreeMap::new();
    for i in 0..last {
        let p = find_path(&v071.files, &v071.seg_names[i]).unwrap();
        let (ver, _, _) = verify_segment_readable(&v071.seg_names[i], &v071.files[&p], v071.generation);
        assert_eq!(ver, VERSION_PAGES, "v0.7.1 fixture segment {i} must be v3");
        v3_seed.insert(p.clone(), v071.files[&p].clone());
    }
    let from_v071 = run_session(
        &epub,
        &v3_seed,
        &RunOpts {
            spine: 0,
            start_page: (last as u32) * PAGES,
            capture_points: true,
            restore_points: true,
            entries_per_attempt: 4,
            max_attempts: 1,
            ..RunOpts::default()
        },
    );
    assert_eq!(from_v071.outcome, CurrentOutcome::Done, "v0.7.1 v3 partial cache must complete");
    assert_eq!(
        find_bytes(&from_v071.files, &v071.map_name),
        find_bytes(&v071.files, &v071.map_name),
        "v0.7.1 v3 mixed upgrade must emit exact NTSM map"
    );
    for (i, name) in v071.seg_names.iter().enumerate() {
        let bytes = find_bytes(&from_v071.files, name).unwrap();
        let (ver, cnt, _) = verify_segment_readable(name, bytes, v071.generation);
        assert!(cnt > 0);
        if i < last {
            assert_eq!(ver, VERSION_PAGES);
        }
    }

    // 2. Upgrade from capture-only v4 prefix (capture_points=true, restore_points=false):
    let cap_only = run_session(
        &epub,
        &BTreeMap::new(),
        &RunOpts {
            spine: 0,
            capture_points: true,
            restore_points: false,
            entries_per_attempt: 16,
            max_attempts: 2,
            ..RunOpts::default()
        },
    );
    let mut v4_seed = BTreeMap::new();
    for i in 0..last {
        let p = find_path(&cap_only.files, &cap_only.seg_names[i]).unwrap();
        let (ver, _, _) = verify_segment_readable(&cap_only.seg_names[i], &cap_only.files[&p], cap_only.generation);
        assert_eq!(ver, VERSION_POINT, "capture-only fixture interior segment {i} must be v4");
        v4_seed.insert(p.clone(), cap_only.files[&p].clone());
    }
    let from_v4 = run_session(
        &epub,
        &v4_seed,
        &RunOpts {
            spine: 0,
            start_page: (last as u32) * PAGES,
            capture_points: true,
            restore_points: true,
            entries_per_attempt: 4,
            max_attempts: 1,
            ..RunOpts::default()
        },
    );
    let oracle_last = run_session(
        &epub,
        &v4_seed,
        &RunOpts {
            spine: 0,
            start_page: (last as u32) * PAGES,
            capture_points: true,
            restore_points: false,
            entries_per_attempt: 4,
            max_attempts: 1,
            ..RunOpts::default()
        },
    );
    assert_eq!(from_v4.outcome, CurrentOutcome::Done, "v4 capture-only cache must restore cleanly");
    assert!(from_v4.env_begins >= 1, "restore must read the pre-captured v4 point");
    assert!(
        from_v4.break_calls * 2 < oracle_last.break_calls
            && from_v4.book_read_bytes * 2 < oracle_last.book_read_bytes,
        "v4 upgrade must actually restore instead of byte-0 fallback (breaks {} vs {}, bytes {} vs {})",
        from_v4.break_calls,
        oracle_last.break_calls,
        from_v4.book_read_bytes,
        oracle_last.book_read_bytes
    );
    assert_eq!(
        find_bytes(&from_v4.files, &cap_only.seg_names[last]),
        find_bytes(&oracle_last.files, &cap_only.seg_names[last]),
        "segment restored from pre-captured v4 point must be byte-identical to oracle"
    );
    assert_eq!(
        find_bytes(&from_v4.files, &cap_only.map_name),
        find_bytes(&oracle_last.files, &cap_only.map_name)
    );
    println!("PASS upgrade_v3_and_v4_fixtures");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert!(args.len() >= 2, "usage: resume_restore <fixtures-dir> [--case <name>]");
    let fixtures = Path::new(&args[1]);
    let only = if args.len() >= 4 && args[2] == "--case" {
        Some(args[3].as_str())
    } else {
        None
    };

    let cases: [(&str, fn(&Path)); 7] = [
        ("restore_equals_byte0_oracle", case_restore_equals_byte0_oracle),
        ("d3_target_first_and_decreasing_candidates", case_d3_target_first_and_decreasing_candidates),
        ("peak_open_files_with_duty", case_peak_open_files_with_duty),
        ("pass_b_tamper_and_replacement", case_pass_b_tamper_and_replacement),
        ("adversarial_valid_crc_points", case_adversarial_valid_crc_points),
        ("real_cancel_duty_terminal_and_stale_receipt", case_real_cancel_duty_terminal_and_stale_receipt),
        ("damaged_deflate_tail", case_damaged_deflate_tail),
    ];
    let extra: [(&str, fn(&Path)); 3] = [
        ("missing_map_and_corrupt_prefix_withholding", case_missing_map_and_corrupt_prefix_withholding),
        ("upgrade_v3_and_v4_fixtures", case_upgrade_v3_and_v4_fixtures),
        ("zero_heap_alloc_on_missing_and_backup_envelope_reads", case_zero_heap_alloc_on_missing_and_backup_envelope_reads),
    ];

    let mut ran = 0usize;
    for (name, f) in cases.iter().chain(extra.iter()) {
        if only.is_none_or(|want| want == *name) {
            f(fixtures);
            ran += 1;
        }
    }
    assert!(ran > 0, "unknown --case {:?}", only);
    if only.is_none() {
        println!("PASS resume restore ({ran} cases)");
    }
}
