use super::expat;
use newt_core::xml_session::{Attributes, Handler, ParserBackend};
#[derive(Default)]
struct Lookup {
    calls: usize,
}
impl Handler for Lookup {
    fn start_element(&mut self, _: &[u8], a: &mut dyn Attributes) {
        assert_eq!(a.local(b"href"), Some(b"first".as_slice()));
        while a.next().is_some() {}
        assert_eq!(a.exact(b"href"), Some(b"last".as_slice()));
        assert_eq!(a.local(b"href"), Some(b"first".as_slice()));
        assert_eq!(a.exact(b"type"), None);
        assert_eq!(a.exact(b"epub:type"), Some(b"toc".as_slice()));
        self.calls += 1;
    }
}
#[test]
fn complete_native_view_lookup_survives_iterator_exhaustion() {
    let mut xml = String::from("<n x:href='first' epub:type='toc'");
    for n in 0..300 {
        xml.push_str(&format!(" a{n}='v'"));
    }
    xml.push_str(" href='last'/>");
    let mut parser = expat::Expat::default();
    let mut sink = Lookup::default();
    assert!(parser.create());
    assert!(parser.feed(xml.as_bytes(), true, &mut sink));
    parser.close();
    assert_eq!(sink.calls, 1);
}

#[path = "../../../target-probe/src/package_parsers_probe.rs"]
mod probes;
use newt_core::package_parsers::*;
use newt_core::{member_check::Checked, zip_container::Status};
use newt_core::zip_window::{arena::Arena, source::BookSource};
struct BorrowedSource<'a>(&'a [u8]);
impl BookSource for BorrowedSource<'_> {
    fn size(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&mut self, at: u64, output: &mut [u8]) -> i32 {
        let Some(rest) = self.0.get(at as usize..) else {
            return -1;
        };
        let n = rest.len().min(output.len());
        output[..n].copy_from_slice(&rest[..n]);
        n as i32
    }
}
struct BorrowedParser<'a> {
    parser: expat::Expat,
    creates: &'a mut usize,
}
impl ParserBackend for BorrowedParser<'_> {
    fn create(&mut self) -> bool {
        *self.creates += 1;
        self.parser.create()
    }
    fn close(&mut self) {
        self.parser.close();
    }
    fn feed(&mut self, bytes: &[u8], final_chunk: bool, handler: &mut dyn Handler) -> bool {
        self.parser.feed(bytes, final_chunk, handler)
    }
}
fn book_xml() -> String {
    String::from(
        "<r><rootfile full-path='b.opf'/><manifest><item id='a' href='a'/></manifest><spine><itemref idref='missing'/><itemref idref='a'/></spine><nav epub:type='toc'><a href='a'>A</a></nav></r>",
    )
}
#[test]
fn all_target_probe_signatures_accept_borrowed_source_and_backend() {
    let xml = book_xml();
    let mut source = BorrowedSource(xml.as_bytes());
    let mut creates = 0;
    let mut backend = BorrowedParser {
        parser: expat::Expat::default(),
        creates: &mut creates,
    };
    let mut buffer = [0; 2048];
    let mut memory = [0; 256];
    let mut arena = Arena::new(&mut memory);
    let mut workflow: Workflow<'_, dyn BookSource + '_, dyn ParserBackend + '_> = Workflow {
                policy: None,
        source: &mut source,
        backend: &mut backend,
        decoder: None,
        buffer: Some(&mut buffer),
        filter: None,
    };
    let entry = Checked::raw(xml.len() as u32);
    let mut path = None;
    assert_eq!(
        probes::newt_rust_container_probe(&mut workflow, &entry, &mut arena, &mut path),
        Status::Ok
    );
    let mut manifest = [ManifestItem::default(); 1];
    let mut hashes = [0; 2];
    let mut spine = [0; 2];
    let mut out = PackageResult::default();
    assert_eq!(
        probes::newt_rust_package_probe(
            &mut workflow,
            &entry,
            b"",
            &mut arena,
            PackageStorage {
                manifest: &mut manifest,
                spine_hashes: &mut hashes,
                spine: &mut spine
            },
            &mut out
        ),
        Status::Ok
    );
    let mut toc = [TocEntry::default(); 1];
    let mut count = 77;
    assert_eq!(
        probes::newt_rust_toc_probe(
            &mut workflow,
            &entry,
            b"",
            &mut arena,
            TocKind::Nav,
            &mut toc,
            &mut count
        ),
        Status::Ok
    );
    assert_eq!(count, 1);
    drop(workflow);
    assert_eq!(*backend.creates, 5);
}
#[test]
fn typed_admission_boundaries_preserve_publication_and_pass_order() {
    let xml = book_xml();
    for (manifest_cap, hash_cap, spine_cap, passes, expected) in [
        (0, 2, 2, 1, Status::MetadataTooLarge),
        (1, 1, 2, 1, Status::MetadataTooLarge),
        (1, 2, 1, 2, Status::MetadataTooLarge),
        (1, 2, 2, 2, Status::Ok),
    ] {
        let mut source = BorrowedSource(xml.as_bytes());
        let mut creates = 0;
        let mut backend = BorrowedParser {
            parser: expat::Expat::default(),
            creates: &mut creates,
        };
        let mut buffer = [0; 2048];
        let mut memory = [0; 256];
        let mut arena = Arena::new(&mut memory);
        let mut manifest = [ManifestItem::default(); 1];
        let mut hashes = [0; 2];
        let mut spine = [0; 2];
        let mut out = PackageResult {
            spine_count: 77,
            ..PackageResult::default()
        };
        let saved = out;
        let status = {
            let mut workflow = Workflow {
                policy: None,
                source: &mut source,
                backend: &mut backend,
                decoder: None,
                buffer: Some(&mut buffer),
                filter: None,
            };
            parse_package(
                &mut workflow,
                &Checked::raw(xml.len() as u32),
                b"",
                &mut arena,
                PackageStorage {
                    manifest: &mut manifest[..manifest_cap],
                    spine_hashes: &mut hashes[..hash_cap],
                    spine: &mut spine[..spine_cap],
                },
                &mut out,
            )
        };
        assert_eq!(status, expected);
        assert_eq!(*backend.creates, passes);
        if expected != Status::Ok {
            assert_eq!(out, saved);
        } else {
            assert_eq!(out.spine_count, 1);
            assert_eq!(spine[0], 0);
        }
    }
    for cap in [0, 1] {
        let mut source = BorrowedSource(xml.as_bytes());
        let mut creates = 0;
        let mut backend = BorrowedParser {
            parser: expat::Expat::default(),
            creates: &mut creates,
        };
        let mut buffer = [0; 2048];
        let mut memory = [0; 256];
        let mut arena = Arena::new(&mut memory);
        let mut entries = [TocEntry::default(); 1];
        let mut count = 77;
        let status = {
            let mut workflow = Workflow {
                policy: None,
                source: &mut source,
                backend: &mut backend,
                decoder: None,
                buffer: Some(&mut buffer),
                filter: None,
            };
            parse_toc(
                &mut workflow,
                &Checked::raw(xml.len() as u32),
                b"",
                &mut arena,
                TocKind::Nav,
                &mut entries[..cap],
                &mut count,
            )
        };
        assert_eq!(
            status,
            if cap == 0 {
                Status::OutOfMemory
            } else {
                Status::Ok
            }
        );
        assert_eq!(*backend.creates, if cap == 0 { 1 } else { 2 });
        assert_eq!(count, if cap == 0 { 77 } else { 1 });
    }
}

#[test]
fn container_capacity_failure_keeps_the_first_rootfile_decision() {
    let xml = b"<c><rootfile full-path='long'/><rootfile full-path='x'/></c>";
    for capacity in [2, 5] {
        let mut source = BorrowedSource(xml);
        let mut backend = expat::Expat::default();
        let mut buffer = [0; 2048];
        let mut memory = [0; 5];
        let mut arena = Arena::new(&mut memory[..capacity]);
        let mut path = None;
        let status = parse_container(&mut Workflow {
            policy: None, source: &mut source, backend: &mut backend,
            decoder: None, buffer: Some(&mut buffer), filter: None,
        }, &Checked::raw(xml.len() as u32), &mut arena, &mut path);
        if capacity == 2 {
            assert_eq!(status, Status::MetadataTooLarge);
            assert_eq!(path, None);
            assert_eq!(arena.used(), 0, "the shorter later rootfile must not be allocated");
        } else {
            assert_eq!(status, Status::Ok);
            assert!(arena.c_string_eq(path.unwrap(), b"long"));
            assert_eq!(arena.used(), 5);
        }
    }
}

struct CountingParser {
    parser: expat::Expat,
    creates: usize,
    closes: usize,
    feeds: usize,
}
impl CountingParser {
    fn new() -> Self {
        Self {
            parser: expat::Expat::default(),
            creates: 0,
            closes: 0,
            feeds: 0,
        }
    }
}
impl ParserBackend for CountingParser {
    fn create(&mut self) -> bool {
        self.creates += 1;
        self.parser.create()
    }
    fn close(&mut self) {
        self.closes += 1;
        self.parser.close();
    }
    fn feed(&mut self, bytes: &[u8], final_chunk: bool, handler: &mut dyn Handler) -> bool {
        self.feeds += 1;
        self.parser.feed(bytes, final_chunk, handler)
    }
}
struct ChunkedSource<'a> {
    bytes: &'a [u8],
    max_read: usize,
}
impl BookSource for ChunkedSource<'_> {
    fn size(&self) -> u64 {
        self.bytes.len() as u64
    }
    fn read_at(&mut self, at: u64, output: &mut [u8]) -> i32 {
        let Some(rest) = self.bytes.get(at as usize..) else {
            return -1;
        };
        let n = rest.len().min(self.max_read).min(output.len());
        output[..n].copy_from_slice(&rest[..n]);
        n as i32
    }
}
fn opf_expansion_xml(nested: bool, attribute: bool) -> String {
    let mut xml = String::from("<!DOCTYPE package [<!ENTITY a '");
    for _ in 0..1024 {
        xml.push('x');
    }
    xml.push_str("'>");
    if nested {
        xml.push_str("<!ENTITY b '");
        for _ in 0..32 {
            xml.push_str("&a;");
        }
        xml.push_str("'>");
    }
    xml.push_str("]><package><metadata><dc:title>");
    if !attribute {
        for _ in 0..(if nested { 32 } else { 1024 }) {
            xml.push_str(if nested { "&b;" } else { "&a;" });
        }
    } else {
        xml.push_str("Title");
    }
    xml.push_str("</dc:title></metadata><manifest><item id='c' href='");
    if attribute {
        for _ in 0..(if nested { 32 } else { 1024 }) {
            xml.push_str(if nested { "&b;" } else { "&a;" });
        }
    } else {
        xml.push_str("chapter.xhtml");
    }
    xml.push_str("' media-type='application/xhtml+xml'/></manifest><spine><itemref idref='c'/></spine></package>");
    xml
}
#[test]
fn opf_entity_expansion_refused_without_publishing_and_large_plain_opf_opens() {
    let mut large_plain = String::from("<package><metadata><dc:title>Plain &amp; &#65;</dc:title></metadata><manifest>");
    for i in 0..250 {
        large_plain.push_str(&format!(
            "<item id='c{i}' href='chapters/chapter-{i:04}-ordinary.xhtml' media-type='application/xhtml+xml'/>"
        ));
    }
    large_plain.push_str("</manifest><spine><itemref idref='c0'/></spine></package>");
    assert!(large_plain.len() > 20 * 1024);

    let tiny_entity = "<!DOCTYPE package [<!ENTITY t 'Small'>]><package><metadata><dc:title>&t;</dc:title></metadata><manifest><item id='c' href='a.xhtml' media-type='application/xhtml+xml'/></manifest><spine><itemref idref='c'/></spine></package>";
    let external_only = "<!DOCTYPE package SYSTEM 'urn:unfetched'><package><metadata><dc:title>&amp;&#65;</dc:title></metadata><manifest><item id='c' href='a.xhtml' media-type='application/xhtml+xml'/></manifest><spine><itemref idref='c'/></spine></package>";
    let mut utf16_opf = vec![0xffu8, 0xfe];
    for &b in tiny_entity.as_bytes() {
        utf16_opf.push(b);
        utf16_opf.push(0);
    }

    for (name, xml_bytes, expected_title) in [
        ("large-plain", large_plain.as_bytes(), b"Plain & A".as_slice()),
        ("tiny-entity", tiny_entity.as_bytes(), b"Small".as_slice()),
        ("external-only", external_only.as_bytes(), b"&A".as_slice()),
        ("utf16", utf16_opf.as_slice(), b"Small".as_slice()),
    ] {
        let mut source = ChunkedSource { bytes: xml_bytes, max_read: 64 };
        let mut backend = CountingParser::new();
        let mut buffer = [0; 2048];
        let mut memory = vec![0; 64 * 1024];
        let mut arena = Arena::new(&mut memory);
        let mut manifest = vec![ManifestItem::default(); 300];
        let mut hashes = vec![0; 300];
        let mut spine = vec![0; 16];
        let mut out = PackageResult::default();
        let status = parse_package(
            &mut Workflow {
                policy: None,
                source: &mut source,
                backend: &mut backend,
                decoder: None,
                buffer: Some(&mut buffer),
                filter: None,
            },
            &Checked::raw(xml_bytes.len() as u32),
            b"",
            &mut arena,
            PackageStorage {
                manifest: &mut manifest,
                spine_hashes: &mut hashes,
                spine: &mut spine,
            },
            &mut out,
        );
        assert_eq!(status, Status::Ok, "{name}");
        assert_eq!(backend.creates, 2, "{name}");
        assert_eq!(backend.closes, backend.creates, "{name}");
        assert_eq!(out.spine_count, 1, "{name}");
        assert_eq!(arena.c_string(out.metadata.title.unwrap()), Some(expected_title), "{name}");
    }

    for nested in [false, true] {
        for attribute in [false, true] {
            let xml = opf_expansion_xml(nested, attribute);
            for max_read in [64usize, 2048] {
                let mut source = ChunkedSource { bytes: xml.as_bytes(), max_read };
                let mut backend = CountingParser::new();
                let mut buffer = [0; 2048];
                let mut memory = vec![0; 16 * 1024];
                let mut arena = Arena::new(&mut memory);
                let mut manifest = [ManifestItem::default(); 8];
                let mut hashes = [0; 8];
                let mut spine = [0; 8];
                let mut out = PackageResult {
                    spine_count: 77,
                    ..PackageResult::default()
                };
                let saved = out;
                let status = parse_package(
                    &mut Workflow {
                        policy: None,
                        source: &mut source,
                        backend: &mut backend,
                        decoder: None,
                        buffer: Some(&mut buffer),
                        filter: None,
                    },
                    &Checked::raw(xml.len() as u32),
                    b"",
                    &mut arena,
                    PackageStorage {
                        manifest: &mut manifest,
                        spine_hashes: &mut hashes,
                        spine: &mut spine,
                    },
                    &mut out,
                );
                assert_eq!(status, Status::ParseError, "nested={nested} attr={attribute} max={max_read}");
                assert_eq!(out, saved, "partial package must not be published");
                assert_eq!(backend.creates, 1, "must fail in pass 1 without pass 2");
                assert_eq!(backend.closes, 1, "parser must be closed on ParseError");
                let total_chunks = xml.len().div_ceil(max_read);
                if !nested && !attribute {
                    assert!(backend.feeds < total_chunks, "flat element expansion must stop before EOF: feeds={} total={}", backend.feeds, total_chunks);
                } else {
                    assert!(backend.feeds <= total_chunks + 1, "bounded expansion feeds");
                }
            }
        }
    }
}
