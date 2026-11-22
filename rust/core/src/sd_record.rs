//! NRS2 SD state envelopes. This is an encoding, not an SD durability barrier.
use crate::wire::{crc32c, get32, put32};

pub const SECTOR_BYTES: usize = 512;
pub const RECORD_BYTES: usize = 256;
pub const FILE_BYTES: usize = 8 * SECTOR_BYTES;
pub const HEADER_BYTES: usize = 20;
pub const PAYLOAD_BYTES: usize = RECORD_BYTES - HEADER_BYTES - 4;

/// Payload must already be in record[HEADER_BYTES..]. Like C strlen, names
/// stop at the first NUL. Invalid input leaves the record untouched.
pub fn seal(
    record: &mut [u8; RECORD_BYTES],
    tag: &[u8; 4],
    name: &[u8],
    size: usize,
    generation: u32,
) -> bool {
    let length = name
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(name.len());
    if length == 0 || length > 7 || size > PAYLOAD_BYTES {
        return false;
    }
    record[..HEADER_BYTES].fill(0);
    record[HEADER_BYTES + size..].fill(0);
    record[..4].copy_from_slice(tag);
    record[4..4 + length].copy_from_slice(&name[..length]);
    put32(record, 12, size as u32);
    put32(record, 16, generation);
    let checksum = crc32c(&record[..RECORD_BYTES - 4]);
    put32(record, RECORD_BYTES - 4, checksum);
    true
}

pub fn intact(record: &[u8; RECORD_BYTES], tag: &[u8; 4]) -> bool {
    &record[..4] == tag && get32(record, RECORD_BYTES - 4) == crc32c(&record[..RECORD_BYTES - 4])
}

pub fn header(record: &mut [u8; RECORD_BYTES], namespace: &[u8]) -> bool {
    let length = namespace
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(namespace.len());
    if length >= 32 {
        return false;
    }
    record[HEADER_BYTES..HEADER_BYTES + 32].fill(0);
    record[HEADER_BYTES..HEADER_BYTES + length].copy_from_slice(&namespace[..length]);
    seal(record, b"NRS2", b"v2", 32, 0)
}
