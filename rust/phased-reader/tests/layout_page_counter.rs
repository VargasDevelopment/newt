// SPDX-License-Identifier: MIT
//! Focused host test for PhasedReader's layout-page counter across open,
//! early-stop miss passes, cache hits, and ReaderState round-trips (#168),
//! the page cache with non-default display preferences (#187), and the work
//! phases a Contents anchor jump reads the book in (#188).
use newt_core::{
    chapter_layout::types::LineBreak,
    page_cache::CacheStorage,
    preferences::{Orientation, ViewSettings},
    xml_session::{Attributes, Handler, ParserBackend},
    zip_container::Status,
    zip_window::source::BookSource,
};
use newt_phased_reader::{
    CachePresence, CacheRequest, OperationPhased, PageInfo, PhasedChapterCache, PhasedReader,
    Selection,
    hash_provider::CacheHash,
    imported_revision::Digest,
    phase_storage::{BANK_BYTES, BuildLoan, ReadScope, Storage as PhaseStorage},
};
use newt_phased_sink::{FRAME_BYTES, Frame, Plan, RUNS, Run, TEXT, Target};
use std::collections::BTreeMap;

#[repr(align(16))]
struct Bank([u8; BANK_BYTES]);

#[repr(align(16))]
struct PlanText([u8; TEXT]);

struct VecSource(Vec<u8>);
impl newt_core::producer_policy::ProducerReceipt for VecSource {
    fn producer_receipt_live(&mut self) -> bool {
        true
    }
}
impl BookSource for VecSource {
    fn size(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        if out.is_empty() || at >= self.0.len() as u64 {
            return 0;
        }
        let start = at as usize;
        let n = out.len().min(self.0.len() - start);
        out[..n].copy_from_slice(&self.0[start..start + n]);
        n as i32
    }
}

struct HostLoan<'a>(&'a mut [u8]);
impl BuildLoan for HostLoan<'_> {
    fn with_parse<R>(&mut self, op: impl for<'a> FnOnce(&'a mut [u8]) -> R) -> Result<R, Status> {
        let out = op(self.0);
        self.0.fill(0xff);
        Ok(out)
    }
}

struct SliceAttrs<'a> {
    items: Vec<(&'a [u8], &'a [u8])>,
    pos: usize,
}
impl<'a> Attributes for SliceAttrs<'a> {
    fn next(&mut self) -> Option<(&[u8], &[u8])> {
        let item = *self.items.get(self.pos)?;
        self.pos += 1;
        Some(item)
    }
    fn local(&self, name: &[u8]) -> Option<&[u8]> {
        self.items.iter().find_map(|&(k, v)| {
            let local = k.rsplit(|&b| b == b':').next().unwrap_or(k);
            (local == name).then_some(v)
        })
    }
    fn exact(&self, name: &[u8]) -> Option<&[u8]> {
        self.items.iter().find_map(|&(k, v)| (k == name).then_some(v))
    }
}

#[derive(Default)]
struct MiniXml {
    buf: Vec<u8>,
    consumed: i64,
}
impl ParserBackend for MiniXml {
    fn create(&mut self) -> bool {
        self.buf.clear();
        self.consumed = 0;
        true
    }
    fn producer_backend(&mut self) -> Option<&mut dyn newt_core::xml_session::ProducerBackend> {
        Some(self)
    }
    fn feed(&mut self, bytes: &[u8], last: bool, h: &mut dyn Handler) -> bool {
        self.buf.extend_from_slice(bytes);
        let mut i = 0usize;
        let data = core::mem::take(&mut self.buf);
        while i < data.len() {
            if data[i] == b'<' {
                let Some(rel) = data[i..].iter().position(|&b| b == b'>') else {
                    if !last {
                        self.buf.extend_from_slice(&data[i..]);
                        self.consumed += i as i64;
                        return true;
                    }
                    return false;
                };
                let tag = &data[i + 1..i + rel];
                i += rel + 1;
                if tag.starts_with(b"?") || tag.starts_with(b"!") {
                    continue;
                }
                if let Some(name) = tag.strip_prefix(b"/") {
                    h.end_element(name.trim_ascii());
                    continue;
                }
                let self_close = tag.ends_with(b"/");
                let body = if self_close { &tag[..tag.len() - 1] } else { tag }.trim_ascii();
                let split = body.iter().position(|b| b.is_ascii_whitespace()).unwrap_or(body.len());
                let name = &body[..split];
                let mut rest = &body[split..];
                let mut attrs = Vec::new();
                while !rest.is_empty() {
                    rest = rest.trim_ascii_start();
                    if rest.is_empty() { break; }
                    let Some(eq) = rest.iter().position(|&b| b == b'=') else { break; };
                    let key = rest[..eq].trim_ascii();
                    rest = rest[eq + 1..].trim_ascii_start();
                    if rest.is_empty() { break; }
                    let quote = rest[0];
                    rest = &rest[1..];
                    let Some(end_q) = rest.iter().position(|&b| b == quote) else { break; };
                    attrs.push((key, &rest[..end_q]));
                    rest = &rest[end_q + 1..];
                }
                let mut iter = SliceAttrs { items: attrs, pos: 0 };
                h.start_element(name, &mut iter);
                if self_close {
                    h.end_element(name);
                }
            } else {
                let next = data[i..].iter().position(|&b| b == b'<').unwrap_or(data.len() - i);
                h.text(&data[i..i + next]);
                i += next;
            }
        }
        self.consumed += i as i64;
        true
    }
    fn close(&mut self) {
        self.buf.clear();
        self.consumed = 0;
    }
}
impl newt_core::xml_session::ProducerBackend for MiniXml {
    fn create_producer(&mut self) -> bool {
        self.create()
    }
    fn current_byte_index(&self) -> i64 {
        self.consumed
    }
}

struct SimpleBreak;
impl LineBreak for SimpleBreak {
    fn breaks(&mut self, text: &[u8], _: Option<&core::ffi::CStr>, out: &mut [u8]) {
        for (i, &b) in text.iter().enumerate() {
            out[i] = if b == b'\n' {
                2
            } else if b == b' ' {
                1
            } else {
                0
            };
        }
    }
}

#[derive(Default)]
struct MemCache {
    files: BTreeMap<Vec<u8>, Vec<u8>>,
    pending: Option<(Vec<u8>, Vec<u8>)>,
}
fn c_key(name: &[u8]) -> Vec<u8> {
    name.split(|&b| b == 0).next().unwrap_or(&[]).to_vec()
}
impl CacheStorage for MemCache {
    fn exists(&mut self, name: &[u8]) -> bool {
        self.files.contains_key(&c_key(name))
    }
    fn remove(&mut self, name: &[u8]) -> bool {
        let key = c_key(name);
        if self.pending.as_ref().is_some_and(|(n, _)| n == &key) {
            self.pending = None;
        }
        self.files.remove(&key);
        true
    }
    fn file_size(&mut self, name: &[u8]) -> i64 {
        self.files
            .get(&c_key(name))
            .map_or(-1, |v| v.len() as i64)
    }
    fn read_at(&mut self, name: &[u8], offset: u32, dst: &mut [u8]) -> i32 {
        let Some(data) = self.files.get(&c_key(name)) else {
            return -1;
        };
        let start = (offset as usize).min(data.len());
        let n = dst.len().min(data.len() - start);
        dst[..n].copy_from_slice(&data[start..start + n]);
        n as i32
    }
    fn begin_write(&mut self, name: &[u8]) -> bool {
        self.pending = Some((c_key(name), Vec::new()));
        true
    }
    fn write(&mut self, bytes: &[u8]) -> bool {
        let Some((_, buf)) = self.pending.as_mut() else {
            return false;
        };
        buf.extend_from_slice(bytes);
        true
    }
    fn end_write(&mut self) -> bool {
        let Some((name, buf)) = self.pending.take() else {
            return false;
        };
        self.files.insert(name, buf);
        true
    }
}
impl CachePresence for MemCache {
    fn present(&mut self, name: &[u8], _opening: bool) -> bool {
        self.exists(name)
    }
    fn with_index<R>(
        &mut self,
        _: &[u8; 32],
        _: impl FnOnce(&mut dyn newt_phased_reader::index_build::Store) -> R,
    ) -> Option<R> {
        None
    }
}
impl ReadScope for MemCache {
    fn query<R>(
        &mut self,
        name: &[u8],
        _opening: bool,
        _scratch: &mut [u8],
        source_live: &mut dyn FnMut() -> bool,
        read: impl FnOnce(&mut dyn CacheStorage) -> R,
    ) -> Result<(R, bool), Status> {
        if !self.exists(name) {
            return Err(Status::NotFound);
        }
        if !source_live() {
            return Err(Status::Stale);
        }
        let value = read(self);
        Ok((value, source_live()))
    }
}

struct Hash64;
impl CacheHash for Hash64 {
    fn hash(&mut self, bytes: &[u8], out: &mut Digest) -> bool {
        let mut a = 0xcbf29ce484222325u64;
        let mut b = 0x100000001b3u64;
        for &byte in bytes {
            a = (a ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            b = (b ^ u64::from(byte).wrapping_add(17)).wrapping_mul(0x9e3779b97f4a7c15);
        }
        out[..8].copy_from_slice(&a.to_le_bytes());
        out[8..16].copy_from_slice(&b.to_le_bytes());
        out[16..24].copy_from_slice(&a.rotate_left(19).to_le_bytes());
        out[24..32].copy_from_slice(&b.rotate_right(13).to_le_bytes());
        true
    }
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut local = Vec::new();
    let mut central = Vec::new();
    for &(name, data) in entries {
        let offset = local.len() as u32;
        let crc = crc32(data);
        let len = data.len() as u32;
        let nlen = name.len() as u16;
        local.extend_from_slice(&0x04034b50u32.to_le_bytes());
        local.extend_from_slice(&20u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&crc.to_le_bytes());
        local.extend_from_slice(&len.to_le_bytes());
        local.extend_from_slice(&len.to_le_bytes());
        local.extend_from_slice(&nlen.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(name.as_bytes());
        local.extend_from_slice(data);

        central.extend_from_slice(&0x02014b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&len.to_le_bytes());
        central.extend_from_slice(&len.to_le_bytes());
        central.extend_from_slice(&nlen.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let cd_offset = local.len() as u32;
    let cd_size = central.len() as u32;
    let count = entries.len() as u16;
    let mut out = local;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn synthetic_epub(paragraphs: usize) -> Vec<u8> {
    let mut ch = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <html xmlns=\"http://www.w3.org/1999/xhtml\" lang=\"en\"><head><title>Ch 1</title></head><body><h1>Chapter 1</h1>",
    );
    for i in 0..paragraphs {
        ch.push_str(&format!(
            "<p>Paragraph {i}: A narrow footpath crossed the meadow toward the quiet creek while clear ripples met the reeds along the willow bank.</p>"
        ));
    }
    ch.push_str("</body></html>");
    let container = b"<?xml version=\"1.0\"?><container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>";
    let opf = b"<?xml version=\"1.0\"?><package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"u\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:identifier id=\"u\">urn:test:1</dc:identifier><dc:title>Test</dc:title><dc:language>en</dc:language></metadata><manifest><item id=\"c0\" href=\"c0.xhtml\" media-type=\"application/xhtml+xml\"/></manifest><spine><itemref idref=\"c0\"/></spine></package>";
    stored_zip(&[
        ("mimetype", b"application/epub+zip"),
        ("META-INF/container.xml", container),
        ("OEBPS/content.opf", opf),
        ("OEBPS/c0.xhtml", ch.as_bytes()),
    ])
}

#[test]
fn layout_page_counter_tracks_open_early_stop_misses_and_zero_layout_cache_hits() {
    let epub = synthetic_epub(60);
    let mut source = VecSource(epub);
    let mut shared = Box::new(Bank([0; BANK_BYTES]));
    let mut parse = vec![0u8; FRAME_BYTES];
    let mut storage = PhaseStorage::new(&mut shared.0);
    let mut loan = HostLoan(&mut parse);
    let mut parser = MiniXml::default();
    let mut breaker = SimpleBreak;
    let mut runs = Box::new([Run::default(); RUNS]);
    let mut text = Box::new(PlanText([0; TEXT]));
    let mut plan = Plan::new(&mut *runs, &mut text.0);
    let mut selections = [Selection::default(), Selection::default()];
    let mut frame: Box<Frame> = Box::new([0u8; FRAME_BYTES]);
    let settings = ViewSettings {
        orientation: Orientation::Portrait,
        ..ViewSettings::default()
    };
    let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), settings.page_layout());
    assert_eq!(reader.open_phased(&mut source, &mut storage, &mut loan, &mut parser), Status::Ok);
    assert_eq!((reader.last_layout_pages(), reader.total_layout_pages()), (0, 0));

    // Certifying open pass lays out the entire chapter.
    assert_eq!(
        reader.capture_phased(&mut source, 0, 0, Target::Page, &mut storage, &mut loan, &mut parser, &mut breaker),
        Status::Ok
    );
    assert_eq!(reader.publish(&mut source, &mut frame), Status::Ok);
    let total_pages = reader.info().page_count;
    assert!(total_pages >= 10, "expected multi-page chapter, got {total_pages}");
    assert_eq!(reader.last_layout_pages(), total_pages);
    assert_eq!(reader.total_layout_pages(), u64::from(total_pages));

    // Reuse miss at page p stops early at p + 1 (or slightly past on feed overshoot, bounded <= total_pages).
    let mut cache_store = MemCache::default();
    let mut hash = Hash64;
    let digest = [0x42u8; 32];
    let mut cumulative = u64::from(total_pages);
    for target in [2u32, 6u32] {
        let mut op = OperationPhased::new(&mut reader, &mut storage, &mut loan, &mut parser, &mut breaker);
        let mut cache = PhasedChapterCache { store: &mut cache_store, hash: &mut hash, settings };
        let shown = cache.capture(&mut source, &digest, CacheRequest { spine: 0, target, anchor: false, staged: false, cache_only: false, build_on_miss: false }, &mut op);
        assert_eq!(shown.status, Status::Ok);
        assert!(!shown.hit);
        assert_eq!(op.reader.publish(&mut source, &mut frame), Status::Ok);
        let laid = op.reader.last_layout_pages();
        assert!(laid >= target + 1 && laid <= total_pages, "expected early-stop pass >= {} <= {}, got {}", target + 1, total_pages, laid);
        cumulative += u64::from(laid);
        assert_eq!(op.reader.total_layout_pages(), cumulative);
    }

    // Populate cache for spine 0 via the real producer path; subsequent hit must lay out 0 pages.
    {
        use newt_core::producer_policy::{Cancellation, Hooks, Stop};
        fn never_stop(_: &()) -> Stop {
            Stop::None
        }
        let cancel = Cancellation::new(Hooks {
            context: &(),
            stop: Some(never_stop),
            yield_now: None,
            pause: None,
        });
        let info = reader.info();
        let mut route = newt_phased_reader::producer::Route {
            settings,
            digest: &digest,
            info,
            hash: &mut hash,
        };
        let produced = storage
            .with_producer(
                &mut loan,
                &cancel,
                &mut cache_store,
                &mut source,
                info.page_count,
                None,
                |producer| {
                    Ok(producer.produce_current(&mut reader, &mut parser, &mut breaker, &mut route))
                },
            )
            .unwrap();
        assert_eq!(produced, newt_core::producer_visit::CurrentOutcome::Done);
    }
    {
        let mut op = OperationPhased::new(&mut reader, &mut storage, &mut loan, &mut parser, &mut breaker);
        let mut cache = PhasedChapterCache { store: &mut cache_store, hash: &mut hash, settings };
        let shown = cache.capture(&mut source, &digest, CacheRequest { spine: 0, target: 5, anchor: false, staged: false, cache_only: false, build_on_miss: false }, &mut op);
        assert_eq!(shown.status, Status::Ok);
        assert!(shown.hit);
        assert_eq!(op.reader.publish(&mut source, &mut frame), Status::Ok);
        assert_eq!(op.reader.last_layout_pages(), 0, "cache hit must record 0 layout pages");
        assert_eq!(op.reader.total_layout_pages(), cumulative, "cache hit must not change total");
    }

    // Negative control: clearing the cache turns the exact same request back into a layout pass.
    cache_store.files.clear();
    {
        let mut op = OperationPhased::new(&mut reader, &mut storage, &mut loan, &mut parser, &mut breaker);
        let mut cache = PhasedChapterCache { store: &mut cache_store, hash: &mut hash, settings };
        let shown = cache.capture(&mut source, &digest, CacheRequest { spine: 0, target: 5, anchor: false, staged: false, cache_only: false, build_on_miss: false }, &mut op);
        assert_eq!(shown.status, Status::Ok);
        assert!(!shown.hit);
        assert_eq!(op.reader.publish(&mut source, &mut frame), Status::Ok);
        assert!(op.reader.last_layout_pages() >= 6, "negative control: miss must lay out >= 6 pages");
        cumulative += u64::from(op.reader.last_layout_pages());
        assert_eq!(op.reader.total_layout_pages(), cumulative);
    }

    // ReaderState/ReaderView round-trip preserves counts.
    let expected_last = reader.last_layout_pages();
    let expected_total = reader.total_layout_pages();
    assert!(expected_last >= 6);
    let state = reader.into_state();
    assert_eq!(state.last_layout_pages(), expected_last);
    assert_eq!(state.total_layout_pages(), expected_total);
    assert_eq!(state.view(&selections).last_layout_pages(), expected_last);
    assert_eq!(state.view(&selections).total_layout_pages(), expected_total);

    // Rejected explicit-TOC capture_chapter on a planless reader must reset last_layout_pages
    // while preserving total_layout_pages.
    let mut planless = state.lend(&mut selections, None);
    assert_eq!(planless.last_layout_pages(), expected_last);
    {
        use newt_core::chapter_layout::types::{Element, Image, Line, Link, ParLink, Run, Span, Storage};
        use newt_core::css::CssRule;
        use newt_core::package_parsers::{ManifestItem, TocEntry};
        use newt_core::zip_container::Entry;
        use newt_core::zip_window::arena::Arena;
        use newt_phased_reader::canonical;
        use newt_phased_reader::styles::{Member, Slot, Sources, State};
        use newt_phased_reader::{CataloglessImages, LayoutResources, StyleResources};
        let mut meta_buf = [0u8; 64];
        let mut parse_buf = [0u8; 64];
        let mut path_buf = [0u8; 64];
        let mut img_path = [0u8; 64];
        let mut dir_buf = [0u8; 512];
        let mut scan_buf = [0u8; 4099];
        let mut seq_buf = [0u8; 1024];
        let mut entries: [Entry; 0] = [];
        let mut manifest: [ManifestItem; 0] = [];
        let mut spine_hashes: [u32; 0] = [];
        let mut spine_ids: [u16; 0] = [];
        let mut text = [0u8; 1];
        let mut breaks = [0u8; 1];
        let mut levels = [0u8; 1];
        let mut spans: [Span; 0] = [];
        let mut lines: [Line; 0] = [];
        let mut elements: [Element; 0] = [];
        let mut runs: [Run; 0] = [];
        let mut images: [Image; 0] = [];
        let mut links: [Link; 0] = [];
        let mut page = [0u8; 1];
        let mut style_text = [0u8; 1];
        let mut src_text = [0u8; 1];
        let mut paragraph_links: [ParLink; 0] = [];
        let mut rules = [CssRule::default(); 64];
        let mut meta_arena = Arena::new(&mut meta_buf);
        let mut parse_arena = Arena::new(&mut parse_buf);
        let mut path_arena = Arena::new(&mut path_buf);
        let mut res = canonical::Resources {
            policy: None,
            scan: &mut scan_buf,
            sequential: &mut seq_buf,
            entries: &mut entries,
            manifest: &mut manifest,
            spine_hashes: &mut spine_hashes,
            spine: &mut spine_ids,
            backend: &mut parser,
            decoder: None,
            chunk: None,
            xml_buffer: None,
            filter: None,
        };
        let mut toc: [TocEntry; 0] = [];
        let mut sr = StyleResources {
            directory: &mut dir_buf,
            paths: &mut path_arena,
            build: None,
        };
        let mut sources = Sources {
            links: [Member::default(); 4],
            start: [0; 8],
            length: [0; 8],
            issues: 0,
            used: 0,
            events: 0,
            external_mask: 0,
            text: &mut src_text,
        };
        let mut slot = Slot {
            rules: &mut rules,
            count: 0,
            hash: 0,
            spine: 0,
            issues: 0,
            state: State::Empty,
        };
        let mut lr = LayoutResources {
            storage: Storage {
                text: &mut text,
                breaks: &mut breaks,
                levels: &mut levels,
                spans: &mut spans,
                lines: &mut lines,
                elements: &mut elements,
                runs: &mut runs,
                images: &mut images,
                links: &mut links,
                page: &mut page,
                style_text: &mut style_text,
                paragraph_links: &mut paragraph_links,
            },
            breaker: &mut breaker,
            images: CataloglessImages {
                directory: &[],
                path_scratch: &mut img_path,
            },
        };
        let rejected = planless.capture_chapter(
            &mut source,
            0,
            0,
            &mut meta_arena,
            &mut parse_arena,
            &mut res,
            &mut toc,
            &mut sr,
            &mut sources,
            &mut slot,
            &mut lr,
            None,
        );
        assert_eq!(rejected, Status::Unsupported);
    }
    assert_eq!(planless.last_layout_pages(), 0, "rejected capture_chapter must reset last_layout_pages");
    assert_eq!(planless.total_layout_pages(), expected_total, "rejected capture_chapter must preserve total");

    // Rejected capture_chapter_owned on a provisional revision must also reset last_layout_pages
    // while preserving total_layout_pages.
    let mut relended = planless.into_state().lend(&mut selections, Some(&mut plan));
    {
        let mut op = OperationPhased::new(&mut relended, &mut storage, &mut loan, &mut parser, &mut breaker);
        let mut cache = PhasedChapterCache { store: &mut cache_store, hash: &mut hash, settings };
        let shown = cache.capture(
            &mut source,
            &digest,
            CacheRequest { spine: 0, target: 5, anchor: false, staged: false, cache_only: false, build_on_miss: false },
            &mut op,
        );
        assert_eq!(shown.status, Status::Ok);
        assert!(op.reader.last_layout_pages() >= 6);
        cumulative += u64::from(op.reader.last_layout_pages());
        assert_eq!(op.reader.total_layout_pages(), cumulative);
    }
    {
        use newt_phased_reader::imported_revision::session::Session;
        use newt_phased_reader::imported_revision::{
            BLOCK_BYTES, Digest, ImportedRevision, RevisionHasher, RevisionStorage,
        };
        struct MemRevStore {
            data: Vec<u8>,
            generation: u64,
        }
        impl RevisionStorage for MemRevStore {
            fn load(&mut self, _: &mut u32) -> bool { false }
            fn begin(&mut self, bytes: u32) -> bool {
                self.data.clear();
                self.data.resize(bytes as usize, 0);
                self.generation += 1;
                true
            }
            fn read(&mut self, off: u32, out: &mut [u8]) -> bool {
                let s = off as usize;
                if s + out.len() > self.data.len() { return false; }
                out.copy_from_slice(&self.data[s..s + out.len()]);
                true
            }
            fn write(&mut self, off: u32, inp: &[u8]) -> bool {
                let s = off as usize;
                if s + inp.len() > self.data.len() { return false; }
                self.data[s..s + inp.len()].copy_from_slice(inp);
                true
            }
            fn seal(&mut self) -> bool { true }
            fn abort(&mut self) { self.generation += 1; }
            fn generation(&self) -> u64 { self.generation }
            fn stable(&mut self, generation: u64) -> bool { self.generation == generation }
        }
        struct SimpleRevHash(u64);
        impl RevisionHasher for SimpleRevHash {
            fn begin(&mut self) -> bool { self.0 = 0x517cc1b727220a95; true }
            fn update(&mut self, input: &[u8]) -> bool {
                for &b in input {
                    self.0 = self.0.wrapping_mul(0x100000001b3) ^ u64::from(b);
                }
                true
            }
            fn finish(&mut self, output: &mut Digest) -> bool {
                output.fill(0);
                output[..8].copy_from_slice(&self.0.to_le_bytes());
                true
            }
            fn hash(&mut self, input: &[u8], output: &mut Digest) -> bool {
                let mut h = 0x517cc1b727220a95u64;
                for &b in input {
                    h = h.wrapping_mul(0x100000001b3) ^ u64::from(b);
                }
                output.fill(0);
                output[..8].copy_from_slice(&h.to_le_bytes());
                true
            }
        }
        let mut rev_store = MemRevStore { data: Vec::new(), generation: 1 };
        let mut rev_hash = SimpleRevHash(0);
        let mut rev_scratch = [0u8; BLOCK_BYTES];
        let mut rev = ImportedRevision::full(&mut rev_store, &mut rev_hash, &mut rev_scratch);
        assert!(rev.begin_import(&mut source));
        assert!(rev.provisional());
        let mut op = OperationPhased::new(&mut relended, &mut storage, &mut loan, &mut parser, &mut breaker);
        let mut cache = PhasedChapterCache { store: &mut cache_store, hash: &mut hash, settings };
        let mut session = Session { revision: &mut rev, reader: &mut op, cache: &mut cache };
        assert_eq!(session.capture_chapter_owned(0, 0), Status::Unsupported);
        assert_eq!(session.reader.reader.last_layout_pages(), 0, "provisional capture_chapter_owned must reset last_layout_pages");
        assert_eq!(session.reader.reader.total_layout_pages(), cumulative, "provisional capture_chapter_owned must preserve total");
    }
    relended.close();
    assert_eq!((relended.last_layout_pages(), relended.total_layout_pages()), (0, 0));
    let _ = PageInfo::default();
}

/// The slim bar and refresh cadence change no page, so they must not turn the
/// page-turn cache off (#187): the producer seals segments and a turn hits
/// them with no layout. A different text size still never reuses them.
#[test]
fn display_preferences_still_produce_and_hit_the_page_cache() {
    use newt_core::preferences::TextSize;
    use newt_core::producer_policy::{Cancellation, Hooks, Stop};
    fn never_stop(_: &()) -> Stop {
        Stop::None
    }
    for (slim_progress_bar, refresh_pages) in [(false, 5), (true, 10), (false, 1)] {
        let settings = ViewSettings {
            orientation: Orientation::Portrait,
            slim_progress_bar,
            refresh_pages,
            ..ViewSettings::default()
        };
        let mut source = VecSource(synthetic_epub(60));
        let mut shared = Box::new(Bank([0; BANK_BYTES]));
        let mut parse = vec![0u8; FRAME_BYTES];
        let mut storage = PhaseStorage::new(&mut shared.0);
        let mut loan = HostLoan(&mut parse);
        let mut parser = MiniXml::default();
        let mut breaker = SimpleBreak;
        let mut runs = Box::new([Run::default(); RUNS]);
        let mut text = Box::new(PlanText([0; TEXT]));
        let mut plan = Plan::new(&mut *runs, &mut text.0);
        let mut selections = [Selection::default(), Selection::default()];
        let mut frame: Box<Frame> = Box::new([0u8; FRAME_BYTES]);
        let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), settings.page_layout());
        assert_eq!(reader.open_phased(&mut source, &mut storage, &mut loan, &mut parser), Status::Ok);
        assert_eq!(
            reader.capture_phased(&mut source, 0, 0, Target::Page, &mut storage, &mut loan, &mut parser, &mut breaker),
            Status::Ok
        );
        assert_eq!(reader.publish(&mut source, &mut frame), Status::Ok);

        let mut cache_store = MemCache::default();
        let mut hash = Hash64;
        let digest = [0x42u8; 32];
        let cancel = Cancellation::new(Hooks { context: &(), stop: Some(never_stop), yield_now: None, pause: None });
        let info = reader.info();
        let mut route = newt_phased_reader::producer::Route { settings, digest: &digest, info, hash: &mut hash };
        let produced = storage
            .with_producer(&mut loan, &cancel, &mut cache_store, &mut source, info.page_count, None, |producer| {
                Ok(producer.produce_current(&mut reader, &mut parser, &mut breaker, &mut route))
            })
            .unwrap();
        let case = format!("slim bar {slim_progress_bar}, refresh {refresh_pages}");
        assert_eq!(produced, newt_core::producer_visit::CurrentOutcome::Done, "{case}: producer must run");
        assert!(!cache_store.files.is_empty(), "{case}: producer must seal segments");

        let request = CacheRequest { spine: 0, target: 5, anchor: false, staged: false, cache_only: false, build_on_miss: false };
        let mut op = OperationPhased::new(&mut reader, &mut storage, &mut loan, &mut parser, &mut breaker);
        let mut cache = PhasedChapterCache { store: &mut cache_store, hash: &mut hash, settings };
        let shown = cache.capture(&mut source, &digest, request, &mut op);
        assert_eq!(shown.status, Status::Ok, "{case}");
        assert!(shown.hit, "{case}: turn must hit the cache");
        assert_eq!(op.reader.publish(&mut source, &mut frame), Status::Ok);
        assert_eq!(op.reader.last_layout_pages(), 0, "{case}: a hit lays out no page");

        // Control: another text size is another page layout and never reuses these pages.
        let other = ViewSettings { text_size: TextSize::Px22, ..settings };
        let mut cache = PhasedChapterCache { store: &mut cache_store, hash: &mut hash, settings: other };
        assert!(!cache.capture(&mut source, &digest, request, &mut op).hit, "{case}: other layout must miss");
    }
}

thread_local! {
    static PHASE: core::cell::Cell<newt_core::admission::WorkPhase> =
        const { core::cell::Cell::new(newt_core::admission::WorkPhase::Idle) };
    static READS_OUTSIDE_PHASE: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
}
fn track_phase(event: newt_core::admission::WorkEvent) {
    if event.kind == newt_core::admission::WorkKind::Phase {
        PHASE.set(event.phase);
    }
}
/// Counts book reads made while no work phase is open.
struct PhaseSource(VecSource);
impl newt_core::producer_policy::ProducerReceipt for PhaseSource {
    fn producer_receipt_live(&mut self) -> bool {
        true
    }
}
impl BookSource for PhaseSource {
    fn size(&self) -> u64 {
        self.0.size()
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        if !out.is_empty() && PHASE.get() == newt_core::admission::WorkPhase::Idle {
            READS_OUTSIDE_PHASE.set(READS_OUTSIDE_PHASE.get() + 1);
        }
        self.0.read_at(at, out)
    }
}

/// The firmware gives each work phase of a Contents jump a fresh open budget
/// (#188), so every chapter pass of an anchor jump must run inside a phase,
/// including the exact-anchor scan between the two layout passes.
#[test]
fn anchor_jump_reads_the_book_only_inside_work_phases() {
    let mut chapter = String::from(
        "<?xml version=\"1.0\"?><html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>C</title></head><body><h1>Chapter</h1>",
    );
    for i in 0..60 {
        if i == 40 {
            chapter.push_str("<h2 id=\"mid\">Middle</h2>");
        }
        chapter.push_str(&format!("<p>Paragraph {i}: the river path bent past the mill toward the old stone bridge.</p>"));
    }
    chapter.push_str("</body></html>");
    let container = b"<?xml version=\"1.0\"?><container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>";
    let opf = b"<?xml version=\"1.0\"?><package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"u\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:identifier id=\"u\">urn:test:188</dc:identifier><dc:title>Test</dc:title><dc:language>en</dc:language></metadata><manifest><item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/><item id=\"c0\" href=\"c0.xhtml\" media-type=\"application/xhtml+xml\"/></manifest><spine><itemref idref=\"c0\"/></spine></package>";
    let nav = b"<?xml version=\"1.0\"?><html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\"><head><title>N</title></head><body><nav epub:type=\"toc\"><ol><li><a href=\"c0.xhtml\">Chapter</a></li><li><a href=\"c0.xhtml#mid\">Middle</a></li></ol></nav></body></html>";
    let epub = stored_zip(&[
        ("mimetype", b"application/epub+zip"),
        ("META-INF/container.xml", container),
        ("OEBPS/content.opf", opf),
        ("OEBPS/nav.xhtml", nav),
        ("OEBPS/c0.xhtml", chapter.as_bytes()),
    ]);
    let mut source = PhaseSource(VecSource(epub));
    let mut shared = Box::new(Bank([0; BANK_BYTES]));
    let mut parse = vec![0u8; FRAME_BYTES];
    let mut storage = PhaseStorage::new(&mut shared.0);
    let mut loan = HostLoan(&mut parse);
    let mut parser = MiniXml::default();
    let mut breaker = SimpleBreak;
    let mut runs = Box::new([Run::default(); RUNS]);
    let mut text = Box::new(PlanText([0; TEXT]));
    let mut plan = Plan::new(&mut *runs, &mut text.0);
    let mut selections = [Selection::default(), Selection::default()];
    let mut frame: Box<Frame> = Box::new([0u8; FRAME_BYTES]);
    let settings = ViewSettings { orientation: Orientation::Portrait, ..ViewSettings::default() };
    let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), settings.page_layout());
    assert_eq!(reader.open_phased(&mut source, &mut storage, &mut loan, &mut parser), Status::Ok);
    assert_eq!(
        reader.capture_phased(&mut source, 0, 0, Target::Page, &mut storage, &mut loan, &mut parser, &mut breaker),
        Status::Ok
    );
    assert_eq!(reader.publish(&mut source, &mut frame), Status::Ok);
    assert_eq!(reader.prepare_chapter_menu_phased(&mut source, 0, &mut storage, &mut loan, &mut parser), Status::Ok);
    assert!(reader.chapter_menu().row(1).fragment, "row 1 must be the anchor entry");

    reader.work = Some(track_phase);
    READS_OUTSIDE_PHASE.set(0);
    assert_eq!(
        reader.capture_chapter_phased(&mut source, 0, 1, &mut storage, &mut loan, &mut parser, &mut breaker),
        Status::Ok
    );
    assert_eq!(reader.publish(&mut source, &mut frame), Status::Ok);
    assert!(reader.info().page > 0, "the jump must land past the first page");
    assert_eq!(READS_OUTSIDE_PHASE.get(), 0, "every chapter pass of the jump must run inside a work phase");
}
