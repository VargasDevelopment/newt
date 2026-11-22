//! Exact little-endian alpha15 list format (ESP RV32 and pinned host domain).
use crate::{library_name::name_bytes, listable};
pub const HEADER: u32 = 32;
pub const RECORD: u32 = 100;
pub const BASIS: u64 = 14695981039346656037;
/// One call performs one `LibraryFiles` operation. No retry, sync or rename.
/// `read`/`write` return false on a short transfer, including a partial side effect.
/// Paths are NUL-terminated; descriptors are native, exclusively used by caller.
pub trait Files {
    fn open(&mut self, path: &[u8], write: bool) -> i32;
    fn read(&mut self, fd: i32, offset: u32, out: &mut [u8]) -> bool;
    fn write(&mut self, fd: i32, offset: u32, data: &[u8]) -> bool;
    fn size(&mut self, fd: i32) -> i64;
    fn close(&mut self, fd: i32) -> bool;
    fn make_folder(&mut self, path: &[u8]) -> bool;
    fn remove(&mut self, path: &[u8]) -> bool;
}
pub fn hash(mut value: u64, data: &[u8]) -> u64 {
    for &b in data {
        value = (value ^ u64::from(b)).wrapping_mul(1099511628211);
    }
    value
}
fn check(data: &[u8]) -> u32 {
    let h = hash(BASIS, data);
    (h ^ (h >> 32)) as u32
}
pub fn at(base: u32, index: u32) -> u32 {
    base + index * RECORD
}
pub fn read_record(files: &mut dyn Files, fd: i32, offset: u32, out: &mut [u8; 96]) -> bool {
    let mut record = [0; 100];
    if !files.read(fd, offset, &mut record) || record[96..] != check(&record[..96]).to_le_bytes() {
        return false;
    }
    let Some(name) = name_bytes(&record[..96]) else {
        return false;
    };
    if record[name.len()..96].iter().any(|&b| b != 0) || !listable(&record[..96]) {
        return false;
    }
    out.copy_from_slice(&record[..96]);
    true
}
pub fn write_record(files: &mut dyn Files, fd: i32, offset: u32, name: &[u8]) -> bool {
    // Reject names outside the bounded listable length before writing.
    if !listable(name) {
        return false;
    }
    let name = name_bytes(name).unwrap();
    let mut record = [0; 100];
    record[..name.len()].copy_from_slice(name);
    let sum = check(&record[..96]);
    record[96..].copy_from_slice(&sum.to_le_bytes());
    files.write(fd, offset, &record)
}
pub fn write_header(
    files: &mut dyn Files,
    fd: i32,
    count: u32,
    outside: u32,
    fingerprint: u64,
) -> bool {
    let mut h = [0; 32];
    h[..8].copy_from_slice(b"NEWTLST1");
    h[8..12].copy_from_slice(&count.to_le_bytes());
    h[12..16].copy_from_slice(&outside.to_le_bytes());
    h[16..24].copy_from_slice(&fingerprint.to_le_bytes());
    let sum = check(&h[..28]);
    h[28..].copy_from_slice(&sum.to_le_bytes());
    files.write(fd, 0, &h)
}
pub fn current(
    files: &mut dyn Files,
    path: &[u8],
    count: u32,
    outside: u32,
    fingerprint: u64,
) -> bool {
    let fd = files.open(path, false);
    if fd < 0 {
        return false;
    }
    let mut h = [0; 32];
    let ok = files.read(fd, 0, &mut h)
        && &h[..8] == b"NEWTLST1"
        && h[28..] == check(&h[..28]).to_le_bytes()
        && h[8..12] == count.to_le_bytes()
        && h[12..16] == outside.to_le_bytes()
        && h[16..24] == fingerprint.to_le_bytes()
        && files.size(fd) == i64::from(at(HEADER, count));
    files.close(fd);
    ok
}
