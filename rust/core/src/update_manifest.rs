// SPDX-License-Identifier: MIT
//! Firmware update manifest parser and SemVer comparison.
//! Caller-owned fields only; native Rust layout is never a persisted/ABI record.
#![forbid(unsafe_code)]

#[path = "update_manifest/semver.rs"]
mod semver;
pub use semver::{semver2_compare, semver2_valid, semver2_is_stable, semver2_is_nightly};

pub const MAX_BYTES: usize = 512;
pub const FORMAT: &[u8] = b"newt-update-1";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum UpdateKind {
    #[default]
    Newt,
    Firmware,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UpdateManifest {
    pub kind: UpdateKind,
    pub version: [u8; 32],
    pub build: [u8; 16],
    pub elf_sha256: [u8; 32],
    pub size: u32,
    pub sha256: [u8; 32],
}

fn field<'a>(at: &mut &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    if at.len() <= key.len() || !at.starts_with(key) || at[key.len()] != b'=' {
        return None;
    }
    let tail = &at[key.len() + 1..];
    let end = tail.iter().position(|&c| c == b'\n')?;
    let value = &tail[..end];
    if value.is_empty() || value.iter().any(|&c| !(b'!'..=b'~').contains(&c)) {
        return None;
    }
    *at = &tail[end + 1..];
    Some(value)
}

fn copy_text(value: &[u8], out: &mut [u8]) -> bool {
    if value.len() >= out.len() {
        return false;
    }
    out[..value.len()].copy_from_slice(value);
    out[value.len()] = 0;
    true
}

fn hex_digest(value: &[u8], out: &mut [u8; 32]) -> bool {
    if value.len() != 64 {
        return false;
    }
    for (i, &c) in value.iter().enumerate() {
        let nibble = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            _ => return false,
        };
        // Preserve the observable high-nibble-only byte on a later failure.
        out[i / 2] = if i % 2 == 0 { nibble } else { (out[i / 2] << 4) | nibble };
    }
    true
}

fn decimal_size(value: &[u8], out: &mut u32) -> bool {
    if value.is_empty() || value.len() > 10 || value[0] == b'0' {
        return false;
    }
    let mut number = 0u64;
    for &c in value {
        if !c.is_ascii_digit() {
            return false;
        }
        number = number * 10 + u64::from(c - b'0');
    }
    if number > u64::from(u32::MAX) {
        return false;
    }
    *out = number as u32;
    true
}

/// None models null. Only the first `length` readable bytes are consumed.
/// Output is reset first, then retains exact source partial writes on failure.
/// An extent beyond the lent slice is safely rejected, never read out of bounds.
pub fn parse_update_manifest(text: Option<&[u8]>, length: usize, manifest: &mut UpdateManifest) -> bool {
    *manifest = UpdateManifest::default();
    let Some(text) = text else { return false };
    if length == 0 || length > MAX_BYTES {
        return false;
    }
    let Some(mut at) = text.get(..length) else { return false };
    if at.contains(&0) {
        return false;
    }
    if field(&mut at, b"format") != Some(FORMAT) || field(&mut at, b"kind") != Some(b"newt".as_slice()) {
        return false;
    }
    manifest.kind = UpdateKind::Newt;
    let Some(version) = field(&mut at, b"version") else { return false };
    if !copy_text(version, &mut manifest.version[..24]) || !semver::valid_bytes(version) {
        return false;
    }
    let Some(build) = field(&mut at, b"build") else { return false };
    if build.len() != 12 || !copy_text(build, &mut manifest.build) {
        return false;
    }
    if build.iter().any(|&c| !(c.is_ascii_digit() || (b'a'..=b'f').contains(&c))) {
        return false;
    }
    let Some(size) = field(&mut at, b"size") else { return false };
    if !decimal_size(size, &mut manifest.size) {
        return false;
    }
    let Some(sha) = field(&mut at, b"sha256") else { return false };
    if !hex_digest(sha, &mut manifest.sha256) {
        return false;
    }
    at.is_empty()
}
