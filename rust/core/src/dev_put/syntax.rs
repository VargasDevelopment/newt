// SPDX-License-Identifier: MIT
//! The lease's line syntax: requests, canonical numbers, the digest's hex,
//! raw-block lengths and a bounded reply buffer.
use core::fmt;

#[derive(Clone, Copy)]
pub(super) enum Verb { File, Data, Done }

/// A current lease request; `arguments` is the byte range after its verb.
pub(super) struct Request {
    pub boot: u32,
    pub session: u32,
    pub id: u32,
    pub verb: Verb,
    pub arguments: (usize, usize),
}
impl Request {
    /// `N1 BOOT SESSION ID VERB[ ARGUMENTS]`, with canonical numbers and
    /// single spaces, as the bridge's own commands.
    pub fn parse(line: &[u8]) -> Option<Self> {
        let text = core::str::from_utf8(line).ok()?;
        let mut fields = text.splitn(6, ' ');
        if fields.next()? != "N1" { return None; }
        let boot = number(fields.next()?)?;
        let session = number(fields.next()?)?;
        let id = number(fields.next()?)?;
        let verb = fields.next()?;
        let rest = fields.next();
        let verb = match (verb, rest.is_some()) {
            ("FILE", true) => Verb::File,
            ("DATA", true) => Verb::Data,
            ("DONE", false) => Verb::Done,
            _ => return None,
        };
        let start = rest.map_or(line.len(), |rest| line.len() - rest.len());
        Some(Self { boot, session, id, verb, arguments: (start, line.len()) })
    }
}

pub(super) fn number(text: &str) -> Option<u32> { wide_number(text)?.try_into().ok() }
pub(super) fn wide_number(text: &str) -> Option<u64> {
    if text.is_empty() || (text.starts_with('0') && text.len() != 1) { return None; }
    text.bytes().try_fold(0u64, |value, byte| {
        if !byte.is_ascii_digit() { return None; }
        value.checked_mul(10)?.checked_add(u64::from(byte - b'0'))
    })
}

pub(super) const HEX: &[u8; 16] = b"0123456789abcdef";

/// Exactly 64 lowercase hexadecimal characters.
pub(super) fn digest_hex(text: &str) -> Option<[u8; 32]> {
    let text = text.as_bytes();
    if text.len() != 64 { return None; }
    let nibble = |b: u8| HEX.iter().position(|&h| h == b).map(|v| v as u8);
    let mut digest = [0; 32];
    for (byte, pair) in digest.iter_mut().zip(text.chunks_exact(2)) {
        *byte = (nibble(pair[0])? << 4) | nibble(pair[1])?;
    }
    Some(digest)
}

pub(super) struct Cursor<'a> { pub bytes: &'a mut [u8], pub used: usize }
impl fmt::Write for Cursor<'_> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let end = self.used.checked_add(value.len()).filter(|&end| end <= self.bytes.len()).ok_or(fmt::Error)?;
        self.bytes[self.used..end].copy_from_slice(value.as_bytes());
        self.used = end;
        Ok(())
    }
}
