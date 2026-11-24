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
use newt_core::zip_container::{Entry, Status};
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
    let entry = Entry::raw(xml.len() as u32);
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
                &Entry::raw(xml.len() as u32),
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
                &Entry::raw(xml.len() as u32),
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
