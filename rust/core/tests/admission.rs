use newt_core::{utf8, zip_record_rules as zip};

#[test]
fn scalar_boundaries_and_embedded_nul() {
    for cp in [
        0, 0x7f, 0x80, 0x7ff, 0x800, 0xd7ff, 0xe000, 0xffff, 0x10000, 0x10ffff, 0xfeff, 0xfdd0,
    ] {
        let mut bytes = [0; 4];
        let text = char::from_u32(cp)
            .unwrap()
            .encode_utf8(&mut bytes)
            .as_bytes();
        let (mut offset, mut output) = (0, u32::MAX);
        assert!(utf8::next(Some(text), &mut offset, &mut output));
        assert_eq!((offset, output), (text.len(), cp));
        assert!(!utf8::next(Some(text), &mut offset, &mut output));
        assert_eq!((offset, output), (text.len(), cp));
    }
}

#[test]
fn malformed_attempts_consume_one_byte_preserving_output() {
    for bytes in [
        &b"\x80"[..],
        b"\xc0\x80",
        b"\xc1\xbf",
        b"\xe0\x80\x80",
        b"\xed\xa0\x80",
        b"\xed\xbf\xbf",
        b"\xf0\x80\x80\x80",
        b"\xf4\x90\x80\x80",
        b"\xf5\x80\x80\x80",
        b"\xff",
        b"\xc2",
        b"\xe0\xa0",
        b"\xf0\x90\x80",
        b"\xe2A\xac",
        b"\xf0\x90A\x80",
    ] {
        let (mut offset, mut cp) = (0, 0xdeadbeef);
        assert!(!utf8::next(Some(bytes), &mut offset, &mut cp), "{bytes:x?}");
        assert_eq!((offset, cp), (1, 0xdeadbeef));
    }
    let (mut offset, mut cp) = (0, 77);
    let input = b"\xe2A\xc2\xa2\0";
    for (ok, at, value) in [
        (false, 1, 77),
        (true, 2, 65),
        (true, 4, 162),
        (true, 5, 0),
        (false, 5, 0),
    ] {
        assert_eq!(utf8::next(Some(input), &mut offset, &mut cp), ok);
        assert_eq!((offset, cp), (at, value));
    }
}

#[test]
fn null_empty_exhausted_and_arbitrary_start_offsets() {
    for input in [None, Some(&b""[..]), Some(&b"abc"[..])] {
        let (mut offset, mut cp) = (usize::MAX, 123);
        assert!(!utf8::next(input, &mut offset, &mut cp));
        assert_eq!((offset, cp), (usize::MAX, 123));
    }
    let (mut offset, mut cp) = (0, 123);
    assert!(!utf8::next(None, &mut offset, &mut cp));
    assert_eq!((offset, cp), (0, 123));
    offset = 1;
    assert!(!utf8::next(Some(b"\xc2\xa2Z"), &mut offset, &mut cp));
    assert_eq!((offset, cp), (2, 123));
    assert!(utf8::next(Some(b"\xc2\xa2Z"), &mut offset, &mut cp));
    assert_eq!((offset, cp), (3, 90));
}

#[test]
fn zip_masks_limits_and_encryption_precedence() {
    use zip::ZipCentralRecordRule::{Encrypted, Ok, Unsupported};
    assert_eq!(zip::EPUB_MAX_EXPANDED_BYTES, 67_108_864);
    assert_eq!(zip::zip_flag_mask(Ok), 0xdf9e);
    assert_eq!(zip::zip_flag_mask(Encrypted), 0x2041);
    assert_eq!(zip::zip_flag_mask(Unsupported), 0x20);
    for flags in 0..=u16::MAX {
        let expected = if flags & 0x2041 != 0 {
            Encrypted
        } else if flags & 0x20 != 0 {
            Unsupported
        } else {
            Ok
        };
        assert_eq!(zip::zip_member_flag_rule(flags), expected);
    }
    for method in [0, 8] {
        for name in [1, 511] {
            for size in [0, 4_194_304] {
                assert_eq!(zip::zip_central_record_rule(0xdf9e, method, name, size), Ok);
            }
        }
    }
    for (method, name, size) in [
        (1, 1, 0),
        (9, 1, 0),
        (0, 0, 0),
        (8, 512, 0),
        (8, 1, 4_194_305),
        (65535, 65535, u32::MAX),
    ] {
        assert_eq!(
            zip::zip_central_record_rule(0, method, name, size),
            Unsupported
        );
        assert_eq!(
            zip::zip_central_record_rule(0x2061, method, name, size),
            Encrypted
        );
    }
}

#[test]
fn local_equality_is_independent_of_central_admission() {
    use zip::ZipLocalHeaderRule::{Mismatch, Ok};
    // Unsupported/encrypted but equal metadata still matches this pure rule.
    assert_eq!(
        zip::zip_local_header_rule(0x04034b50, 0xffff, 99, 0, 0xffff, 99, 0),
        Ok
    );
    for (signature, flags, method, name) in [
        (0, 8, 8, 511),
        (0x04034b51, 8, 8, 511),
        (0x04034b50, 0, 8, 511),
        (0x04034b50, 8, 0, 511),
        (0x04034b50, 8, 8, 512),
    ] {
        assert_eq!(
            zip::zip_local_header_rule(signature, flags, method, name, 8, 8, 511),
            Mismatch
        );
    }
}
