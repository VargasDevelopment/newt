//! Retained-name validation from src/library_catalog.cpp and include/newt/utf8.h.
//! This is the legacy record validator, not catalog path authorization.

pub const FILENAME_BYTES: usize = 96;

/// Return the bounded C string before its first NUL. Bytes after it are ignored.
pub fn name_bytes(name: &[u8]) -> Option<&[u8]> {
    let length = name.iter().take(FILENAME_BYTES).position(|&b| b == 0)?;
    Some(&name[..length])
}

pub fn valid_library_book_name(name: &[u8]) -> bool {
    let Some(name) = name_bytes(name) else {
        return false;
    };
    if name.len() < 6
        || name[0] == b'.'
        || name.contains(&b'/')
        || name.contains(&b'\\')
        || !name[name.len() - 5..].eq_ignore_ascii_case(b".epub")
    {
        return false;
    }
    // Rust's decoder accepts the same Unicode scalars as utf8::next: no
    // overlong forms, surrogates, invalid continuation bytes or > U+10FFFF.
    core::str::from_utf8(name).is_ok_and(|text| {
        text.chars()
            .all(|cp| cp >= '\u{20}' && !('\u{7f}'..='\u{9f}').contains(&cp))
    })
}
