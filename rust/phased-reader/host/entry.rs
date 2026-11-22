// Source-only native operation driver. Link to the SAME adopted newt-core as
// Reader and newt-target-expat; use the existing approved native link recipe.
use newt_core::{
    chapter_layout::types::*,
    css::CssRule,
    inflate::{BitBuffer, State},
    package_parsers::{ManifestItem, TocEntry},
    xml_input::EntityFilter,
    xml_session::{Handler, ParserBackend},
    zip_container::{Entry, InflateScratch, Status},
    zip_window::{arena::Arena, source::BookSource},
};
use newt_phased_reader::{
    styles::{Slot, Sources, State as StyleState},
    *,
};
use newt_phased_sink::{Frame, Plan, RUNS, Run as PlanRun, TEXT, Target};
use newt_target_expat::Expat;
use std::{
    path::Path,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst},
};
#[path = "../../host/src/chapter_layout/linebreak.rs"]
mod linebreak;
static LIVE: AtomicBool = AtomicBool::new(false);
static HELD: AtomicBool = AtomicBool::new(false);
static RETURNS: AtomicUsize = AtomicUsize::new(0);
struct Backend {
    xml: Expat,
    creates: usize,
}
impl ParserBackend for Backend {
    fn create(&mut self) -> bool {
        assert!(!LIVE.load(SeqCst));
        let ok = self.xml.create();
        LIVE.store(ok, SeqCst);
        self.creates += usize::from(ok);
        ok
    }
    fn feed(&mut self, bytes: &[u8], last: bool, h: &mut dyn Handler) -> bool {
        assert!(LIVE.load(SeqCst));
        self.xml.feed(bytes, last, h)
    }
    fn close(&mut self) {
        self.xml.close();
        LIVE.store(false, SeqCst);
    }
}
fn acquire() -> bool {
    assert!(!HELD.swap(true, SeqCst));
    true
}
fn returned() {
    assert!(!LIVE.load(SeqCst), "lease returned with live XML");
    assert!(HELD.swap(false, SeqCst));
    RETURNS.fetch_add(1, SeqCst);
}
static EXACT_ACQUIRES: AtomicUsize = AtomicUsize::new(0);
static FAIL_AFTER_RETURN: AtomicBool = AtomicBool::new(false);
static PROBE_FAULT: AtomicBool = AtomicBool::new(false);
static METADATA_PHASE: AtomicBool = AtomicBool::new(false);
fn observe_metadata(event: newt_core::admission::WorkEvent) {
    if event.kind == newt_core::admission::WorkKind::Phase {
        METADATA_PHASE.store(
            event.phase == newt_core::admission::WorkPhase::Metadata,
            SeqCst,
        );
    }
}
fn exact_denied() -> bool {
    if EXACT_ACQUIRES.fetch_add(1, SeqCst) == 1 {
        false
    } else {
        acquire()
    }
}
fn fault_returned() {
    returned();
    if FAIL_AFTER_RETURN.swap(false, SeqCst) {
        PROBE_FAULT.store(true, SeqCst);
    }
}
struct Source {
    bytes: Vec<u8>,
    fail: Option<(u64, u64)>,
    probe_fail: bool,
    metadata_only: bool,
    failed_reads: usize,
    reads: usize,
}
impl BookSource for Source {
    fn size(&self) -> u64 {
        self.bytes.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        self.reads += 1;
        if out.is_empty() {
            return if self.probe_fail || PROBE_FAULT.load(SeqCst) {
                -1
            } else {
                0
            };
        }
        if (!self.metadata_only || METADATA_PHASE.load(SeqCst))
            && self
                .fail
                .is_some_and(|(start, end)| at >= start && at < end)
        {
            self.failed_reads += 1;
            return -1;
        }
        let at = (at as usize).min(self.bytes.len());
        let n = out.len().min(self.bytes.len() - at);
        out[..n].copy_from_slice(&self.bytes[at..at + n]);
        n as i32
    }
}
#[derive(Clone, Copy)]
pub enum Case {
    Oracle,
    MenuOracle { anchors: bool },
    Menu,
    Plain,
    LateMalformed,
    Deferred,
    Pending,
    OptionalMalformed,
    OptionalLiveFault,
    Duplicate,
    Collision,
    PrefixAnchor,
    CrossSpine,
}
fn locate(bytes: &[u8], needle: &[u8]) -> usize {
    bytes
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("fixture marker")
}
fn receipt(op: &str, status: Status, reader: &PhasedReader<'_, '_>, frame: &Frame) {
    use std::io::Write;
    let p = reader.info();
    let hash = frame.iter().fold(14695981039346656037u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(1099511628211)
    });
    println!(
        "{op} {} {} {} {} {} {} {hash}",
        status as u8, p.spine, p.page, p.page_count, p.char_start, p.total_chars
    );
    let mut out = std::io::stdout().lock();
    write!(out, "{op} frame ").unwrap();
    for b in frame {
        write!(out, "{b:02x}").unwrap();
    }
    writeln!(out).unwrap();
}
fn menu_receipt(op: &str, status: Status, reader: &PhasedReader<'_, '_>) {
    let menu = reader.chapter_menu();
    let i = menu.info();
    println!(
        "{op} menu {} {} {} {} {} {} {} {}",
        status as u8,
        u8::from(i.valid),
        i.count,
        i.focus,
        i.first,
        i.visible,
        u8::from(i.authored),
        u8::from(i.current_exact)
    );
    for n in i.first..i.first + u32::from(i.visible) {
        let row = menu.row(n);
        println!(
            "{op} row {} {} {} {} {} {} {} {}",
            row.index,
            row.spine,
            row.toc,
            u8::from(row.generic),
            u8::from(row.fragment),
            u8::from(row.current),
            u8::from(row.valid),
            std::str::from_utf8(row.label).expect("fixture UTF-8")
        );
    }
}
/// Called serially with host/fixtures.py files. Each case drives actual reader
/// operations; backend callbacks and source reads are never simulated success.
pub fn run(path: &Path, case: Case, landscape: bool) {
    assert!(!LIVE.load(SeqCst) && !HELD.load(SeqCst));
    let mut source = Source {
        bytes: std::fs::read(path).unwrap(),
        fail: None,
        probe_fail: false,
        metadata_only: matches!(case, Case::OptionalLiveFault),
        failed_reads: 0,
        reads: 0,
    };
    if matches!(case, Case::OptionalLiveFault) {
        let at = locate(&source.bytes, b"<html xmlns:epub");
        source.fail = Some((at as u64, (at + 1024) as u64));
    }
    let mut selections = [Selection::default(), Selection::default()];
    let mut runs = vec![PlanRun::default(); RUNS];
    let mut text = vec![0; TEXT];
    let mut plan = Plan::new(
        runs.as_mut_slice().try_into().unwrap(),
        text.as_mut_slice().try_into().unwrap(),
    );
    let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), landscape);
    METADATA_PHASE.store(false, SeqCst);
    if matches!(case, Case::OptionalLiveFault) {
        reader.work = Some(observe_metadata);
    }
    let mut frame: Box<Frame> = vec![0; newt_phased_sink::FRAME_BYTES]
        .into_boxed_slice()
        .try_into()
        .unwrap();
    let lease = Lease { acquire, returned };
    let mut backend = Backend {
        xml: Expat::default(),
        creates: 0,
    };
    let mut state = Box::new(State::new(BitBuffer::Bits64));
    let mut dict = vec![0; 32768];
    let mut input = vec![0; 2048];
    let mut meta_bytes = vec![0; 48 * 1024];
    let mut parse_bytes = vec![0; 48 * 1024];
    let mut paths_bytes = vec![0; 512];
    let mut metadata = Arena::new(&mut meta_bytes);
    let mut parse = Arena::new(&mut parse_bytes);
    let mut paths = Arena::new(&mut paths_bytes);
    let mut scan = [0; 4099];
    let mut sequential = [0; 1024];
    let mut entries = vec![Entry::default(); 128];
    let mut manifest = vec![ManifestItem::default(); 128];
    let mut hashes = vec![0; 128];
    let mut spines = vec![0; 128];
    let mut chunk = [0; 2048];
    let mut xml = [0; 2048];
    let mut filter = EntityFilter::new();
    let mut toc = vec![TocEntry::default(); 128];
    let mut mapping = vec![0; toc.len()];
    let mut directory = [0; 512];
    let mut rules = vec![CssRule::default(); 64];
    let mut source_text = vec![0; styles::EMBEDDED];
    let mut sources = Sources {
        links: [styles::Member::default(); 4],
        start: [0; 8],
        length: [0; 8],
        issues: 0,
        used: 0,
        events: 0,
        external_mask: 0,
        text: &mut source_text,
    };
    let mut slot = Slot {
        rules: rules.as_mut_slice().try_into().unwrap(),
        count: 0,
        hash: 0,
        spine: 0,
        issues: 0,
        state: StyleState::Empty,
    };
    // Match the pinned SMALL logical capacities: paragraph segmentation affects pages.
    let mut layout_text = vec![0; 3072];
    let mut breaks = vec![0; 3072];
    let mut levels = vec![0; 3072];
    let mut spans = vec![Span::default(); 96];
    let mut lines = vec![Line::default(); 176];
    let mut elements = vec![Element::default(); 16];
    let mut layout_runs = vec![Run::default(); 304];
    let mut images = vec![Image::default(); 8];
    let mut links = vec![Link::default(); 16];
    let mut page = vec![0; 7 * 1024];
    let mut style_text = vec![0; 3 * 1024];
    let mut parlinks = vec![ParLink::default(); 6];
    let mut rule_text = vec![0; styles::RULE_BYTES];
    let mut marks = vec![0; styles::RULE_BYTES / 4];
    let mut batch = vec![0; styles::RULE_BYTES];
    let mut sdk = vec![CssRule::default(); styles::SDK_BATCH];
    let mut pending = vec![CssRule::default(); styles::MAX_RULES];
    let mut css_chunk = [0; 256];
    let mut image_path = [0; 1024];
    let mut breaker = linebreak::NativeBreak;
    {
        let mut r = include!("../../tests/phased_reader/host_resources.rs");
        let mut sr = StyleResources {
            directory: &mut directory,
            paths: &mut paths,
            build: Some(styles::BuildScratch {
                text: rule_text.as_mut_slice().try_into().unwrap(),
                marks: marks.as_mut_slice().try_into().unwrap(),
                batch: batch.as_mut_slice().try_into().unwrap(),
                sdk: sdk.as_mut_slice().try_into().unwrap(),
                pending: pending.as_mut_slice().try_into().unwrap(),
                chunk: &mut css_chunk,
            }),
        };
        let mut lr = LayoutResources {
            storage: Storage {
                text: &mut layout_text,
                breaks: &mut breaks,
                levels: &mut levels,
                spans: &mut spans,
                lines: &mut lines,
                elements: &mut elements,
                runs: &mut layout_runs,
                images: &mut images,
                links: &mut links,
                page: &mut page,
                style_text: &mut style_text,
                paragraph_links: &mut parlinks,
            },
            breaker: &mut breaker,
            images: CataloglessImages {
                directory: b"OPS",
                path_scratch: &mut image_path,
            },
        };
        let opened = reader.open(
            &mut source,
            &mut metadata,
            &mut parse,
            &mut r,
            &mut toc,
            &mut sr,
            &mut sources,
            &mut slot,
            Some(&lease),
        );
        if matches!(case, Case::Oracle | Case::MenuOracle { .. }) {
            receipt("open", opened, &reader, &frame);
            if opened != Status::Ok {
                return;
            }
            macro_rules! page {
                ($n:expr) => {
                    reader.show(
                        &mut source,
                        0,
                        $n,
                        Target::Page,
                        &mut frame,
                        &mut metadata,
                        &mut parse,
                        &mut r,
                        &mut toc,
                        &mut sr,
                        &mut sources,
                        &mut slot,
                        &mut lr,
                        Some(&lease),
                    )
                };
            }
            let status = page!(0);
            receipt("first", status, &reader, &frame);
            if let Case::MenuOracle { anchors } = case {
                for (op, focus) in [
                    ("natural", u32::MAX),
                    ("same", 2),
                    ("last", 6),
                    ("return", u32::MAX),
                    ("clamp", u32::MAX - 1),
                ] {
                    let status = reader.prepare_chapter_menu(
                        &mut source,
                        focus,
                        &mut metadata,
                        &mut parse,
                        &mut r,
                        &mut toc,
                        sr.directory,
                        &mut mapping,
                        Some(&lease),
                    );
                    menu_receipt(op, status, &reader);
                    let status = reader.repaint(&mut source, &mut frame);
                    receipt(op, status, &reader, &frame);
                }
                if anchors {
                    for (op, toc_index) in [("anchor0", 0), ("anchor1", 1)] {
                        let status = reader.show_chapter(
                            &mut source,
                            0,
                            toc_index,
                            &mut frame,
                            &mut metadata,
                            &mut parse,
                            &mut r,
                            &mut toc,
                            &mut sr,
                            &mut sources,
                            &mut slot,
                            &mut lr,
                            Some(&lease),
                        );
                        receipt(op, status, &reader, &frame);
                        let status = reader.prepare_chapter_menu(
                            &mut source,
                            u32::MAX,
                            &mut metadata,
                            &mut parse,
                            &mut r,
                            &mut toc,
                            sr.directory,
                            &mut mapping,
                            Some(&lease),
                        );
                        menu_receipt(op, status, &reader);
                        let status = reader.repaint(&mut source, &mut frame);
                        receipt(&format!("{op}-repaint"), status, &reader, &frame);
                    }
                }
                return;
            }
            let status = reader.show_chapter(
                &mut source,
                0,
                0,
                &mut frame,
                &mut metadata,
                &mut parse,
                &mut r,
                &mut toc,
                &mut sr,
                &mut sources,
                &mut slot,
                &mut lr,
                Some(&lease),
            );
            receipt("jump", status, &reader, &frame);
            let status = reader.repaint(&mut source, &mut frame);
            receipt("after-jump-repaint", status, &reader, &frame);
            let status = page!(u32::MAX);
            receipt("invalid", status, &reader, &frame);
            let status = page!(0);
            receipt("rebuilt", status, &reader, &frame);
            let status = reader.repaint(&mut source, &mut frame);
            receipt("repaint", status, &reader, &frame);
            return;
        }
        if matches!(case, Case::OptionalLiveFault) {
            assert_eq!(opened, Status::IoError);
            assert!(
                source.failed_reads > 0,
                "optional navigation must reach the armed read"
            );
            assert!(!reader.opened());
            return;
        }
        assert_eq!(opened, Status::Ok);
        let before_creates = r.backend.creates;
        let first = reader.show(
            &mut source,
            0,
            0,
            Target::Page,
            &mut frame,
            &mut metadata,
            &mut parse,
            &mut r,
            &mut toc,
            &mut sr,
            &mut sources,
            &mut slot,
            &mut lr,
            Some(&lease),
        );
        if matches!(case, Case::LateMalformed) {
            assert_eq!(first, Status::ParseError);
            assert!(!reader.has_page());
            let entry = reader.selection().entry;
            assert_eq!(
                validate_exact_anchor(&mut source, &entry, b"absent", &mut r, None),
                Status::ParseError
            );
            return;
        }
        assert_eq!(first, Status::Ok);
        if matches!(case, Case::Menu) {
            use newt_reader_navigation::FOCUS_CURRENT;
            let before_info = reader.info();
            let before_page = frame.to_vec();
            let before_href = reader.selection().href().to_vec();
            let before_title = reader.selection().title().to_vec();
            let before_entry = reader.selection().entry;
            let before_spines = reader.selection().spines;
            macro_rules! section_identity {
                () => {{
                    let s = reader.current_section();
                    (
                        s.label.to_vec(),
                        s.href.to_vec(),
                        s.fragment.to_vec(),
                        s.spine,
                        s.char_start,
                        s.toc,
                        s.depth,
                        s.generic,
                        s.inherited,
                        s.target_known,
                        s.anchor_known,
                    )
                }};
            }
            let before_section = section_identity!();
            assert_eq!(reader.current_section().toc, 0);
            macro_rules! menu {
                ($focus:expr, $toc:expr, $map:expr, $lease:expr) => {{
                    let status = reader.prepare_chapter_menu(
                        &mut source,
                        $focus,
                        &mut metadata,
                        &mut parse,
                        &mut r,
                        $toc,
                        sr.directory,
                        $map,
                        $lease,
                    );
                    assert_eq!(reader.info(), before_info);
                    assert_eq!(reader.selection().href(), before_href);
                    assert_eq!(reader.selection().title(), before_title);
                    assert_eq!(reader.selection().entry, before_entry);
                    assert_eq!(reader.selection().spines, before_spines);
                    assert_eq!(section_identity!(), before_section);
                    assert!(reader.has_retained_page_plan());
                    assert_eq!(&frame[..], before_page);
                    status
                }};
            }
            assert_eq!(
                menu!(FOCUS_CURRENT, &mut toc, &mut mapping, Some(&lease)),
                Status::Ok
            );
            let info = reader.chapter_menu().info();
            assert!(info.valid && info.authored && info.current_exact);
            assert_eq!(
                (info.count, info.focus, info.first, info.visible),
                (7, 0, 0, 5)
            );
            // Literal fixture expectations, never recomputed by menu policy.
            for (index, label) in [b"A0", b"A1", b"A2", b"A3", b"A4"].iter().enumerate() {
                let menu = reader.chapter_menu();
                let row = menu.row(index as u32);
                assert!(row.valid && !row.generic && !row.fragment);
                assert_eq!(row.label, *label);
                assert_eq!(
                    (row.spine, row.toc, row.current),
                    (index as u32, index as u16, index == 0)
                );
            }
            assert!(!reader.chapter_menu().row(5).valid);
            let reads = source.reads;
            let creates = r.backend.creates;
            let returns = RETURNS.load(SeqCst);
            source.probe_fail = true;
            assert_eq!(menu!(2, &mut [], &mut [], None), Status::Ok);
            assert_eq!(
                (source.reads, r.backend.creates, RETURNS.load(SeqCst)),
                (reads, creates, returns)
            );
            source.probe_fail = false;
            assert_eq!(menu!(6, &mut toc, &mut mapping, Some(&lease)), Status::Ok);
            assert_eq!(reader.chapter_menu().info().first, 2);
            assert!(source.reads > reads && r.backend.creates > creates);
            assert_eq!(RETURNS.load(SeqCst), returns + 1);
            {
                let menu = reader.chapter_menu();
                let row = menu.row(6);
                assert_eq!((row.label, row.spine, row.toc), (&b"A6"[..], 6, 6));
                assert!(!row.current);
            }
            assert_eq!(menu!(0, &mut toc, &mut mapping, None), Status::OutOfMemory);
            assert!(!reader.chapter_menu().info().valid);
            let denied = Lease {
                acquire: || false,
                returned: || panic!("denied lease returned"),
            };
            assert_eq!(
                menu!(0, &mut toc, &mut mapping, Some(&denied)),
                Status::OutOfMemory
            );
            // Canonical metadata exhaustion is hard OOM, not optional fallback.
            let mut empty = [];
            let mut empty_metadata = Arena::new(&mut empty);
            assert_eq!(
                reader.prepare_chapter_menu(
                    &mut source,
                    0,
                    &mut empty_metadata,
                    &mut parse,
                    &mut r,
                    &mut toc,
                    sr.directory,
                    &mut mapping,
                    Some(&lease),
                ),
                Status::OutOfMemory
            );
            assert!(!reader.chapter_menu().info().valid);
            // A successfully parsed optional TOC needs mapping scratch. Pinned
            // source overload returns Ok with an invalid menu on its exhaustion.
            assert_eq!(menu!(0, &mut toc, &mut [], Some(&lease)), Status::Ok);
            assert!(!reader.chapter_menu().info().valid);
            // Optional TOC output exhaustion degrades to generic rows instead.
            assert_eq!(menu!(0, &mut [], &mut [], Some(&lease)), Status::Ok);
            let generic = reader.chapter_menu().info();
            assert!(generic.valid && !generic.authored && !generic.current_exact);
            assert_eq!(generic.count, 7);
            {
                let menu = reader.chapter_menu();
                let row = menu.row(0);
                assert_eq!(row.label, b"Section 1");
                assert!(row.generic && !row.current);
            }
            // Force a new window before arming navigation failure.
            let at = locate(&source.bytes, b"<html xmlns:epub");
            source.fail = Some((at as u64, (at + 1) as u64));
            assert_eq!(
                menu!(6, &mut toc, &mut mapping, Some(&lease)),
                Status::IoError
            );
            assert!(source.failed_reads > 0);
            assert!(!reader.chapter_menu().info().valid);
            source.fail = None;
            source.bytes.push(0);
            assert_eq!(
                menu!(0, &mut toc, &mut mapping, Some(&lease)),
                Status::Stale
            );
            source.bytes.pop();
            let faulty = Lease {
                acquire,
                returned: fault_returned,
            };
            FAIL_AFTER_RETURN.store(true, SeqCst);
            assert_eq!(
                menu!(0, &mut toc, &mut mapping, Some(&faulty)),
                Status::IoError
            );
            PROBE_FAULT.store(false, SeqCst);
            assert!(!reader.chapter_menu().info().valid);
            assert_eq!(menu!(0, &mut toc, &mut mapping, Some(&lease)), Status::Ok);
            assert_eq!(
                reader.show(
                    &mut source,
                    0,
                    0,
                    Target::Page,
                    &mut frame,
                    &mut metadata,
                    &mut parse,
                    &mut r,
                    &mut toc,
                    &mut sr,
                    &mut sources,
                    &mut slot,
                    &mut lr,
                    Some(&lease)
                ),
                Status::Ok
            );
            assert!(
                !reader.chapter_menu().info().valid,
                "layout replaces menu projection"
            );
            let creates = r.backend.creates;
            assert_eq!(menu!(0, &mut toc, &mut mapping, Some(&lease)), Status::Ok);
            assert!(
                r.backend.creates > creates,
                "layout must invalidate menu reuse"
            );
            assert_eq!(reader.info(), before_info);
            assert_eq!(reader.selection().href(), before_href);
            assert!(reader.has_retained_page_plan());
            assert!(!LIVE.load(SeqCst) && !HELD.load(SeqCst));
            assert_eq!(&frame[..], before_page);
            drop(lr);
            drop(sr);
            drop(r);
            assert_eq!(reader.repaint(&mut source, &mut frame), Status::Ok);
            assert_eq!(&frame[..], before_page);
            reader.close();
            assert!(!reader.chapter_menu().info().valid);
            assert!(reader.current_section().generic);
            return;
        }
        if matches!(case, Case::Deferred | Case::Pending) {
            assert_eq!(slot.state, StyleState::Settled);
            assert!(
                r.backend.creates > before_creates + 1,
                "stopped pass must retry actual XML"
            );
        }
        if matches!(case, Case::Duplicate | Case::Collision) {
            let entry = reader.selection().entry;
            let wanted: &[u8] = if matches!(case, Case::Duplicate) {
                b"start"
            } else {
                b"costarring"
            };
            assert_eq!(
                validate_exact_anchor(&mut source, &entry, wanted, &mut r, None),
                Status::NotFound
            );
        }
        let previous = reader.info();
        let previous_frame = frame.to_vec();
        if matches!(case, Case::Duplicate | Case::Collision) {
            assert_eq!(
                reader.show_chapter(
                    &mut source,
                    0,
                    0,
                    &mut frame,
                    &mut metadata,
                    &mut parse,
                    &mut r,
                    &mut toc,
                    &mut sr,
                    &mut sources,
                    &mut slot,
                    &mut lr,
                    Some(&lease)
                ),
                Status::NotFound
            );
            assert_eq!(reader.info(), previous);
            assert_eq!(reader.repaint(&mut source, &mut frame), Status::Ok);
            assert_eq!(&frame[..], previous_frame);
        }
        if matches!(case, Case::CrossSpine) {
            EXACT_ACQUIRES.store(0, SeqCst);
            let denied_exact = Lease {
                acquire: exact_denied,
                returned,
            };
            assert_eq!(
                reader.show_chapter(
                    &mut source,
                    1,
                    1,
                    &mut frame,
                    &mut metadata,
                    &mut parse,
                    &mut r,
                    &mut toc,
                    &mut sr,
                    &mut sources,
                    &mut slot,
                    &mut lr,
                    Some(&denied_exact)
                ),
                Status::OutOfMemory
            );
            assert_eq!(
                slot.spine, 1,
                "resolve must have selected the second chapter"
            );
            assert_eq!(reader.info(), previous);
            assert_eq!(reader.repaint(&mut source, &mut frame), Status::Ok);
            assert_eq!(&frame[..], previous_frame);
            assert_eq!(
                reader.show(
                    &mut source,
                    0,
                    0,
                    Target::Page,
                    &mut frame,
                    &mut metadata,
                    &mut parse,
                    &mut r,
                    &mut toc,
                    &mut sr,
                    &mut sources,
                    &mut slot,
                    &mut lr,
                    Some(&lease)
                ),
                Status::Ok
            );
            assert_eq!(slot.spine, 0);
            assert_eq!(reader.info(), previous);
            assert_eq!(&frame[..], previous_frame);
        }
        if matches!(case, Case::Plain) {
            EXACT_ACQUIRES.store(0, SeqCst);
            let denied_exact = Lease {
                acquire: exact_denied,
                returned,
            };
            assert_eq!(
                reader.show_chapter(
                    &mut source,
                    0,
                    0,
                    &mut frame,
                    &mut metadata,
                    &mut parse,
                    &mut r,
                    &mut toc,
                    &mut sr,
                    &mut sources,
                    &mut slot,
                    &mut lr,
                    Some(&denied_exact)
                ),
                Status::OutOfMemory
            );
            assert_eq!(reader.repaint(&mut source, &mut frame), Status::Ok);
            assert_eq!(&frame[..], previous_frame);
            FAIL_AFTER_RETURN.store(true, SeqCst);
            let live_fault = Lease {
                acquire,
                returned: fault_returned,
            };
            assert_eq!(
                reader.show_chapter(
                    &mut source,
                    0,
                    0,
                    &mut frame,
                    &mut metadata,
                    &mut parse,
                    &mut r,
                    &mut toc,
                    &mut sr,
                    &mut sources,
                    &mut slot,
                    &mut lr,
                    Some(&live_fault)
                ),
                Status::IoError
            );
            PROBE_FAULT.store(false, SeqCst);
            assert_eq!(reader.repaint(&mut source, &mut frame), Status::Ok);
            assert_eq!(&frame[..], previous_frame);
        }
        // Exact validation preflight failure also leaves the old capture intact.
        // Failed rebuild invalidates it; public receipt and selection survive.
        let denied = Lease {
            acquire: || false,
            returned: || panic!("denied release"),
        };
        assert_eq!(
            reader.show(
                &mut source,
                0,
                0,
                Target::Page,
                &mut frame,
                &mut metadata,
                &mut parse,
                &mut r,
                &mut toc,
                &mut sr,
                &mut sources,
                &mut slot,
                &mut lr,
                Some(&denied)
            ),
            Status::OutOfMemory
        );
        assert_eq!(reader.info(), previous);
        assert_eq!(reader.repaint(&mut source, &mut frame), Status::Unsupported);
        // Exercise typed layout shortage in production, then restore/rebuild.
        macro_rules! shortage {
            ($field:ident) => {{
                let saved = std::mem::take(&mut lr.storage.$field);
                assert_eq!(
                    reader.show(
                        &mut source,
                        0,
                        0,
                        Target::Page,
                        &mut frame,
                        &mut metadata,
                        &mut parse,
                        &mut r,
                        &mut toc,
                        &mut sr,
                        &mut sources,
                        &mut slot,
                        &mut lr,
                        Some(&lease)
                    ),
                    Status::OutOfMemory
                );
                lr.storage.$field = saved;
                assert_eq!(reader.info(), previous);
            }};
        }
        shortage!(text);
        shortage!(breaks);
        shortage!(levels);
        shortage!(spans);
        shortage!(lines);
        shortage!(elements);
        shortage!(runs);
        shortage!(images);
        shortage!(links);
        shortage!(page);
        shortage!(paragraph_links);
        assert_eq!(reader.info(), previous);
        assert_eq!(
            reader.show(
                &mut source,
                0,
                0,
                Target::Page,
                &mut frame,
                &mut metadata,
                &mut parse,
                &mut r,
                &mut toc,
                &mut sr,
                &mut sources,
                &mut slot,
                &mut lr,
                Some(&lease)
            ),
            Status::Ok
        );
        reader.discard_chapter_publication(previous, true);
        assert_eq!(reader.info(), previous);
        assert_eq!(reader.repaint(&mut source, &mut frame), Status::Unsupported);
        if matches!(case, Case::PrefixAnchor) {
            // Fixture is stored, long, with this marker after multiple pages.
            let at = locate(&source.bytes, b"SUFFIX_FAULT");
            source.fail = Some((at as u64, (at + 4096) as u64));
            // Rollback invalidated certification; first recertify without fault.
            source.fail = None;
            assert_eq!(
                reader.show(
                    &mut source,
                    0,
                    0,
                    Target::Page,
                    &mut frame,
                    &mut metadata,
                    &mut parse,
                    &mut r,
                    &mut toc,
                    &mut sr,
                    &mut sources,
                    &mut slot,
                    &mut lr,
                    Some(&lease)
                ),
                Status::Ok
            );
            source.fail = Some((at as u64, (at + 4096) as u64));
            assert_eq!(
                reader.show(
                    &mut source,
                    0,
                    0,
                    Target::Anchor,
                    &mut frame,
                    &mut metadata,
                    &mut parse,
                    &mut r,
                    &mut toc,
                    &mut sr,
                    &mut sources,
                    &mut slot,
                    &mut lr,
                    Some(&lease)
                ),
                Status::Ok
            );
            source.fail = None;
        }
        let returns = RETURNS.load(SeqCst);
        let before_capture = reader.info();
        let capture_page = u32::from(before_capture.page_count > 1);
        frame.fill(0x5a);
        assert_eq!(
            reader.capture(
                &mut source,
                0,
                capture_page,
                Target::Page,
                &mut metadata,
                &mut parse,
                &mut r,
                &mut toc,
                &mut sr,
                &mut sources,
                &mut slot,
                &mut lr,
                Some(&lease)
            ),
            Status::Ok
        );
        assert_eq!(reader.info(), before_capture, "capture must not publish");
        assert!(frame.iter().all(|b| *b == 0x5a), "capture must not paint");
        assert_eq!(RETURNS.load(SeqCst), returns + 1);
        assert!(!HELD.load(SeqCst) && !LIVE.load(SeqCst));
    } // actual typed phase views are dropped before any paint loan
    drop(backend);
    drop(metadata);
    drop(parse);
    drop(paths);
    assert_eq!(reader.publish(&mut source, &mut frame), Status::Ok);
    let captured = frame.to_vec();
    frame.fill(0x5a);
    assert_eq!(reader.repaint(&mut source, &mut frame), Status::Ok);
    assert_eq!(&frame[..], captured);
}
