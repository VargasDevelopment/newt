//! Strict UTF-8 scalar iterator. MIT.

/// Decode one scalar. A failed nonempty attempt consumes only its first byte;
/// every failure preserves `codepoint`. `None` models the original null pointer.
/// The slice is the original caller's readable `length`, including embedded NULs.
pub fn next(text: Option<&[u8]>, offset: &mut usize, codepoint: &mut u32) -> bool {
    let Some(text) = text else { return false };
    if *offset >= text.len() {
        return false;
    }
    let start = *offset;
    *offset += 1;
    let first = text[start];
    if first < 0x80 {
        *codepoint = u32::from(first);
        return true;
    }
    let extra = match first {
        0xc2..=0xdf => 1,
        0xe0..=0xef => 2,
        0xf0..=0xf4 => 3,
        _ => 0,
    };
    if extra == 0 || text.len() - start - 1 < extra {
        return false;
    }
    let mut value = u32::from(first & (0x7f >> (extra + 1)));
    for i in 1..=extra {
        let byte = text[start + i];
        if byte & 0xc0 != 0x80 {
            return false;
        }
        value = (value << 6) | u32::from(byte & 0x3f);
    }
    if (extra == 1 && value < 0x80)
        || (extra == 2 && value < 0x800)
        || (extra == 3 && value < 0x10000)
        || value > 0x10ffff
        || (0xd800..=0xdfff).contains(&value)
    {
        return false;
    }
    *offset = start + extra + 1;
    *codepoint = value;
    true
}
