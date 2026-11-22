extern crate std;
use super::{identity::*, namespace::*, segment::*};
use std::{format, vec::Vec};
#[test]
fn literal_identity_and_forms() {
    let mut key = [0; 61];
    make(&[0; 32], 2, &mut key);
    assert_eq!(&key[..3], b"c2-");
    assert_eq!(&key[3..55], &[b'0'; 52]);
    assert_eq!(&key[55..], b".ntci\0");
    assert!(matches(&key[..60]));
    key[54] = b'g';
    assert!(matches(&key[..60]));
    key[54] = b'1';
    assert!(!matches(&key[..60]));
    let route = "0".repeat(64);
    let page = format!("{route}-{route}.fibp");
    for (tail, form) in [
        ("", Form::Canonical),
        (".bak", Form::Backup),
        (".0123abcd.tmp", Form::Temporary),
    ] {
        let p = PageName::parse(Some(format!("{page}{tail}").as_bytes()));
        assert_eq!((p.kind, p.form, p.canonical_bytes), (Kind::Page, form, 134));
    }
    assert_eq!(PageName::parse(Some(b"bad.bak")), PageName::default());
    let p = PageName::parse(Some(format!("{route}-unknown.bak").as_bytes()));
    assert_eq!(
        (p.kind, p.form, p.canonical_bytes),
        (Kind::Unknown, Form::Backup, 0)
    );
    assert_eq!(
        cache_folder(Some(format!("{route}.fibp\0ignored").as_bytes())),
        None
    );
    assert_eq!(cache_folder(Some(page.as_bytes())), Some("Meta"));
}
#[test]
fn limits_and_wrapping() {
    assert_eq!(
        (
            PAGE_QUOTA,
            PAGE_CANDIDATE,
            PAGE_CREDIT_UNIT,
            PAGE_REQUIRED_UNITS
        ),
        (33554432, 4198464, 65536, 65)
    );
    assert_eq!(
        (
            PAGE_ENTRY_CAP,
            PAGE_ENTRY_RESERVE,
            PAGE_ENTRY_GRANT,
            PAGE_SEGMENT_BATCH
        ),
        (128, 8, 120, 4)
    );
    for (pages, count) in [
        (0, 0),
        (128, 0),
        (129, 2),
        (4096, 32),
        (4097, 33),
        (u32::MAX - 127, 33554431),
        (u32::MAX - 126, 0),
        (u32::MAX, 0),
    ] {
        assert_eq!(ChapterSegment::count(pages), count);
    }
    let s = ChapterSegment {
        index: u32::MAX,
        pages: 1,
        chars: 0,
    };
    assert_eq!((s.first(), s.file_pages()), (4294967168, 128));
    let mut n = u64::MAX;
    assert!(!add(&mut n, 1));
    assert_eq!(n, u64::MAX);
    assert!(add(&mut n, 0));
    let mut set = PageNameSet::default();
    assert!(!set.may_contain(b""));
    set.add(b"a\0b");
    assert!(set.may_contain(b"a\0b"));
    let need = PageCacheNeed::default();
    assert_eq!((need.files, need.short_chapter), (1, false));
}
#[test]
fn map_failure_outputs_and_bound_sha_inputs() {
    let mut name = [b'z'; 128];
    let before = name;
    let segment = ChapterSegment {
        index: 7,
        pages: 257,
        chars: u32::MAX,
    };
    let mut seen = Vec::new();
    assert!(!segment_name(segment, &mut name, &mut |input, digest| {
        seen.extend_from_slice(input);
        digest.fill(9);
        false
    }));
    assert_eq!(name, before);
    assert_eq!(&seen[..64], &[b'z'; 64]);
    assert_eq!(&seen[64..], &[7, 0, 0, 0, 1, 1, 0, 0, 255, 255, 255, 255]);
    let mut bytes = [165; 156];
    let mut calls = 0;
    assert_eq!(
        encode(&mut bytes, 3, 128, 100, &[], &mut |_, _| {
            calls += 1;
            false
        }),
        0
    );
    assert_eq!(bytes, [165; 156]);
    assert_eq!(calls, 0);
    assert_eq!(
        encode(&mut bytes, 3, 129, 100, &[0, 50], &mut |input, _| {
            assert_eq!(input.len(), 32);
            false
        }),
        0
    );
    assert_eq!(&bytes[..4], b"NTSM");
    assert_eq!(&bytes[32..], &[165; 124]);
    let mut out = SegmentAnchor { segment, start: 44 };
    let old = out;
    assert!(!find(&bytes[..36], 3, 0, &mut out, &mut |_, _| false));
    assert_eq!(out, old);
}
