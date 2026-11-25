// SPDX-License-Identifier: MIT
//! Open-time ZIP member check: the one owner of the local-header rules.
//!
//! Admission reads only the central directory. Before Newt reads any member
//! (container, OPF, nav, chapter, stylesheet, image or cover), the caller
//! looks it up in a live catalog and passes it through [`check`]. A damaged
//! member then fails where it is used, and the rest of the book stays readable.
use crate::zip_container::{Entry, RAW_HEADER_OFFSET, Status};
use crate::zip_record_rules::{
    EPUB_MAX_MEMBER_NAME_BYTES, ZipCentralRecordRule, ZipLocalHeaderRule, zip_local_header_rule,
    zip_member_flag_rule,
};
use crate::zip_window::source::BookSource;

/// An entry whose local header agrees with its central record. Only [`check`]
/// and [`Checked::raw`] make one, so a retained entry cannot skip the check.
/// The catalog name is dropped: it never outlives the catalog's arena.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Checked(Entry);
impl Checked {
    /// A decoded in-memory stream has no ZIP headers to check.
    pub const fn raw(size: u32) -> Self {
        Self(Entry::raw(size))
    }
}
/// Read-only: a `Checked` reads as its entry, but only [`check`] makes one.
impl core::ops::Deref for Checked {
    type Target = Entry;
    fn deref(&self) -> &Entry {
        &self.0
    }
}
impl Default for Checked {
    fn default() -> Self {
        Self::raw(0)
    }
}

const NAME_CHUNK: usize = 64;

/// Check `entry`'s local header against its central record and return it with
/// the offset of its data. `name` is the central name without its terminator;
/// `directory_offset` is where the central directory starts, so member data
/// must end at or before it. Statuses match what whole-book admission returned
/// for the same faults, except header disagreement, which is `Damaged`.
pub fn check<S: BookSource + ?Sized>(
    source: &mut S,
    entry: &Entry,
    name: &[u8],
    directory_offset: u32,
) -> Result<(Checked, u64), Status> {
    let Ok(name_bytes) = u16::try_from(name.len()) else {
        return Err(Status::Unsupported);
    };
    if entry.local_header_offset == RAW_HEADER_OFFSET
        || name.is_empty()
        || name.len() > EPUB_MAX_MEMBER_NAME_BYTES
        || name.contains(&0)
    {
        return Err(Status::Damaged);
    }
    let local = u64::from(entry.local_header_offset);
    let directory = u64::from(directory_offset);
    if local + 30 > directory {
        return Err(Status::Truncated);
    }
    let mut header = [0; 30];
    if !read_fully(source, local, &mut header) {
        return Err(Status::IoError);
    }
    if zip_local_header_rule(
        le32(&header, 0),
        le16(&header, 6),
        le16(&header, 8),
        le16(&header, 26),
        entry.flags,
        entry.method,
        name_bytes,
    ) == ZipLocalHeaderRule::Mismatch
    {
        return Err(Status::Damaged);
    }
    match zip_member_flag_rule(entry.flags) {
        ZipCentralRecordRule::Encrypted => return Err(Status::Encrypted),
        ZipCentralRecordRule::Unsupported => return Err(Status::Unsupported),
        ZipCentralRecordRule::Ok => (),
    }
    let data = local + 30 + u64::from(name_bytes) + u64::from(le16(&header, 28));
    if data > directory || u64::from(entry.compressed_size) > directory - data {
        return Err(Status::Truncated);
    }
    let mut local_name = [0; NAME_CHUNK];
    for (index, expected) in name.chunks(NAME_CHUNK).enumerate() {
        let at = local + 30 + (index * NAME_CHUNK) as u64;
        let got = &mut local_name[..expected.len()];
        if !read_fully(source, at, got) {
            return Err(Status::IoError);
        }
        if got != expected {
            return Err(Status::Damaged);
        }
    }
    Ok((Checked(Entry { name: None, ..*entry }), data))
}

fn read_fully<S: BookSource + ?Sized>(source: &mut S, mut at: u64, mut out: &mut [u8]) -> bool {
    while !out.is_empty() {
        let got = source.read_at(at, out);
        if got <= 0 || got as usize > out.len() {
            return false;
        }
        at += got as u64;
        out = &mut out[got as usize..];
    }
    true
}
fn le16(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn le32(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

// Entry.flags must fill padding: a bigger Entry shrinks the metadata bank's
// catalog capacity for every book.
const _: () = assert!(size_of::<Entry>() == if cfg!(target_pointer_width = "32") { 24 } else { 32 });

#[cfg(test)]
#[path = "member_check_tests.rs"]
mod tests;
