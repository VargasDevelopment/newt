// Included privately: unchanged names accepted by the same native owner.
/// C-string APIs consume the bytes before the first NUL.
fn c_str(name: &[u8]) -> &[u8] {
    &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())]
}

/// The original validName; its result is the only UTF-8 conversion.
fn valid_name(name: &[u8]) -> Option<&str> {
    let name = c_str(name);
    if newt_core::book_index::FileName::parse(name).is_some_and(|name|
        name.kind()==newt_core::book_index::FileKind::Index) {return core::str::from_utf8(name).ok();}
    let valid = !name.is_empty()
        && name.len() <= 127
        && !name.windows(2).any(|pair| pair == b"..")
        && !name.ends_with(b".bak")
        && name
            .iter()
            .all(|&b| b.is_ascii_digit() || b.is_ascii_lowercase() || b == b'-' || b == b'.');
    if valid {
        core::str::from_utf8(name).ok()
    } else {
        None
    }
}
