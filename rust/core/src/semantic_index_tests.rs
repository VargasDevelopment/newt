use super::*;
const P: u32 = POLICY_VERSION;

fn start(c: &mut Counter, name: &[u8]) {
    c.start_element(name, false, false, None);
}
#[test]
fn extraction_events_and_finish() {
    let mut c = Counter::default();
    c.text(b"outside");
    start(&mut c, b"x:body");
    c.text(b" \tA\n");
    start(&mut c, b"a:b:script");
    c.text(b"skip");
    start(&mut c, b"title");
    c.text(b"skip");
    c.end_element(b"title");
    c.end_element(b"script");
    c.text(b"\xc3\xa9");
    c.start_element(b"span", true, false, None);
    c.text(b"hidden");
    start(&mut c, b"br");
    c.end_element(b"br");
    c.end_element(b"span");
    c.text(b"\x80\xbf\x0b\0"); // Continuations do not count; VT and NUL do.
    assert_eq!(c.finish(), Some(6));
    c.text(b"ignored");
    start(&mut c, b"br");
    c.end_element(b"body");
    assert_eq!(c.finish(), Some(6));
    c.reset();
    assert_eq!(c.finish(), Some(0));
}
#[test]
fn every_boundary_tag_and_image_presence() {
    for name in [
        b"p".as_slice(),
        b"h1",
        b"h2",
        b"h3",
        b"h4",
        b"h5",
        b"h6",
        b"blockquote",
        b"li",
        b"div",
        b"section",
        b"article",
        b"figure",
        b"aside",
        b"figcaption",
        b"ul",
        b"ol",
        b"table",
        b"tr",
        b"td",
        b"th",
        b"dt",
        b"dd",
        b"hr",
    ] {
        let mut c = Counter::default();
        start(&mut c, b"body");
        c.text(b"a ");
        start(&mut c, name);
        c.text(b" b");
        c.end_element(name);
        c.text(b" c");
        assert_eq!(c.finish(), Some(if name == b"hr" { 4 } else { 3 }));
    }
    // A picture with a source breaks the text and, unless its alt marks it
    // decorative, takes one position (#179); "a b" without a source is 3.
    for name in [b"img".as_slice(), b"image"] {
        for (source, alt, chars) in [(false, None, 3), (true, None, 3), (true, Some(&b"Map"[..]), 3),
            (true, Some(&b""[..]), 2), (true, Some(&b" \t "[..]), 2)] {
            let mut c = Counter::default();
            start(&mut c, b"body");
            c.text(b"a ");
            c.start_element(name, false, source, alt);
            c.end_element(name);
            c.text(b" b");
            assert_eq!(c.finish(), Some(chars), "{name:?} source={source} alt={alt:?}");
        }
    }
}
#[test]
fn depth_fifteen_and_overflow_inherit() {
    for hidden_top in [false, true] {
        let mut c = Counter::default();
        start(&mut c, b"body");
        for _ in 0..14 {
            start(&mut c, b"span");
        }
        c.start_element(b"span", hidden_top, false, None);
        c.start_element(b"span", !hidden_top, false, None);
        c.text(b"x");
        c.end_element(b"span");
        c.text(b"y");
        c.end_element(b"span");
        c.text(b"z");
        assert_eq!(c.finish(), Some(if hidden_top { 1 } else { 3 }));
    }
}
#[test]
fn seams_and_chunking() {
    for n in [3070, 3071, 3072] {
        let mut c = Counter::default();
        start(&mut c, b"body");
        for _ in 0..n {
            c.text(b"a");
        }
        c.text(b" ");
        c.text(b"b c");
        assert_eq!(c.finish(), Some(n + 4));
    }
    for n in [3070, 3071, 3072] {
        let mut c = Counter::default();
        start(&mut c, b"body");
        for _ in 0..n {
            c.text(b"a");
        }
        c.text(b" ");
        start(&mut c, b"br");
        c.end_element(b"br");
        c.text(b"b");
        assert_eq!(c.finish(), Some(n + if n == 3071 { 2 } else { 3 }));
    }
    let mut c = Counter::default();
    start(&mut c, b"body");
    for _ in 0..3070 {
        c.text(b"a");
    }
    c.text(b"\xf0\x9f");
    c.text(b"\x98\x80 z");
    assert_eq!(c.finish(), Some(3073));
}
#[test]
fn leading_byte_count_overflow_and_sticky_failure() {
    let mut c = Counter {
        in_body: true,
        characters: u32::MAX - 1,
        ..Counter::default()
    };
    c.text(b"\xc3\xa9");
    assert_eq!(c.finish(), Some(u32::MAX));
    c.reset();
    c.in_body = true;
    c.characters = u32::MAX;
    c.text(b"\x80");
    assert!(!c.failed);
    c.text(b"x");
    assert_eq!(c.finish(), None);
    assert_eq!(c.finish(), None);
    c.reset();
    assert_eq!(c.finish(), Some(0));
}
#[test]
fn all_digest_bytes_and_policy_are_binding() {
    let digest = [0x5a; 32];
    for byte in 0..32 {
        let mut i = Index::default();
        assert!(i.bind(&digest, P, 1));
        assert!(i.record(&digest, P, 0, 9));
        let mut other = digest;
        other[byte] ^= 1;
        assert!(!i.complete(&other, P));
        assert_eq!(i.spine_count(&other, P), 0);
        assert_eq!(i.chapter_count(&other, P, 0), None);
        assert_eq!(i.project(&other, P, 0, 0), None);
        assert!(i.complete(&digest, P));
        assert!(!i.record(&other, P, 0, 9));
        assert!(!i.complete(&digest, P));
    }
    let mut i = Index::default();
    i.bind(&digest, P, 1);
    i.record(&digest, P, 0, 9);
    assert!(!i.complete(&digest, P + 1));
    assert_eq!(i.spine_count(&digest, P + 1), 0);
    assert_eq!(i.chapter_count(&digest, P + 1, 0), None);
    assert_eq!(i.project(&digest, P + 1, 0, 0), None);
    assert!(!i.record(&digest, P + 1, 0, 9));
    assert_eq!(i.spine_count(&digest, P), 0);
}
#[test]
fn canonical_cap_holes_conflicts_and_wide_sum() {
    let d = [7; 32];
    let mut i = Index::default();
    assert!(!i.bind(&d, P, 0));
    assert!(!i.bind(&d, P + 1, 1));
    assert!(!i.bind(&d, P, 1025));
    assert!(i.bind(&d, P, 1024));
    assert!(i.record(&d, P, 1023, u32::MAX));
    assert_eq!(i.chapter_count(&d, P, 0), None);
    assert_eq!(i.chapter_count(&d, P, 1023), Some(u32::MAX));
    assert_eq!(i.project(&d, P, 1023, 0), None);
    for spine in 0..1023 {
        assert!(i.record(&d, P, spine, u32::MAX));
    }
    assert!(i.complete(&d, P));
    assert!(i.record(&d, P, 0, u32::MAX));
    let total = 1024 * u64::from(u32::MAX);
    assert_eq!(i.project(&d, P, 1023, u32::MAX), Some((total, total)));
    assert_eq!(i.project(&d, P, 1024, 0), None);
    assert!(!i.record(&d, P, 0, 0));
    assert_eq!(i.spine_count(&d, P), 0);
    i.bind(&d, P, 1);
    assert!(!i.record(&d, P, 1, 0));
    assert_eq!(i.spine_count(&d, P), 0);
    i.bind(&d, P, 1);
    i.record(&d, P, 0, 0);
    assert!(i.complete(&d, P));
    assert_eq!(i.project(&d, P, 0, 0), None);
    i.bind(&d, P, 2);
    i.record(&d, P, 1, 3);
    i.record(&d, P, 0, 0);
    assert_eq!(i.project(&d, P, 1, 3), Some((3, 3)));
    assert_eq!(i.project(&d, P, 1, 4), None);
    assert!(!i.bind(&d, 0, 1));
    assert_eq!(i.chapter_count(&d, P, 1), None);
}
#[test]
fn c_string_local_name_and_unsigned_depth_wrap() {
    let mut c = Counter::default();
    start(&mut c, b"x:y:body\0ignored");
    c.text(b"a");
    c.suppressed = u32::MAX;
    start(&mut c, b"script");
    assert_eq!(c.suppressed, 0);
    c.stack_top = 15;
    c.overflow_depth = u32::MAX;
    start(&mut c, b"span");
    assert_eq!(c.overflow_depth, 0);
    assert_eq!(c.finish(), Some(1));
}
