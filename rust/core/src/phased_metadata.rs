//! Pure admission subset of phased_reader_metadata.h. Rust slices omit the C terminator.
pub fn admissible_href(path: &[u8], capacity: usize) -> bool {
    path.len() < capacity
        && !path.is_empty()
        && !path.contains(&0)
        && core::str::from_utf8(path).is_ok()
        && path[0] != b'/'
        && !path.contains(&b'\\')
        && path
            .split(|b| *b == b'/')
            .all(|s| !s.is_empty() && s != b"." && s != b"..")
}
/// C-string domain: None models null; input must include a terminator.
pub fn bounded_utf8(source: Option<&[u8]>, capacity: usize) -> Option<usize> {
    let source = source?;
    let length = source.iter().take(capacity).position(|b| *b == 0)?;
    core::str::from_utf8(&source[..length]).ok()?;
    Some(length)
}
pub fn copy_utf8(dest: &mut [u8], source: Option<&[u8]>) -> bool {
    let Some(length) = bounded_utf8(source, dest.len()) else {
        return false;
    };
    let Some(source) = source else {
        return false;
    };
    dest[..=length].copy_from_slice(&source[..=length]);
    true
}
pub fn copy_display_title(dest: &mut [u8; 256], source: Option<&[u8]>) {
    let source = source.unwrap_or(b"");
    let length = source
        .iter()
        .take(256)
        .position(|b| *b == 0)
        .unwrap_or(source.len().min(256));
    let valid = match core::str::from_utf8(&source[..length]) {
        Ok(_) => length,
        Err(e) => e.valid_up_to(),
    };
    let mut copied = valid.min(255);
    while copied > 0 && core::str::from_utf8(&source[..copied]).is_err() {
        copied -= 1;
    }
    if copied == 0 {
        dest[..13].copy_from_slice(b"Your SD book\0");
    } else {
        dest[..copied].copy_from_slice(&source[..copied]);
        dest[copied] = 0;
    }
}
/// Pointer-range arithmetic as integers only; no pointer creation/dereference.
/// Actual Rust slice APIs enforce nonaliasing without address comparisons.
pub fn overlaps(a: usize, a_bytes: usize, b: usize, b_bytes: usize) -> bool {
    let (Some(a_end), Some(b_end)) = (a.checked_add(a_bytes), b.checked_add(b_bytes)) else {
        return true;
    };
    a < b_end && b < a_end
}
