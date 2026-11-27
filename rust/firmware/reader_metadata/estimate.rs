// SPDX-License-Identifier: MIT
//! firmware/progress_estimate_record.h: exact NTPE0001 fields and publication.
use newt_core::{book_semantic_index::POLICY_VERSION, page_cache::CacheStorage,
    wire::{get32, put32}};
use newt_reader_navigation::Estimate;
pub const BYTES: usize = 80;
pub type Key = [u8; 73];

pub fn make_key(digest: &[u8; 32], key: &mut Key) {
    key[..3].copy_from_slice(b"pe-");
    for (index, byte) in digest.iter().copied().enumerate() {
        key[3 + 2 * index] = b"0123456789abcdef"[usize::from(byte >> 4)];
        key[4 + 2 * index] = b"0123456789abcdef"[usize::from(byte & 15)];
    }
    key[67..].copy_from_slice(b".ntpe\0");
}
fn load<S: CacheStorage>(store: &mut S, digest: &[u8; 32], policy: u32) -> Option<Estimate> {
    let mut key = [0; 73];
    let mut record = [0; BYTES];
    make_key(digest, &mut key);
    if store.file_size(&key) != BYTES as i64
        || store.read_at(&key, 0, &mut record) != BYTES as i32
        || &record[..8] != b"NTPE0001" || get32(&record, 8) != policy
        || get32(&record, 20) != 0 || &record[24..56] != digest { return None; }
    let decoded = Estimate { before: get64(&record, 56), current: get64(&record, 64),
        total: get64(&record, 72), spine: get32(&record, 12), chapter_chars: get32(&record, 16) };
    decoded.valid().then_some(decoded)
}
pub fn load_for_home<S: CacheStorage>(store: &mut S, digest: &[u8; 32], spine: u32,
    character: u32, policy: u32,
) -> Option<Estimate> {
    load(store, digest, policy).filter(|estimate| estimate.spine == spine && character <= estimate.chapter_chars)
}
pub fn save<S: CacheStorage>(store: &mut S, digest: &[u8; 32], estimate: Estimate,
    stop: &dyn Fn() -> bool,
) -> bool {
    if !estimate.valid() { return false; }
    if load(store, digest, POLICY_VERSION) == Some(estimate) { return true; }
    let mut key = [0; 73];
    let mut record = [0; BYTES];
    make_key(digest, &mut key);
    record[..8].copy_from_slice(b"NTPE0001");
    put32(&mut record, 8, POLICY_VERSION); put32(&mut record, 12, estimate.spine);
    put32(&mut record, 16, estimate.chapter_chars);
    record[24..56].copy_from_slice(digest);
    record[56..64].copy_from_slice(&estimate.before.to_le_bytes());
    record[64..72].copy_from_slice(&estimate.current.to_le_bytes());
    record[72..80].copy_from_slice(&estimate.total.to_le_bytes());
    if stop() { return false; }
    if !store.begin_write(&key) { store.remove(&key); return false; }
    let good = store.write(&record) && store.end_write();
    if !good { store.remove(&key); }
    good
}
fn get64(record: &[u8], at: usize) -> u64 {
    u64::from(get32(record, at)) | (u64::from(get32(record, at + 4)) << 32)
}
