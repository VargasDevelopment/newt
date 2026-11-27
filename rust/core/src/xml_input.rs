// Copyright (c) 2026 FreeInk. MIT; see rust/THIRD_PARTY.md.
//! Allocation-free XML input and entity helpers derived from FreeInk; see tests/xml_input/CONTRACT.md.
use crate::zip_container::Reader;
use crate::zip_window::arena::{Arena, ByteRange};
use crate::zip_window::source::BookSource;

mod events;

pub trait BytePull {
    /// Returns 0 at EOF, negative on error, or at most dst.len() bytes.
    fn pull(&mut self, dst: &mut [u8]) -> i32;
}
impl<S: BookSource + ?Sized> BytePull for Reader<'_, S> {
    fn pull(&mut self, dst: &mut [u8]) -> i32 {
        self.read(dst)
    }
}

pub struct EntityFilter {
    entity: [u8; 13],
    entity_len: usize,
    in_entity: bool,
    pending: [u8; 32],
    pending_len: usize,
    pending_pos: usize,
    raw: [u8; 512],
    raw_pos: usize,
    raw_len: usize,
    source_done: bool,
}
impl Default for EntityFilter {
    fn default() -> Self {
        Self::new()
    }
}
impl EntityFilter {
    pub const fn new() -> Self {
        Self {
            entity: [0; 13],
            entity_len: 0,
            in_entity: false,
            pending: [0; 32],
            pending_len: 0,
            pending_pos: 0,
            raw: [0; 512],
            raw_pos: 0,
            raw_len: 0,
            source_done: false,
        }
    }
    pub fn reset(&mut self) {
        self.entity_len = 0;
        self.in_entity = false;
        self.pending_len = 0;
        self.pending_pos = 0;
        self.raw_pos = 0;
        self.raw_len = 0;
        self.source_done = false;
    }
    /// Serialize the live filter carry (entity, pending and unread raw tail).
    pub fn save_resume(&self, out: &mut dyn crate::resume_point::PointOut) -> bool {
        if self.entity_len > self.entity.len()
            || self.pending_len > self.pending.len()
            || self.pending_pos > self.pending_len
            || self.raw_pos > self.raw_len
            || self.raw_len > self.raw.len()
        {
            return false;
        }
        let raw_unread = (self.raw_len - self.raw_pos) as u16;
        let mut h = [0u8; 10];
        h[..4].copy_from_slice(b"FLT1");
        h[4] = self.entity_len as u8;
        h[5] = u8::from(self.in_entity);
        h[6] = self.pending_len as u8;
        h[7] = self.pending_pos as u8;
        h[8..10].copy_from_slice(&raw_unread.to_le_bytes());
        out.put(&h)
            && out.put(&self.entity[..self.entity_len])
            && out.put(&self.pending[..self.pending_len])
            && out.put(&self.raw[self.raw_pos..self.raw_len])
            && out.put(&[u8::from(self.source_done)])
    }
    /// Restore the live filter carry from a bounded resume-point section.
    /// Fails closed on any out-of-range length, invalid boolean, or short read.
    #[inline(never)]
    pub fn load_resume(&mut self, sec: &mut crate::resume_point::SectionIn<'_, '_>) -> bool {
        self.reset();
        self.entity.fill(0);
        self.pending.fill(0);
        self.raw.fill(0);
        let mut h = [0u8; 10];
        if !sec.read_exact(&mut h) || &h[..4] != b"FLT1" {
            return false;
        }
        let entity_len = usize::from(h[4]);
        if h[5] > 1 {
            return false;
        }
        let in_entity = h[5] == 1;
        let pending_len = usize::from(h[6]);
        let pending_pos = usize::from(h[7]);
        let raw_unread = usize::from(u16::from_le_bytes([h[8], h[9]]));
        if entity_len > 10
            || (!in_entity && entity_len != 0)
            || pending_len > self.pending.len()
            || (pending_len == 0 && pending_pos != 0)
            || (pending_len > 0 && pending_pos >= pending_len)
            || (in_entity && pending_len > 0)
            || raw_unread > self.raw.len()
        {
            return false;
        }
        let mut done = [0u8; 1];
        if !sec.read_exact(&mut self.entity[..entity_len])
            || !self.entity[..entity_len]
                .iter()
                .all(|&b| b.is_ascii_alphanumeric() || b == b'#')
            || !sec.read_exact(&mut self.pending[..pending_len])
            || !sec.read_exact(&mut self.raw[..raw_unread])
            || !sec.read_exact(&mut done)
            || done[0] > 1
            || (done[0] == 1 && (raw_unread > 0 || in_entity))
        {
            self.reset();
            return false;
        }
        self.entity_len = entity_len;
        self.in_entity = in_entity;
        self.pending_len = pending_len;
        self.pending_pos = pending_pos;
        self.raw_pos = 0;
        self.raw_len = raw_unread;
        self.source_done = done[0] == 1;
        true
    }
    pub fn source_done(&self) -> bool {
        self.source_done
    }
    pub fn raw_unread(&self) -> usize {
        self.raw_len.saturating_sub(self.raw_pos)
    }
    fn emit(&mut self, bytes: &[u8]) {
        let n = bytes.len().min(32 - self.pending_len);
        self.pending[self.pending_len..self.pending_len + n].copy_from_slice(&bytes[..n]);
        self.pending_len += n;
    }
    fn emit_entity(&mut self) {
        let entity = self.entity;
        self.emit(&entity[..self.entity_len]);
    }
    fn resolve(&mut self) {
        let name = &self.entity[..self.entity_len];
        if name.first() == Some(&b'#')
            || [b"amp".as_slice(), b"lt", b"gt", b"quot", b"apos"].contains(&name)
        {
            self.emit(b"&");
            self.emit_entity();
            self.emit(b";");
        } else {
            let replacement = ENTITIES
                .iter()
                .find(|(n, _)| *n == name)
                .map_or("�", |(_, s)| *s);
            self.emit(replacement.as_bytes());
        }
        self.entity_len = 0;
        self.in_entity = false;
    }
    pub fn read<P: BytePull + ?Sized>(&mut self, reader: &mut P, dst: &mut [u8]) -> i32 {
        // Return type is i32, so reject destinations larger than i32::MAX.
        if dst.len() > i32::MAX as usize {
            return -1;
        }
        let mut produced = 0;
        loop {
            while self.pending_pos < self.pending_len && produced < dst.len() {
                dst[produced] = self.pending[self.pending_pos];
                produced += 1;
                self.pending_pos += 1;
            }
            if self.pending_pos >= self.pending_len {
                self.pending_pos = 0;
                self.pending_len = 0;
            }
            if produced == dst.len() {
                return produced as i32;
            }
            if self.raw_pos >= self.raw_len {
                if self.source_done {
                    if self.in_entity {
                        self.emit(b"&amp;");
                        self.emit_entity();
                        self.entity_len = 0;
                        self.in_entity = false;
                        continue;
                    }
                    return produced as i32;
                }
                let n = reader.pull(&mut self.raw);
                if n < 0 || n as usize > 512 {
                    return -1;
                }
                if n == 0 {
                    self.source_done = true;
                    continue;
                }
                self.raw_pos = 0;
                self.raw_len = n as usize;
            }
            while self.raw_pos < self.raw_len && produced < dst.len() && self.pending_len == 0 {
                let c = self.raw[self.raw_pos];
                if !self.in_entity {
                    if c == b'&' {
                        self.in_entity = true;
                        self.entity_len = 0;
                    } else {
                        dst[produced] = c;
                        produced += 1;
                    }
                    self.raw_pos += 1;
                    continue;
                }
                if c == b';' {
                    self.raw_pos += 1;
                    self.resolve();
                    break;
                }
                if !(c.is_ascii_alphanumeric() || c == b'#') || self.entity_len >= 10 {
                    self.emit(b"&amp;");
                    self.emit_entity();
                    self.entity_len = 0;
                    self.in_entity = false;
                    break;
                }
                self.entity[self.entity_len] = c;
                self.entity_len += 1;
                self.raw_pos += 1;
            }
        }
    }
}

const ENTITIES: &[(&[u8], &str)] = &[
    (b"aelig", "æ"),
    (b"agrave", "à"),
    (b"auml", "ä"),
    (b"bull", "•"),
    (b"ccedil", "ç"),
    (b"cent", "¢"),
    (b"copy", "©"),
    (b"dagger", "†"),
    (b"Dagger", "‡"),
    (b"deg", "°"),
    (b"divide", "÷"),
    (b"eacute", "é"),
    (b"egrave", "è"),
    (b"euro", "€"),
    (b"frac12", "½"),
    (b"frac14", "¼"),
    (b"hellip", "…"),
    (b"laquo", "«"),
    (b"ldquo", "“"),
    (b"lsquo", "‘"),
    (b"mdash", "—"),
    (b"middot", "·"),
    (b"minus", "−"),
    (b"nbsp", " "),
    (b"ndash", "–"),
    (b"ntilde", "ñ"),
    (b"oelig", "œ"),
    (b"ouml", "ö"),
    (b"para", "¶"),
    (b"permil", "‰"),
    (b"plusmn", "±"),
    (b"pound", "£"),
    (b"prime", "′"),
    (b"Prime", "″"),
    (b"raquo", "»"),
    (b"rdquo", "”"),
    (b"reg", "®"),
    (b"rsquo", "’"),
    (b"sect", "§"),
    (b"shy", "­"),
    (b"sup2", "²"),
    (b"sup3", "³"),
    (b"szlig", "ß"),
    (b"times", "×"),
    (b"trade", "™"),
    (b"uuml", "ü"),
    (b"yen", "¥"),
];

pub fn c_bytes(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len())]
}
pub fn local_name(qname: &[u8]) -> &[u8] {
    let qname = c_bytes(qname);
    &qname[qname.iter().rposition(|b| *b == b':').map_or(0, |i| i + 1)..]
}
pub fn local_is(qname: &[u8], local: &[u8]) -> bool {
    local_name(qname) == c_bytes(local)
}
pub type Attribute<'a> = (&'a [u8], &'a [u8]);
pub fn attr_local<'a>(atts: &[Attribute<'a>], local: &[u8]) -> Option<&'a [u8]> {
    atts.iter()
        .find(|(n, _)| local_is(n, local))
        .map(|(_, v)| c_bytes(v))
}
pub fn attr_exact<'a>(atts: &[Attribute<'a>], name: &[u8]) -> Option<&'a [u8]> {
    atts.iter()
        .find(|(n, _)| c_bytes(n) == c_bytes(name))
        .map(|(_, v)| c_bytes(v))
}
fn space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}
pub fn has_token(value: Option<&[u8]>, token: &[u8]) -> bool {
    let Some(value) = value.map(c_bytes) else {
        return false;
    };
    let token = c_bytes(token);
    let mut p = 0;
    while p < value.len() {
        while p < value.len() && space(value[p]) {
            p += 1;
        }
        let start = p;
        while p < value.len() && !space(value[p]) {
            p += 1;
        }
        if &value[start..p] == token {
            return true;
        }
    }
    false
}
pub const MAX_TEXT_CAPTURE: usize = 255;
pub struct TextCapture {
    pub buf: [u8; 256],
    pub len: usize,
    pub active: bool,
}
#[derive(Default)]
pub struct EncryptionScan {
    pub content_encrypted: bool,
}

pub const MAX_PATH: usize = 512;
pub fn dir_name(path: &[u8], out: &mut [u8]) -> bool {
    let path = c_bytes(path);
    let len = path.iter().rposition(|b| *b == b'/').unwrap_or(0);
    if len >= out.len() {
        return false;
    }
    out[..len].copy_from_slice(&path[..len]);
    out[len] = 0;
    true
}
fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HrefError { Missing, TooLong, Capacity }

pub fn resolve_href(
    arena: &mut Arena<'_>,
    base_dir: Option<&[u8]>,
    href: Option<&[u8]>,
    fragment_out: Option<&mut Option<ByteRange>>,
) -> Option<ByteRange> {
    resolve_href_checked(arena, base_dir, href, fragment_out).ok()
}

/// Keep path-policy refusal distinct from the caller's arena capacity.
/// Fragment allocation order and partial arena writes match the Option adapter.
pub fn resolve_href_checked(
    arena: &mut Arena<'_>,
    base_dir: Option<&[u8]>,
    href: Option<&[u8]>,
    mut fragment_out: Option<&mut Option<ByteRange>>,
) -> Result<ByteRange, HrefError> {
    if let Some(out) = fragment_out.as_deref_mut() {
        *out = None;
    }
    let href = c_bytes(href.ok_or(HrefError::Missing)?);
    let (raw, fragment) = match href.iter().position(|b| *b == b'#') {
        Some(at) => (&href[..at], Some(&href[at + 1..])),
        None => (href, None),
    };
    let mut decoded = HrefBytes(raw);
    let mut decoded_len = 0;
    let mut path_end = raw.len();
    let mut terminated = false;
    while !decoded.0.is_empty() {
        let at = raw.len() - decoded.0.len();
        let c = decoded.next().ok_or(HrefError::Missing)?;
        if decoded_len + 1 >= MAX_PATH {
            return Err(HrefError::TooLong);
        }
        decoded_len += 1;
        if c == 0 && !terminated {
            path_end = at;
            terminated = true;
        }
    }
    let mut path = HrefBytes(&raw[..path_end]);
    let absolute = HrefBytes(path.0).next() == Some(b'/');
    let base = if absolute {
        path.next();
        &[][..]
    } else {
        base_dir.map(c_bytes).unwrap_or(&[])
    };
    if base.len() >= MAX_PATH {
        return Err(HrefError::TooLong);
    }
    // Track separators, not path bytes: the final allocation may fit when a
    // long intermediate path would not fit in the arena's remaining space.
    let mut joined = JoinedLength {
        len: base.len(),
        slashes: [0; MAX_PATH / 32],
    };
    for (at, b) in base.iter().enumerate() {
        if *b == b'/' {
            joined.slashes[at / 32] |= 1 << (at % 32);
        }
    }
    let mut scan = HrefBytes(path.0);
    while let Some((_, len, dots)) = scan.segment() {
        joined.push(len, dots).ok_or(HrefError::TooLong)?;
    }
    let joined_len = joined.len;
    if let (Some(fragment), Some(out)) = (fragment, fragment_out) {
        *out = arena.copy_nul(Some(fragment));
        if out.is_none() {
            return Err(HrefError::Capacity);
        }
    }
    let result = arena.alloc(joined_len + 1, 1).ok_or(HrefError::Capacity)?;
    let output = arena.view_mut(result).ok_or(HrefError::Capacity)?;
    output[joined_len] = 0;
    // Emit backwards into the exact final extent, skipping segments removed
    // by later parents instead of keeping either 512-byte scratch string.
    let mut at = joined_len;
    let mut parents = 0;
    while let Some((raw, len, dots)) = path.segment_back() {
        if dots && len == 2 {
            parents += 1;
        } else if len != 0 && !(dots && len == 1) {
            if parents != 0 {
                parents -= 1;
            } else {
                if at != joined_len {
                    at -= 1;
                    output[at] = b'/';
                }
                for b in HrefBytes(raw).rev() {
                    at -= 1;
                    output[at] = b;
                }
            }
        }
    }
    let mut base_len = base.len();
    while parents != 0 && base_len != 0 {
        base_len = base[..base_len].iter().rposition(|b| *b == b'/').unwrap_or(0);
        parents -= 1;
    }
    if base_len != 0 {
        if at != joined_len {
            at -= 1;
            output[at] = b'/';
        }
        output[..base_len].copy_from_slice(&base[..base_len]);
    }
    Ok(result)
}

struct HrefBytes<'a>(&'a [u8]);
fn percent_byte(a: u8, b: u8) -> Option<u8> {
    let a = hex_digit(a)?;
    let b = hex_digit(b)?;
    Some(a * 16 + b)
}
impl Iterator for HrefBytes<'_> {
    type Item = u8;
    fn next(&mut self) -> Option<u8> {
        let (&c, rest) = self.0.split_first()?;
        self.0 = rest;
        if c == b'%' && rest.len() >= 2 {
            if let Some(c) = percent_byte(rest[0], rest[1]) {
                self.0 = &rest[2..];
                return Some(c);
            }
        }
        Some(c)
    }
}
impl DoubleEndedIterator for HrefBytes<'_> {
    fn next_back(&mut self) -> Option<u8> {
        let len = self.0.len();
        if len >= 3 && self.0[len - 3] == b'%' {
            if let Some(c) = percent_byte(self.0[len - 2], self.0[len - 1]) {
                self.0 = &self.0[..len - 3];
                return Some(c);
            }
        }
        let (&c, rest) = self.0.split_last()?;
        self.0 = rest;
        Some(c)
    }
}
impl<'a> HrefBytes<'a> {
    fn segment(&mut self) -> Option<(&'a [u8], usize, bool)> {
        let raw = self.0;
        if raw.is_empty() {
            return None;
        }
        let mut len = 0;
        let mut dots = true;
        while !self.0.is_empty() {
            let end = raw.len() - self.0.len();
            let c = self.next()?;
            if c == b'/' {
                return Some((&raw[..end], len, dots));
            }
            dots &= c == b'.';
            len += 1;
        }
        Some((raw, len, dots))
    }
    fn segment_back(&mut self) -> Option<(&'a [u8], usize, bool)> {
        let raw = self.0;
        if raw.is_empty() {
            return None;
        }
        let mut len = 0;
        let mut dots = true;
        while !self.0.is_empty() {
            let start = self.0.len();
            let c = self.next_back()?;
            if c == b'/' {
                return Some((&raw[start..], len, dots));
            }
            dots &= c == b'.';
            len += 1;
        }
        Some((raw, len, dots))
    }
}

struct JoinedLength {
    len: usize,
    slashes: [u32; MAX_PATH / 32],
}
impl JoinedLength {
    fn push(&mut self, len: usize, dots: bool) -> Option<()> {
        if dots && len == 2 {
            self.len = 0;
            for (word, bits) in self.slashes.iter_mut().enumerate().rev() {
                if *bits != 0 {
                    let bit = 31 - bits.leading_zeros() as usize;
                    *bits &= !(1 << bit);
                    self.len = word * 32 + bit;
                    break;
                }
            }
        } else if len != 0 && !(dots && len == 1) {
            let separator = usize::from(self.len != 0);
            if self.len + separator + len >= MAX_PATH {
                return None;
            }
            if separator != 0 {
                self.slashes[self.len / 32] |= 1 << (self.len % 32);
            }
            self.len += separator + len;
        }
        Some(())
    }
}
