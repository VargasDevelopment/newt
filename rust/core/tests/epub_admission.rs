use newt_core::{
    admission::*,
    zip_container::{Entry, Status},
    zip_window::{arena::Arena, source::BookSource},
};
struct Source {
    bytes: Vec<u8>,
    max: usize,
    fail: Option<usize>,
    calls: usize,
}
impl Source {
    fn new(bytes: Vec<u8>, max: usize) -> Self {
        Self {
            bytes,
            max,
            fail: None,
            calls: 0,
        }
    }
}
impl BookSource for Source {
    fn size(&self) -> u64 {
        self.bytes.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let call = self.calls;
        self.calls += 1;
        if self.fail == Some(call) {
            return -1;
        }
        if at >= self.size() {
            return 0;
        }
        let n = out.len().min(self.max).min(self.bytes.len() - at as usize);
        out[..n].copy_from_slice(&self.bytes[at as usize..at as usize + n]);
        n as i32
    }
}
fn archive(flags: u16, local_flags: u16) -> Vec<u8> {
    let mut b = vec![0; 30];
    b[..4].copy_from_slice(&0x04034b50u32.to_le_bytes());
    b[6..8].copy_from_slice(&local_flags.to_le_bytes());
    b[26..28].copy_from_slice(&1u16.to_le_bytes());
    b.push(b'x');
    let mut c = [0; 46];
    c[..4].copy_from_slice(&0x02014b50u32.to_le_bytes());
    c[8..10].copy_from_slice(&flags.to_le_bytes());
    c[28..30].copy_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&c);
    b.push(b'x');
    let mut e = [0; 22];
    e[..4].copy_from_slice(&0x06054b50u32.to_le_bytes());
    e[8..10].copy_from_slice(&1u16.to_le_bytes());
    e[10..12].copy_from_slice(&1u16.to_le_bytes());
    e[12..16].copy_from_slice(&47u32.to_le_bytes());
    e[16..20].copy_from_slice(&31u32.to_le_bytes());
    b.extend_from_slice(&e);
    b
}
fn inspected(bytes: Vec<u8>, max: usize, cap: usize) -> Status {
    let mut source = Source::new(bytes, max);
    let mut scheduled = ScheduledSource::new(&mut source, None, None);
    let mut memory = vec![0; cap];
    let mut arena = Arena::new(&mut memory);
    let mut scan = [0; 4099];
    let status = inspect(&mut scheduled, &mut arena, &mut scan);
    assert_eq!(arena.used(), 0);
    status
}
#[test]
fn all_headers_short_read_and_cache_fallback() {
    for max in [1, 7, 32768] {
        for cap in [0, 4096] {
            assert_eq!(inspected(archive(0, 0), max, cap), Status::Ok);
        }
    }
}
#[test]
fn local_mismatch_precedes_encryption() {
    assert_eq!(inspected(archive(1, 0), 7, 4096), Status::Unsupported);
    assert_eq!(inspected(archive(1, 1), 7, 4096), Status::Encrypted);
    assert_eq!(inspected(archive(32, 32), 7, 0), Status::Unsupported);
}
#[test]
fn every_member_name_must_match() {
    let mut b = archive(0, 0);
    b[30] = b'y';
    assert_eq!(inspected(b, 3, 0), Status::Unsupported);
}
#[test]
fn premature_zero_is_scheduled_error() {
    let mut s = Source::new(vec![1], 0);
    let mut scheduled = ScheduledSource::new(&mut s, None, None);
    assert_eq!(scheduled.read_at(0, &mut [0]), -1);
    assert_eq!(scheduled.read_at(1, &mut [0]), 0);
    assert_eq!(scheduled.read_at(0, &mut []), 0);
}
#[test]
fn chapter_sequential_eof_and_failure() {
    let mut s = Source::new(b"abc".to_vec(), 1);
    let e = Entry::raw(3);
    let mut c = ChapterSource::open(&mut s, &e, None, None, None).unwrap();
    assert_eq!(c.read_at(1, &mut [0]), -1);
    assert_eq!(c.read_at(0, &mut [0; 8]), 1);
    assert!(!c.finish());
    assert_eq!(c.read_at(1, &mut [0; 8]), 1);
    assert_eq!(c.read_at(2, &mut [0; 8]), 1);
    assert_eq!(c.read_at(3, &mut [0]), 0);
    assert!(c.finish());
}
#[test]
fn required_stream_probe_and_oom() {
    let mut s = Source::new(b"abc".to_vec(), 1);
    let mut chunk = [0; 2048];
    assert_eq!(
        expand(
            &mut s,
            &Entry::raw(3),
            0,
            b"x.xml",
            None,
            Some(&mut chunk),
            None,
            None
        ),
        Status::Ok
    );
    assert_eq!(
        expand(&mut s, &Entry::raw(3), 0, b"x", None, None, None, None),
        Status::OutOfMemory
    );
    let e = Entry::raw(4);
    assert_eq!(
        expand(&mut s, &e, 0, b"x", None, Some(&mut chunk), None, None),
        Status::IoError
    );
}
#[test]
fn inflate_budget_boundary() {
    let mut e = Entry::raw(8);
    e.method = 8;
    e.compressed_size = 65545;
    assert!(inflate_input_bounded(&e));
    e.compressed_size += 1;
    assert!(!inflate_input_bounded(&e));
}
#[test]
fn path_and_category() {
    use newt_core::phased_metadata::admissible_href as safe;
    assert!(safe(b"OPS/a.xhtml", 512));
    for p in [
        b"../x".as_slice(),
        b"/x",
        b"x//y",
        b"x/./y",
        b"x\\y",
        b"x\0y",
        b"\xff",
    ] {
        assert!(!safe(p, 512));
    }
    assert_eq!(category(b"OPS/NAV.XHTML"), 2);
    assert_eq!(category(b"a.JPG"), 1);
}
/// Home's title record (#317) rejects a split character, so a long title
/// must reach it cut at a whole character with the record's own predicate.
#[test]
fn display_title_cut_keeps_whole_characters() {
    use newt_core::{phased_metadata::copy_display_title, utf8};
    let long = "é".repeat(200); // 400 bytes; a plain 255-byte cut splits a character
    let mut title = [0xaa; 256];
    copy_display_title(&mut title, Some(long.as_bytes()));
    let length = title.iter().position(|&b| b == 0).unwrap();
    assert_eq!(length, 254);
    let (mut at, mut scalar) = (0, 0);
    while at < length {
        assert!(utf8::next(Some(&title[..length]), &mut at, &mut scalar) && scalar != 0);
    }
}
static EVENTS: std::sync::Mutex<Vec<WorkEvent>> = std::sync::Mutex::new(Vec::new());
fn record(e: WorkEvent) {
    EVENTS.lock().unwrap().push(e);
}
#[test]
fn exact_required_events() {
    let mut s = Source::new(b"ab".to_vec(), 1);
    let mut chunk = [0; 2048];
    EVENTS.lock().unwrap().clear();
    assert_eq!(
        expand(
            &mut s,
            &Entry::raw(2),
            7,
            b"a.XML",
            None,
            Some(&mut chunk),
            None,
            Some(record)
        ),
        Status::Ok
    );
    let e = EVENTS.lock().unwrap();
    assert_eq!(
        e.iter().map(|v| v.kind).collect::<Vec<_>>(),
        vec![
            WorkKind::MemberBegin,
            WorkKind::StreamBegin,
            WorkKind::StreamEnd,
            WorkKind::StreamBegin,
            WorkKind::StreamEnd,
            WorkKind::StreamBegin,
            WorkKind::StreamEnd,
            WorkKind::MemberEnd
        ]
    );
    assert_eq!(e[0].member, 7);
    assert_eq!(e[0].category, 2);
    assert_eq!([e[2].result, e[4].result, e[6].result], [1, 1, 0]);
    assert_eq!(e[7].result, 0);
}
struct RejectXml {
    created: usize,
}
impl newt_core::xml_session::ParserBackend for RejectXml {
    fn create(&mut self) -> bool {
        self.created += 1;
        false
    }
    fn feed(
        &mut self,
        _: &[u8],
        _: bool,
        _: &mut dyn newt_core::xml_session::Handler,
    ) -> bool {
        panic!("no XML feed expected")
    }
    fn close(&mut self) {}
}
#[test]
fn canonical_missing_container_and_duplicate_reset_without_xml() {
    use newt_core::{canonical, package_parsers::ManifestItem};
    for duplicate in [false, true] {
        let mut data = archive(0, 0);
        if duplicate {
            let central = data[31..78].to_vec();
            data.splice(78..78, central);
            let end = data.len() - 22;
            data[end + 8..end + 10].copy_from_slice(&2u16.to_le_bytes());
            data[end + 10..end + 12].copy_from_slice(&2u16.to_le_bytes());
            data[end + 12..end + 16].copy_from_slice(&94u32.to_le_bytes());
        }
        let mut source = Source::new(data, 3);
        let mut names = [0; 32];
        let mut parse_bytes = [0; 4096];
        let mut names = Arena::new(&mut names);
        let mut parse = Arena::new(&mut parse_bytes);
        let mut scan = [0; 4099];
        let mut seq = [0; 1024];
        let mut entries = [Entry::default(); 2];
        let mut manifest = [ManifestItem::default(); 1];
        let mut hashes = [0; 1];
        let mut spine = [0; 1];
        let mut backend = RejectXml { created: 0 };
        let mut chunk = [0; 2048];
        let mut xml = [0; 2048];
        let mut r = canonical::Resources {
                policy: None,
            scan: &mut scan,
            sequential: &mut seq,
            entries: &mut entries,
            manifest: &mut manifest,
            spine_hashes: &mut hashes,
            spine: &mut spine,
            backend: &mut backend,
            decoder: None,
            chunk: Some(&mut chunk),
            xml_buffer: Some(&mut xml),
            filter: None,
        };
        let status = admit(&mut source, &mut names, &mut parse, &mut r, None, None);
        assert_eq!(
            status,
            if duplicate {
                Status::Unsupported
            } else {
                Status::NotEpub
            }
        );
        assert_eq!(names.used(), 0);
        assert_eq!(parse.used(), 0);
        assert!(names.high_water() > 0);
        assert_eq!(backend.created, 0);
    }
}
