// SPDX-License-Identifier: MIT
//! The single card-root boundary, shared by FILE and its directory creation.
use super::{PATH_BYTES, ROOT};

/// FAT long names, without aliases from trailing dots/spaces, DOS devices,
/// separators or parent traversal. UTF-8 is decoded before checking names.
pub fn name_allowed(name: &[u8]) -> bool {
    let Ok(text) = core::str::from_utf8(name) else { return false };
    let stem = text.split('.').next().unwrap_or_default();
    let device = ["CON", "PRN", "AUX", "NUL"].iter().any(|word| stem.eq_ignore_ascii_case(word))
        || ((stem.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("COM") || p.eq_ignore_ascii_case("LPT")))
            && stem.len() == 4 && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    !text.is_empty() && ![b".newt-put".as_slice(), b".newt-old", b".newt-old-name"].iter().any(|reserved| name.eq_ignore_ascii_case(reserved))
        && text.encode_utf16().count() <= 255 && !device
        && !name.windows(2).any(|pair| pair == b"..")
        && !matches!(name.last(), Some(b'.' | b' '))
        && text.chars().all(|c| !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
}

/// Hex keeps spaces and UTF-8 off the line grammar. Every component is checked
/// before any card call; neither a host prefix nor an absolute path is accepted.
pub(super) fn target_path(hex: &str) -> Option<[u8; PATH_BYTES]> {
    let prefix = ROOT.to_bytes();
    let size = hex.len() / 2;
    if hex.is_empty() || hex.len() % 2 != 0 || prefix.len() + 1 + size >= PATH_BYTES { return None; }
    let mut out = [0; PATH_BYTES];
    out[..prefix.len()].copy_from_slice(prefix);
    out[prefix.len()] = b'/';
    for (byte, pair) in out[prefix.len() + 1..][..size].iter_mut().zip(hex.as_bytes().chunks_exact(2)) {
        let nibble = |c: u8| c.to_ascii_lowercase().checked_sub(b'0').filter(|&n| n < 10)
            .or_else(|| c.to_ascii_lowercase().checked_sub(b'a').filter(|&n| n < 6).map(|n| n + 10));
        *byte = nibble(pair[0])? * 16 + nibble(pair[1])?;
    }
    let relative = &out[prefix.len() + 1..][..size];
    if !relative.split(|&b| b == b'/').all(name_allowed) { return None; }
    Some(out)
}
