//! Compare the index with the real package parser and phased navigation adapter.
use super::*;
use newt_core::{book_index::{self as index, NONE}, book_index_build as builder,
    canonical, package_parsers, phased_metadata::{copy_display_title, BookLanguage},
    xml_input::dir_name};
use newt_reader_navigation::{metadata::Metadata, Navigation};

pub fn agreement(source: &mut impl BookSource, name: &str) -> usize {
    let mut peak = 0;
    for (bytes, read, run, variant) in [(32768, 512, 32768, "normal"), (512, 32, 128, "spill"), (448, 32, 64, "tiny")] {
        peak = peak.max(agreement_variant(source, &format!("{name}/{variant}"), bytes, read, run));
    }
    peak
}
fn agreement_variant(source: &mut impl BookSource, name: &str, buffer_bytes: usize, min_read: usize, run_bytes: usize) -> usize {
    let mut host = HostScratch::new();
    host.with_resources(|catalog, parse, resources| {
        assert_eq!(admission::inspect(&mut admission::ScheduledSource::new(source, None, None),
            parse, resources.scan), Status::Ok, "{name}: inspect");
        let book = canonical::open(source, catalog, resources, false, None, None)
            .unwrap_or_else(|s| panic!("{name}: canonical {s:?}"));
        let mut parts = book.into_parts();
        let view = canonical::CatalogView::new(parts.entries, parts.arena, parts.directory_offset);
        let container = view.find(b"META-INF/container.xml").unwrap().1;
        let container = view.check(parts.workflow.source, &container).unwrap();
        let mut path_bytes = [0; 2048];
        let mut paths = Arena::new(&mut path_bytes);
        let mut opf = None;
        assert_eq!(package_parsers::parse_container(&mut parts.workflow, &container, &mut paths, &mut opf), Status::Ok);
        let opf = paths.c_string(opf.unwrap()).unwrap();
        let mut dir = [0; 512];
        assert!(dir_name(opf, &mut dir));
        let entry = view.find(opf).unwrap().1;
        let entry = view.check(parts.workflow.source, &entry).unwrap();
        let mut buffer = vec![0; buffer_bytes];
        let mut store = BuildStore::default();
        let mut builder = builder::Builder::new(&mut buffer, min_read, run_bytes).unwrap();
        let mut scan = [0; 4099];
        assert_eq!(admission::inspect_with_visitor(
            &mut admission::ScheduledSource::new(parts.workflow.source, None, None), parse, &mut scan,
            |m| builder.visit(&mut store, m, &mut |_| true)), Status::Ok);
        let identity = index::Identity { digest: [17; 32], policy: 1 };
        let (mut positions, mut cursors, mut saved) =
            ([0; builder::POSITION_WORDS], [builder::Cursor::default(); builder::CURSORS], [0; builder::WRITER_PREFIX]);
        let mut state = builder::JoinState::new(&mut positions, &mut cursors, &mut saved);
        let prepared = builder.parse(parts.workflow.reborrow(), &entry, &dir, identity, &mut store, &mut |_| true)
            .and_then(|parsed| parsed.join(&mut store, &mut state, &mut |_| true))
            .unwrap_or_else(|e| panic!("{name}: parse {e:?}"));
        let report = prepared.publish(&mut store, &mut state, &mut |_| true, &mut || true)
            .unwrap_or_else(|e| panic!("{name}: publish {e:?}"));
        assert!(report.work_removed && store.work.is_empty());
        if buffer_bytes < 1024 { assert!(report.statistics.runs > 0, "{name}: forced spill"); }
        let reader = index::Reader::open(&mut store, &identity).expect("index readback");
        let h = reader.header();
        assert_eq!(h.spine as usize, parts.result.spine_count, "{name}: spine count");
        assert_eq!(h.members as usize, view.len(), "{name}: members count");
        let bytes = |value: Option<newt_core::zip_window::arena::CStringRef>| value.and_then(|v| parts.arena.c_string(v));
        let member_for = |item: &ManifestItem| bytes(item.href).and_then(|p| view.find(p)).map(|(_, e)| e);
        let same_member = |member: Option<index::Member>, entry: Option<Entry>| {
            assert_eq!(member.map(|m| (m.local_offset, m.compressed, m.uncompressed, m.method, m.flags)),
                entry.map(|e| (e.local_header_offset, e.compressed_size, e.uncompressed_size, e.method, e.flags)), "{name}: member");
        };
        let read_member = |store: &mut BuildStore, n| if n == NONE { None } else { Some(reader.member(store, n).unwrap()) };
        let metadata = Metadata { arena: parts.arena, package: &parts.result,
            manifest: parts.manifest, spine: parts.spine, toc: &[], entries: parts.entries,
            directory_offset: parts.directory_offset };
        for i in 0..h.spine {
            let s = reader.spine(&mut store, i).unwrap();
            let item = &parts.manifest[usize::from(parts.spine[i as usize])];
            same_member(read_member(&mut store, s.member), member_for(item));
            let mut navigation = Navigation::default();
            assert!(navigation.bind(&metadata, i), "{name}: bind {i}");
            let p = navigation.progress_inputs();
            let next = if i + 1 < h.spine { reader.spine(&mut store, i + 1).unwrap().before } else { h.spine_bytes };
            assert_eq!((s.before, next - s.before, h.spine_bytes, h.spine_bytes > 0),
                (p.before, p.current, p.total, p.valid), "{name}: progress {i}");
        }
        for i in 0..h.members {
            let m = reader.member(&mut store, i).unwrap();
            let expected = (0..parts.result.spine_count).find(|&s| {
                member_for(&parts.manifest[usize::from(parts.spine[s])])
                    .is_some_and(|e| e.local_header_offset == m.local_offset)
            }).map_or(NONE, |s| s as u32);
            assert_eq!(m.first_spine, expected, "{name}: first spine {i}");
        }
        let chosen = parts.result.nav_index.or(parts.result.ncx_index)
            .and_then(|i| member_for(&parts.manifest[i]));
        same_member(read_member(&mut store, if h.nav != NONE { h.nav } else { h.ncx }), chosen);
        assert_eq!(h.nav != NONE, chosen.is_some() && parts.result.nav_index.is_some(), "{name}: nav kind");
        let cover = parts.manifest[..parts.result.manifest_count].iter().find(|m| m.is_cover_image).and_then(member_for);
        same_member(read_member(&mut store, h.cover), cover);
        let mut title = [0; 256];
        copy_display_title(&mut title, bytes(parts.result.metadata.title));
        assert_eq!(h.title, title, "{name}: title");
        assert_eq!(h.language, BookLanguage::from_opf(bytes(parts.result.metadata.language)), "{name}: language");
        println!("INDEX {name} buffer={} statistics={:?}", buffer_bytes, report.statistics);
        builder::STACK_BYTES + builder::JOIN_STATE_BYTES + buffer_bytes
    })
}

struct MemorySource(Vec<u8>);
impl BookSource for MemorySource {
    fn size(&self) -> u64 { self.0.len() as u64 }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let at = (at as usize).min(self.0.len());
        let n = out.len().min(self.0.len() - at);
        out[..n].copy_from_slice(&self.0[at..at + n]);
        n as i32
    }
}

// A stored ZIP writer keeps every fixture synthetic and in memory.
fn book(opf: &str, extra: &[(&str, &[u8])]) -> MemorySource {
    fn u16le(out: &mut Vec<u8>, value: u16) { out.extend_from_slice(&value.to_le_bytes()); }
    fn u32le(out: &mut Vec<u8>, value: u32) { out.extend_from_slice(&value.to_le_bytes()); }
    fn crc(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 { crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1)); }
        }
        !crc
    }
    let mut files = vec![
        ("mimetype", b"application/epub+zip".as_slice()),
        ("META-INF/container.xml", b"<container><rootfiles><rootfile full-path='OEBPS/book.opf' media-type='application/oebps-package+xml'/></rootfiles></container>"),
        ("OEBPS/book.opf", opf.as_bytes()),
    ];
    files.extend_from_slice(extra);
    let mut out = Vec::new();
    let mut central = Vec::new();
    for &(name, data) in &files {
        let offset = out.len() as u32;
        u32le(&mut out, 0x04034b50);
        for value in [20, 0, 0, 0, 0] { u16le(&mut out, value); }
        for value in [crc(data), data.len() as u32, data.len() as u32] { u32le(&mut out, value); }
        u16le(&mut out, name.len() as u16); u16le(&mut out, 0);
        out.extend_from_slice(name.as_bytes()); out.extend_from_slice(data);
        u32le(&mut central, 0x02014b50);
        for value in [20, 20, 0, 0, 0, 0] { u16le(&mut central, value); }
        for value in [crc(data), data.len() as u32, data.len() as u32] { u32le(&mut central, value); }
        for value in [name.len() as u16, 0, 0, 0, 0] { u16le(&mut central, value); }
        u32le(&mut central, 0); u32le(&mut central, offset);
        central.extend_from_slice(name.as_bytes());
    }
    let offset = out.len() as u32;
    out.extend_from_slice(&central);
    u32le(&mut out, 0x06054b50);
    for value in [0, 0, files.len() as u16, files.len() as u16] { u16le(&mut out, value); }
    u32le(&mut out, central.len() as u32); u32le(&mut out, offset); u16le(&mut out, 0);
    MemorySource(out)
}
fn package(metadata: &str, manifest: &str, spine: &str) -> String {
    format!("<package><metadata>{metadata}</metadata><manifest>{manifest}</manifest><spine>{spine}</spine></package>")
}
fn item(id: &str, href: &str, attrs: &str) -> String {
    format!("<item id='{id}' href='{href}' media-type='application/xhtml+xml' {attrs}/>")
}

fn normalized_hrefs_and_progress_agree() {
    let manifest: String = ["a%20b.xhtml", "./a b.xhtml", "x/../a b.xhtml", "a b.xhtml#p", "/OEBPS/a b.xhtml"]
        .iter().enumerate().map(|(i, href)| item(&format!("i{i}"), href, "")).collect();
    let refs: String = (0..5).map(|i| format!("<itemref idref='i{i}'/>" )).collect();
    let opf = package("<title>  A generated 本 book  </title><language>JA_jp</language>", &manifest, &refs);
    agreement(&mut book(&opf, &[("OEBPS/a b.xhtml", b"<html/>")]), "hrefs");
}
fn first_match_dangling_and_repeated_idrefs_agree() {
    let manifest = item("a", "a.xhtml", "") + &item("a", "b.xhtml", "") + &item("b", "b.xhtml", "");
    let opf = package("", &manifest, "<itemref idref='unknown'/><itemref idref='a'/><itemref idref='a'/><itemref idref='b' linear='no'/>");
    agreement(&mut book(&opf, &[("OEBPS/a.xhtml", b"a"), ("OEBPS/b.xhtml", b"bbb")]), "idrefs");
}
fn spine_before_manifest_agrees() {
    let opf = format!("<package><spine><itemref idref='a'/></spine><manifest>{}</manifest></package>", item("a", "a.xhtml", ""));
    agreement(&mut book(&opf, &[("OEBPS/a.xhtml", b"a")]), "early-spine");
}
fn missing_member_keeps_spine_and_invalidates_progress() {
    let opf = package("", &(item("a", "a.xhtml", "") + &item("b", "missing.xhtml", "")), "<itemref idref='a'/><itemref idref='b'/>");
    let mut source = book(&opf, &[("OEBPS/a.xhtml", b"a")]);
    agreement(&mut source, "missing-member");
    let mut host = HostScratch::new();
    host.with_resources(|catalog, _, resources| {
        let book = canonical::open(&mut source, catalog, resources, false, None, None).unwrap();
        assert_eq!(book.package().spine_count, 2);
        let parts = book.into_parts();
        let href = parts.arena.c_string(parts.manifest[usize::from(parts.spine[1])].href.unwrap()).unwrap();
        let catalog = canonical::CatalogView::new(parts.entries, parts.arena, parts.directory_offset);
        assert!(matches!(catalog.member(parts.workflow.source, href), Err(Status::NotFound)));
    });
}
fn navigation_choices_and_covers_agree() {
    let chapter = item("a", "a.xhtml", "");
    for (name, nav, toc, extra) in [
        ("missing-nav", item("nav", "missing.xhtml", "properties='nav'"), "", vec![("OEBPS/n.ncx", b"ncx".as_slice())]),
        ("toc-ncx", item("n", "n.ncx", ""), "toc='n'", vec![("OEBPS/n.ncx", b"ncx".as_slice())]),
        ("media-ncx", String::new(), "", vec![("OEBPS/n.ncx", b"ncx".as_slice())]),
    ] {
        let ncx = "<item id='ncx' href='n.ncx' media-type='application/x-dtbncx+xml'/>";
        let opf = format!("<package><metadata><meta name='cover' content='a'/></metadata><manifest>{chapter}{nav}{ncx}</manifest><spine {toc}><itemref idref='a'/></spine></package>");
        let mut extra = extra;
        extra.push(("OEBPS/a.xhtml", b"a"));
        agreement(&mut book(&opf, &extra), name);
    }
}
fn distinct_32_bit_collision_names_agree() {
    let (a, b) = ("e/0317786", "e/1056240");
    assert_eq!(newt_core::zip_container::hash_path(a.as_bytes()), newt_core::zip_container::hash_path(b.as_bytes()));
    assert_ne!(index::name_hash(a.as_bytes()), index::name_hash(b.as_bytes()));
    assert!(index::name_hash(a.as_bytes()) > u64::from(u32::MAX), "collision: full 64-bit hash");
    assert!(index::name_hash(b.as_bytes()) > u64::from(u32::MAX), "collision: full 64-bit hash");
    let opf = package("", &(item("a", "/e/0317786", "") + &item("b", "/e/1056240", "")), "<itemref idref='a'/><itemref idref='b'/>");
    agreement(&mut book(&opf, &[(a, b"a"), (b, b"bbb")]), "collision");
}

pub fn small_cases() {
    normalized_hrefs_and_progress_agree();
    first_match_dangling_and_repeated_idrefs_agree();
    spine_before_manifest_agrees();
    missing_member_keeps_spine_and_invalidates_progress();
    navigation_choices_and_covers_agree();
    distinct_32_bit_collision_names_agree();
}

#[derive(Default)]
struct BuildStore { work: Vec<u8>, cache: Vec<u8>, pending: Vec<u8> }
impl builder::WorkFile for BuildStore {
    fn append(&mut self, bytes: &[u8]) -> bool { self.work.extend_from_slice(bytes); true }
    fn read_at(&mut self, at: u32, out: &mut [u8]) -> bool {
        let Some(input) = self.work.get(at as usize..at as usize + out.len()) else { return false; };
        out.copy_from_slice(input); true
    }
    fn remove_work(&mut self) -> bool { self.work.clear(); true }
    fn sync_work(&mut self) -> bool { true }
}
impl CacheStorage for BuildStore {
    fn remove(&mut self, _: &[u8]) -> bool { self.cache.clear(); true }
    fn file_size(&mut self, _: &[u8]) -> i64 { self.cache.len() as i64 }
    fn read_at(&mut self, _: &[u8], at: u32, out: &mut [u8]) -> i32 {
        let Some(input) = self.cache.get(at as usize..at as usize + out.len()) else { return -1; };
        out.copy_from_slice(input); out.len() as i32
    }
    fn begin_write(&mut self, _: &[u8]) -> bool { self.pending.clear(); true }
    fn write(&mut self, bytes: &[u8]) -> bool { self.pending.extend_from_slice(bytes); true }
    fn end_write(&mut self) -> bool { self.cache = std::mem::take(&mut self.pending); true }
}
