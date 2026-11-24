// SPDX-License-Identifier: MIT
//! Rust-only clockwise landscape storage, outside the frozen C++ v1/v2 contract. MIT. See the repository LICENSE.
use newt_core::preferences::{DecodeError, Orientation, PACKED_SETTINGS, Preferences, ViewSettings};
use newt_core::wire::{crc32, put32};

fn every_setting() -> impl Iterator<Item = ViewSettings> {
    Orientation::ALL.into_iter().flat_map(|orientation| {
        [false, true].into_iter().flat_map(move |slim_progress_bar| {
            [1, 5, 10].map(|refresh_pages| ViewSettings { orientation, slim_progress_bar, refresh_pages })
        })
    })
}

fn sealed(mut bytes: [u8; 24]) -> [u8; 24] {
    let checksum = crc32(&bytes[..20]);
    put32(&mut bytes, 20, checksum);
    bytes
}

/// Only clockwise is written as v3, so a build without it refuses that record
/// and still reads every other choice.
#[test]
fn only_clockwise_is_written_as_version_three() {
    for settings in every_setting() {
        let stored = Preferences { sequence: 7, settings };
        let bytes = stored.encode().unwrap();
        let version = if settings.orientation == Orientation::LandscapeCw { 3 } else { 2 };
        assert_eq!(bytes[4..8], [version, 0, 0, 0]);
        assert_eq!(Preferences::decode(&bytes), Ok(stored));
    }
}

#[test]
fn clockwise_needs_version_three_and_version_four_is_unknown() {
    let clockwise = Preferences {
        sequence: 9,
        settings: ViewSettings { orientation: Orientation::LandscapeCw, ..ViewSettings::default() },
    }
    .encode()
    .unwrap();
    for (version, expected) in [(1, Err(DecodeError::Corrupt)), (2, Err(DecodeError::Corrupt)), (4, Err(DecodeError::Unsupported))] {
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
}
