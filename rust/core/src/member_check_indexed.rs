//! Local names are read only for the member being opened, never retained as a catalog.
use super::*;
use crate::book_index::{self, Member};

/// Resolve the name from the local header, then apply the shared ZIP check.
/// A hash hit is never sufficient for a link: its exact requested name must
/// agree too. `name` is caller scratch and returns a NUL-terminated path.
pub fn check_indexed(
    source: &mut dyn BookSource, member: &Member, expected: Option<&[u8]>,
    name: &mut [u8], directory_offset: u32,
) -> Result<Checked, Status> {
    let local = u64::from(member.local_offset);
    if local + 30 > u64::from(directory_offset) { return Err(Status::Truncated); }
    let mut header = [0; 30];
    if !read_fully(source, local, &mut header) { return Err(Status::IoError); }
    let length = usize::from(le16(&header, 26));
    if length == 0 || length > EPUB_MAX_MEMBER_NAME_BYTES { return Err(Status::Damaged); }
    if local + 30 + length as u64 > u64::from(directory_offset) { return Err(Status::Truncated); }
    if length >= name.len() { return Err(Status::PathTooLong); }
    if !read_fully(source, local + 30, &mut name[..length]) { return Err(Status::IoError); }
    name[length] = 0;
    let path = &name[..length];
    if book_index::name_hash(path) != member.hash || expected.is_some_and(|expected| {
        let end = expected.iter().position(|&byte| byte == 0).unwrap_or(expected.len());
        path != &expected[..end]
    }) { return Err(Status::Damaged); }
    let entry = Entry { compressed_size: member.compressed, uncompressed_size: member.uncompressed,
        local_header_offset: member.local_offset, method: member.method, flags: member.zip_flags(),
        ..Entry::default() };
    check(source, &entry, path, directory_offset).map(|(checked, _)| checked)
}
