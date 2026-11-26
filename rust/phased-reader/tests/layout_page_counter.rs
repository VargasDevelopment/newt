// SPDX-License-Identifier: MIT
//! Focused host test for PhasedReader's layout-page counter across open,
//! early-stop miss passes, cache hits, and ReaderState round-trips (#168).
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
        assert_eq!(produced, Status::Ok);
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

    // ReaderState/ReaderView round-trip preserves counts; close() resets them.
    let expected_last = reader.last_layout_pages();
    let expected_total = reader.total_layout_pages();
    let state = reader.into_state();
    assert_eq!(state.last_layout_pages(), expected_last);
    assert_eq!(state.total_layout_pages(), expected_total);
    assert_eq!(state.view(&selections).last_layout_pages(), expected_last);
    assert_eq!(state.view(&selections).total_layout_pages(), expected_total);
    let mut relended = state.lend(&mut selections, Some(&mut plan));
    assert_eq!(relended.last_layout_pages(), expected_last);
    assert_eq!(relended.total_layout_pages(), expected_total);
    relended.close();
    assert_eq!((relended.last_layout_pages(), relended.total_layout_pages()), (0, 0));
    let _ = PageInfo::default();
}
