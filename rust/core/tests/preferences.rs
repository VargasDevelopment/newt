// SPDX-License-Identifier: MIT
//! Rust-only clockwise and text-size storage, outside the frozen C++ v1/v2 contract. MIT. See the repository LICENSE.
use newt_core::preferences::{DecodeError, Orientation, PACKED_SETTINGS, Preferences, TextSize, ViewSettings};
use newt_core::wire::{crc32, put32};

fn every_setting() -> impl Iterator<Item = ViewSettings> {
    Orientation::ALL.into_iter().flat_map(|orientation| {
        [false, true].into_iter().flat_map(move |slim_progress_bar| {
            [1, 5, 10].into_iter().flat_map(move |refresh_pages| {
                TextSize::ALL.map(|text_size| ViewSettings { orientation, slim_progress_bar, refresh_pages, text_size })
            })
        })
    })
}

fn sealed(mut bytes: [u8; 24]) -> [u8; 24] {
    let checksum = crc32(&bytes[..20]);
    put32(&mut bytes, 20, checksum);
    bytes
}

/// Each record takes the lowest version that holds it, so an older build
/// refuses only the choices it lacks and still reads every other record.
#[test]
fn records_use_the_lowest_version_that_holds_them() {
    for settings in every_setting() {
        let stored = Preferences { sequence: 7, settings };
        let bytes = stored.encode().unwrap();
        let version = match settings {
            ViewSettings { text_size: TextSize::Px30, orientation: Orientation::LandscapeCw, .. } => 3,
            ViewSettings { text_size: TextSize::Px30, .. } => 2,
            _ => 4,
        };
        assert_eq!(bytes[4..8], [version, 0, 0, 0]);
        assert_eq!(bytes[15], settings.text_size.byte());
        assert_eq!(Preferences::decode(&bytes), Ok(stored));
    }
}

#[test]
fn a_text_size_needs_version_four_and_a_known_size() {
    let resized = Preferences {
        sequence: 3,
        settings: ViewSettings { text_size: TextSize::Px22, ..ViewSettings::default() },
    }
    .encode()
    .unwrap();
    for version in [2, 3] {
        let mut bytes = resized;
        put32(&mut bytes, 4, version);
        assert_eq!(Preferences::decode(&sealed(bytes)), Err(DecodeError::Corrupt), "size under v{version}");
    }
    for byte in [23, 30, 255] {
        let mut bytes = resized;
        bytes[15] = byte;
        assert_eq!(Preferences::decode(&sealed(bytes)), Err(DecodeError::Corrupt), "size byte {byte}");
    }
    let mut future = resized;
    put32(&mut future, 4, 5);
    assert_eq!(Preferences::decode(&sealed(future)), Err(DecodeError::Unsupported));
}

#[test]
fn clockwise_needs_version_three_or_later() {
    let clockwise = Preferences {
        sequence: 9,
        settings: ViewSettings { orientation: Orientation::LandscapeCw, ..ViewSettings::default() },
    }
    .encode()
    .unwrap();
    for (version, expected) in [(1, Err(DecodeError::Corrupt)), (2, Err(DecodeError::Corrupt)), (5, Err(DecodeError::Unsupported))] {
        let mut bytes = clockwise;
        put32(&mut bytes, 4, version);
        if version == 1 {
            bytes[14] = 0;
        }
        assert_eq!(Preferences::decode(&sealed(bytes)), expected, "version {version}");
    }
    let mut unknown = clockwise;
    unknown[12] = 3;
    assert_eq!(Preferences::decode(&sealed(unknown)), Err(DecodeError::Corrupt));
}

#[test]
fn packed_words_round_trip_and_reject_a_half_turn_without_landscape() {
    for settings in every_setting() {
        let word = settings.packed();
        assert_eq!(word & !PACKED_SETTINGS, 0);
        assert_eq!(ViewSettings::unpacked(word), Some(settings));
        // Record-owned bits 2 and 3 do not change the shared settings.
        assert_eq!(ViewSettings::unpacked(word | 0xc), Some(settings));
    }
    let portrait = ViewSettings { orientation: Orientation::Portrait, ..ViewSettings::default() }.packed();
    assert_eq!(ViewSettings::unpacked(portrait | 0x10), None);
    assert_eq!(ViewSettings::unpacked(portrait & !0xff00), None, "refresh 0");
    assert_eq!(ViewSettings::unpacked(portrait & !0xff00 | 2 << 8), None, "refresh 2");
    assert_eq!(ViewSettings::unpacked(portrait | 23 << 16), None, "unknown size");
    // The default size adds no bits, so older builds keep reading the word.
    assert_eq!(portrait >> 16, 0);
}
