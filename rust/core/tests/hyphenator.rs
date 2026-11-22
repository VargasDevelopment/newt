use newt_core::hyphenator::Hyphenator;
const BASIC: &[u8] = b"FIBH\x01\0\x06\0\x01\0\0\0\0\0\0\0\x06abcdef\0\0\x01\x01\0\0\0";

#[test]
fn borrowed_state_failure_and_recovery() {
    let mut h = Hyphenator::new();
    let mut out = [0xa5; 8];
    assert!(!h.ready());
    assert_eq!(h.break_positions(b"abcdef", &mut out), 0);
    assert!(h.init(Some(BASIC)));
    assert_eq!(h.pattern_count(), 1);
    assert_eq!(h.max_pattern_len(), 6);
    assert_eq!(h.break_positions(b"ABCDEF", &mut out), 2);
    assert_eq!(out, [2, 3, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5]);
    assert!(!h.init(None));
    assert!(!h.ready());
    assert_eq!(h.break_positions(b"abcdef", &mut out), 0);
    assert!(h.init(Some(BASIC)));
    assert_eq!(h.break_positions(b"abcdef", &mut out[..1]), 1);
}

#[test]
fn malformed_records_keep_header_state_but_refuse_search() {
    let mut h = Hyphenator::new();
    for end in 16..BASIC.len() {
        assert!(h.init(Some(&BASIC[..end])));
        assert!(h.ready());
        assert!(!h.records_valid());
        let mut out = [0xa5; 8];
        assert_eq!(h.break_positions(b"abcdef", &mut out), 0);
        assert_eq!(out, [0xa5; 8]);
    }
}

#[test]
fn unsupported_words_preserve_output() {
    let mut h = Hyphenator::new();
    assert!(h.init(Some(BASIC)));
    for word in [
        b"ab\x80cde".as_slice(),
        b"abcde\xc3",
        "😀abcde".as_bytes(),
        b"ab\xed\xa0\x80cde",
        &[b'a'; 63],
    ] {
        let mut out = [0xa5; 8];
        assert_eq!(h.break_positions(word, &mut out), 0);
        assert_eq!(out, [0xa5; 8]);
    }
}

#[test]
fn header_maximum_changes_before_offset_table_failure() {
    let mut h = Hyphenator::new();
    assert!(h.init(Some(BASIC)));
    let header = b"FIBH\x01\0\x09\0\x01\0\0\0";
    assert!(!h.init(Some(header)));
    assert!(!h.ready());
    assert_eq!(h.max_pattern_len(), 9);
    assert!(!h.init(None));
    assert_eq!(h.max_pattern_len(), 9);
    assert!(h.init(Some(BASIC)));
    assert_eq!(h.max_pattern_len(), 6);
}

#[test]
fn record_validation_resets_across_failed_empty_and_valid_adoption() {
    let mut h = Hyphenator::default();
    assert!(h.records_valid());
    assert!(h.init(Some(&BASIC[..16])));
    assert!(h.ready());
    assert!(!h.records_valid());
    assert!(!h.init(None));
    assert!(!h.ready());
    assert!(h.records_valid());
    assert!(h.init(Some(b"FIBH\x01\0\0\0\0\0\0\0")));
    assert!(!h.ready());
    assert!(h.records_valid());
    assert!(h.init(Some(BASIC)));
    assert!(h.records_valid());
    let mut out = [0xa5; 8];
    assert_eq!(h.break_positions(b"abcdef", &mut out), 2);
    assert_eq!(out, [2, 3, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5]);
}
