// SPDX-License-Identifier: MIT
//! Bounded no-catalog cover ZIP walk.
use super::{Member, MetadataStatus as S, read};
use crate::{zip_container::{Entry, hash_path}, zip_record_rules::*, zip_window::source::BookSource};
pub fn le16(p: &[u8]) -> u16 { u16::from_le_bytes([p[0], p[1]]) }
pub fn le32(p: &[u8]) -> u32 { u32::from_le_bytes([p[0], p[1], p[2], p[3]]) }
pub fn cbytes(p: &[u8]) -> &[u8] { &p[..p.iter().position(|&c| c == 0).unwrap_or(p.len())] }
pub fn path(base: &[u8], href: &[u8], out: &mut [u8; 512]) -> bool {
    let (base, href) = (cbytes(base), cbytes(href));
    if href.is_empty() || href[0] == b'/' || href.len() >= 512
        || href.iter().any(|&c| c < 32 || c == 127 || b"\\:%#?".contains(&c)) { return false; }
    let prefix = base.iter().rposition(|&c| c == b'/').map_or(0, |i| i + 1);
    let mut combined = [0; 1024];
    if prefix > 511 || prefix + href.len() >= combined.len() { return false; }
    combined[..prefix].copy_from_slice(&base[..prefix]);
    combined[prefix..prefix + href.len()].copy_from_slice(href);
    let mut length = 0;
    for part in combined[..prefix + href.len()].split(|&c| c == b'/') {
        if part.is_empty() { return false; }
        if part == b".." {
            if length == 0 { return false; }
            while length != 0 && out[length - 1] != b'/' { length -= 1; }
            length = length.saturating_sub(1);
        } else if part != b"." {
            if length + part.len() + usize::from(length != 0) > 511 { return false; }
            if length != 0 { out[length] = b'/'; length += 1; }
            out[length..length + part.len()].copy_from_slice(part); length += part.len();
        }
    }
    out[length] = 0;
    length != 0
}
#[derive(Clone, Copy)]
struct Directory { pub offset: u32, pub bytes: u32, pub count: u16, pub eocd: u64 }
fn directory(source: &mut dyn BookSource) -> Result<Directory, S> {
    let mut tail = [0; 512];
    let lower = source.size().saturating_sub(65557);
    let mut end = source.size();
    while end > lower {
        let start = end.saturating_sub(512).max(lower);
        let size = (end - start) as usize;
        if !read(source, start, &mut tail[..size]) { return Err(S::IoError); }
        for i in (0..size.saturating_sub(21)).rev() {
            let p = &tail[i..];
            if le32(p) != 0x0605_4b50 || start + i as u64 + 22 + u64::from(le16(&p[20..])) != source.size()
                || le16(&p[4..]) != 0 || le16(&p[6..]) != 0 || le16(&p[10..]) == 0
                || le16(&p[8..]) != le16(&p[10..])
                || le32(&p[12..]) < u32::from(le16(&p[10..])) * 46
                || u64::from(le32(&p[16..])) + u64::from(le32(&p[12..])) != start + i as u64 { continue; }
            return Ok(Directory { offset: le32(&p[16..]), bytes: le32(&p[12..]),
                count: le16(&p[10..]), eocd: start + i as u64 });
        }
        if start == lower { break; }
        end = start + 21;
    }
    Err(S::Malformed)
}
fn directory_member(source: &mut dyn BookSource, at: u64, end: u64, directory: u32,
    expanded: &mut u64, header: &mut [u8; 46], name: &mut [u8; 512]) -> Result<(u64, u64), S> {
    if at > end || end - at < 46 { return Err(S::Malformed); }
    if !read(source, at, header) { return Err(S::IoError); }
    if le32(header) != 0x0201_4b50 { return Err(S::Malformed); }
    let (flags, method, names) = (le16(&header[8..]), le16(&header[10..]), le16(&header[28..]));
    let (compressed, uncompressed, local) = (le32(&header[20..]), le32(&header[24..]), le32(&header[42..]));
    if zip_central_record_rule(0, method, names, uncompressed) != ZipCentralRecordRule::Ok
        || u64::from(compressed) > source.size() { return Err(S::Unsupported); }
    *expanded += u64::from(uncompressed);
    if *expanded > EPUB_MAX_EXPANDED_BYTES { return Err(S::Unsupported); }
    let record = 46 + u64::from(names) + u64::from(le16(&header[30..])) + u64::from(le16(&header[32..]));
    if record > end - at || u64::from(local) + 30 > u64::from(directory) { return Err(S::Malformed); }
    if !read(source, at + 46, &mut name[..usize::from(names)]) { return Err(S::IoError); }
    if name[..usize::from(names)].contains(&0) { return Err(S::Malformed); }
    name[usize::from(names)] = 0;
    let mut lh = [0; 30];
    if !read(source, u64::from(local), &mut lh) { return Err(S::IoError); }
    if zip_local_header_rule(le32(&lh), le16(&lh[6..]), le16(&lh[8..]), le16(&lh[26..]),
        flags, method, names) == ZipLocalHeaderRule::Mismatch { return Err(S::Malformed); }
    if zip_member_flag_rule(flags) != ZipCentralRecordRule::Ok { return Err(S::Unsupported); }
    let data = u64::from(local) + 30 + u64::from(names) + u64::from(le16(&lh[28..]));
    if data > u64::from(directory) || u64::from(compressed) > u64::from(directory) - data { return Err(S::Malformed); }
    let mut compare = [0; 64];
    for n in (0..usize::from(names)).step_by(64) {
        let take = (usize::from(names) - n).min(64);
        if !read(source, u64::from(local) + 30 + n as u64, &mut compare[..take]) { return Err(S::IoError); }
        if name[n..n + take] != compare[..take] { return Err(S::Malformed); }
    }
    Ok((at + record, data))
}
pub fn locate(source: &mut dyn BookSource, wanted: &[u8], out: &mut Member) -> S {
    *out = Member::default();
    let wanted = cbytes(wanted);
    let mut canonical = [0; 512];
    if !path(b"", wanted, &mut canonical) || wanted != cbytes(&canonical)
        || source.size() < 22 || source.size() > 64 * 1024 * 1024 { return S::Unsupported; }
    let d = match directory(source) { Ok(d) => d, Err(e) => return e };
    if d.count > 1024 || d.bytes > 1024 * 1024 { return S::Unsupported; }
    let end = u64::from(d.offset) + u64::from(d.bytes);
    if end != d.eocd || end > source.size() || source.size() - end < 22 { return S::Malformed; }
    let mut header = [0; 46];
    if !read(source, end, &mut header[..22]) { return S::IoError; }
    if le32(&header) != 0x0605_4b50 || le16(&header[4..]) != 0 || le16(&header[6..]) != 0
        || le16(&header[8..]) != d.count || le16(&header[10..]) != d.count
        || le32(&header[12..]) != d.bytes || le32(&header[16..]) != d.offset
        || end + 22 + u64::from(le16(&header[20..])) != source.size() { return S::Malformed; }
    let (mut at, mut expanded, mut found) = (u64::from(d.offset), 0, false);
    let mut name = [0; 512];
    for _ in 0..d.count {
        let (next, data) = match directory_member(source, at, end, d.offset, &mut expanded, &mut header, &mut name) {
            Ok(v) => v, Err(e) => return e,
        };
        if cbytes(&name) == wanted {
            if found { return S::Malformed; }
            let method = le16(&header[10..]);
            let compressed_size = le32(&header[20..]);
            let uncompressed_size = le32(&header[24..]);
            if method == 0 && compressed_size != uncompressed_size { return S::Unsupported; }
            found = true;
            out.entry = Entry { name: None, name_hash: hash_path(wanted), compressed_size,
                uncompressed_size, local_header_offset: le32(&header[42..]), method };
            out.crc32 = le32(&header[16..]); out.data = data;
        }
        at = next;
    }
    if at != end { S::Malformed } else if found { S::Ok } else { S::NotFound }
}
