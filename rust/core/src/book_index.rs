//! Book index codec (NEWT-166). One block-envelope payload per verified book
//! revision: a header, every ZIP member sorted by name hash, then the spine in
//! reading order. Records have fixed sizes so a reader fetches one by position
//! instead of parsing the ZIP directory and OPF again.
#![forbid(unsafe_code)]

use crate::cache_names::Digest;
use crate::page_cache::CacheStorage;
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

/// Why a write stopped. Only `Storage` is a card fault: `Collision` refuses
/// the book, and `Invalid` is a builder bug the writer will not publish.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteError {
    Storage,
    /// Two members share a name hash: a duplicate ZIP name or a 64-bit collision.
    Collision,
    /// A record out of order, outside its bounds, or beyond the header counts.
    Invalid,
}

/// Streams one index in payload order (header, members by ascending hash,
/// spine) and publishes it at `finish`. Every record passes the reader's own
/// decode first, so a published index never fails it. A failed writer is
/// consumed; the store releases its temporary on the next write or close,
/// and the census sweeps any leftover.
pub struct Writer<'b> {
    header: Header,
    buffer: &'b mut [u8],
    used: usize,
    members: u32,
    spine: u32,
    last_hash: u64,
    last_before: u64,
}

impl<'b> Writer<'b> {
    /// `buffer` batches records into fewer card writes; it must hold a header.
    pub fn begin<S: CacheStorage + ?Sized>(
        s: &mut S,
        header: Header,
        buffer: &'b mut [u8],
    ) -> Result<Self, WriteError> {
        let mut bytes = [0; HEADER_BYTES as usize];
        header.encode(&mut bytes);
        let payload = header.payload_bytes().ok_or(WriteError::Invalid)?;
        if buffer.len() < bytes.len() || Header::decode(&bytes, &header.identity, payload) != Some(header) {
            return Err(WriteError::Invalid);
        }
        let mut file = [0; 69];
        name(&header.identity.digest, &mut file);
        if !s.begin_write(&file) {
            return Err(WriteError::Storage);
        }
        buffer[..bytes.len()].copy_from_slice(&bytes);
        Ok(Self { header, buffer, used: bytes.len(), members: 0, spine: 0, last_hash: 0, last_before: 0 })
    }
    pub fn member<S: CacheStorage + ?Sized>(mut self, s: &mut S, member: Member) -> Result<Self, WriteError> {
        let mut bytes = [0; MEMBER_BYTES as usize];
        member.encode(&mut bytes);
        if self.members == self.header.members || Member::decode(&bytes, &self.header) != Some(member) {
            return Err(WriteError::Invalid);
        }
        if self.members > 0 && member.hash <= self.last_hash {
            return Err(if member.hash == self.last_hash { WriteError::Collision } else { WriteError::Invalid });
        }
        self.last_hash = member.hash;
        self.members += 1;
        self.put(s, &bytes)
    }
    /// Spine items follow every member; `before` never decreases.
    pub fn spine<S: CacheStorage + ?Sized>(mut self, s: &mut S, item: SpineItem) -> Result<Self, WriteError> {
        let mut bytes = [0; SPINE_BYTES as usize];
        item.encode(&mut bytes);
        if self.members != self.header.members
            || self.spine == self.header.spine
            || item.before < self.last_before
            || SpineItem::decode(&bytes, &self.header) != Some(item)
        {
            return Err(WriteError::Invalid);
        }
        self.last_before = item.before;
        self.spine += 1;
        self.put(s, &bytes)
    }
    /// `receipt_live` is the store's final destination check before publication.
    pub fn finish<S: CacheStorage + ?Sized>(
        mut self,
        s: &mut S,
        receipt_live: &mut dyn FnMut() -> bool,
    ) -> Result<(), WriteError> {
        if self.members != self.header.members || self.spine != self.header.spine {
            return Err(WriteError::Invalid);
        }
        self.flush(s)?;
        if s.end_write_checked(receipt_live) { Ok(()) } else { Err(WriteError::Storage) }
    }
    fn put<S: CacheStorage + ?Sized>(mut self, s: &mut S, bytes: &[u8]) -> Result<Self, WriteError> {
        if self.buffer.len() - self.used < bytes.len() {
            self.flush(s)?;
        }
        self.buffer[self.used..self.used + bytes.len()].copy_from_slice(bytes);
        self.used += bytes.len();
        Ok(self)
    }
    fn flush<S: CacheStorage + ?Sized>(&mut self, s: &mut S) -> Result<(), WriteError> {
        let written = self.used == 0 || s.write(&self.buffer[..self.used]);
        self.used = 0;
        if written { Ok(()) } else { Err(WriteError::Storage) }
    }
}

/// The store could not return a record the header promised; on a verified
/// envelope that means a card fault or damage, so the caller rebuilds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unreadable;

/// A checked header; records are read on demand through the same store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reader {
    header: Header,
    name: [u8; 69],
}

impl Reader {
    /// None for a missing, damaged, stale or foreign index: rebuild it.
    pub fn open<S: CacheStorage + ?Sized>(s: &mut S, identity: &Identity) -> Option<Self> {
        let mut file = [0; 69];
        name(&identity.digest, &mut file);
        let payload = u32::try_from(s.file_size(&file)).ok()?;
        let mut bytes = [0; HEADER_BYTES as usize];
        read_exact(s, &file, 0, &mut bytes)?;
        let header = Header::decode(&bytes, identity, payload)?;
        Some(Self { header, name: file })
    }
    pub fn header(&self) -> &Header {
        &self.header
    }
    /// None when unreadable or past the header's member count.
    pub fn member<S: CacheStorage + ?Sized>(&self, s: &mut S, index: u32) -> Option<Member> {
        let mut bytes = [0; MEMBER_BYTES as usize];
        read_exact(s, &self.name, self.header.member_offset(index)?, &mut bytes)?;
        Member::decode(&bytes, &self.header)
    }
    /// None when unreadable or past the header's spine count.
    pub fn spine<S: CacheStorage + ?Sized>(&self, s: &mut S, index: u32) -> Option<SpineItem> {
        let mut bytes = [0; SPINE_BYTES as usize];
        read_exact(s, &self.name, self.header.spine_offset(index)?, &mut bytes)?;
        SpineItem::decode(&bytes, &self.header)
    }
    /// Binary search by name hash. A hit only matches the hash; the caller
    /// still compares the member's local name before trusting it.
    pub fn find<S: CacheStorage + ?Sized>(&self, s: &mut S, hash: u64) -> Result<Option<(u32, Member)>, Unreadable> {
        let (mut low, mut high) = (0, self.header.members);
        while low < high {
            let middle = low + (high - low) / 2;
            let member = self.member(s, middle).ok_or(Unreadable)?;
            match member.hash.cmp(&hash) {
                core::cmp::Ordering::Less => low = middle + 1,
                core::cmp::Ordering::Greater => high = middle,
                core::cmp::Ordering::Equal => return Ok(Some((middle, member))),
            }
        }
        Ok(None)
    }
}

fn read_exact<S: CacheStorage + ?Sized>(s: &mut S, file: &[u8], offset: u32, out: &mut [u8]) -> Option<()> {
    (s.read_at(file, offset, out) == out.len() as i32).then_some(())
}

#[cfg(test)]
#[path = "book_index_tests.rs"]
mod tests;
