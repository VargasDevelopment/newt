// SPDX-License-Identifier: MIT
//! Frozen cover_discovery/decode policy. No heap, filesystem or native parser.
#![forbid(unsafe_code)]
pub mod discovery;
pub mod xml;
pub mod jpeg;
pub mod png;
use crate::zip_window::source::BookSource;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetadataStatus { Ok, NotFound, Unsupported, Malformed, IoError, OutOfMemory, Busy }
#[derive(Clone, Copy, Default)]
pub struct Member {
    pub entry: crate::zip_container::Entry,
    pub crc32: u32,
    pub data: u64,
}
pub fn read(source: &mut dyn BookSource, mut at: u64, mut out: &mut [u8]) -> bool {
    while !out.is_empty() {
        let take = out.len().min(1024);
        let got = source.read_at(at, &mut out[..take]);
        if got <= 0 || got as usize > take { return false; }
        at += got as u64;
        out = &mut out[got as usize..];
    }
    true
}
/// CRC-32 (IEEE, reflected) without its final inversion: start from
/// `u32::MAX` and invert the result. One table step per byte, because a
/// cover reads its whole book about three times and a bitwise loop made the
/// CRC the slowest part of that (#74).
pub fn crc(mut value: u32, bytes: &[u8]) -> u32 {
    for &byte in bytes {
        value = (value >> 8) ^ CRC_TABLE[usize::from(value as u8 ^ byte)];
    }
    value
}
static CRC_TABLE: [u32; 256] = {
    let mut table = [0; 256];
    let mut index = 0;
    while index < 256 {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 8 {
            value = (value >> 1) ^ if value & 1 != 0 { 0xedb8_8320 } else { 0 };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
};
pub fn fnv(mut value: u32, bytes: &[u8]) -> u32 {
    for &byte in bytes { value = (value ^ u32::from(byte)).wrapping_mul(16_777_619); }
    value
}
pub fn name_key(value: u32, name: &[u8]) -> u32 { fnv(fnv(value, name), &[0]) }
pub fn version_key(name: &[u8], size: u64, mtime: u64) -> u32 {
    let hash = fnv(fnv(name_key(2_166_136_261, name), &size.to_le_bytes()), &mtime.to_le_bytes());
    if hash == 0 { 1 } else { hash }
}
pub const SIGNATURE: [u8; 8] = *b"NLC2\x90\0\xbc\0";
pub const PIXELS: usize = 3384;
/// Why a book settled with no cover art, stored in the 4th byte of a bare NLC2 route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum NoCoverReason {
    Missing = 1,
    Unsupported = 2,
    Incomplete = 3,
    Invalid = 4,
}
impl NoCoverReason {
    pub const fn byte(self) -> u8 {
        match self {
            Self::Missing => b'M',
            Self::Unsupported => b'U',
            Self::Incomplete => b'I',
            Self::Invalid => b'X',
        }
    }
    pub const fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            b'M' => Some(Self::Missing),
            b'U' => Some(Self::Unsupported),
            b'I' => Some(Self::Incomplete),
            b'X' => Some(Self::Invalid),
            _ => None,
        }
    }
    pub const fn note(self) -> &'static [u8] {
        match self {
            Self::Missing => b"No cover in this book",
            Self::Unsupported => b"Cover format not supported",
            Self::Incomplete => b"Cover file is incomplete",
            Self::Invalid => b"Cover file is damaged",
        }
    }
}
/// Bare record version byte: `CJ` + version + reason (`CJCM`, `CJCU`, `CJCI`, `CJCX`).
/// Bump this byte when a decoder change can turn an older negative into art;
/// every older version retries once without a growing legacy list.
pub const NO_ART_VERSION: u8 = b'C';
pub const fn no_art_route(reason: NoCoverReason) -> u32 {
    u32::from_le_bytes([b'C', b'J', NO_ART_VERSION, reason.byte()])
}
/// What a 28-byte bare NLC2 route word means for cover display and generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BareRecord {
    Current(NoCoverReason),
    Retry,
    Other,
}
/// Classify a bare route word against `version` (`NO_ART_VERSION` in production).
pub const fn bare_record_at(recorded: u32, version: u8) -> BareRecord {
    let bytes = recorded.to_le_bytes();
    if bytes[0] != b'C' || bytes[1] != b'J' {
        return BareRecord::Other;
    }
    if bytes[2] == version {
        return match NoCoverReason::from_byte(bytes[3]) {
            Some(reason) => BareRecord::Current(reason),
            None => BareRecord::Other,
        };
    }
    // Legacy unreasoned negatives (CJB3..=CJB7) and earlier versioned negatives retry once.
    if (bytes[2] == b'B' && matches!(bytes[3], b'3'..=b'7'))
        || (bytes[2] >= b'C' && bytes[2] < version && NoCoverReason::from_byte(bytes[3]).is_some())
    {
        return BareRecord::Retry;
    }
    BareRecord::Other
}
pub const fn bare_record(recorded: u32) -> BareRecord {
    bare_record_at(recorded, NO_ART_VERSION)
}
/// What a cover record bound to its book's size and mtime means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Record { Current, Generate }
/// `pixels` is the payload CRC, or None for a bare record. Art stays valid across versions.
pub fn record(pixels: Option<u32>, recorded: u32) -> Record {
    match pixels {
        None if matches!(bare_record(recorded), BareRecord::Current(_)) => Record::Current,
        Some(crc) if crc == recorded => Record::Current,
        _ => Record::Generate,
    }
}
pub fn header(size: u64, mtime: u64, check: u32) -> [u8; 28] {
    let mut out = [0; 28];
    out[..8].copy_from_slice(&SIGNATURE);
    out[8..16].copy_from_slice(&size.to_le_bytes());
    out[16..24].copy_from_slice(&(mtime & !1).to_le_bytes());
    out[24..].copy_from_slice(&check.to_le_bytes());
    out
}
