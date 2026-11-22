//! Verified book-source change hints wire format and policy. MIT.
//! Callers keep route, FilePort and live verification.
#![forbid(unsafe_code)]
use crate::wire::{crc32, get32, put32};

pub type Digest = [u8; 32];
pub const WIRE_BYTES: usize = 72;
pub const SCHEMA: u32 = 1;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SourceChangeHints { pub bytes: u64, pub modified: i64 }
impl SourceChangeHints {
    pub fn valid(self) -> bool { self.bytes != 0 && self.bytes <= 64 * 1024 * 1024 && self.modified > 0 }
}
fn get64(wire: &[u8], at: usize) -> u64 { u64::from(get32(wire, at)) | (u64::from(get32(wire, at + 4)) << 32) }
fn put64(wire: &mut [u8], at: usize, value: u64) { put32(wire, at, value as u32); put32(wire, at + 4, (value >> 32) as u32); }

/// Public save's validity guard and zero-initialized wire, including68..71.
/// Refusal leaves the caller's previous wire intact; no storage is attempted.
pub fn encode(digest: &Digest, hints: SourceChangeHints, wire: &mut [u8; WIRE_BYTES]) -> bool {
    if !hints.valid() { return false; }
    wire.fill(0);
    wire[..8].copy_from_slice(b"NTSH0001");
    put32(wire, 8, SCHEMA); put32(wire, 12, WIRE_BYTES as u32);
    put64(wire, 16, hints.bytes); put64(wire, 24, hints.modified as u64);
    wire[32..64].copy_from_slice(digest);
    let checksum = crc32(&wire[..64]); put32(wire, 64, checksum);
    true
}

/// Exact private decoder ordering, not the FilePort recorded/load wrapper.
/// Negative/unreadable/wrong extents reject before any caller output changes.
/// After envelope/expected checks, digest may change even on later rejection;
/// hints are assigned before valid(), but after the unsigned modified bound.
pub fn decode_into(wire: Option<&[u8]>, length: i64, expected: Option<&Digest>, digest: Option<&mut Digest>, hints: &mut SourceChangeHints) -> bool {
    if length != WIRE_BYTES as i64 { return false; }
    let Some(wire) = wire.filter(|wire| wire.len() >= WIRE_BYTES) else { return false };
    if &wire[..8] != b"NTSH0001" || get32(wire, 8) != SCHEMA || get32(wire, 12) != WIRE_BYTES as u32 ||
        get32(wire, 64) != crc32(&wire[..64]) || get32(wire, 68) != 0 || expected.is_some_and(|expected| wire[32..64] != expected[..]) { return false; }
    if let Some(digest) = digest { digest.copy_from_slice(&wire[32..64]); }
    let modified = get64(wire, 24);
    if modified > i64::MAX as u64 { return false; }
    *hints = SourceChangeHints { bytes: get64(wire, 16), modified: modified as i64 };
    hints.valid()
}
/// Recorded-mode decode only. Root keeps public read/reset/stage/fresh/close.
pub fn decode_recorded(wire: Option<&[u8]>, length: i64, digest: &mut Digest, hints: &mut SourceChangeHints) -> bool {
    decode_into(wire, length, None, Some(digest), hints)
}
pub fn decode_expected(wire: Option<&[u8]>, length: i64, expected: &Digest, hints: &mut SourceChangeHints) -> bool {
    decode_into(wire, length, Some(expected), None, hints)
}
/// The recorded() result is a required caller fact, never a freshness default.
/// Digest was already exposed by recorded() before this equality decision.
pub fn verified_source_digest(recorded_ok: bool, recorded: SourceChangeHints, current: SourceChangeHints) -> bool {
    recorded_ok && recorded == current
}
