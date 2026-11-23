// SPDX-License-Identifier: MIT
//! The lease's line syntax: requests, canonical numbers, the digest's hex,
//! strict base64 and a bounded reply buffer.
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

pub(super) fn number(text: &str) -> Option<u32> {
    if text.is_empty() || (text.starts_with('0') && text.len() != 1) { return None; }
    text.bytes().try_fold(0u32, |value, byte| {
        if !byte.is_ascii_digit() { return None; }
        value.checked_mul(10)?.checked_add(u32::from(byte - b'0'))
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

/// Strict, canonical standard base64, decoded over its own first bytes (the
/// output never overtakes the input).
pub(super) fn decode_base64_in_place(text: &mut [u8]) -> Option<usize> {
    if text.is_empty() || text.len() % 4 != 0 { return None; }
    let value = |b: u8| -> Option<u32> {
        Some(u32::from(match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        }))
    };
    let groups = text.len() / 4;
    let mut out = 0;
    for group in 0..groups {
        let at = group * 4;
        let quad = [text[at], text[at + 1], text[at + 2], text[at + 3]];
        let last = group + 1 == groups;
        let padding = match quad {
            [_, _, b'=', b'='] if last => 2,
            [_, _, _, b'='] if last => 1,
            _ => 0,
        };
        let mut bits = 0;
        for &b in &quad[..4 - padding] { bits = (bits << 6) | value(b)?; }
        bits <<= 6 * padding as u32;
        let bytes = [(bits >> 16) as u8, (bits >> 8) as u8, bits as u8];
        // Canonical: bits under the padding must be zero.
        if bytes[3 - padding..].iter().any(|&b| b != 0) { return None; }
        text[out..out + 3 - padding].copy_from_slice(&bytes[..3 - padding]);
        out += 3 - padding;
    }
    Some(out)
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
