// Independent field/byte expectations; root compiles and executes.
use newt_core::{xml_input::*, zip_window::arena::Arena};
struct Pull<'a> {
    data: &'a [u8],
    chunk: usize,
    call: usize,
    fail: Option<usize>,
}
impl BytePull for Pull<'_> {
    fn pull(&mut self, out: &mut [u8]) -> i32 {
        let call = self.call;
        self.call += 1;
        if self.fail == Some(call) {
            return -7;
        }
        let n = self.data.len().min(self.chunk).min(out.len());
        out[..n].copy_from_slice(&self.data[..n]);
        self.data = &self.data[n..];
        n as i32
    }
}
#[test]
fn filter_error_after_output_and_retry() {
    let mut p = Pull {
        data: b"ab&copy;",
        chunk: 2,
        call: 0,
        fail: Some(1),
    };
    let mut f = EntityFilter::new();
    let mut out = [0xa5; 9];
    assert_eq!(f.read(&mut p, &mut out), -1);
    assert_eq!(&out[..3], &[b'a', b'b', 0xa5]);
    assert_eq!(f.read(&mut p, &mut out), 2);
    assert_eq!(&out[..2], "©".as_bytes());
    assert_eq!(f.read(&mut p, &mut out), 0);
}
#[test]
fn filter_carry_zero_cap_eof_reset() {
    let mut p = Pull {
        data: b"&notknown;&amp;&#x0;&&x",
        chunk: 1,
        call: 0,
        fail: None,
    };
    let mut f = EntityFilter::new();
    assert_eq!(f.read(&mut p, &mut []), 0);
    assert_eq!(p.call, 0);
    let expected = "�&amp;&#x0;&amp;&amp;x".as_bytes();
    for b in expected {
        let mut out = [0xa5];
        assert_eq!(f.read(&mut p, &mut out), 1);
        assert_eq!(out[0], *b);
    }
    let mut out = [0xa5; 2];
    assert_eq!(f.read(&mut p, &mut out), 0);
    assert_eq!(out, [0xa5; 2]);
    f.reset();
    let mut p = Pull {
        data: b"&yen;",
        chunk: 512,
        call: 0,
        fail: None,
    };
    assert_eq!(f.read(&mut p, &mut out), 2);
    assert_eq!(&out, "¥".as_bytes());
}
#[test]
fn raw_carry_and_pending_discard_on_reset() {
    let mut f = EntityFilter::new();
    let mut p = Pull {
        data: b"&mdash;tail",
        chunk: 512,
        call: 0,
        fail: None,
    };
    let mut one = [0];
    assert_eq!(f.read(&mut p, &mut one), 1);
    assert_eq!(one, [0xe2]);
    f.reset();
    let mut p = Pull {
        data: b"Q",
        chunk: 1,
        call: 0,
        fail: None,
    };
    assert_eq!(f.read(&mut p, &mut one), 1);
    assert_eq!(one, *b"Q");
}
#[test]
fn xml_fields_and_trim_storage() {
    assert_eq!(local_name(b"a:b:c\0:d"), b"c");
    assert!(!local_is(b"a:b:c", b"b:c"));
    let atts = [
        (b"a:k".as_slice(), b"first".as_slice()),
        (b"k".as_slice(), b"second".as_slice()),
    ];
    assert_eq!(attr_local(&atts, b"k"), Some(b"first".as_slice()));
    assert_eq!(attr_exact(&atts, b"k"), Some(b"second".as_slice()));
    assert!(!has_token(Some(b"x\x0by"), b"y"));
    assert!(has_token(Some(b"x \t"), b""));
    assert!(!has_token(Some(b""), b""));
    let mut t = TextCapture::default();
    t.add(b"ignored");
    assert_eq!(t.len, 0);
    t.begin();
    t.add(b" \ta \r");
    assert_eq!(t.trimmed(), b"a");
    assert_eq!(t.len, 5);
    assert_eq!(&t.buf[..6], b" \ta\0\r\0");
    t.add(b"Z");
    assert_eq!(t.len, 6);
    assert_eq!(t.trimmed(), b"a");
    t.end();
    t.add(b"bad");
    assert_eq!(t.len, 6);
    t.begin();
    t.add(&[b'X'; 300]);
    assert_eq!(t.len, 255);
    assert_eq!(t.buf[255], 0);
}
#[test]
fn encryption_all_matching_attributes_and_sticky() {
    let mut s = EncryptionScan::default();
    s.on_start_element(b"other", &[(b"Algorithm", b"bad")]);
    assert!(!s.content_encrypted);
    s.on_start_element(
        b"EncryptionMethod",
        &[
            (b"Algorithm", b"http://www.idpf.org/2008/embedding"),
            (b"x:Algorithm", b"http://ns.adobe.com/pdf/enc#RC"),
        ],
    );
    assert!(!s.content_encrypted);
    s.on_start_element(
        b"EncryptionMethod",
        &[
            (b"Algorithm", b"http://www.idpf.org/2008/embedding"),
            (b"Algorithm", b"bad"),
        ],
    );
    assert!(s.content_encrypted);
    s.on_start_element(b"EncryptionMethod", &[]);
    assert!(s.content_encrypted);
}
#[test]
fn path_outputs_caps_nuls_and_partial_oom() {
    let mut out = [0xa5; 3];
    assert!(!dir_name(b"abc/d", &mut out));
    assert_eq!(out, [0xa5; 3]);
    assert!(dir_name(b"abc\0/d", &mut out));
    assert_eq!(out, [0, 0xa5, 0xa5]);
    let mut mem = [0; 32];
    let mut a = Arena::new(&mut mem);
    let mut frag = None;
    let p = resolve_href(
        &mut a,
        Some(b"a/b"),
        Some(b"../c%2fd%00ignored#%31"),
        Some(&mut frag),
    )
    .unwrap();
    assert_eq!(a.view(p), Some(b"a/c/d\0".as_slice()));
    assert_eq!(a.view(frag.unwrap()), Some(b"%31\0".as_slice()));
    let mut mem = [0; 2];
    let mut a = Arena::new(&mut mem);
    let mut frag = None;
    assert!(resolve_href(&mut a, None, Some(b"xyz#f"), Some(&mut frag)).is_none());
    assert_eq!(a.used(), 2);
    assert_eq!(a.failed_alloc_size(), 4);
    assert_eq!(a.view(frag.unwrap()), Some(b"f\0".as_slice()));
    let mut mem = [0; 1];
    let mut a = Arena::new(&mut mem);
    assert!(resolve_href(&mut a, None, Some(b"#f"), Some(&mut frag)).is_none());
    assert!(frag.is_none());
    assert_eq!(a.used(), 0);
    assert_eq!(a.failed_alloc_size(), 2);
    let mut mem = [0; 512];
    let mut a = Arena::new(&mut mem);
    assert!(resolve_href(&mut a, None, Some(&[b'x'; 511]), None).is_some());
    let used = a.used();
    assert!(resolve_href(&mut a, None, Some(&[b'x'; 512]), None).is_none());
    assert_eq!(a.used(), used);
}

#[test]
fn fragment_slot_is_cleared_before_early_failures() {
    let mut memory = [0; 16];
    let mut arena = Arena::new(&mut memory);
    let prior = arena.copy_nul(Some(b"old")).unwrap();
    let mut fragment = Some(prior);
    let used = arena.used();
    assert!(resolve_href(&mut arena, None, None, Some(&mut fragment)).is_none());
    assert!(fragment.is_none());
    assert_eq!(arena.used(), used);
    fragment = Some(prior);
    assert!(resolve_href(&mut arena, None, Some(&[b'x'; 512]), Some(&mut fragment)).is_none());
    assert!(fragment.is_none());
    assert_eq!(arena.used(), used);
    assert_eq!(arena.failed_alloc_size(), 0);
}

#[test]
fn long_href_uses_only_its_normalized_arena_extent() {
    for prefix in ["a".repeat(500), "%61".repeat(500)] {
        let href = format!("{prefix}/../%62#f");
        let mut memory = [0xa5; 10];
        let mut arena = Arena::new(&mut memory);
        let prior = arena.copy_nul(Some(b"fixed")).unwrap();
        let mut fragment = None;
        let path = resolve_href(&mut arena, None, Some(href.as_bytes()), Some(&mut fragment))
            .unwrap();
        assert_eq!(arena.view(prior), Some(b"fixed\0".as_slice()));
        assert_eq!(arena.view(fragment.unwrap()), Some(b"f\0".as_slice()));
        assert_eq!(arena.view(path), Some(b"b\0".as_slice()));
        assert_eq!(arena.used(), 10);
        assert_eq!(arena.high_water(), 10);
        assert_eq!(arena.failed_alloc_size(), 0);
    }
}

#[test]
fn intermediate_path_limit_precedes_allocation_even_when_parents_shorten_it() {
    let base = [b'x'; 506];
    let mut memory = [0xa5; 16];
    let mut arena = Arena::new(&mut memory);
    let mut fragment = None;
    assert!(
        resolve_href(&mut arena, Some(&base), Some(b"abcde/../b#f"), Some(&mut fragment)).is_none()
    );
    assert_eq!(arena.used(), 0);
    assert_eq!(arena.high_water(), 0);
    assert_eq!(arena.failed_alloc_size(), 0);
    assert!(fragment.is_none());
}

#[test]
fn signed_text_events_and_embedded_nul_are_length_based() {
    let mut text = TextCapture::default();
    text.begin();
    text.add_event(b"unused", -3);
    text.add_event(b"unused", 0);
    assert_eq!(text.len, 0);
    text.add_event(b"a\0b-more", 3);
    assert_eq!(text.len, 3);
    assert_eq!(&text.buf[..4], b"a\0b\0");
    assert_eq!(text.trimmed(), b"a");
    text.end();
    text.add_event(&[], 100); // inactive: no source access, as in the original
    assert_eq!(text.len, 3);
}
