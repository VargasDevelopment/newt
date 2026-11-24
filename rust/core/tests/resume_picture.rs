use newt_core::frame::{FRAME_BYTES, Frame};
use newt_core::preferences::{Orientation, ViewSettings};
use newt_core::resume_picture::*;
use newt_core::wire::{get32, put32};

// These are the six fixtures authored by tests/resume_picture_test.cpp.
fn fixture(variant: usize) -> Record {
    let mut record = Record::default();
    for i in 0..32 {
        record.binding.firmware[i] = (0x10 + i + variant) as u8;
        record.binding.book[i] = (0xa0 ^ (i * 7 + variant)) as u8;
    }
    record.binding.anchor.spine = (variant * 3 + 1) as u32;
    record.binding.anchor.character = (variant * 997 + 17) as u32;
    record.binding.settings = [
        ViewSettings {
            orientation: Orientation::Portrait,
            slim_progress_bar: true,
            refresh_pages: 10,
        },
        ViewSettings {
            orientation: Orientation::LandscapeCcw,
            slim_progress_bar: false,
            refresh_pages: 1,
        },
        ViewSettings {
            orientation: Orientation::Portrait,
            slim_progress_bar: false,
            refresh_pages: 5,
        },
    ][variant % 3];
    let long = [b'w'; 255];
    let labels: [&[u8]; 4] = [b"", b"Chapter 1", "Élan — 第3".as_bytes(), &long];
    assert!(record.footer.assign(FooterView {
        spine: variant as u32,
        page: (variant * 2) as u32,
        pages: (variant * 2 + 9) as u32,
        overall_known: variant % 2 == 0,
        overall_percent: (variant * 13 % 101) as u8,
        section_label: Some(labels[variant % 4]),
    }));
    record
}

fn seal(bytes: &mut [u8; HEADER_BYTES]) {
    let checksum = Record::crc(&bytes[..508]);
    put32(bytes, 508, checksum);
}

fn rejects_unchanged(bytes: &[u8; HEADER_BYTES]) {
    let mut output = fixture(2);
    output.frame_crc = 0x1234_abcd;
    let before = output.clone();
    assert!(!output.decode(bytes));
    assert_eq!(output, before);
}

#[test]
fn frozen_cpp_fixtures_round_trip_with_exact_wire_layout() {
    // IEEE CRC-32 of the exact documented C++ fixture headers (bytes 0..507).
    let checksums = [
        0x61ab_e61f,
        0x40fc_e60c,
        0xc002_0169,
        0x7863_ccea,
        0xbda4_1683,
        0x65f8_41e4,
    ];
    for (variant, checksum) in checksums.into_iter().enumerate() {
        let input = fixture(variant);
        let mut bytes = [0xff; HEADER_BYTES];
        input.encode(&mut bytes);
        assert_eq!(&bytes[..8], b"NWRP\x01\0\0\0");
        assert_eq!(&bytes[8..40], &input.binding.firmware);
        assert_eq!(&bytes[40..72], &input.binding.book);
        assert_eq!(get32(&bytes, 72), input.binding.anchor.spine);
        assert_eq!(get32(&bytes, 76), input.binding.anchor.character);
        assert_eq!(
            &bytes[80..84],
            &[
                input.binding.settings.orientation as u8,
                u8::from(input.binding.settings.slim_progress_bar),
                input.binding.settings.refresh_pages,
                0,
            ]
        );
        assert_eq!(get32(&bytes, 84), input.footer.spine);
        assert_eq!(get32(&bytes, 88), input.footer.page);
        assert_eq!(get32(&bytes, 92), input.footer.pages);
        assert_eq!(bytes[96], u8::from(input.footer.overall_known));
        assert_eq!(bytes[97], input.footer.overall_percent);
        let length = input.footer.label.iter().position(|&b| b == 0).unwrap();
        assert_eq!(u16::from_le_bytes([bytes[98], bytes[99]]) as usize, length);
        assert_eq!(&bytes[100..100 + length], &input.footer.label[..length]);
        assert!(bytes[100 + length..356].iter().all(|&b| b == 0));
        assert_eq!(get32(&bytes, 356), 0);
        assert!(bytes[360..508].iter().all(|&b| b == 0));
        assert_eq!(get32(&bytes, 508), checksum);
        let mut decoded = fixture((variant + 1) % 6);
        assert!(decoded.decode(&bytes));
        assert_eq!(decoded, input);
    }
    assert_eq!(HEADER_BYTES, 512);
    assert_eq!(FILE_BYTES, 512 + FRAME_BYTES);
    assert_eq!(Record::crc(b"123456789"), 0xcbf4_3926);
}

#[test]
fn every_damaged_header_byte_and_partial_extent_preserve_record() {
    let mut clean = [0; HEADER_BYTES];
    fixture(3).encode(&mut clean);
    for at in 0..HEADER_BYTES {
        let mut bytes = clean;
        bytes[at] ^= 0x20;
        rejects_unchanged(&bytes);
    }
    // The decoder's array borrow excludes truncated and oversized reads.
    for length in [0, 1, 99, 100, 355, 508, 511, 513] {
        let data = [0; HEADER_BYTES + 1];
        assert!(<&[u8; HEADER_BYTES]>::try_from(&data[..length]).is_err());
    }
}

#[test]
fn valid_checksum_does_not_bypass_header_field_checks() {
    let mut clean = [0; HEADER_BYTES];
    fixture(3).encode(&mut clean);
    for (at, value) in [
        (0, b'X'),
        (4, 0),
        (4, 2),
        (80, 3),
        (80, 255),
        (81, 2),
        (81, 255),
        (83, 1),
        (96, 2),
        (96, 255),
        (120, 0),
    ] {
        let mut bytes = clean;
        bytes[at] = value;
        seal(&mut bytes);
        rejects_unchanged(&bytes);
    }
    for length in [256u16, 257, 511, 512, u16::MAX] {
        let mut bytes = clean;
        bytes[98..100].copy_from_slice(&length.to_le_bytes());
        seal(&mut bytes);
        rejects_unchanged(&bytes);
    }
    // Clockwise landscape (2) is a valid orientation byte, not a corrupt one.
    let mut bytes = clean;
    bytes[80] = Orientation::LandscapeCw as u8;
    seal(&mut bytes);
    let mut output = Record::default();
    assert!(output.decode(&bytes));
    assert_eq!(output.binding.settings.orientation, Orientation::LandscapeCw);
    // Every unused label/tail byte must be zero, excluding the frame CRC.
    fixture(0).encode(&mut clean);
    for at in (100..356).chain(360..508) {
        let mut bytes = clean;
        bytes[at] = 1;
        seal(&mut bytes);
        rejects_unchanged(&bytes);
    }
    // Reducing the declared length cannot leave the previous label behind.
    fixture(3).encode(&mut clean);
    for length in [0u16, 1, 254] {
        let mut bytes = clean;
        bytes[98..100].copy_from_slice(&length.to_le_bytes());
        seal(&mut bytes);
        rejects_unchanged(&bytes);
    }
}

#[test]
fn frozen_decoder_accepts_unrestricted_integer_and_byte_fields() {
    for maximum in [false, true] {
        let word = if maximum { u32::MAX } else { 0 };
        let mut input = Record::default();
        input.binding.firmware.fill(if maximum { 255 } else { 0 });
        input.binding.book.fill(if maximum { 255 } else { 0 });
        input.binding.anchor.spine = word;
        input.binding.anchor.character = word;
        input.footer.spine = word;
        input.footer.page = word;
        input.footer.pages = word;
        input.frame_crc = word;
        for byte in 0..=255 {
            // Resume pictures do not apply the preference record's 1/5/10 rule
            // or clamp percentages, footer page counts, or digest contents.
            input.binding.settings.refresh_pages = byte;
            input.footer.overall_percent = byte;
            let mut bytes = [0; HEADER_BYTES];
            input.encode(&mut bytes);
            let mut output = fixture(3);
            assert!(output.decode(&bytes));
            assert_eq!(output, input);
        }
    }
}

#[test]
fn footer_assignment_is_whole_or_refuse_with_c_string_semantics() {
    for length in [0, 1, 254, 255] {
        let label = [0xfe; 255];
        let mut footer = fixture(1).footer;
        assert!(footer.assign(FooterView {
            spine: u32::MAX,
            page: 0,
            pages: 0,
            overall_known: true,
            overall_percent: 255,
            section_label: Some(&label[..length]),
        }));
        assert_eq!(&footer.label[..length], &label[..length]);
        assert!(footer.label[length..].iter().all(|&b| b == 0));
        let mut record = Record {
            footer,
            ..Record::default()
        };
        let before = record.clone();
        let mut bytes = [0; HEADER_BYTES];
        record.encode(&mut bytes);
        assert!(record.decode(&bytes));
        assert_eq!(record, before);
    }
    let mut footer = fixture(3).footer;
    let before = footer.clone();
    let long = [b'x'; 257];
    for length in [256, 257] {
        assert!(!footer.assign(FooterView {
            section_label: Some(&long[..length]),
            ..before.view()
        }));
        assert_eq!(footer, before);
    }
    assert!(footer.assign(FooterView {
        section_label: Some(b"short\0ignored"),
        ..before.view()
    }));
    assert_eq!(&footer.label[..6], b"short\0");
    assert!(footer.label[6..].iter().all(|&b| b == 0));
    assert!(footer.assign(FooterView {
        section_label: None,
        ..before.view()
    }));
    assert_eq!(footer.label, [0; LABEL_BYTES]);
    assert_eq!(footer.spine, before.spine);
}

#[test]
fn binding_equality_includes_every_identity_locator_and_setting_field() {
    let binding = fixture(2).binding;
    for at in 0..32 {
        let mut changed = binding.clone();
        changed.firmware[at] ^= 1;
        assert_ne!(changed, binding);
        changed = binding.clone();
        changed.book[at] ^= 1;
        assert_ne!(changed, binding);
    }
    let mut changed = binding.clone();
    changed.anchor.spine += 1;
    assert_ne!(changed, binding);
    changed = binding.clone();
    changed.anchor.character += 1;
    assert_ne!(changed, binding);
    changed = binding.clone();
    changed.settings.orientation = Orientation::LandscapeCcw;
    assert_ne!(changed, binding);
    changed = binding.clone();
    changed.settings.slim_progress_bar = true;
    assert_ne!(changed, binding);
    changed = binding.clone();
    changed.settings.refresh_pages = 10;
    assert_ne!(changed, binding);
    // C++ footer equality also compares bytes beyond its first NUL.
    let footer = Footer::default();
    let mut changed = footer.clone();
    changed.label[255] = 1;
    assert_ne!(changed, footer);
}

#[test]
fn mark_compares_whole_frames_and_consumes_every_first_check() {
    let mut frame: Frame = [0; FRAME_BYTES];
    let mut mark = Mark::new();
    assert_eq!(core::mem::size_of::<Mark>(), 8);
    assert_eq!(mark.check(1, &frame), Check::None);
    for pattern in [0u8, 0xff, 0x55, 0xa7] {
        for (at, byte) in frame.iter_mut().enumerate() {
            *byte = if pattern == 0xa7 {
                (at.wrapping_mul(31) ^ (at >> 8)) as u8
            } else {
                pattern
            };
        }
        for generation in [1, 2, u32::MAX] {
            mark.shown(generation, &frame);
            assert_eq!(mark.check(generation, &frame), Check::Covered);
            assert_eq!(mark.check(generation, &frame), Check::None);
            for at in [0, FRAME_BYTES / 2, FRAME_BYTES - 1] {
                mark.shown(generation, &frame);
                frame[at] ^= 1;
                assert_eq!(mark.check(generation, &frame), Check::Differs);
                frame[at] ^= 1;
                assert_eq!(mark.check(generation, &frame), Check::None);
            }
            mark.shown(generation, &frame);
            assert_eq!(mark.check(generation.wrapping_add(1), &frame), Check::None);
            assert_eq!(mark.check(generation, &frame), Check::None);
        }
    }
    mark.shown(0, &frame);
    assert_eq!(mark.check(0, &frame), Check::None);
    mark.shown(1, &frame);
    mark.shown(2, &frame);
    assert_eq!(mark.check(2, &frame), Check::Covered);
    mark.shown(2, &frame);
    assert_eq!(mark.check(0, &frame), Check::None);
    assert_eq!(mark.check(2, &frame), Check::None);
}

#[test]
fn result_names_and_discriminants_match_cpp() {
    for (index, (value, name)) in [
        (Load::Shown, "shown"),
        (Load::Missing, "missing"),
        (Load::Unreadable, "unreadable"),
        (Load::Corrupt, "corrupt"),
        (Load::Mismatch, "mismatch"),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(value as usize, index);
        assert_eq!(value.name(), name);
    }
    assert_eq!(
        [Save::Written as u8, Save::Same as u8, Save::Failed as u8],
        [0, 1, 2]
    );
    assert_eq!(
        [
            Check::None as u8,
            Check::Covered as u8,
            Check::Differs as u8
        ],
        [0, 1, 2]
    );
}
