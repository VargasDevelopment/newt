extern crate std;
use std::{collections::BTreeMap, vec::Vec};

use super::*;
use crate::cache_names::namespace::{Kind, PageName, cache_folder, envelope_kind};
use crate::page_envelope::{route, MAX_PAYLOAD};

const BOOK: Identity = Identity { digest: [7; 32], policy: 3 };

fn title(text: &[u8]) -> [u8; TITLE_BYTES] {
    let mut out = [0; TITLE_BYTES];
    out[..text.len()].copy_from_slice(text);
    out
}
fn header(members: u32, spine: u32) -> Header {
    Header {
        identity: BOOK, members, spine, spine_bytes: 900, nav: 1, ncx: NONE, cover: 0,
        language: BookLanguage::Japanese, title: title("A Book — 本".as_bytes()),
    }
}
fn encoded(h: &Header) -> [u8; HEADER_BYTES as usize] {
    let mut out = [0; HEADER_BYTES as usize];
    h.encode(&mut out);
    out
}

#[test]
fn layout_bounds_and_offsets() {
    assert_eq!(payload_bytes(1, 1), Some(384 + 32 + 16));
    for (members, spine) in [(0, 1), (1, 0), (MAX_MEMBERS + 1, 1), (1, MAX_SPINE + 1)] {
        assert_eq!(payload_bytes(members, spine), None, "{members} members, {spine} spine");
    }
    let largest = payload_bytes(MAX_MEMBERS, MAX_SPINE).unwrap();
    assert_eq!(largest, 2_621_760);
    assert!(largest <= MAX_PAYLOAD);
    let h = header(3, 2);
    assert_eq!((h.member_offset(0), h.member_offset(2), h.member_offset(3)), (Some(384), Some(448), None));
    assert_eq!((h.spine_offset(0), h.spine_offset(1), h.spine_offset(2)), (Some(480), Some(496), None));
    assert_eq!(h.payload_bytes(), Some(512));
}

#[test]
fn header_round_trip_and_every_refusal() {
    let h = header(3, 2);
    let bytes = encoded(&h);
    assert_eq!(Header::decode(&bytes, &BOOK, 512), Some(h));
    // Another book, another admission policy, or a payload of another size.
    assert_eq!(Header::decode(&bytes, &Identity { digest: [8; 32], ..BOOK }, 512), None);
    assert_eq!(Header::decode(&bytes, &Identity { policy: 4, ..BOOK }, 512), None);
    assert_eq!(Header::decode(&bytes, &BOOK, 513), None);
    // Magic, schema, an unknown language, padding, a title byte past its
    // NUL, a title that is not UTF-8, and the header's last byte.
    let end = 80 + h.title().len();
    let mutations: [(usize, u8); 7] = [(0, b'X'), (4, 2), (72, 4), (73, 1), (end + 1, b'x'), (80, 0xff), (383, 1)];
    for (at, value) in mutations {
        let mut bad = bytes;
        bad[at] = value;
        assert_eq!(Header::decode(&bad, &BOOK, 512), None, "byte {at}");
    }
    for bad in [
        Header { nav: 3, ..h },
        Header { cover: 3, ..h },
        Header { spine_bytes: EPUB_MAX_EXPANDED_BYTES + 1, ..h },
        // Only one navigation choice, and a title is never empty.
        Header { ncx: 0, ..h },
        Header { title: [0; TITLE_BYTES], ..h },
        Header { title: [b'x'; TITLE_BYTES], ..h },
    ] {
        assert_eq!(Header::decode(&encoded(&bad), &BOOK, 512), None, "{bad:?}");
    }
    let none = Header { nav: NONE, ncx: NONE, cover: NONE, language: BookLanguage::Default, ..h };
    assert_eq!(Header::decode(&encoded(&none), &BOOK, 512), Some(none));
}

#[test]
fn member_and_spine_round_trip_and_bounds() {
    let h = header(3, 2);
    let member = Member {
        hash: 0x0123_4567_89ab_cdef,
        local_offset: 40,
        compressed: 70_000,
        uncompressed: 4 * 1024 * 1024,
        method: 8,
        flags: 0x0808,
        first_spine: 1,
    };
    let mut m = [0; MEMBER_BYTES as usize];
    member.encode(&mut m);
    assert_eq!(Member::decode(&m, &h), Some(member));
    let unread = Member { first_spine: NONE, ..member };
    unread.encode(&mut m);
    assert_eq!(Member::decode(&m, &h), Some(unread));
    Member { first_spine: 2, ..member }.encode(&mut m);
    assert_eq!(Member::decode(&m, &h), None);
    member.encode(&mut m);
    m[31] = 1;
    assert_eq!(Member::decode(&m, &h), None);

    let mut s = [0; SPINE_BYTES as usize];
    for item in [SpineItem { member: 2, before: 0 }, SpineItem { member: 0, before: 900 }, SpineItem { member: NONE, before: 900 }] {
        item.encode(&mut s);
        assert_eq!(SpineItem::decode(&s, &h), Some(item));
    }
    for bad in [SpineItem { member: 3, before: 0 }, SpineItem { member: 0, before: 901 }] {
        bad.encode(&mut s);
        assert_eq!(SpineItem::decode(&s, &h), None, "{bad:?}");
    }
    SpineItem { member: 0, before: 0 }.encode(&mut s);
    s[15] = 1;
    assert_eq!(SpineItem::decode(&s, &h), None);
}

#[test]
fn index_names_route_to_their_own_folder() {
    let mut suffix = [0; 69];
    name(&[0xa5; 32], &mut suffix);
    assert_eq!(&suffix[..4], b"a5a5");
    assert_eq!(&suffix[64..], b".fibx");
    assert_eq!(envelope_kind(&suffix), Some(Kind::Index));
    assert_eq!(cache_folder(Some(&suffix)), Some("Index"));
    let mut digest = [0; 32];
    assert!(route(Some(&suffix), &mut digest, true));
    assert_eq!(digest, [0xa5; 32]);

    let key = [b'0'; 64];
    let mut full = [0; 134];
    full[..64].copy_from_slice(&key);
    full[64] = b'-';
    full[65..].copy_from_slice(&suffix);
    let parsed = PageName::parse(Some(&full));
    assert_eq!((parsed.kind, parsed.canonical_bytes), (Kind::Index, 134));
    // Pages keep their folder; an unknown envelope extension is metadata.
    suffix[68] = b'p';
    assert_eq!((envelope_kind(&suffix), cache_folder(Some(&suffix))), (Some(Kind::Page), None));
    suffix[68] = b'q';
    assert_eq!((envelope_kind(&suffix), cache_folder(Some(&suffix))), (None, Some("Meta")));
    assert!(!route(Some(&suffix), &mut digest, true));
}

/// Publishes at end_write like the card store; records each write's length.
#[derive(Default)]
struct Memory {
    files: BTreeMap<Vec<u8>, Vec<u8>>,
    open: Option<(Vec<u8>, Vec<u8>)>,
    writes: Vec<usize>,
    refuse_writes: bool,
}
impl CacheStorage for Memory {
    fn remove(&mut self, name: &[u8]) -> bool {
        self.files.remove(name).is_some()
    }
    fn file_size(&mut self, name: &[u8]) -> i64 {
        self.files.get(name).map_or(-1, |f| f.len() as i64)
    }
    fn read_at(&mut self, name: &[u8], offset: u32, dst: &mut [u8]) -> i32 {
        let Some(src) = self.files.get(name).and_then(|f| f.get(offset as usize..offset as usize + dst.len()))
        else { return -1 };
        dst.copy_from_slice(src);
        dst.len() as i32
    }
    fn begin_write(&mut self, name: &[u8]) -> bool {
        self.open = Some((name.to_vec(), Vec::new()));
        true
    }
    fn write(&mut self, bytes: &[u8]) -> bool {
        self.writes.push(bytes.len());
        !self.refuse_writes && self.open.as_mut().map(|(_, f)| f.extend_from_slice(bytes)).is_some()
    }
    fn end_write(&mut self) -> bool {
        self.open.take().map(|(name, file)| self.files.insert(name, file)).is_some()
    }
}

fn members(hashes: &[u64]) -> impl Iterator<Item = Member> + '_ {
    hashes.iter().enumerate().map(|(i, &hash)| Member {
        hash,
        local_offset: 100 * i as u32,
        first_spine: if i < 2 { i as u32 } else { NONE },
        ..Member::default()
    })
}
const SPINE: [SpineItem; 2] = [SpineItem { member: 1, before: 0 }, SpineItem { member: 0, before: 400 }];

fn build(s: &mut Memory, hashes: &[u64], buffer: &mut [u8]) -> Result<(), WriteError> {
    let mut w = Writer::begin(s, header(hashes.len() as u32, 2), buffer)?;
    for m in members(hashes) {
        w = w.member(s, m)?;
    }
    for item in SPINE {
        w = w.spine(s, item)?;
    }
    w.finish(s, &mut || true)
}

#[test]
fn written_index_reads_back_by_hash_and_position() {
    let hashes: Vec<u64> = (1..=200).map(|i| i * 1_000_003).collect();
    let mut s = Memory::default();
    // A 4 KiB buffer turns 202 records into two card writes, not 202.
    build(&mut s, &hashes, &mut [0; 4096]).unwrap();
    assert_eq!(s.writes, [4096, 384 + 200 * 32 + 2 * 16 - 4096]);

    let index = Reader::open(&mut s, &BOOK).unwrap();
    assert_eq!(*index.header(), header(200, 2));
    for (i, m) in members(&hashes).enumerate() {
        assert_eq!(index.find(&mut s, m.hash), Ok(Some((i as u32, m))));
    }
    for absent in [0, 1_000_004, u64::MAX] {
        assert_eq!(index.find(&mut s, absent), Ok(None));
    }
    assert_eq!([index.spine(&mut s, 0), index.spine(&mut s, 1)], SPINE.map(Some));
    assert_eq!((index.member(&mut s, 200), index.spine(&mut s, 2)), (None, None));

    // The same file under another admission policy is stale, not readable.
    assert_eq!(Reader::open(&mut s, &Identity { policy: 4, ..BOOK }), None);
    let file = s.files.values_mut().next().unwrap();
    file.truncate(file.len() - 1);
    assert_eq!(Reader::open(&mut s, &BOOK), None);
}

#[test]
fn writer_refuses_collisions_order_and_counts_before_publishing() {
    let refused = |hashes: &[u64]| {
        let mut s = Memory::default();
        let result = build(&mut s, hashes, &mut [0; 384]);
        assert!(s.files.is_empty());
        result
    };
    assert_eq!(refused(&[5, 9, 9]), Err(WriteError::Collision));
    assert_eq!(refused(&[5, 9, 7]), Err(WriteError::Invalid));

    let mut s = Memory::default();
    let mut buffer = [0; 384];
    // Header counts outside the layout, or no room for a header, never reach the card.
    for bad in [header(0, 2), Header { nav: 3, ..header(3, 2) }] {
        assert_eq!(Writer::begin(&mut s, bad, &mut buffer).err(), Some(WriteError::Invalid));
    }
    assert_eq!(Writer::begin(&mut s, header(3, 2), &mut [0; 383]).err(), Some(WriteError::Invalid));
    assert!(s.open.is_none());

    let one = Member { hash: 1, ..Member::default() };
    fn begin<'b>(s: &mut Memory, buffer: &'b mut [u8]) -> Writer<'b> {
        Writer::begin(s, Header { nav: NONE, ..header(1, 2) }, buffer).unwrap()
    }
    let w = begin(&mut s, &mut buffer);
    let first = SpineItem { member: 0, before: 0 };
    assert_eq!(w.spine(&mut s, first).err(), Some(WriteError::Invalid), "spine before members");
    let w = begin(&mut s, &mut buffer).member(&mut s, one).unwrap();
    assert_eq!(w.member(&mut s, Member { hash: 2, ..one }).err(), Some(WriteError::Invalid), "extra member");
    let w = begin(&mut s, &mut buffer).member(&mut s, one).unwrap().spine(&mut s, SPINE[1]).unwrap();
    assert_eq!(w.spine(&mut s, SpineItem { member: 0, before: 399 }).err(), Some(WriteError::Invalid), "backwards");
    let w = begin(&mut s, &mut buffer).member(&mut s, one).unwrap();
    assert_eq!(w.finish(&mut s, &mut || true), Err(WriteError::Invalid), "short spine");
    assert!(s.files.is_empty());
}

#[test]
fn storage_faults_never_publish() {
    let mut s = Memory { refuse_writes: true, ..Memory::default() };
    assert_eq!(build(&mut s, &[1, 2, 3], &mut [0; 384]), Err(WriteError::Storage));
    let mut s = Memory::default();
    let mut buffer = [0; 4096];
    let mut w = Writer::begin(&mut s, header(3, 2), &mut buffer).unwrap();
    for m in members(&[1, 2, 3]) {
        w = w.member(&mut s, m).unwrap();
    }
    for item in SPINE {
        w = w.spine(&mut s, item).unwrap();
    }
    assert_eq!(w.finish(&mut s, &mut || false), Err(WriteError::Storage));
    assert!(s.files.is_empty());
    assert_eq!(Reader::open(&mut s, &BOOK), None);
}

#[test]
fn schema_name_hash_vectors() {
    assert_eq!(name_hash(b""), 0xefd01f60ba992926);
    assert_eq!(name_hash(b"a"), 0x82a2a958a9bece5b);
    assert_eq!(name_hash(b"OEBPS/chapter.xhtml"), 0x7edbfdbcb01ee0f8);
    assert_eq!(name_hash(b"a\0ignored"), 0x82a2a958a9bece5b);
}
