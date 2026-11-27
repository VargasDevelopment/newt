// SPDX-License-Identifier: MIT
//! End-to-end host proof of resume-point capture over real Expat,
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
use newt_core::{
    cache_names::segment::{ChapterSegment, PAGES, map_name, segment_name},
    chapter_layout::{
        engine::EngineState,
        session::{ChapterSession, LayoutOperation, OperationHandler},
        types::{Image, Link, Run, Sink, SplitBackend},
    },
    css::CssStylesheetBuilder,
    member_check::Checked,
    page_cache::{
        Anchor, CacheStorage, Entry, PageCacheReader, PageCacheWriter, PageMemory, PointRange,
        Profile, ReaderMemory, VERSION_PAGES, VERSION_POINT, WriterMemory, IndexChunk,
    },
    page_cache_access::VerifiedAccess,
    page_envelope::{self, Query, RawRead, VerifiedStore},
    preferences::{Orientation, ViewSettings},
    producer_policy::{Cancellation, Hooks, ProducerReceipt, Stop},
    producer_visit::CurrentOutcome,
    resume_point::{
        BINDING_BYTES, DEPTH_MAX, Decline, HEADER_BYTES, Layout, Mode, NAMES_MAX, PROLOG_MAX,
        Section, SOURCE_BYTES, TRAILER_BYTES,
    },
    xml_input::EntityFilter,
    xml_session::{self, BorrowedPolicy, ParserBackend, ProducerBackend, XmlStream},
    zip_container::Status,
    zip_window::{arena::Arena, source::BookSource},
};
use newt_native_storage::{
    Authority, RouteState,
    chapter_cache::{ChapterCacheState, ChapterCacheStorage},
};
use newt_phased_reader::{
    PageInfo, PageLayout, PhasedReader, Selection,
    hash_provider::CacheHash,
    imported_revision::Digest,
    phase_storage::{BANK_BYTES, BuildLoan, Storage},
    producer::Route,
    resume_capture::{
        BoundaryInput, CaptureOutcome, CaptureRoute, CaptureState, CaptureWriter, LIVE_BITS_BYTES,
        maybe_capture,
    },
};
use newt_phased_sink::{FRAME_BYTES, Plan, RUNS, TEXT, Target};
use std::collections::BTreeMap;
use std::path::Path;

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
        // identity_at_generation hashes exactly 99 bytes to form the chapter base ID.
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

struct BytesSource(Vec<u8>);
impl BookSource for BytesSource {
    fn size(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let Ok(at) = usize::try_from(at) else {
            return -1;
        };
        let Some(bytes) = self.0.get(at..) else {
            return -1;
        };
        let count = bytes.len().min(out.len());
        out[..count].copy_from_slice(&bytes[..count]);
        count as i32
    }
}
impl ProducerReceipt for BytesSource {
    fn producer_receipt_live(&mut self) -> bool {
        true
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
    fn with_parse<R>(&mut self, operation: impl for<'a> FnOnce(&'a mut [u8]) -> R) -> core::result::Result<R, Status> {
        let res = operation(&mut self.0);
        self.0.fill(0xff);
        Ok(res)
    }
}

fn stop_none(_: &()) -> Stop {
    Stop::None
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

struct CardRaw(Vec<u8>);
impl RawRead for CardRaw {
    fn read(&mut self, _: &Query<'_>, offset: u32, out: &mut [u8]) -> bool {
        let at = offset as usize;
        self.0
            .get(at..at + out.len())
            .map(|s| out.copy_from_slice(s))
            .is_some()
    }
}

#[derive(Debug, PartialEq, Eq)]
struct DecodedPage {
    char_start: u32,
    runs: Vec<(i16, i16, u16, u8, Vec<u8>)>,
    images: Vec<(i16, i16, u16, u16, Vec<u8>)>,
    links: Vec<(i16, i16, u16, u16, Vec<u8>, Vec<u8>)>,
}

#[derive(Debug)]
struct InspectedSegment {
    version: u16,
    page_count: u32,
    total_chars: u32,
    anchors: Vec<(u32, u32)>,
    pages: Vec<DecodedPage>,
    point_range: Option<PointRange>,
    point_bytes: Option<Vec<u8>>,
}

fn inspect_segment_file(name: &[u8; 69], file_bytes: &[u8], generation: u32) -> InspectedSegment {
    let mut raw = CardRaw(file_bytes.to_vec());
    let (mut dir_buf, mut win_buf) = ([0u8; 4160], [0u8; 4096]);
    let mut vs = VerifiedStore::new(Some(name), &mut dir_buf, &mut win_buf);
    vs.query.extent = file_bytes.len() as u32;
    vs.query.bound = true;
    assert!(vs.attach(&mut raw), "verified envelope must attach cleanly");
    let mut access = VerifiedAccess { store: &mut vs, source: &mut raw };
    let mut hdr = [0u8; 12];
    assert_eq!(access.read_at(name, 0, &mut hdr), 12);
    assert_eq!(&hdr[..4], b"FIBP");
    let version = u16::from_le_bytes([hdr[4], hdr[5]]);
    let mut entries = [Entry::default(); 256];
    let mut anchors = [Anchor::default(); 192];
    let mut rm = ReaderMemory { pages: &mut entries, anchors: &mut anchors };
    let mut name_buf = [0u8; 128];
    let mut names = Arena::new(&mut name_buf);
    let mut reader = PageCacheReader::default();
    assert_eq!(
        reader.open(&mut access, name, generation, &mut names, &mut rm),
        Status::Ok,
        "PageCacheReader::open must succeed on verified segment"
    );
    let page_count = reader.page_count();
    let total_chars = reader.total_chars();
    let mut pages = Vec::with_capacity(page_count as usize);
    for idx in 0..page_count {
        let mut scratch_buf = [0u8; 32 * 1024];
        let mut scratch = Arena::new(&mut scratch_buf);
        let mut runs_buf = [Run::default(); 304];
        let mut imgs_buf = [Image::default(); 8];
        let mut links_buf = [Link::default(); 16];
        let mut pm = PageMemory {
            runs: &mut runs_buf,
            images: &mut imgs_buf,
            links: &mut links_buf,
        };
        let page = reader
            .read_page(&mut access, &rm, &names, idx, &mut scratch, &mut pm)
            .unwrap_or_else(|e| panic!("verified page {idx} must decode cleanly: {e:?}"));
        let runs = page
            .runs
            .iter()
            .map(|r| (r.x, r.baseline, r.size, r.flags, r.text.get(page.bytes).to_vec()))
            .collect();
        let images = page
            .images
            .iter()
            .map(|i| (i.x, i.y, i.width, i.height, i.href.get(page.bytes).to_vec()))
            .collect();
        let links = page
            .links
            .iter()
            .map(|l| {
                (
                    l.x,
                    l.y,
                    l.width,
                    l.height,
                    l.target.get(page.bytes).to_vec(),
                    l.fragment.get(page.bytes).to_vec(),
                )
            })
            .collect();
        pages.push(DecodedPage { char_start: page.char_start, runs, images, links });
    }
    let point_range = reader.point();
    let point_bytes = point_range.map(|pr| {
        let mut buf = vec![0u8; pr.length as usize];
        let mut got = 0;
        while got < buf.len() {
            let n = access.read_at(name, pr.offset + got as u32, &mut buf[got..]);
            assert!(n > 0, "verified point read failed at {got}");
            got += n as usize;
        }
        buf
    });
    let anchor_vec = rm
        .anchors
        .iter()
        .take_while(|a| a.hash != 0 || a.chars != 0)
        .map(|a| (a.hash, a.chars))
        .collect();
    InspectedSegment {
        version,
        page_count,
        total_chars,
        anchors: anchor_vec,
        pages,
        point_range,
        point_bytes,
    }
}

struct ParsedPoint {
    layout: Layout,
    segment_index: u32,
    pages_at: u32,
    cut: u64,
    fed: u64,
    source_consumed: u32,
    prolog: Vec<u8>,
    names: Vec<Vec<u8>>,
    replay: Vec<u8>,
    engine: Vec<u8>,
}

fn verify_and_parse_point(
    raw: &[u8],
    expected_mode: Mode,
    expected_digest: &Digest,
    expected_spine: u32,
    expected_gen: u32,
    expected_edge: u32,
    expected_pages: u32,
    expected_chars: u32,
    expected_source_bytes: u64,
) -> ParsedPoint {
    assert!(raw.len() >= HEADER_BYTES + TRAILER_BYTES as usize);
    let hdr: &[u8; HEADER_BYTES] = raw[..HEADER_BYTES].try_into().unwrap();
    let layout = Layout::parse(hdr, raw.len() as u32).expect("FIRP header must parse");
    assert_eq!(layout.mode, expected_mode, "FIRP mode must match member method");
    let trailer = layout.trailer() as usize;
    assert_eq!(trailer + TRAILER_BYTES as usize, raw.len());
    let stored_crc = u32::from_le_bytes(raw[trailer..trailer + 4].try_into().unwrap());
    let calc_crc = page_envelope::crc(u32::MAX, &raw[..trailer]) ^ u32::MAX;
    assert_eq!(stored_crc, calc_crc, "FIRP CRC-32 must match header + all sections");

    let section_slice = |s: Section| -> &[u8] {
        let (off, len) = layout.range(s);
        &raw[off as usize..(off + len) as usize]
    };

    // Section 0: Binding (108 B fixed BND1)
    let bnd = section_slice(Section::Binding);
    assert_eq!(bnd.len(), BINDING_BYTES);
    assert_eq!(&bnd[0..4], b"BND1");
    assert_eq!(&bnd[4..36], expected_digest);
    let u32_at = |at: usize| u32::from_le_bytes(bnd[at..at + 4].try_into().unwrap());
    let u64_at = |at: usize| {
        u64::from(u32_at(at)) | (u64::from(u32_at(at + 4)) << 32)
    };
    let spine = u32_at(36);
    let generation = u32_at(40);
    let segment_index = u32_at(44);
    let page_count = u32_at(48);
    let total_chars = u32_at(52);
    let pages_at = u32_at(72);
    let source_bytes = u64_at(76);
    let cut = u64_at(84);
    let fed = u64_at(92);
    assert_eq!(spine, expected_spine);
    assert_eq!(generation, expected_gen);
    assert_eq!(segment_index, expected_edge / PAGES);
    assert_eq!(page_count, expected_pages);
    assert_eq!(total_chars, expected_chars);
    assert_eq!(source_bytes, expected_source_bytes);
    assert!(
        pages_at < expected_edge && expected_edge - pages_at <= 4,
        "pages_at={pages_at} must lie in [{}, {})",
        expected_edge - 4,
        expected_edge
    );
    assert!(cut <= fed, "cut={cut} must not exceed fed={fed}");

    // Section 1: Source (8 B SRC1)
    let src = section_slice(Section::Source);
    assert_eq!(src.len(), SOURCE_BYTES);
    assert_eq!(&src[0..4], b"SRC1");
    let source_consumed = u32::from_le_bytes(src[4..8].try_into().unwrap());

    // Section 2: Stream (XST1 + FLT1)
    let xst = section_slice(Section::Stream);
    assert!(xst.len() >= 17 && &xst[0..4] == b"XST1");
    let xst_fed = u64::from_le_bytes(xst[4..12].try_into().unwrap());
    let xst_produced = u32::from_le_bytes(xst[12..16].try_into().unwrap());
    assert_eq!(xst_fed, fed);
    assert_eq!(xst_produced, source_consumed);
    if xst[16] == 1 {
        assert!(xst.len() >= 27 && &xst[17..21] == b"FLT1");
    }

    // Section 3: Engine (ENG1)
    let eng = section_slice(Section::Engine);
    assert!(eng.len() >= 81 && &eng[0..4] == b"ENG1");
    let eng_pages = u32::from_le_bytes(eng[4..8].try_into().unwrap());
    assert_eq!(eng_pages, pages_at);

    // Section 4: Prolog
    let prolog = section_slice(Section::Prolog).to_vec();

    // Section 5: Names (u8 depth, then (u8 len, bytes) * depth)
    let names_raw = section_slice(Section::Names);
    assert!(!names_raw.is_empty());
    let depth = usize::from(names_raw[0]);
    assert!(depth > 0);
    let mut at = 1usize;
    let mut names = Vec::with_capacity(depth);
    for _ in 0..depth {
        let n = usize::from(names_raw[at]);
        at += 1;
        names.push(names_raw[at..at + n].to_vec());
        at += n;
    }
    assert_eq!(at, names_raw.len(), "Names section must decode exactly");

    // Section 6: Replay
    let replay = section_slice(Section::Replay).to_vec();
    assert_eq!(fed - cut, replay.len() as u64, "fed - cut must equal replay length");

    ParsedPoint {
        layout,
        segment_index,
        pages_at,
        cut,
        fed,
        source_consumed,
        prolog,
        names,
        replay,
        engine: eng.to_vec(),
    }
}

struct RunResult {
    outcome: CurrentOutcome,
    info: PageInfo,
    generation: u32,
    digest: Digest,
    files: BTreeMap<String, Vec<u8>>,
    seg_names: Vec<[u8; 69]>,
    map_file: Option<[u8; 69]>,
    fault_firp_range: Option<(u32, u32)>,
    fault_offset: Option<u32>,
    fault_len: Option<u32>,
}

struct StoreOverride<'a, S: CacheStorage> {
    inner: &'a mut S,
    /// Enforced credit for segment 0 (reported AND strictly debited on write/end_write).
    enforced_seg0_credit: Option<u64>,
    active_credit: Option<u64>,
    /// 0-based `begin_write` index on which to inject a strictly mid-point write fault.
    fault_target_seg: Option<usize>,
    begin_count: usize,
    seg_payload: u32,
    firp_start: Option<u32>,
    firp_end: Option<u32>,
    target_buf: Vec<u8>,
    fault_firp_range: Option<(u32, u32)>,
    fault_offset: Option<u32>,
    fault_len: Option<u32>,
}
impl<'a, S: CacheStorage> StoreOverride<'a, S> {
    fn new(inner: &'a mut S, enforced_seg0_credit: Option<u64>, fault_target_seg: Option<usize>) -> Self {
        Self {
            inner,
            enforced_seg0_credit,
            active_credit: None,
            fault_target_seg,
            begin_count: 0,
            seg_payload: 0,
            firp_start: None,
            firp_end: None,
            target_buf: Vec::new(),
            fault_firp_range: None,
            fault_offset: None,
            fault_len: None,
        }
    }
    fn scan_firp(&mut self, bytes: &[u8]) {
        self.target_buf.extend_from_slice(bytes);
        if self.firp_start.is_none() {
            if let Some(pos) = self.target_buf.windows(4).position(|w| w == b"FIRP") {
                self.firp_start = Some(pos as u32);
            }
        }
        if self.firp_end.is_none() {
            if let Some(start) = self.firp_start {
                let s = start as usize;
                if self.target_buf.len() >= s + HEADER_BYTES {
                    let hdr: &[u8; HEADER_BYTES] =
                        self.target_buf[s..s + HEADER_BYTES].try_into().unwrap();
                    let mut total = HEADER_BYTES as u32 + TRAILER_BYTES;
                    for i in 0..7 {
                        let at = 8 + 4 * i;
                        let len = u32::from_le_bytes(hdr[at..at + 4].try_into().unwrap());
                        total = total.saturating_add(len);
                    }
                    if Layout::parse(hdr, total).is_some() {
                        self.firp_end = Some(start + total);
                        self.fault_firp_range = Some((start, start + total));
                    }
                }
            }
        }
    }
}
impl<S: CacheStorage> CacheStorage for StoreOverride<'_, S> {
    fn remove(&mut self, name: &[u8]) -> bool {
        self.active_credit = None;
        self.inner.remove(name)
    }
    fn file_size(&mut self, name: &[u8]) -> i64 {
        self.inner.file_size(name)
    }
    fn read_at(&mut self, name: &[u8], offset: u32, out: &mut [u8]) -> i32 {
        self.inner.read_at(name, offset, out)
    }
    fn begin_write(&mut self, name: &[u8]) -> bool {
        let seg_idx = self.begin_count;
        self.begin_count += 1;
        self.seg_payload = 0;
        self.firp_start = None;
        self.firp_end = None;
        self.target_buf.clear();
        self.active_credit = if seg_idx == 0 { self.enforced_seg0_credit } else { None };
        self.inner.begin_write(name)
    }
    fn write(&mut self, bytes: &[u8]) -> bool {
        let cur_off = self.seg_payload;
        let targeting = self.fault_target_seg.is_some_and(|t| self.begin_count == t + 1);
        if targeting {
            if let (Some(start), Some(end)) = (self.firp_start, self.firp_end) {
                let flush_end = cur_off.saturating_add(bytes.len() as u32);
                if cur_off > start && flush_end <= end {
                    self.fault_offset = Some(cur_off);
                    self.fault_len = Some(bytes.len() as u32);
                    return false;
                }
            }
            self.scan_firp(bytes);
        }
        if let Some(rem) = self.active_credit.as_mut() {
            let need = bytes.len() as u64;
            if *rem < need {
                return false;
            }
            *rem -= need;
        }
        if !self.inner.write(bytes) {
            return false;
        }
        self.seg_payload = self.seg_payload.saturating_add(bytes.len() as u32);
        true
    }
    fn end_write(&mut self) -> bool {
        if let Some(rem) = self.active_credit.as_mut() {
            let overhead = u64::from(
                4 * page_envelope::blocks(self.seg_payload) + page_envelope::MANIFEST_BYTES,
            );
            if *rem < overhead {
                return false;
            }
            *rem -= overhead;
        }
        self.active_credit = None;
        self.inner.end_write()
    }
    fn read_back_at(&mut self, offset: u32, out: &mut [u8]) -> i32 {
        self.inner.read_back_at(offset, out)
    }
    fn producer_can_begin(&self) -> bool {
        self.inner.producer_can_begin()
    }
    fn producer_remaining_credit(&self) -> Option<u64> {
        self.active_credit.or_else(|| self.inner.producer_remaining_credit())
    }
    fn last_write_published(&self) -> bool {
        self.inner.last_write_published()
    }
}

#[allow(clippy::too_many_arguments)]
fn run_producer_chapter(
    epub: &[u8],
    spine: u32,
    start_page: u32,
    landscape: bool,
    capture_points: bool,
    bank_fill: u8,
    enforced_seg0_credit: Option<u64>,
    fault_target_seg: Option<usize>,
) -> RunResult {
    run_producer_chapter_seeded(
        epub,
        spine,
        start_page,
        landscape,
        capture_points,
        bank_fill,
        enforced_seg0_credit,
        fault_target_seg,
        &BTreeMap::new(),
    )
}

#[allow(clippy::too_many_arguments)]
fn run_producer_chapter_seeded(
    epub: &[u8],
    spine: u32,
    start_page: u32,
    landscape: bool,
    capture_points: bool,
    bank_fill: u8,
    enforced_seg0_credit: Option<u64>,
    fault_target_seg: Option<usize>,
    seed_files: &BTreeMap<String, Vec<u8>>,
) -> RunResult {
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
    if !seed_files.is_empty() {
        let mut st = card.0.borrow_mut();
        for dir in ["/Newt", "/Newt/Cache", "/Newt/Cache/Pages"] {
            st.model.dirs.insert(dir.to_string());
        }
        for (path, bytes) in seed_files {
            st.model.files.insert(
                path.clone(),
                model::File { durable: bytes.clone(), readable: bytes.clone() },
            );
        }
    }
    let mut out = None;
    board
        .with_reader_io(|panel, io| {
            let mut raw = BytesSource(epub.to_vec());
            let mut bank = [bank_fill; BANK_BYTES];
            let mut storage = Storage::new(&mut bank);
            let mut parser = newt_target_expat::Expat::default();
            let mut breaker = linebreak::NativeBreak;
            let mut selections = [Selection::default(), Selection::default()];
            let mut runs = [newt_phased_sink::Run::default(); RUNS];
            let mut text = [0u8; TEXT];
            let mut plan = Plan::new(&mut runs, &mut text);
            let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), layout_for(landscape));
            let mut frame = [0u8; FRAME_BYTES];
            assert_eq!(reader.open_phased(&mut raw, &mut storage, panel, &mut parser), Status::Ok);
            assert_eq!(
                reader.capture_phased(
                    &mut raw, spine, start_page, Target::Page, &mut storage, panel, &mut parser,
                    &mut breaker,
                ),
                Status::Ok
            );
            assert_eq!(reader.publish(&mut raw, &mut frame), Status::Ok);
            let info = reader.info();
            let settings = settings_for(landscape);

            let route_state = RouteState::new(
                "/Newt/Cache",
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            );
            let route_loan = route_state.lend(&Never, clock_now);
            let mut native = card.port(io);
            let mut cache_state = ChapterCacheState::new();
            let mut cache = ChapterCacheStorage::lend(&mut cache_state, &mut native, &route_loan).unwrap();

            let cancel = Cancellation::new(Hooks {
                context: &(),
                stop: Some(stop_none),
                yield_now: None,
                pause: None,
            });
            let mut frame_loan = FrameLoan::default();
            let mut hash = Hasher::default();
            let mut generation = 0u32;

            let policy = newt_native_storage::ProducerPolicy::new(&Never);
            // Two attempts when a 4+ segment chapter plus map exceeds the 4-entry grant.
            let mut outcome = CurrentOutcome::NoProgress(Status::Unsupported);
            let mut fault_firp_range = None;
            let mut fault_offset = None;
            let mut fault_len = None;
            for _ in 0..2 {
                let step = cache
                    .produce(&policy, |access| {
                        access.set_producer_allowance(127, 16);
                        let mut wrapped =
                            StoreOverride::new(access, enforced_seg0_credit, fault_target_seg);
                        let mut route = Route { settings, digest: &digest, info, hash: &mut hash };
                        let r = if capture_points {
                            storage
                                .with_producer(
                                    &mut frame_loan,
                                    &cancel,
                                    &mut wrapped,
                                    &mut raw,
                                    info.page_count,
                                    None,
                                    |producer| {
                                        let r = producer.produce_current(
                                            &mut reader, &mut parser, &mut breaker, &mut route,
                                        );
                                        generation = producer.state.generation;
                                        Ok(r)
                                    },
                                )
                                .unwrap()
                        } else {
                            // Independent v3 reference pass without capture_points (same route & styles).
                            storage
                                .with_producer(
                                    &mut frame_loan,
                                    &cancel,
                                    &mut wrapped,
                                    &mut raw,
                                    info.page_count,
                                    None,
                                    |producer| {
                                        producer.state.set_language(reader.book_language());
                                        let r = producer.with_buffered(|phase, mut attempt| {
                                            if attempt.state.count != 0 {
                                                attempt.writer.segmented = true;
                                                attempt.writer.capture_points = false;
                                                attempt.writer.mapping = true;
                                                Ok(attempt.produce_segments_and_map_outcome(
                                                    &reader, phase, &mut parser, &mut breaker, &mut route,
                                                ))
                                            } else {
                                                Ok(CurrentOutcome::NoProgress(Status::Unsupported))
                                            }
                                        })?;
                                        generation = producer.state.generation;
                                        Ok(r)
                                    },
                                )
                                .unwrap()
                        };
                        if wrapped.fault_firp_range.is_some() {
                            fault_firp_range = wrapped.fault_firp_range;
                        }
                        if wrapped.fault_offset.is_some() {
                            fault_offset = wrapped.fault_offset;
                            fault_len = wrapped.fault_len;
                        }
                        r
                    })
                    .unwrap();
                outcome = step;
                if outcome != CurrentOutcome::Progress {
                    break;
                }
            }

            let base_id = hash.base_id.expect("producer must hash chapter base identity");

            let seg_count = ChapterSegment::count(info.page_count);
            let mut seg_names = Vec::new();
            let mut map_file = None;
            if seg_count > 0 {
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
                map_file = Some(mname[..69].try_into().unwrap());
            } else {
                seg_names.push(base_id);
            }
            let files: BTreeMap<String, Vec<u8>> = card
                .0
                .borrow()
                .model
                .files
                .iter()
                .filter(|(k, _)| k.contains("/Pages/"))
                .map(|(k, v)| (k.clone(), v.durable.clone()))
                .collect();
            out = Some(RunResult {
                outcome,
                info,
                generation,
                digest,
                files,
                seg_names,
                map_file,
                fault_firp_range,
                fault_offset,
                fault_len,
            });
        })
        .unwrap();
    out.unwrap()
}

fn find_file<'a>(files: &'a BTreeMap<String, Vec<u8>>, name: &[u8; 69]) -> Option<&'a Vec<u8>> {
    let suffix = core::str::from_utf8(name).unwrap();
    files.iter().find(|(k, _)| k.ends_with(suffix)).map(|(_, v)| v)
}

fn case_portrait_and_landscape_stored_and_deflated(dir: &Path) {
    let stored = std::fs::read(dir.join("stored.epub")).unwrap();
    let deflated = std::fs::read(dir.join("deflated.epub")).unwrap();
    for (label, epub, mode) in [
        ("stored", stored.as_slice(), Mode::Seek),
        ("deflated", deflated.as_slice(), Mode::Reinflate),
    ] {
        for landscape in [false, true] {
            let cap = run_producer_chapter(epub, 0, 0, landscape, true, 0, None, None);
            assert_eq!(cap.outcome, CurrentOutcome::Done, "{label} landscape={landscape} cap outcome");
            let ref_run = run_producer_chapter(epub, 0, 0, landscape, false, 0, None, None);
            assert_eq!(ref_run.outcome, CurrentOutcome::Done, "{label} landscape={landscape} ref outcome");
            assert_eq!(cap.info.page_count, ref_run.info.page_count);
            assert_eq!(cap.info.total_chars, ref_run.info.total_chars);
            assert!(
                cap.seg_names.len() >= 4,
                "{label} landscape={landscape}: expected >=4 segments, got {}",
                cap.seg_names.len()
            );
            // Exact same map file bytes.
            let cap_map = find_file(&cap.files, &cap.map_file.unwrap()).expect("cap map");
            let ref_map = find_file(&ref_run.files, &ref_run.map_file.unwrap()).expect("ref map");
            assert_eq!(cap_map, ref_map, "{label} landscape={landscape}: map bytes must match");

            let last_idx = cap.seg_names.len() - 1;
            let mut captured_edges = Vec::new();
            let mut expected_edges = Vec::new();
            for (idx, name) in cap.seg_names.iter().enumerate() {
                let cap_bytes = find_file(&cap.files, name).expect("cap segment file");
                let ref_bytes = find_file(&ref_run.files, name).expect("ref segment file");
                let cap_seg = inspect_segment_file(name, cap_bytes, cap.generation);
                let ref_seg = inspect_segment_file(name, ref_bytes, ref_run.generation);
                assert_eq!(ref_seg.version, VERSION_PAGES);
                assert_eq!(cap_seg.page_count, ref_seg.page_count);
                assert_eq!(cap_seg.total_chars, ref_seg.total_chars);
                assert_eq!(cap_seg.anchors, ref_seg.anchors, "{label} seg {idx} anchors");
                assert_eq!(cap_seg.pages, ref_seg.pages, "{label} seg {idx} decoded pages");
                if idx < last_idx {
                    let edge = (idx as u32 + 1) * PAGES;
                    expected_edges.push(edge);
                    assert_eq!(cap_seg.version, VERSION_POINT, "{label} interior seg {idx} must be v4");
                    let point_raw = cap_seg
                        .point_bytes
                        .as_ref()
                        .unwrap_or_else(|| panic!("{label} landscape={landscape} seg {idx} missing point"));
                    verify_and_parse_point(
                        point_raw,
                        mode,
                        &cap.digest,
                        0,
                        cap.generation,
                        edge,
                        cap.info.page_count,
                        cap.info.total_chars,
                        epub.len() as u64,
                    );
                    captured_edges.push(edge);
                } else {
                    assert_eq!(cap_seg.version, VERSION_PAGES, "{label} final seg {idx} must stay v3");
                    assert!(cap_seg.point_range.is_none());
                }
            }
            assert!(!captured_edges.is_empty(), "captured edge set must be non-empty");
            assert_eq!(captured_edges, expected_edges, "exact captured edge set must match");
        }
        // Short single-segment chapter (spine 1) must stay v3 with no point.
        let short = run_producer_chapter(epub, 1, 0, false, true, 0, None, None);
        assert_eq!(short.outcome, CurrentOutcome::Done);
        assert_eq!(short.seg_names.len(), 1);
        let short_bytes = find_file(&short.files, &short.seg_names[0]).expect("short file");
        let short_seg = inspect_segment_file(&short.seg_names[0], short_bytes, short.generation);
        assert_eq!(short_seg.version, VERSION_PAGES, "{label} single-segment chapter must stay v3");
        assert!(short_seg.point_range.is_none());
    }
}

fn case_target_first_start_gt_zero(dir: &Path) {
    let epub = std::fs::read(dir.join("stored.epub")).unwrap();
    // 4-segment chapter + 1 map = 5 files, while PAGE_ENTRY_GRANT caps one attempt at 4 files.
    // Pass 0 seals segs 1, 2, 3 and pass 1 seals seg 0 (4 entries -> Progress), leaving all
    // interior segments {0, 1, 2} with valid points for edges {128, 256, 384}.
    let cap = run_producer_chapter(&epub, 0, 200, false, true, 0, None, None);
    assert!(
        matches!(cap.outcome, CurrentOutcome::Done | CurrentOutcome::Progress),
        "wrapped target-first attempt must succeed or make Progress, got {:?}",
        cap.outcome
    );
    let last_idx = cap.seg_names.len() - 1;
    assert!(last_idx >= 3);
    for (idx, name) in cap.seg_names.iter().enumerate() {
        let bytes = find_file(&cap.files, name).expect("segment present after wrapped pass");
        let seg = inspect_segment_file(name, bytes, cap.generation);
        if idx < last_idx {
            let edge = (idx as u32 + 1) * PAGES;
            assert_eq!(seg.version, VERSION_POINT, "wrapped interior seg {idx} must be v4");
            let pt = seg.point_bytes.as_ref().expect("wrapped interior seg must have point");
            verify_and_parse_point(
                pt, Mode::Seek, &cap.digest, 0, cap.generation, edge,
                cap.info.page_count, cap.info.total_chars, epub.len() as u64,
            );
        } else {
            assert_eq!(seg.version, VERSION_PAGES);
        }
    }
}

fn case_known_utf8_encoding_gate(dir: &Path) {
    let epub = std::fs::read(dir.join("encoding.epub")).unwrap();

    // Spine 0: ISO-8859-1 with spaced `encoding = 'ISO-8859-1'` + non-ASCII `<caf\xe9>`.
    let latin1 = run_producer_chapter(&epub, 0, 0, false, true, 0, None, None);
    assert_eq!(latin1.outcome, CurrentOutcome::Done);
    assert!(latin1.seg_names.len() >= 2);
    let s0 = inspect_segment_file(
        &latin1.seg_names[0],
        find_file(&latin1.files, &latin1.seg_names[0]).unwrap(),
        latin1.generation,
    );
    assert_eq!(s0.version, VERSION_POINT);
    assert!(
        s0.point_range.is_none(),
        "spaced ISO-8859-1 XMLDecl must decline capture"
    );

    // Spine 1: UTF-16 LE BOM + XMLDecl.
    let utf16 = run_producer_chapter(&epub, 1, 0, false, true, 0, None, None);
    assert_eq!(utf16.outcome, CurrentOutcome::Done);
    assert!(utf16.seg_names.len() >= 2);
    let s1 = inspect_segment_file(
        &utf16.seg_names[0],
        find_file(&utf16.files, &utf16.seg_names[0]).unwrap(),
        utf16.generation,
    );
    assert_eq!(s1.version, VERSION_POINT);
    assert!(
        s1.point_range.is_none(),
        "UTF-16 LE BOM chapter must decline capture"
    );

    // Spine 2: UTF-8 BOM + spaced `encoding = "UTF-8"` + non-ASCII `<café>`.
    let utf8 = run_producer_chapter(&epub, 2, 0, false, true, 0, None, None);
    assert_eq!(utf8.outcome, CurrentOutcome::Done);
    assert!(utf8.seg_names.len() >= 2);
    let s2 = inspect_segment_file(
        &utf8.seg_names[0],
        find_file(&utf8.files, &utf8.seg_names[0]).unwrap(),
        utf8.generation,
    );
    assert_eq!(s2.version, VERSION_POINT);
    let raw_pt = s2
        .point_bytes
        .as_ref()
        .expect("UTF-8 BOM + non-ASCII name chapter must capture resume point");
    let parsed = verify_and_parse_point(
        raw_pt, Mode::Seek, &utf8.digest, 2, utf8.generation, PAGES,
        utf8.info.page_count, utf8.info.total_chars, epub.len() as u64,
    );
    assert!(
        parsed.prolog.starts_with(&[0xef, 0xbb, 0xbf]),
        "Prolog must preserve UTF-8 BOM bytes"
    );
    assert_eq!(
        parsed.names.first().map(|v| v.as_slice()),
        Some("café".as_bytes()),
        "Names must carry exact UTF-8 bytes for non-ASCII open root element"
    );
}

fn case_refusal_then_eligibility_in_window(dir: &Path) {
    let epub = std::fs::read(dir.join("declines.epub")).unwrap();
    let cap = run_producer_chapter(&epub, 0, 0, false, true, 0, None, None);
    assert_eq!(cap.outcome, CurrentOutcome::Done);
    assert!(cap.seg_names.len() >= 2);
    let seg0 = inspect_segment_file(
        &cap.seg_names[0],
        find_file(&cap.files, &cap.seg_names[0]).unwrap(),
        cap.generation,
    );
    assert_eq!(seg0.version, VERSION_POINT);
    let raw_pt = seg0
        .point_bytes
        .as_ref()
        .expect("refusal inside [edge-4, edge) must not burn edge_done before later eligible boundary");
    let parsed = verify_and_parse_point(
        raw_pt, Mode::Seek, &cap.digest, 0, cap.generation, PAGES,
        cap.info.page_count, cap.info.total_chars, epub.len() as u64,
    );
    assert!(
        parsed.pages_at >= 125 && parsed.pages_at < 128,
        "expected capture after refusal unwinds inside [125, 128), got pages_at={}",
        parsed.pages_at
    );
}

fn case_explicit_declines(dir: &Path) {
    let epub = std::fs::read(dir.join("declines.epub")).unwrap();
    for (spine, label) in [
        (1u32, "full-window CDATA"),
        (2u32, "open depth > 48 across window"),
        (4u32, "prolog > 512 B"),
        (5u32, "Late jump across window"),
    ] {
        let cap = run_producer_chapter(&epub, spine, 0, false, true, 0, None, None);
        assert_eq!(cap.outcome, CurrentOutcome::Done, "{label}: segment must still seal cleanly");
        assert!(cap.seg_names.len() >= 2, "{label}: must be segmented");
        let seg0 = inspect_segment_file(
            &cap.seg_names[0],
            find_file(&cap.files, &cap.seg_names[0]).unwrap(),
            cap.generation,
        );
        assert_eq!(seg0.version, VERSION_POINT, "{label}: interior segment opens v4");
        assert!(
            seg0.point_range.is_none(),
            "{label}: expected declined point (point_range == None), got {:?}",
            seg0.point_range
        );
        assert_eq!(seg0.page_count, PAGES, "{label}: all 128 pages must remain readable");
    }
    // Spine 3: depth 52 earlier in chapter, unwound before page 124 -> MUST capture!
    let unwound = run_producer_chapter(&epub, 3, 0, false, true, 0, None, None);
    assert_eq!(unwound.outcome, CurrentOutcome::Done);
    let seg0 = inspect_segment_file(
        &unwound.seg_names[0],
        find_file(&unwound.files, &unwound.seg_names[0]).unwrap(),
        unwound.generation,
    );
    assert_eq!(seg0.version, VERSION_POINT);
    let pt = seg0
        .point_bytes
        .as_ref()
        .expect("unwound deep subtree before capture window must recover eligibility and capture");
    verify_and_parse_point(
        pt, Mode::Seek, &unwound.digest, 3, unwound.generation, PAGES,
        unwound.info.page_count, unwound.info.total_chars, epub.len() as u64,
    );
}

fn case_tight_credit_declines_budget(dir: &Path) {
    // Sparse 124-page prefix (~4.5 KiB) followed by a dense 4-page tail (~12-14 KiB):
    // a 2x-average heuristic would underestimate the tail, whereas the proven
    // MAX_PAGE_WIRE_BYTES preflight reserves enough headroom for the real dense tail.
    let epub = std::fs::read(dir.join("budget.epub")).unwrap();
    // Reference v3 pass (no capture) tells us the exact seg 0 page-only card size.
    let ref_run = run_producer_chapter(&epub, 0, 0, false, false, 0, None, None);
    assert_eq!(ref_run.outcome, CurrentOutcome::Done);
    assert!(ref_run.seg_names.len() >= 2);
    let ref_seg0_bytes = find_file(&ref_run.files, &ref_run.seg_names[0]).unwrap().len() as u64;
    // Enforced marginal credit: fits the page-only v4 segment (v3 + 8 B footer + up to 4 B
    // extra block CRC) + dense tail + envelope with 128 B margin, but CANNOT fit a ~1+ KiB point.
    let marginal_credit = ref_seg0_bytes + 128;
    let cap = run_producer_chapter(&epub, 0, 0, false, true, 0, Some(marginal_credit), None);
    assert_eq!(
        cap.outcome,
        CurrentOutcome::Done,
        "enforced marginal credit must still seal page-only v4 segment"
    );
    let seg0 = inspect_segment_file(
        &cap.seg_names[0],
        find_file(&cap.files, &cap.seg_names[0]).unwrap(),
        cap.generation,
    );
    assert_eq!(seg0.version, VERSION_POINT);
    assert!(
        seg0.point_range.is_none(),
        "tight write credit must decline resume point via preflight_fit and seal page-only v4"
    );
    assert_eq!(seg0.page_count, PAGES);
}

fn case_mid_point_write_fault_aborts_temp(dir: &Path) {
    // Spine 2 of stored.epub has a 420 B prolog comment + dense replay so every
    // interior FIRP point exceeds 1,024 B (spanning multiple ProducerStore flush windows).
    // Let seg 0 seal cleanly, then inject a write failure strictly inside seg 1's FIRP point.
    let epub = std::fs::read(dir.join("stored.epub")).unwrap();
    let cap = run_producer_chapter(&epub, 2, 0, false, true, 0, None, Some(1));
    let (firp_start, firp_end) = cap
        .fault_firp_range
        .expect("seg 1 must emit a valid FIRP header before fault");
    assert!(
        firp_end - firp_start > 2048,
        "test point must span >2 1 KiB store windows (got {} B)",
        firp_end - firp_start
    );
    let fault_at = cap
        .fault_offset
        .expect("write fault must trigger on a flush wholly inside seg 1's FIRP point");
    let fault_len = cap.fault_len.expect("fault flush length must be recorded");
    assert!(fault_len > 0);
    assert!(
        fault_at > firp_start && fault_at.saturating_add(fault_len) <= firp_end,
        "failed flush [{fault_at}, {}) must lie wholly inside FIRP point [{firp_start}, {firp_end})",
        fault_at.saturating_add(fault_len)
    );
    assert_eq!(
        cap.outcome,
        CurrentOutcome::NoProgress(Status::IoError),
        "mid-point write fault must fail attempt instead of publishing a partial point"
    );
    // Previously sealed seg 0 must stay intact and readable on the card; seg 1 must be absent.
    let seg0_bytes = find_file(&cap.files, &cap.seg_names[0])
        .expect("previously sealed seg 0 must be preserved after seg 1 mid-point fault");
    let seg0 = inspect_segment_file(&cap.seg_names[0], seg0_bytes, cap.generation);
    assert_eq!(seg0.version, VERSION_POINT);
    assert_eq!(seg0.page_count, PAGES);
    assert!(seg0.point_bytes.is_some());
    assert!(
        find_file(&cap.files, &cap.seg_names[1]).is_none(),
        "aborted seg 1 must not be published onto the card"
    );
}

fn case_canonical_pool_bytes_under_poisoned_bank(dir: &Path) {
    let epub = std::fs::read(dir.join("stored.epub")).unwrap();
    let run_a = run_producer_chapter(&epub, 0, 0, true, true, 0xa5, None, None);
    let run_b = run_producer_chapter(&epub, 0, 0, true, true, 0x5a, None, None);
    assert_eq!(run_a.outcome, CurrentOutcome::Done);
    assert_eq!(run_b.outcome, CurrentOutcome::Done);
    let last_idx = run_a.seg_names.len() - 1;
    assert!(last_idx >= 3);
    let mut saw_live_pool_text = false;
    for idx in 0..last_idx {
        let name = &run_a.seg_names[idx];
        let seg_a = inspect_segment_file(name, find_file(&run_a.files, name).unwrap(), run_a.generation);
        let seg_b = inspect_segment_file(name, find_file(&run_b.files, name).unwrap(), run_b.generation);
        assert_eq!(
            seg_a.point_bytes, seg_b.point_bytes,
            "landscape seg {idx} canonical point bytes must not depend on prior bank poison"
        );
        let raw_pt = seg_a.point_bytes.as_ref().expect("point expected");
        let parsed = verify_and_parse_point(
            raw_pt, Mode::Seek, &run_a.digest, 0, run_a.generation, (idx as u32 + 1) * PAGES,
            run_a.info.page_count, run_a.info.total_chars, epub.len() as u64,
        );
        // Verify ENG1 pool contains real live ASCII bytes (not all zeros).
        let eng = &parsed.engine;
        let u16_at = |at: usize| u16::from_le_bytes([eng[at], eng[at + 1]]) as usize;
        let text_len = u16_at(36);
        let span_count = u16_at(38);
        let top = u16_at(42);
        let run_count = u16_at(56);
        let img_count = u16_at(58);
        let link_count = u16_at(60);
        let pool_len = u16_at(62);
        let pool_off = 81 + text_len + span_count * 8 + (top + 1) * 7 + run_count * 13 + img_count * 14 + link_count * 20;
        let pool = &eng[pool_off..pool_off + pool_len];
        if pool_len > 0 && pool.iter().any(|&b| b.is_ascii_alphabetic()) {
            saw_live_pool_text = true;
        }
    }
    assert!(saw_live_pool_text, "canonical ENG1 pool must retain live text bytes, not zero all bytes");
}

/// Independently filter `entry` through `EntityFilter` up to `limit` filtered bytes.
fn filter_chapter_prefix(epub: &[u8], entry: &Checked, limit: usize) -> Vec<u8> {
    let mut raw = BytesSource(epub.to_vec());
    let mut reader = newt_core::zip_container::Reader::new(&mut raw, None);
    assert_eq!(reader.open(entry), Status::Ok);
    let mut filter = EntityFilter::new();
    let mut out = Vec::with_capacity(limit);
    let mut buf = [0u8; 2048];
    while out.len() < limit {
        let n = filter.read(&mut reader, &mut buf);
        assert!(n >= 0);
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
    }
    out.truncate(limit);
    out
}

/// Drive real Expat + EntityFilter + ChapterSession + maybe_capture directly on
/// `epub`/`spine` and return the observed sequence of non-Idle `CaptureOutcome`s
/// alongside the selected chapter entry and final captured point bytes (if any).
fn probe_capture_seam(
    epub: &[u8],
    spine: u32,
    credit: Option<u64>,
) -> (Checked, Vec<CaptureOutcome>, Option<Vec<u8>>) {
    let mut digest = [0u8; 32];
    assert!(sha256(epub, &mut digest));
    let mut tokens = [0u8; 2];
    let (mut board, pending, _) = unsafe {
        newt_c3_board::Board::acquire_x3_optional_sd(
            (&mut tokens[0] as *mut u8).cast(),
            (&mut tokens[1] as *mut u8).cast(),
        )
    }
    .unwrap();
    drop(pending);

    let mut result = None;
    board
        .with_reader_io(|panel, _io| {
            let mut raw = BytesSource(epub.to_vec());
            let mut bank = [0u8; BANK_BYTES];
            let mut storage = Storage::new(&mut bank);
            let mut parser = newt_target_expat::Expat::default();
            let mut breaker = linebreak::NativeBreak;
            let mut selections = [Selection::default(), Selection::default()];
            let mut runs = [newt_phased_sink::Run::default(); RUNS];
            let mut text = [0u8; TEXT];
            let mut plan = Plan::new(&mut runs, &mut text);
            let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), PageLayout::PORTRAIT);
            let mut frame = [0u8; FRAME_BYTES];
            assert_eq!(reader.open_phased(&mut raw, &mut storage, panel, &mut parser), Status::Ok);
            assert_eq!(
                reader.capture_phased(
                    &mut raw, spine, 0, Target::Page, &mut storage, panel, &mut parser, &mut breaker,
                ),
                Status::Ok
            );
            assert_eq!(reader.publish(&mut raw, &mut frame), Status::Ok);
            let info = reader.info();
            let entry = reader.selection().entry;

            // Open a standalone in-memory v4 writer + real Expat capture loop for seg 0.
            #[derive(Default)]
            struct MemBuf(Vec<u8>);
            impl CacheStorage for MemBuf {
                fn remove(&mut self, _: &[u8]) -> bool { self.0.clear(); true }
                fn file_size(&mut self, _: &[u8]) -> i64 { self.0.len() as i64 }
                fn read_at(&mut self, _: &[u8], _: u32, _: &mut [u8]) -> i32 { -1 }
                fn begin_write(&mut self, _: &[u8]) -> bool { self.0.clear(); true }
                fn write(&mut self, b: &[u8]) -> bool { self.0.extend_from_slice(b); true }
                fn end_write(&mut self) -> bool { true }
            }
            struct WriterSink<'a, 'index> {
                cache: &'a mut PageCacheWriter,
                store: &'a mut MemBuf,
                wm: &'a mut WriterMemory<'index>,
                seen: &'a mut u32,
            }
            impl Sink for WriterSink<'_, '_> {
                fn page(&mut self, page: newt_core::chapter_layout::types::Page<'_>) -> bool {
                    if *self.seen < PAGES {
                        assert!(self.cache.on_page(self.store, self.wm, &page));
                    }
                    *self.seen += 1;
                    true
                }
            }
            struct NeverCancel;
            impl xml_session::ProcessingPolicy for NeverCancel {
                fn cancelled(&mut self) -> bool { false }
            }

            let mut mem_store = MemBuf::default();
            let mut chunks = [IndexChunk::default(); 1];
            let mut anchors = [Anchor::default(); 192];
            let mut wm = WriterMemory { chunks: &mut chunks, anchors: &mut anchors };
            let mut cache = PageCacheWriter::new(Profile::Small);
            const DUMMY_NAME: &[u8; 69] =
                b"00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff.fibp";
            assert!(cache.begin_v4(&mut mem_store, DUMMY_NAME, 0x1234_5678, &wm));

            let mut prolog = [0u8; PROLOG_MAX];
            let mut names = [0u8; NAMES_MAX];
            let mut lens = [0u8; DEPTH_MAX];
            let mut live_bits = [0u8; LIVE_BITS_BYTES];
            let mut cap_state = CaptureState::new(&mut prolog, &mut names, &mut lens, &mut live_bits);

            let mut text_buf = [0u8; 3072];
            let mut breaks_buf = [0u8; 3072];
            let mut levels_buf = [0u8; 3072];
            let mut spans_buf = [newt_core::chapter_layout::types::Span::default(); 96];
            let mut lines_buf = [newt_core::chapter_layout::types::Line::default(); 176];
            let mut elems_buf = [newt_core::chapter_layout::types::Element::default(); 16];
            let mut runs_buf = [Run::default(); 304];
            let mut imgs_buf = [Image::default(); 8];
            let mut links_buf = [Link::default(); 16];
            let mut page_buf = [0u8; 7168];
            let mut style_buf = [0u8; 3072];
            let mut plinks_buf = [newt_core::chapter_layout::types::ParLink::default(); 6];
            struct NoImages;
            impl newt_core::chapter_layout::types::Images for NoImages {
                fn resolve(&mut self, _: &[u8], _: &mut [u8], _: &mut [u8]) -> Option<(usize, usize)> {
                    None
                }
                fn probe(&mut self, _: &[u8]) -> Option<(u32, u32)> {
                    None
                }
            }
            let mut chapter_images = NoImages;
            let mut rules_buf = [newt_core::css::CssRule::default(); newt_phased_reader::styles::MAX_RULES];
            let slot = newt_phased_reader::styles::Slot {
                rules: &mut rules_buf,
                count: 0,
                hash: newt_phased_reader::styles::ID,
                spine,
                issues: 0,
                state: newt_phased_reader::styles::State::Settled,
            };
            let g = newt_reader_atlas::Geometry::reader(false);
            let params = newt_core::chapter_layout::types::Params {
                width: g.width as i16,
                height: g.height as i16,
                left: g.margin as i16,
                right: g.margin as i16,
                top: g.margin as i16,
                bottom: (g.height - g.content_bottom) as i16,
                base_size: PageLayout::PORTRAIT.text_size.px(),
                align: newt_core::css::TextAlign::Justify,
                embedded_styles: false,
                inline_ascent: true,
                picture_notes: true,
                language: Some(reader.selection().language.tag()),
                stylesheet: None,
                ..newt_core::chapter_layout::types::Params::default()
            };
            let storage_lay = newt_core::chapter_layout::types::Storage {
                text: &mut text_buf,
                breaks: &mut breaks_buf,
                levels: &mut levels_buf,
                spans: &mut spans_buf,
                lines: &mut lines_buf,
                elements: &mut elems_buf,
                runs: &mut runs_buf,
                images: &mut imgs_buf,
                links: &mut links_buf,
                page: &mut page_buf,
                style_text: &mut style_buf,
                paragraph_links: &mut plinks_buf,
            };
            let mut engine = EngineState::new(
                params,
                storage_lay,
                &mut breaker,
                &mut chapter_images,
                None,
                CssStylesheetBuilder::default(),
            )
            .unwrap();

            let mut raw2 = BytesSource(epub.to_vec());
            let mut scheduled = newt_core::admission::ScheduledSource::new(&mut raw2, None, None);
            let mut chapter =
                newt_core::admission::ChapterSource::open(&mut scheduled, &entry, None, None, None)
                    .unwrap();
            let raw_checked = Checked::raw(entry.uncompressed_size);
            let mut xml_buf = [0u8; 2048];
            let mut filter = EntityFilter::new();
            let mut pol = NeverCancel;
            let stream = XmlStream::with_policy(&mut chapter, None, &mut parser, BorrowedPolicy(&mut pol));
            let mut session = ChapterSession::new(stream);
            let mut status = session.begin_state(
                &raw_checked,
                xml_session::Resources { buffer: Some(&mut xml_buf), filter: Some(&mut filter) },
                &engine,
            );
            let mut seen = 0u32;
            let mut refused = false;
            let mut outcomes = Vec::new();
            let mut captured_bytes = None;
            let route = CaptureRoute {
                digest: &digest,
                spine,
                generation: 0x1234_5678,
                page_count: info.page_count,
                total_chars: info.total_chars,
            };
            while status == Status::Ok && !session.done() && seen < PAGES {
                status = session.step_with(&mut engine, 1, 1, |_, parser, engine, op| {
                    let mut sink = WriterSink { cache: &mut cache, store: &mut mem_store, wm: &mut wm, seen: &mut seen };
                    let mut font = newt_reader_atlas::ReaderAtlas;
                    let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
                    match op {
                        LayoutOperation::Feed { bytes, last } => {
                            cap_state.tap.observe(bytes);
                            let mut oh = OperationHandler { engine, backend: &mut backend };
                            let mut tap = cap_state.tap.wrap(&mut oh);
                            let ok = parser.feed_observed(bytes, last, &mut tap, &mut cap_state.obs).unwrap();
                            cap_state.tap.finish_root(cap_state.obs.root_index);
                            ok
                        }
                        LayoutOperation::Finish => engine.finish(&mut backend),
                    }
                });
                if status != Status::Ok || session.done() {
                    break;
                }
                let outcome = session.stream.with_source_and_view(|ch, p, sv| {
                    let input = BoundaryInput {
                        entry: &entry,
                        source_bytes: epub.len() as u64,
                        source_consumed: ch.consumed(),
                        source_method: ch.method(),
                        parser_index: p.current_byte_index(),
                        stream: sv,
                    };
                    let cw = CaptureWriter {
                        cache: &mut cache,
                        first: 0,
                        seen,
                        open: true,
                        refused: &mut refused,
                    };
                    let mut pt_buf = Vec::new();
                    let mut write = |bytes: &[u8]| {
                        pt_buf.extend_from_slice(bytes);
                        mem_store.write(bytes)
                    };
                    let res = maybe_capture(&mut cap_state, &engine, &slot, cw, credit, &mut write, &route, &input);
                    if matches!(res, CaptureOutcome::Captured(_)) {
                        captured_bytes = Some(pt_buf);
                    }
                    res
                });
                if outcome != CaptureOutcome::Idle {
                    outcomes.push(outcome);
                }
            }
            session.abort();
            result = Some((entry, outcomes, captured_bytes));
        })
        .unwrap();
    result.unwrap()
}

fn case_nonempty_replay_exact_filtered_bytes(dir: &Path) {
    let epub = std::fs::read(dir.join("stored.epub")).unwrap();
    let cap = run_producer_chapter(&epub, 2, 0, false, true, 0, None, None);
    assert_eq!(cap.outcome, CurrentOutcome::Done);
    assert!(cap.seg_names.len() >= 2);
    let seg0 = inspect_segment_file(
        &cap.seg_names[0],
        find_file(&cap.files, &cap.seg_names[0]).unwrap(),
        cap.generation,
    );
    let raw_pt = seg0.point_bytes.as_ref().expect("replay spine seg 0 must capture point");
    let parsed = verify_and_parse_point(
        raw_pt, Mode::Seek, &cap.digest, 2, cap.generation, PAGES,
        cap.info.page_count, cap.info.total_chars, epub.len() as u64,
    );
    assert!(
        !parsed.replay.is_empty(),
        "replay fixture must land mid-token with nonempty Replay bytes (fed={}, cut={})",
        parsed.fed,
        parsed.cut
    );
    let (entry, _, _) = probe_capture_seam(&epub, 2, None);
    let filtered = filter_chapter_prefix(&epub, &entry, parsed.fed as usize);
    assert!(
        filtered.windows(3).any(|w| w == "\u{2014}".as_bytes())
            && filtered.windows(2).any(|w| w == "\u{00a0}".as_bytes()),
        "filtered stream must contain expanded HTML named entities before the cut"
    );
    assert_eq!(
        parsed.replay.as_slice(),
        &filtered[parsed.cut as usize..parsed.fed as usize],
        "Replay section must equal exact post-entity-filter byte slice [cut..fed)"
    );
}

/// Drive real Expat + EntityFilter + EngineState + maybe_capture across a
/// boundary whose unfinished comment extends before `last_feed`, asserting
/// `Decline::Backlog` followed by `Captured(128)` on a later eligible cut.
fn probe_backlog_decline_and_recovery(epub: &[u8], spine: u32) -> (PageInfo, Digest, Vec<u8>) {
    let mut digest = [0u8; 32];
    assert!(sha256(epub, &mut digest));
    let mut tokens = [0u8; 2];
    let (mut board, pending, _) = unsafe {
        newt_c3_board::Board::acquire_x3_optional_sd(
            (&mut tokens[0] as *mut u8).cast(),
            (&mut tokens[1] as *mut u8).cast(),
        )
    }
    .unwrap();
    drop(pending);

    let mut result = None;
    board
        .with_reader_io(|panel, _io| {
            let mut raw = BytesSource(epub.to_vec());
            let mut bank = [0u8; BANK_BYTES];
            let mut storage = Storage::new(&mut bank);
            let mut parser = newt_target_expat::Expat::default();
            let mut breaker = linebreak::NativeBreak;
            let mut selections = [Selection::default(), Selection::default()];
            let mut runs = [newt_phased_sink::Run::default(); RUNS];
            let mut text = [0u8; TEXT];
            let mut plan = Plan::new(&mut runs, &mut text);
            let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), PageLayout::PORTRAIT);
            let mut frame = [0u8; FRAME_BYTES];
            assert_eq!(reader.open_phased(&mut raw, &mut storage, panel, &mut parser), Status::Ok);
            assert_eq!(
                reader.capture_phased(
                    &mut raw, spine, 0, Target::Page, &mut storage, panel, &mut parser, &mut breaker,
                ),
                Status::Ok
            );
            assert_eq!(reader.publish(&mut raw, &mut frame), Status::Ok);
            let info = reader.info();
            let entry = reader.selection().entry;

            #[derive(Default)]
            struct MemBuf(Vec<u8>);
            impl CacheStorage for MemBuf {
                fn remove(&mut self, _: &[u8]) -> bool { self.0.clear(); true }
                fn file_size(&mut self, _: &[u8]) -> i64 { self.0.len() as i64 }
                fn read_at(&mut self, _: &[u8], _: u32, _: &mut [u8]) -> i32 { -1 }
                fn begin_write(&mut self, _: &[u8]) -> bool { self.0.clear(); true }
                fn write(&mut self, b: &[u8]) -> bool { self.0.extend_from_slice(b); true }
                fn end_write(&mut self) -> bool { true }
            }
            struct WriterSink<'a, 'index> {
                cache: &'a mut PageCacheWriter,
                store: &'a mut MemBuf,
                wm: &'a mut WriterMemory<'index>,
                seen: &'a mut u32,
            }
            impl Sink for WriterSink<'_, '_> {
                fn page(&mut self, page: newt_core::chapter_layout::types::Page<'_>) -> bool {
                    if *self.seen < PAGES {
                        assert!(self.cache.on_page(self.store, self.wm, &page));
                    }
                    *self.seen += 1;
                    true
                }
            }
            struct NoImages;
            impl newt_core::chapter_layout::types::Images for NoImages {
                fn resolve(&mut self, _: &[u8], _: &mut [u8], _: &mut [u8]) -> Option<(usize, usize)> { None }
                fn probe(&mut self, _: &[u8]) -> Option<(u32, u32)> { None }
            }

            let mut mem_store = MemBuf::default();
            let mut chunks = [IndexChunk::default(); 1];
            let mut anchors = [Anchor::default(); 192];
            let mut wm = WriterMemory { chunks: &mut chunks, anchors: &mut anchors };
            let mut cache = PageCacheWriter::new(Profile::Small);
            const DUMMY_NAME: &[u8; 69] =
                b"00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff.fibp";
            assert!(cache.begin_v4(&mut mem_store, DUMMY_NAME, 0x1234_5678, &wm));

            let mut prolog = [0u8; PROLOG_MAX];
            let mut names = [0u8; NAMES_MAX];
            let mut lens = [0u8; DEPTH_MAX];
            let mut live_bits = [0u8; LIVE_BITS_BYTES];
            let mut cap_state = CaptureState::new(&mut prolog, &mut names, &mut lens, &mut live_bits);

            let mut text_buf = [0u8; 3072];
            let mut breaks_buf = [0u8; 3072];
            let mut levels_buf = [0u8; 3072];
            let mut spans_buf = [newt_core::chapter_layout::types::Span::default(); 96];
            let mut lines_buf = [newt_core::chapter_layout::types::Line::default(); 176];
            let mut elems_buf = [newt_core::chapter_layout::types::Element::default(); 16];
            let mut runs_buf = [Run::default(); 304];
            let mut imgs_buf = [Image::default(); 8];
            let mut links_buf = [Link::default(); 16];
            let mut page_buf = [0u8; 7168];
            let mut style_buf = [0u8; 3072];
            let mut plinks_buf = [newt_core::chapter_layout::types::ParLink::default(); 6];
            let mut chapter_images = NoImages;
            let mut rules_buf = [newt_core::css::CssRule::default(); newt_phased_reader::styles::MAX_RULES];
            let slot = newt_phased_reader::styles::Slot {
                rules: &mut rules_buf,
                count: 0,
                hash: newt_phased_reader::styles::ID,
                spine,
                issues: 0,
                state: newt_phased_reader::styles::State::Settled,
            };
            let g = newt_reader_atlas::Geometry::reader(false);
            let params = newt_core::chapter_layout::types::Params {
                width: g.width as i16,
                height: g.height as i16,
                left: g.margin as i16,
                right: g.margin as i16,
                top: g.margin as i16,
                bottom: (g.height - g.content_bottom) as i16,
                base_size: PageLayout::PORTRAIT.text_size.px(),
                align: newt_core::css::TextAlign::Justify,
                embedded_styles: false,
                inline_ascent: true,
                picture_notes: true,
                language: Some(reader.selection().language.tag()),
                stylesheet: None,
                ..newt_core::chapter_layout::types::Params::default()
            };
            let storage_lay = newt_core::chapter_layout::types::Storage {
                text: &mut text_buf,
                breaks: &mut breaks_buf,
                levels: &mut levels_buf,
                spans: &mut spans_buf,
                lines: &mut lines_buf,
                elements: &mut elems_buf,
                runs: &mut runs_buf,
                images: &mut imgs_buf,
                links: &mut links_buf,
                page: &mut page_buf,
                style_text: &mut style_buf,
                paragraph_links: &mut plinks_buf,
            };
            let mut engine = EngineState::new(
                params, storage_lay, &mut breaker, &mut chapter_images, None,
                CssStylesheetBuilder::default(),
            )
            .unwrap();

            let mut raw2 = BytesSource(epub.to_vec());
            let mut zip_reader = newt_core::zip_container::Reader::new(&mut raw2, None);
            assert_eq!(zip_reader.open(&entry), Status::Ok);
            let mut filter = EntityFilter::new();
            assert!(parser.create_producer());

            let mut seen = 0u32;
            let mut refused = false;
            let mut bytes_consumed = 0u64;
            let mut saw_backlog = false;
            let mut captured_pt: Option<Vec<u8>> = None;
            let route = CaptureRoute {
                digest: &digest,
                spine,
                generation: 0x1234_5678,
                page_count: info.page_count,
                total_chars: info.total_chars,
            };

            let mut buf = [0u8; 2048];
            while seen < PAGES {
                // Once we enter the comment inside [124, 128), feed a 512 B continuation
                // chunk so Expat's current_byte_index stays strictly before `last_feed`
                // (fed - cut > last_feed.len()), triggering Decline::Backlog cleanly.
                let chunk_cap = if engine.pages >= PAGES - 4 && !saw_backlog { 512 } else { 2048 };
                let n = filter.read(&mut zip_reader, &mut buf[..chunk_cap]);
                assert!(n > 0, "unexpected EOF before page 128");
                let fed_slice = &buf[..n as usize];
                cap_state.tap.observe(fed_slice);
                let ok = {
                    let mut sink = WriterSink { cache: &mut cache, store: &mut mem_store, wm: &mut wm, seen: &mut seen };
                    let mut font = newt_reader_atlas::ReaderAtlas;
                    let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
                    let mut oh = OperationHandler { engine: &mut engine, backend: &mut backend };
                    let mut tap = cap_state.tap.wrap(&mut oh);
                    parser.feed_observed(fed_slice, false, &mut tap, &mut cap_state.obs).unwrap()
                };
                assert!(ok, "Expat feed_observed failed");
                cap_state.tap.finish_root(cap_state.obs.root_index);
                bytes_consumed += n as u64;

                let cut = parser.current_byte_index();
                // Wait until real Expat is inside the multi-chunk comment before invoking
                // maybe_capture so the first capture attempt hits the true backlog boundary.
                let in_multi_chunk_backlog = cut >= 0
                    && bytes_consumed.saturating_sub(cut as u64) > fed_slice.len() as u64;
                if engine.pages >= PAGES - 4 && (saw_backlog || in_multi_chunk_backlog) {
                    let input = BoundaryInput {
                        entry: &entry,
                        source_bytes: epub.len() as u64,
                        source_consumed: zip_reader.total_produced(),
                        source_method: entry.method,
                        parser_index: cut,
                        stream: xml_session::StreamView {
                            open: true,
                            finished: false,
                            bytes_consumed,
                            reader_produced: zip_reader.total_produced(),
                            last_feed: fed_slice,
                            filter: Some(&filter),
                        },
                    };
                    let cw = CaptureWriter {
                        cache: &mut cache,
                        first: 0,
                        seen,
                        open: true,
                        refused: &mut refused,
                    };
                    let mut pt_buf = Vec::new();
                    let mut write = |bytes: &[u8]| {
                        pt_buf.extend_from_slice(bytes);
                        mem_store.write(bytes)
                    };
                    let outcome = maybe_capture(
                        &mut cap_state, &engine, &slot, cw, None, &mut write, &route, &input,
                    );
                    if !saw_backlog {
                        assert_eq!(
                            outcome,
                            CaptureOutcome::Declined(PAGES, Decline::Backlog),
                            "multi-chunk unfinished token extending before last_feed must decline with Backlog"
                        );
                        saw_backlog = true;
                    } else if outcome == CaptureOutcome::Captured(PAGES) {
                        captured_pt = Some(pt_buf);
                        break;
                    }
                }
            }
            parser.close();
            assert!(saw_backlog, "must observe Decline::Backlog on unfinished multi-chunk comment");
            let pt = captured_pt.expect("later eligible cut after Decline::Backlog must capture");
            result = Some((info, digest, pt));
        })
        .unwrap();
    result.unwrap()
}

fn case_decline_reasons_and_backlog_recovery(dir: &Path) {
    let encoding = std::fs::read(dir.join("encoding.epub")).unwrap();
    let declines = std::fs::read(dir.join("declines.epub")).unwrap();
    let budget = std::fs::read(dir.join("budget.epub")).unwrap();

    // Exact Decline reason assertions at the real-Expat capture seam.
    let (_, lat_out, _) = probe_capture_seam(&encoding, 0, None);
    assert!(lat_out.contains(&CaptureOutcome::Declined(PAGES, Decline::Encoding)), "got {lat_out:?}");
    let (_, u16_out, _) = probe_capture_seam(&encoding, 1, None);
    assert!(u16_out.contains(&CaptureOutcome::Declined(PAGES, Decline::Encoding)), "got {u16_out:?}");
    let (_, cdata_out, _) = probe_capture_seam(&declines, 1, None);
    assert!(cdata_out.contains(&CaptureOutcome::Declined(PAGES, Decline::Cdata)), "got {cdata_out:?}");
    let (_, depth_out, _) = probe_capture_seam(&declines, 2, None);
    assert!(depth_out.contains(&CaptureOutcome::Declined(PAGES, Decline::Depth)), "got {depth_out:?}");
    let (_, prolog_out, _) = probe_capture_seam(&declines, 4, None);
    assert!(prolog_out.contains(&CaptureOutcome::Declined(PAGES, Decline::Prolog)), "got {prolog_out:?}");
    let (_, late_out, _) = probe_capture_seam(&declines, 5, None);
    assert!(late_out.contains(&CaptureOutcome::Declined(PAGES, Decline::Late)), "got {late_out:?}");
    let (_, bud_out, _) = probe_capture_seam(&budget, 0, Some(16_384));
    assert!(bud_out.contains(&CaptureOutcome::Declined(PAGES, Decline::Budget)), "got {bud_out:?}");

    // Backlog decline (real Expat unfinished comment spanning before last_feed)
    // followed by recovery and valid FIRP capture on a later eligible cut inside [124, 128).
    let (info, digest, raw_pt) = probe_backlog_decline_and_recovery(&declines, 6);
    verify_and_parse_point(
        &raw_pt, Mode::Seek, &digest, 6, 0x1234_5678, PAGES,
        info.page_count, info.total_chars, declines.len() as u64,
    );
}

/// Upgrade compatibility from a v0.7.1-format v3 cache:
/// 1) A complete v3 cache (all segments v3 + NTSM map) stays readable page-for-page
///    and untouched when opened/revisited by the v4-capable producer.
/// 2) A partial v3 cache (some interior v3 segments already written before upgrade)
///    keeps those v3 segments byte-for-byte intact while capturing v4 resume points
///    for newly written interior segments and keeping the final segment v3.
fn case_v3_upgrade_compatibility_and_mixed_fill(dir: &Path) {
    let epub = std::fs::read(dir.join("stored.epub")).unwrap();
    // Generate an authentic pre-upgrade v3 cache (v0.7.1 segment format).
    let v3_full = run_producer_chapter(&epub, 0, 0, false, false, 0, None, None);
    assert_eq!(v3_full.outcome, CurrentOutcome::Done);
    assert!(v3_full.seg_names.len() >= 4);
    for (idx, name) in v3_full.seg_names.iter().enumerate() {
        let bytes = find_file(&v3_full.files, name).expect("v3 seed segment");
        let seg = inspect_segment_file(name, bytes, v3_full.generation);
        assert_eq!(seg.version, VERSION_PAGES, "pre-upgrade seed seg {idx} must be v3");
        assert!(seg.point_range.is_none());
    }

    // Part A: complete pre-upgrade v3 cache stays Done and byte-for-byte untouched.
    let after_full = run_producer_chapter_seeded(
        &epub, 0, 42, false, true, 0, None, None, &v3_full.files,
    );
    assert_eq!(after_full.outcome, CurrentOutcome::Done);
    assert_eq!(
        after_full.files, v3_full.files,
        "complete pre-upgrade v3 cache must stay byte-for-byte untouched"
    );

    // Part B: partial pre-upgrade v3 cache (segs 0 and 2 present, segs 1 and final missing)
    // preserves v3 segs 0 & 2 verbatim while writing newly produced interior seg 1 as v4
    // (with a verified FIRP v1 point) and the final segment as v3.
    let seg0_suffix = core::str::from_utf8(&v3_full.seg_names[0]).unwrap();
    let seg2_suffix = core::str::from_utf8(&v3_full.seg_names[2]).unwrap();
    let partial_v3: BTreeMap<String, Vec<u8>> = v3_full
        .files
        .iter()
        .filter(|(k, _)| k.ends_with(seg0_suffix) || k.ends_with(seg2_suffix))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    assert_eq!(partial_v3.len(), 2, "must seed exactly v3 segs 0 and 2");

    let mixed = run_producer_chapter_seeded(
        &epub, 0, 42, false, true, 0, None, None, &partial_v3,
    );
    let mixed_v3_ref = run_producer_chapter_seeded(
        &epub, 0, 42, false, false, 0, None, None, &partial_v3,
    );
    assert_eq!(mixed.outcome, CurrentOutcome::Done);
    assert_eq!(mixed_v3_ref.outcome, CurrentOutcome::Done);
    assert_eq!(
        find_file(&mixed.files, &mixed.map_file.unwrap()).expect("mixed map"),
        find_file(&v3_full.files, &v3_full.map_file.unwrap()).expect("v3 ref map"),
        "mixed v3/v4 upgrade must publish the exact same segment map"
    );

    let last_idx = mixed.seg_names.len() - 1;
    for (idx, name) in mixed.seg_names.iter().enumerate() {
        let got_bytes = find_file(&mixed.files, name).expect("segment after mixed fill");
        let ref_bytes = find_file(&v3_full.files, name).expect("v3 ref segment");
        let same_start_bytes =
            find_file(&mixed_v3_ref.files, name).expect("same-start v3 ref segment");
        let got_seg = inspect_segment_file(name, got_bytes, mixed.generation);
        let ref_seg = inspect_segment_file(name, ref_bytes, v3_full.generation);
        let same_start_seg =
            inspect_segment_file(name, same_start_bytes, mixed_v3_ref.generation);
        assert_eq!(got_seg.pages, ref_seg.pages, "seg {idx} decoded pages must match full v3 ref");
        assert_eq!(
            got_seg.anchors, same_start_seg.anchors,
            "seg {idx} anchors must match same-starting-card v3 ref"
        );
        if idx == 0 || idx == 2 {
            assert_eq!(got_seg.version, VERSION_PAGES, "pre-existing v3 seg {idx} must stay v3");
            assert_eq!(got_bytes, ref_bytes, "pre-existing v3 seg {idx} bytes must be untouched");
        } else if idx < last_idx {
            assert_eq!(got_seg.version, VERSION_POINT, "newly filled interior seg {idx} must be v4");
            let pt = got_seg.point_bytes.as_ref().expect("newly filled interior v4 point");
            verify_and_parse_point(
                pt,
                Mode::Seek,
                &mixed.digest,
                0,
                mixed.generation,
                (idx as u32 + 1) * PAGES,
                mixed.info.page_count,
                mixed.info.total_chars,
                epub.len() as u64,
            );
        } else {
            assert_eq!(got_seg.version, VERSION_PAGES, "final seg {idx} must stay v3");
            assert!(got_seg.point_range.is_none());
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = Path::new(args.get(1).expect("fixture directory argument required"));
    let only = args.windows(2).find(|w| w[0] == "--case").map(|w| w[1].as_str());
    let cases: &[(&str, fn(&Path))] = &[
        ("portrait_and_landscape_stored_and_deflated", case_portrait_and_landscape_stored_and_deflated),
        ("target_first_start_gt_zero", case_target_first_start_gt_zero),
        ("known_utf8_encoding_gate", case_known_utf8_encoding_gate),
        ("refusal_then_eligibility_in_window", case_refusal_then_eligibility_in_window),
        ("explicit_declines", case_explicit_declines),
        ("tight_credit_declines_budget", case_tight_credit_declines_budget),
        ("mid_point_write_fault_aborts_temp", case_mid_point_write_fault_aborts_temp),
        ("canonical_pool_bytes_under_poisoned_bank", case_canonical_pool_bytes_under_poisoned_bank),
        ("nonempty_replay_exact_filtered_bytes", case_nonempty_replay_exact_filtered_bytes),
        ("decline_reasons_and_backlog_recovery", case_decline_reasons_and_backlog_recovery),
        ("v3_upgrade_compatibility_and_mixed_fill", case_v3_upgrade_compatibility_and_mixed_fill),
    ];
    let mut ran = 0usize;
    for &(name, func) in cases {
        if only.is_none_or(|want| want == name) {
            func(dir);
            ran += 1;
        }
    }
    assert!(ran > 0, "unknown --case {:?}", only);
    println!("PASS resume capture ({ran} cases)");
}
