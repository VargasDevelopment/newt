use super::*;
use crate::cache_names::namespace::{Kind, PageName, cache_folder, envelope_kind};
use crate::page_envelope::{route, MAX_PAYLOAD};

const BOOK: Identity = Identity { digest: [7; 32], policy: 3 };

fn header(members: u32, spine: u32) -> Header {
    Header { identity: BOOK, members, spine, spine_bytes: 900, nav: 1, ncx: NONE, cover: 0 }
}
fn encoded(h: &Header) -> [u8; HEADER_BYTES as usize] {
    let mut out = [0; HEADER_BYTES as usize];
    h.encode(&mut out);
    out
}

#[test]
fn layout_bounds_and_offsets() {
    assert_eq!(payload_bytes(1, 1), Some(128 + 32 + 16));
    for (members, spine) in [(0, 1), (1, 0), (MAX_MEMBERS + 1, 1), (1, MAX_SPINE + 1)] {
        assert_eq!(payload_bytes(members, spine), None, "{members} members, {spine} spine");
    }
    let largest = payload_bytes(MAX_MEMBERS, MAX_SPINE).unwrap();
    assert_eq!(largest, 2_621_504);
    assert!(largest <= MAX_PAYLOAD);
    let h = header(3, 2);
    assert_eq!((h.member_offset(0), h.member_offset(2), h.member_offset(3)), (Some(128), Some(192), None));
    assert_eq!((h.spine_offset(0), h.spine_offset(1), h.spine_offset(2)), (Some(224), Some(240), None));
    assert_eq!(h.payload_bytes(), Some(256));
}

#[test]
fn header_round_trip_and_every_refusal() {
    let h = header(3, 2);
    let bytes = encoded(&h);
    assert_eq!(Header::decode(&bytes, &BOOK, 256), Some(h));
    // Another book, another admission policy, or a payload of another size.
    assert_eq!(Header::decode(&bytes, &Identity { digest: [8; 32], ..BOOK }, 256), None);
    assert_eq!(Header::decode(&bytes, &Identity { policy: 4, ..BOOK }, 256), None);
    assert_eq!(Header::decode(&bytes, &BOOK, 257), None);
    let mutations: [(usize, u8); 4] = [(0, b'X'), (4, 2), (72, 1), (127, 1)];
    for (at, value) in mutations {
        let mut bad = bytes;
        bad[at] = value;
        assert_eq!(Header::decode(&bad, &BOOK, 256), None, "byte {at}");
    }
    for bad in [
        Header { nav: 3, ..h },
        Header { cover: 3, ..h },
        Header { spine_bytes: EPUB_MAX_EXPANDED_BYTES + 1, ..h },
    ] {
        assert_eq!(Header::decode(&encoded(&bad), &BOOK, 256), None, "{bad:?}");
    }
    let none = Header { nav: NONE, ncx: NONE, cover: NONE, ..h };
    assert_eq!(Header::decode(&encoded(&none), &BOOK, 256), Some(none));
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
    for item in [SpineItem { member: 2, before: 0 }, SpineItem { member: 0, before: 900 }] {
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
