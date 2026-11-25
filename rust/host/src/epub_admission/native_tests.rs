use crate::expat::Expat;
use newt_core::xml_session::Handler;
use newt_core::{
    xml_input::EntityFilter,
    xml_session::{Resources, XmlStream},
    member_check::Checked,
    zip_container::Status,
    zip_window::source::BookSource,
};
struct Bytes(&'static [u8]);
impl BookSource for Bytes {
    fn size(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let tail = self.0.get(at as usize..).unwrap_or_default();
        let n = tail.len().min(out.len());
        out[..n].copy_from_slice(&tail[..n]);
        n as i32
    }
}
struct Sink;
impl Handler for Sink {}
#[test]
fn native_scope_cleanup_success_parse_failure_early_drop_and_create_failure() {
    for (bytes, early, fail) in [
        (b"<x/>".as_slice(), false, false),
        (b"<".as_slice(), false, false),
        (b"<x/>".as_slice(), true, false),
        (b"<x/>".as_slice(), false, true),
    ] {
        let mut source = Bytes(bytes);
        let mut backend = Expat::default();
        backend.fail_create = fail;
        let entry = Checked::raw(bytes.len() as u32);
        let mut buffer = [0; 2048];
        let mut filter = EntityFilter::new();
        {
            let mut stream = XmlStream::new(&mut source, None, &mut backend);
            let status = stream.open(
                &entry,
                Resources {
                    buffer: Some(&mut buffer),
                    filter: Some(&mut filter),
                },
                false,
            );
            assert_eq!(
                status,
                if fail {
                    Status::OutOfMemory
                } else {
                    Status::Ok
                }
            );
            if !early && !fail {
                let mut end = false;
                let mut status = Status::Ok;
                while !end && status == Status::Ok {
                    status = stream.feed_chunk(&mut Sink, Some(&mut end));
                }
                assert_eq!(
                    status,
                    if bytes == b"<" {
                        Status::ParseError
                    } else {
                        Status::Ok
                    }
                );
            }
        }
        assert_eq!(backend.created, usize::from(!fail));
        assert_eq!(backend.created, backend.closed);
    }
}

struct OwnedBytes(Vec<u8>);
impl BookSource for OwnedBytes {
    fn size(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let tail = self.0.get(at as usize..).unwrap_or_default();
        let n = tail.len().min(out.len());
        out[..n].copy_from_slice(&tail[..n]);
        n as i32
    }
}

fn stored_epub(opf: &[u8]) -> Vec<u8> {
    let container = b"<container><rootfiles><rootfile full-path=\"OPS/book.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>";
    let members: &[(&[u8], &[u8])] = &[
        (b"META-INF/container.xml", container),
        (b"OPS/book.opf", opf),
        (b"OPS/chapter.xhtml", b"<html><body><p>Hello</p></body></html>"),
    ];
    let mut body = Vec::new();
    let mut central = Vec::new();
    for &(name, data) in members {
        let offset = body.len() as u32;
        let len = data.len() as u32;
        let nlen = name.len() as u16;
        body.extend_from_slice(&0x04034b50u32.to_le_bytes());
        body.extend_from_slice(&20u16.to_le_bytes());
        body.extend_from_slice(&0u16.to_le_bytes());
        body.extend_from_slice(&0u16.to_le_bytes());
        body.extend_from_slice(&0u16.to_le_bytes());
        body.extend_from_slice(&0u16.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        body.extend_from_slice(&len.to_le_bytes());
        body.extend_from_slice(&len.to_le_bytes());
        body.extend_from_slice(&nlen.to_le_bytes());
        body.extend_from_slice(&0u16.to_le_bytes());
        body.extend_from_slice(name);
        body.extend_from_slice(data);

        central.extend_from_slice(&0x02014b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&len.to_le_bytes());
        central.extend_from_slice(&len.to_le_bytes());
        central.extend_from_slice(&nlen.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    let count = members.len() as u16;
    let csize = central.len() as u32;
    let coff = body.len() as u32;
    let mut zip = body;
    zip.extend_from_slice(&central);
    zip.extend_from_slice(&0x06054b50u32.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(&count.to_le_bytes());
    zip.extend_from_slice(&count.to_le_bytes());
    zip.extend_from_slice(&csize.to_le_bytes());
    zip.extend_from_slice(&coff.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip
}

fn bomb_opf(nested: bool, attribute: bool) -> Vec<u8> {
    let mut s = String::from("<!DOCTYPE package [<!ENTITY a '");
    for _ in 0..1024 { s.push('x'); }
    s.push_str("'>");
    if nested {
        s.push_str("<!ENTITY b '");
        for _ in 0..32 { s.push_str("&a;"); }
        s.push_str("'>");
    }
    s.push_str("]><package><metadata><title>");
    if !attribute {
        for _ in 0..(if nested { 32 } else { 1024 }) { s.push_str(if nested { "&b;" } else { "&a;" }); }
    } else {
        s.push_str("Book");
    }
    s.push_str("</title></metadata><manifest><item id=\"c\" href=\"");
    if attribute {
        for _ in 0..(if nested { 32 } else { 1024 }) { s.push_str(if nested { "&b;" } else { "&a;" }); }
    } else {
        s.push_str("chapter.xhtml");
    }
    s.push_str("\" media-type=\"application/xhtml+xml\"/></manifest><spine><itemref idref=\"c\"/></spine></package>");
    s.into_bytes()
}

#[test]
fn admit_and_open_bound_opf_entity_expansion_and_allow_large_plain_opf() {
    use newt_core::{
        admission, canonical,
        inflate::{BitBuffer, DICTIONARY_SIZE, State},
        package_parsers::ManifestItem,
        zip_container::{Entry, InflateScratch},
        zip_window::arena::Arena,
    };

    let mut plain = String::from("<package><metadata><title>Plain</title></metadata><manifest><item id=\"c\" href=\"chapter.xhtml\" media-type=\"application/xhtml+xml\"/>");
    for i in 0..280 {
        plain.push_str(&format!("<item id=\"x{i:03}\" href=\"chapter.xhtml\" media-type=\"application/xhtml+xml\"/>"));
    }
    plain.push_str("</manifest><spine><itemref idref=\"c\"/></spine></package>");
    assert!(plain.len() > 20_000);

    let cases: &[(&str, Vec<u8>, Status)] = &[
        ("large-plain", plain.into_bytes(), Status::Ok),
        ("nested-text", bomb_opf(true, false), Status::ParseError),
        ("flat-text", bomb_opf(false, false), Status::ParseError),
        ("nested-attr", bomb_opf(true, true), Status::ParseError),
        ("flat-attr", bomb_opf(false, true), Status::ParseError),
    ];

    for (name, opf, expected) in cases {
        let epub = stored_epub(opf);
        let mut source = OwnedBytes(epub.clone());
        let mut cb = vec![0; 64 * 1024];
        let mut pb = vec![0; 64 * 1024];
        let mut catalog = Arena::new(&mut cb);
        let mut parse = Arena::new(&mut pb);
        let mut scan = [0; 4099];
        let mut sequential = [0; 1024];
        let mut entries = vec![Entry::default(); 64];
        let mut manifest = vec![ManifestItem::default(); 300];
        let mut spine_hashes = vec![0; 300];
        let mut spine = vec![0; 64];
        let mut state = Box::new(State::new(BitBuffer::Bits64));
        let mut dictionary = Box::new([0u8; DICTIONARY_SIZE]);
        let mut input = [0u8; 2048];
        let mut chunk = [0; 2048];
        let mut xml_buffer = [0; 2048];
        let mut filter = EntityFilter::new();
        let mut backend = Expat::default();
        let mut resources = canonical::Resources {
            policy: None,
            scan: &mut scan,
            sequential: &mut sequential,
            entries: &mut entries,
            manifest: &mut manifest,
            spine_hashes: &mut spine_hashes,
            spine: &mut spine,
            backend: &mut backend,
            decoder: Some(InflateScratch {
                state: &mut state,
                dictionary: &mut dictionary,
                input: &mut input,
            }),
            chunk: Some(&mut chunk),
            xml_buffer: Some(&mut xml_buffer),
            filter: Some(&mut filter),
        };
        let status = admission::admit(&mut source, &mut catalog, &mut parse, &mut resources, None, None);
        assert_eq!(status, *expected, "admit {name}");
        assert_eq!(catalog.used(), 0, "admit resets catalog arena: {name}");
        assert_eq!(parse.used(), 0, "admit resets parse arena: {name}");
        assert_eq!(backend.created, backend.closed, "admit closes parser: {name}");

        let mut source = OwnedBytes(epub);
        let mut backend = Expat::default();
        let mut resources = canonical::Resources {
            policy: None,
            scan: &mut scan,
            sequential: &mut sequential,
            entries: &mut entries,
            manifest: &mut manifest,
            spine_hashes: &mut spine_hashes,
            spine: &mut spine,
            backend: &mut backend,
            decoder: Some(InflateScratch {
                state: &mut state,
                dictionary: &mut dictionary,
                input: &mut input,
            }),
            chunk: Some(&mut chunk),
            xml_buffer: Some(&mut xml_buffer),
            filter: Some(&mut filter),
        };
        let opened = canonical::open(&mut source, &mut catalog, &mut resources, false, None, None);
        match *expected {
            Status::Ok => {
                let book = opened.expect(name);
                assert_eq!(book.package().spine_count, 1);
            }
            err => assert_eq!(opened.err(), Some(err), "open {name}"),
        }
        assert_eq!(backend.created, backend.closed, "open closes parser: {name}");
    }
}
