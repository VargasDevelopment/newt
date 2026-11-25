//! Book index codec (NEWT-166). One block-envelope payload per verified book
//! revision: a header, every ZIP member sorted by name hash, then the spine in
//! reading order. Records have fixed sizes so a reader fetches one by position
//! instead of parsing the ZIP directory and OPF again.
#![forbid(unsafe_code)]

use crate::cache_names::Digest;
use crate::page_envelope::MAX_PAYLOAD;
use crate::zip_record_rules::EPUB_MAX_EXPANDED_BYTES;

/// Bump when the payload layout or meaning changes; older indexes rebuild.
pub const SCHEMA: u32 = 1;
pub const HEADER_BYTES: u32 = 128;
pub const MEMBER_BYTES: u32 = 32;
pub const SPINE_BYTES: u32 = 16;
/// 65,535 is the classic ZIP count sentinel for ZIP64.
pub const MAX_MEMBERS: u32 = 65_534;
pub const MAX_SPINE: u32 = 32_768;
/// An absent optional member or spine reference.
pub const NONE: u32 = u32::MAX;
const MAGIC: [u8; 4] = *b"NBIX";

// The largest index always fits one envelope, so no book reaches a separate
// "index too large" refusal under these limits.
const _: () = assert!(
    HEADER_BYTES as u64 + MAX_MEMBERS as u64 * MEMBER_BYTES as u64
        + MAX_SPINE as u64 * SPINE_BYTES as u64
        <= MAX_PAYLOAD as u64
);

/// What the index was built from. A mismatch means rebuild, never repair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    pub digest: Digest,
    /// Admission policy the book passed; bump with any admission rule change.
    pub policy: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub identity: Identity,
    pub members: u32,
    pub spine: u32,
    /// Sum of expanded spine member bytes, counting repeats.
    pub spine_bytes: u64,
    pub nav: u32,
    pub ncx: u32,
    pub cover: u32,
}

/// One ZIP member, keyed by the 64-bit hash of its normalized path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Member {
    pub hash: u64,
    pub local_offset: u32,
    pub compressed: u32,
    pub uncompressed: u32,
    pub method: u16,
    pub flags: u16,
    /// First spine item reading this member, or NONE.
    pub first_spine: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpineItem {
    pub member: u32,
    /// Expanded spine bytes before this item; drives estimated progress.
    pub before: u64,
}

/// Payload bytes for these counts, or None outside the supported envelope.
pub const fn payload_bytes(members: u32, spine: u32) -> Option<u32> {
    if members == 0 || members > MAX_MEMBERS || spine == 0 || spine > MAX_SPINE {
        return None;
    }
    Some(HEADER_BYTES + members * MEMBER_BYTES + spine * SPINE_BYTES)
}

/// `<64 hex digest>.fibx`: the envelope route is the source digest, so block
/// checks also refuse another book's index under this name.
pub fn name(digest: &Digest, out: &mut [u8; 69]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for (i, byte) in digest.iter().enumerate() {
        out[2 * i] = HEX[usize::from(byte >> 4)];
        out[2 * i + 1] = HEX[usize::from(byte & 15)];
    }
    out[64..].copy_from_slice(b".fibx");
}

fn get16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
fn get32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
fn get64(b: &[u8], at: usize) -> u64 {
    u64::from(get32(b, at)) | (u64::from(get32(b, at + 4)) << 32)
}
fn optional(value: u32, count: u32) -> bool {
    value == NONE || value < count
}

impl Header {
    pub fn payload_bytes(&self) -> Option<u32> {
        payload_bytes(self.members, self.spine)
    }
    /// Payload offset of member `index`.
    pub fn member_offset(&self, index: u32) -> Option<u32> {
        (index < self.members).then(|| HEADER_BYTES + index * MEMBER_BYTES)
    }
    /// Payload offset of spine item `index`.
    pub fn spine_offset(&self, index: u32) -> Option<u32> {
        (index < self.spine)
            .then(|| HEADER_BYTES + self.members * MEMBER_BYTES + index * SPINE_BYTES)
    }
    pub fn encode(&self, out: &mut [u8; HEADER_BYTES as usize]) {
        out.fill(0);
        out[..4].copy_from_slice(&MAGIC);
        out[4..8].copy_from_slice(&SCHEMA.to_le_bytes());
        out[8..12].copy_from_slice(&self.identity.policy.to_le_bytes());
        out[12..44].copy_from_slice(&self.identity.digest);
        out[44..48].copy_from_slice(&self.members.to_le_bytes());
        out[48..52].copy_from_slice(&self.spine.to_le_bytes());
        out[52..60].copy_from_slice(&self.spine_bytes.to_le_bytes());
        out[60..64].copy_from_slice(&self.nav.to_le_bytes());
        out[64..68].copy_from_slice(&self.ncx.to_le_bytes());
        out[68..72].copy_from_slice(&self.cover.to_le_bytes());
    }
    /// None unless the header is this schema, built for `expected`, and its
    /// counts describe exactly `payload` bytes.
    pub fn decode(input: &[u8; HEADER_BYTES as usize], expected: &Identity, payload: u32) -> Option<Self> {
        let header = Self {
            identity: Identity {
                digest: input[12..44].try_into().ok()?,
                policy: get32(input, 8),
            },
            members: get32(input, 44),
            spine: get32(input, 48),
            spine_bytes: get64(input, 52),
            nav: get32(input, 60),
            ncx: get32(input, 64),
            cover: get32(input, 68),
        };
        let valid = input[..4] == MAGIC
            && get32(input, 4) == SCHEMA
            && header.identity == *expected
            && header.payload_bytes() == Some(payload)
            && header.spine_bytes <= EPUB_MAX_EXPANDED_BYTES
            && [header.nav, header.ncx, header.cover].iter().all(|&m| optional(m, header.members))
            && input[72..].iter().all(|&b| b == 0);
        valid.then_some(header)
    }
}

impl Member {
    pub fn encode(&self, out: &mut [u8; MEMBER_BYTES as usize]) {
        out.fill(0);
        out[..8].copy_from_slice(&self.hash.to_le_bytes());
        out[8..12].copy_from_slice(&self.local_offset.to_le_bytes());
        out[12..16].copy_from_slice(&self.compressed.to_le_bytes());
        out[16..20].copy_from_slice(&self.uncompressed.to_le_bytes());
        out[20..22].copy_from_slice(&self.method.to_le_bytes());
        out[22..24].copy_from_slice(&self.flags.to_le_bytes());
        out[24..28].copy_from_slice(&self.first_spine.to_le_bytes());
    }
    pub fn decode(input: &[u8; MEMBER_BYTES as usize], header: &Header) -> Option<Self> {
        let member = Self {
            hash: get64(input, 0),
            local_offset: get32(input, 8),
            compressed: get32(input, 12),
            uncompressed: get32(input, 16),
            method: get16(input, 20),
            flags: get16(input, 22),
            first_spine: get32(input, 24),
        };
        (optional(member.first_spine, header.spine) && input[28..].iter().all(|&b| b == 0))
            .then_some(member)
    }
}

impl SpineItem {
    pub fn encode(&self, out: &mut [u8; SPINE_BYTES as usize]) {
        out.fill(0);
        out[..4].copy_from_slice(&self.member.to_le_bytes());
        out[4..12].copy_from_slice(&self.before.to_le_bytes());
    }
    /// Empty chapters may sit at the end, so `before` may equal the total.
    pub fn decode(input: &[u8; SPINE_BYTES as usize], header: &Header) -> Option<Self> {
        let item = Self { member: get32(input, 0), before: get64(input, 4) };
        (item.member < header.members && item.before <= header.spine_bytes
            && input[12..].iter().all(|&b| b == 0))
            .then_some(item)
    }
}

#[cfg(test)]
#[path = "book_index_tests.rs"]
mod tests;
