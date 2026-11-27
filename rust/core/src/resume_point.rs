//! Layout of a resume-point section inside a v4 page segment. MIT.
//!
//! A point lets a later producer pass restart at a page near the segment's
//! end instead of at the member's first byte. This module freezes only the
//! section's outer layout. The producer captures points into interior v4
//! segments; restore is not wired yet.
//!
//! The sections follow the restore's consumption order. Restore reads the
//! section front to back with one small window and never seeks backwards:
//! the binding is checked before any state is used. The source is positioned
//! before the feed buffer receives prolog, names or replay. The replay comes
//! last, so it is still in the feed buffer when the output opens.
//!
//! ```text
//! header   "FIRP" u16 version u8 mode u8 0, then one u32 length per section
//! Binding  route, positions (pages at the cut, edge, cut, fed), member
//! Source   member position: raw bytes consumed
//! Stream   parser input state: counters and the whole entity filter, raw buffer included
//! Engine   layout engine state
//! Prolog   filtered bytes before the root element
//! Names    open element names: u8 depth, then u8 length + bytes per name
//! Replay   filtered bytes from the cut to the end of the last feed
//! trailer  u32 CRC-32 over the header and every section
//! ```
//!
//! A point that fails any check here is lost on its own. The pages of its
//! segment stay valid, because their extents come from the verified index and
//! footer, never from the point's content.
#![forbid(unsafe_code)]

pub const MAGIC: [u8; 4] = *b"FIRP";
pub const VERSION: u16 = 1;
pub const HEADER_BYTES: usize = 8 + 4 * SECTIONS.len();
pub const TRAILER_BYTES: u32 = 4;
pub const PROLOG_MAX: usize = 512;
pub const NAMES_MAX: usize = 256;
pub const DEPTH_MAX: usize = 48;
pub const REPLAY_MAX: usize = crate::xml_session::PARSE_CHUNK;
pub const WINDOW_PAGES: u32 = 4;
pub const BINDING_BYTES: usize = 108;
pub const SOURCE_BYTES: usize = 8;

/// Why a feed boundary near a segment edge did not capture a resume point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decline {
    Prolog,
    Encoding,
    Cdata,
    Depth,
    Backlog,
    Engine,
    Source,
    Budget,
    Late,
}

/// Non-generic byte sink for measuring or streaming a resume point without a RAM buffer.
pub trait PointOut {
    fn put(&mut self, bytes: &[u8]) -> bool;
}

/// Pack little-endian `u32` words into a small stack window and emit through `PointOut`.
#[inline(never)]
pub fn put_u32s(out: &mut dyn PointOut, vals: &[u32]) -> bool {
    let mut buf = [0u8; 32];
    for chunk in vals.chunks(8) {
        for (dst, v) in buf.chunks_exact_mut(4).zip(chunk) {
            dst.copy_from_slice(&v.to_le_bytes());
        }
        if !out.put(&buf[..chunk.len() * 4]) {
            return false;
        }
    }
    true
}

/// Pack little-endian `u16` values into a small stack window and emit through `PointOut`.
#[inline(never)]
pub fn put_u16s(out: &mut dyn PointOut, vals: &[u16]) -> bool {
    let mut buf = [0u8; 32];
    for chunk in vals.chunks(16) {
        for (dst, v) in buf.chunks_exact_mut(2).zip(chunk) {
            dst.copy_from_slice(&v.to_le_bytes());
        }
        if !out.put(&buf[..chunk.len() * 2]) {
            return false;
        }
    }
    true
}

/// Counts emitted bytes without writing or hashing.
#[derive(Default)]
pub struct Measure {
    pub bytes: u32,
    pub overflow: bool,
}
impl PointOut for Measure {
    fn put(&mut self, bytes: &[u8]) -> bool {
        match u32::try_from(bytes.len()).ok().and_then(|n| self.bytes.checked_add(n)) {
            Some(total) => {
                self.bytes = total;
                true
            }
            None => {
                self.overflow = true;
                false
            }
        }
    }
}

/// Streams emitted bytes while checking an exact byte budget and accumulating
/// the shared envelope CRC-32.
pub struct CrcOut<'a> {
    pub crc: u32,
    pub bytes: u32,
    pub limit: u32,
    pub failed: bool,
    write: &'a mut dyn FnMut(&[u8]) -> bool,
}
impl<'a> CrcOut<'a> {
    pub fn new(limit: u32, write: &'a mut dyn FnMut(&[u8]) -> bool) -> Self {
        Self { crc: u32::MAX, bytes: 0, limit, failed: false, write }
    }
    pub fn finish_crc(&self) -> u32 {
        self.crc ^ u32::MAX
    }
}
impl PointOut for CrcOut<'_> {
    #[inline(never)]
    fn put(&mut self, bytes: &[u8]) -> bool {
        if self.failed {
            return false;
        }
        let Some(next) = u32::try_from(bytes.len())
            .ok()
            .and_then(|n| self.bytes.checked_add(n))
            .filter(|&n| n <= self.limit)
        else {
            self.failed = true;
            return false;
        };
        if !(self.write)(bytes) {
            self.failed = true;
            return false;
        }
        self.crc = crate::page_envelope::crc(self.crc, bytes);
        self.bytes = next;
        true
    }
}

/// How restore positions the chapter member at the cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// A stored member: seek to the saved position.
    Seek,
    /// A deflated member: inflate from its start, discarding output.
    Reinflate,
}

/// The sections in wire order, which is also restore's consumption order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Binding,
    Source,
    Stream,
    Engine,
    Prolog,
    Names,
    Replay,
}

pub const SECTIONS: [Section; 7] = [
    Section::Binding,
    Section::Source,
    Section::Stream,
    Section::Engine,
    Section::Prolog,
    Section::Names,
    Section::Replay,
];

impl Section {
    /// Largest accepted length. Prolog, names and replay match the capture
    /// limits; the engine bound covers the largest layout storage profile.
    /// The stream holds two u64 counters and the entity filter, whose raw
    /// buffer can keep up to 512 unread bytes of the last pull, so restore
    /// continues from the source position after that pull: about 600 B.
    pub const fn max_bytes(self) -> u32 {
        match self {
            Self::Binding => 160,
            Self::Source => 16,
            Self::Stream => 640,
            Self::Engine => 24 * 1024,
            Self::Prolog => 512,
            Self::Names => 1 + 48 + 256,
            Self::Replay => 2048,
        }
    }
    /// Prolog and replay may be empty. Names always holds its depth byte.
    const fn min_bytes(self) -> u32 {
        match self {
            Self::Prolog | Self::Replay => 0,
            _ => 1,
        }
    }
}

/// A point's validated outer layout. Offsets are relative to the section start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub mode: Mode,
    lengths: [u32; SECTIONS.len()],
}

impl Layout {
    /// Checks the header against the point's length from the verified footer.
    /// Every length is bounded, so the arithmetic cannot overflow on RV32.
    pub fn parse(header: &[u8; HEADER_BYTES], point_bytes: u32) -> Option<Self> {
        if header[..4] != MAGIC
            || u16::from_le_bytes([header[4], header[5]]) != VERSION
            || header[7] != 0
        {
            return None;
        }
        let mode = match header[6] {
            0 => Mode::Seek,
            1 => Mode::Reinflate,
            _ => return None,
        };
        let mut lengths = [0; SECTIONS.len()];
        let mut total = HEADER_BYTES as u32 + TRAILER_BYTES;
        for (i, section) in SECTIONS.into_iter().enumerate() {
            let at = 8 + 4 * i;
            let length = u32::from_le_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]]);
            if !(section.min_bytes()..=section.max_bytes()).contains(&length) {
                return None;
            }
            lengths[i] = length;
            total += length;
        }
        (total == point_bytes).then_some(Self { mode, lengths })
    }

    /// Validates section lengths against `Section::min_bytes..=max_bytes` and
    /// returns the layout plus total point size (header + sections + trailer).
    pub fn from_lengths(mode: Mode, lengths: [u32; SECTIONS.len()]) -> Option<(Self, u32)> {
        let mut total = HEADER_BYTES as u32 + TRAILER_BYTES;
        for (i, section) in SECTIONS.into_iter().enumerate() {
            let length = lengths[i];
            if !(section.min_bytes()..=section.max_bytes()).contains(&length) {
                return None;
            }
            total = total.checked_add(length)?;
        }
        Some((Self { mode, lengths }, total))
    }

    /// Encodes the 36-byte `FIRP` header matching `Layout::parse`.
    pub fn write_header(&self, out: &mut [u8; HEADER_BYTES]) {
        out[..4].copy_from_slice(&MAGIC);
        out[4..6].copy_from_slice(&VERSION.to_le_bytes());
        out[6] = match self.mode {
            Mode::Seek => 0,
            Mode::Reinflate => 1,
        };
        out[7] = 0;
        for (i, length) in self.lengths.into_iter().enumerate() {
            let at = 8 + 4 * i;
            out[at..at + 4].copy_from_slice(&length.to_le_bytes());
        }
    }

    /// The section's (offset, length) within the point.
    pub fn range(&self, section: Section) -> (u32, u32) {
        let index = section as usize;
        let before: u32 = self.lengths[..index].iter().sum();
        (HEADER_BYTES as u32 + before, self.lengths[index])
    }

    /// Offset of the trailing CRC-32.
    pub fn trailer(&self) -> u32 {
        HEADER_BYTES as u32 + self.lengths.iter().sum::<u32>()
    }
}

/// Route, layout, member and cut metadata bound inside Section 0 (`Binding`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub book_digest: [u8; 32],
    pub spine: u32,
    pub generation: u32,
    pub segment_index: u32,
    pub page_count: u32,
    pub total_chars: u32,
    pub style_hash: u32,
    pub style_count: u16,
    pub style_issues: u16,
    pub style_state: u8,
    pub profile: u8,
    pub member_method: u16,
    pub member_offset: u32,
    pub member_compressed: u32,
    pub member_uncompressed: u32,
    pub source_bytes: u64,
    pub pages_at: u32,
    pub cut: u64,
    pub fed: u64,
}
impl Binding {
    /// Emits the fixed 108-byte `BND1` binding section.
    #[inline(never)]
    pub fn encode(&self, out: &mut dyn PointOut) -> bool {
        let words = [
            self.spine,
            self.generation,
            self.segment_index,
            self.page_count,
            self.total_chars,
            self.style_hash,
            self.member_offset,
            self.member_compressed,
            self.member_uncompressed,
            self.pages_at,
            self.source_bytes as u32,
            (self.source_bytes >> 32) as u32,
            self.cut as u32,
            (self.cut >> 32) as u32,
            self.fed as u32,
            (self.fed >> 32) as u32,
        ];
        out.put(b"BND1")
            && out.put(&self.book_digest)
            && put_u32s(out, &words)
            && put_u16s(out, &[self.style_count, self.style_issues, self.member_method])
            && out.put(&[self.style_state, self.profile])
    }
}

/// Emits Section 1 (`Source`): `"SRC1"` + uncompressed member bytes consumed.
pub fn encode_source(consumed: u32, out: &mut dyn PointOut) -> bool {
    out.put(b"SRC1") && out.put(&consumed.to_le_bytes())
}

struct DeclCursor<'a>(&'a [u8]);
impl<'a> DeclCursor<'a> {
    fn ws(&mut self) -> bool {
        let start = self.0.len();
        while let Some((&b, tail)) = self.0.split_first() {
            if !matches!(b, b' ' | b'\t' | b'\r' | b'\n') {
                break;
            }
            self.0 = tail;
        }
        self.0.len() < start
    }
    #[inline(never)]
    fn attr(&mut self, name: &[u8]) -> Option<&'a [u8]> {
        let mut c = DeclCursor(self.0.strip_prefix(name)?);
        c.ws();
        c.0 = c.0.strip_prefix(b"=")?;
        c.ws();
        let (&q, tail) = c.0.split_first()?;
        if q != b'\'' && q != b'"' {
            return None;
        }
        let end = tail.iter().position(|&b| b == q)?;
        self.0 = &tail[end + 1..];
        Some(&tail[..end])
    }
}

/// Known-UTF-8 prolog gate without heap allocation: checks the original leading
/// bytes (even when `prolog` is empty), bounded prolog/DTD limits, and any
/// leading XML 1.0 declaration's `encoding` pseudo-attribute (accepting only
/// omitted encoding or case-insensitive `UTF-8`).
///
/// Precondition: this is a capture-time predicate over bytes already accepted
/// by Expat during live parsing, not a standalone validator for untrusted
/// on-card bytes. Restore must still validate/parse synthetic prolog bytes with
/// errors handled before publication.
#[inline(never)]
pub fn validate_utf8_prolog(leading: &[u8], prolog: &[u8]) -> Result<(), Decline> {
    if leading.is_empty()
        || leading.iter().any(|&b| b == 0)
        || matches!(
            leading,
            [0xfe, 0xff, ..] | [0xff, 0xfe, ..] | [0x4c, 0x6f, 0xa7, 0x94, ..]
        )
    {
        return Err(Decline::Encoding);
    }
    if prolog.len() > PROLOG_MAX {
        return Err(Decline::Prolog);
    }
    if prolog.is_empty() {
        return if leading[0] == b'<' {
            Ok(())
        } else {
            Err(Decline::Encoding)
        };
    }
    for &b in prolog {
        if b == 0 {
            return Err(Decline::Encoding);
        }
        if b == b'[' {
            return Err(Decline::Prolog);
        }
    }
    let rest = prolog.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(prolog);
    if rest.get(..5).is_some_and(|p| p.eq_ignore_ascii_case(b"<?xml"))
        && rest.get(5).is_none_or(|&b| matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'?'))
    {
        let after = rest.strip_prefix(b"<?xml").ok_or(Decline::Encoding)?;
        let mut close = None;
        let mut i = 0;
        while i + 1 < after.len() {
            if after[i] == b'?' && after[i + 1] == b'>' {
                close = Some(i);
                break;
            }
            i += 1;
        }
        let mut c = DeclCursor(&after[..close.ok_or(Decline::Encoding)?]);
        if !c.ws() || c.attr(b"version").is_none() {
            return Err(Decline::Encoding);
        }
        if !c.0.is_empty() {
            if !c.ws() {
                return Err(Decline::Encoding);
            }
            if let Some(enc) = c.attr(b"encoding") {
                if !enc.eq_ignore_ascii_case(b"utf-8") {
                    return Err(Decline::Encoding);
                }
                if !c.0.is_empty() && !c.ws() {
                    return Err(Decline::Encoding);
                }
            }
            if !c.0.is_empty() {
                if c.attr(b"standalone").is_none() {
                    return Err(Decline::Encoding);
                }
                c.ws();
            }
            if !c.0.is_empty() {
                return Err(Decline::Encoding);
            }
        }
    }
    Ok(())
}
