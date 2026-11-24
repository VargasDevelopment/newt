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
/// A bare record's route ("CJB7"). Change it whenever a verdict can change, so
/// every older negative is read once more. #50 added Adobe RGB JPEG support.
pub const NO_ART: u32 = 0x3742_4a43;
/// Negatives from older decoders: "CJB3", "CJB4" (before #316), "CJB5"
/// (before #46) and "CJB6" (before #50). Each is generated once more; Home
/// shows no cover until then.
pub const RETRIED_NO_ART: [u32; 4] = [0x3342_4a43, 0x3442_4a43, 0x3542_4a43, 0x3642_4a43];
/// What a cover record bound to its book's size and mtime means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Record { Current, Generate }
/// `pixels` is the payload CRC, or None for a bare record. Art stays valid across routes.
pub fn record(pixels: Option<u32>, recorded: u32) -> Record {
    match pixels {
        None if recorded == NO_ART => Record::Current,
        Some(crc) if crc == recorded => Record::Current,
        _ => Record::Generate,
    }
}
/// A bare record Home shows as "no cover", including routes awaiting their retry.
pub fn no_art(recorded: u32) -> bool { recorded == NO_ART || RETRIED_NO_ART.contains(&recorded) }
pub fn header(size: u64, mtime: u64, check: u32) -> [u8; 28] {
    let mut out = [0; 28];
    out[..8].copy_from_slice(&SIGNATURE);
    out[8..16].copy_from_slice(&size.to_le_bytes());
    out[16..24].copy_from_slice(&(mtime & !1).to_le_bytes());
    out[24..].copy_from_slice(&check.to_le_bytes());
    out
}
