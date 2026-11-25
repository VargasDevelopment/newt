//! Stream codecs; wire sizes do not inherit the host's alignment.
use crate::book_index::{Member, get16, get32, get64};

pub(super) trait Record: Copy {
    const BYTES: usize;
    const MEMORY: usize = Self::BYTES;
    const KIND: u32;
    fn decode(bytes: &[u8]) -> Self;
    fn encode(self, bytes: &mut [u8]);
    fn key(self) -> (u64, u32);
    fn memory_decode(bytes: &[u8]) -> Self { Self::decode(bytes) }
    fn memory_encode(self, bytes: &mut [u8]) { self.encode(bytes); }
}
impl Record for Member {
    const BYTES: usize = 24;
    const KIND: u32 = 0;
    fn decode(b: &[u8]) -> Self {
        Self { hash: get64(b, 0), local_offset: get32(b, 8), compressed: get32(b, 12),
            uncompressed: get32(b, 16), method: get16(b, 20),
            flags: get16(b, 22), first_spine: crate::book_index::NONE }
    }
    fn encode(self, b: &mut [u8]) {
        b[..8].copy_from_slice(&self.hash.to_le_bytes()); b[8..12].copy_from_slice(&self.local_offset.to_le_bytes());
        b[12..16].copy_from_slice(&self.compressed.to_le_bytes()); b[16..20].copy_from_slice(&self.uncompressed.to_le_bytes());
        b[20..22].copy_from_slice(&self.method.to_le_bytes()); b[22..24].copy_from_slice(&self.flags.to_le_bytes());
    }
    fn key(self) -> (u64, u32) { (self.hash, 0) }
}
#[derive(Clone, Copy)]
pub(super) struct Item { pub id: u64, pub href: u64, pub sequence: u32, pub flags: u32 }
impl Record for Item {
    const BYTES: usize = 24;
    const KIND: u32 = 1;
    fn decode(b: &[u8]) -> Self { Self { id: get64(b, 0), href: get64(b, 8), sequence: get32(b, 16), flags: get32(b, 20) } }
    fn encode(self, b: &mut [u8]) {
        b[..8].copy_from_slice(&self.id.to_le_bytes()); b[8..16].copy_from_slice(&self.href.to_le_bytes());
        b[16..20].copy_from_slice(&self.sequence.to_le_bytes()); b[20..24].copy_from_slice(&self.flags.to_le_bytes());
    }
    fn key(self) -> (u64, u32) { (self.id, self.sequence) }
}
#[derive(Clone, Copy)]
pub(super) struct Reference { pub id: u64, pub position: u32 }
impl Record for Reference {
    const BYTES: usize = 12;
    const KIND: u32 = 2;
    fn decode(b: &[u8]) -> Self { Self { id: get64(b, 0), position: get32(b, 8) } }
    fn encode(self, b: &mut [u8]) { b[..8].copy_from_slice(&self.id.to_le_bytes()); b[8..12].copy_from_slice(&self.position.to_le_bytes()); }
    fn key(self) -> (u64, u32) { (self.id, self.position) }
}
#[derive(Clone, Copy)]
pub(super) struct Joined { pub href: u64, pub position: u32, pub text: bool }
impl Record for Joined {
    const BYTES: usize = 16;
    // The raw position is bounded to 15 bits, leaving a bit for text. Keeping
    // resident J1 the size of R lets an in-place join handle arbitrary repeats.
    const MEMORY: usize = 12;
    const KIND: u32 = 3;
    fn decode(b: &[u8]) -> Self { Self { href: get64(b, 0), position: get32(b, 8), text: get32(b, 12) != 0 } }
    fn encode(self, b: &mut [u8]) {
        b[..8].copy_from_slice(&self.href.to_le_bytes()); b[8..12].copy_from_slice(&self.position.to_le_bytes());
        b[12..16].copy_from_slice(&u32::from(self.text).to_le_bytes());
    }
    fn memory_decode(b: &[u8]) -> Self {
        let packed = get32(b, 8);
        Self { href: get64(b, 0), position: packed & 0x7fff_ffff, text: packed >> 31 != 0 }
    }
    fn memory_encode(self, b: &mut [u8]) {
        b[..8].copy_from_slice(&self.href.to_le_bytes());
        b[8..12].copy_from_slice(&(self.position | u32::from(self.text) << 31).to_le_bytes());
    }
    fn key(self) -> (u64, u32) { (self.href, self.position) }
}
#[derive(Clone, Copy)]
pub(super) struct First { pub rank: u32, pub position: u32 }
impl Record for First {
    const BYTES: usize = 8;
    const KIND: u32 = 4;
    fn decode(b: &[u8]) -> Self { Self { rank: get32(b, 0), position: get32(b, 4) } }
    fn encode(self, b: &mut [u8]) { b[..4].copy_from_slice(&self.rank.to_le_bytes()); b[4..8].copy_from_slice(&self.position.to_le_bytes()); }
    fn key(self) -> (u64, u32) { (u64::from(self.rank), 0) }
}
#[derive(Clone, Copy)]
pub(super) struct Spine { pub position: u32, pub member: u32, pub weight: u32 }
impl Record for Spine {
    const BYTES: usize = 12;
    const KIND: u32 = 5;
    fn decode(b: &[u8]) -> Self { Self { position: get32(b, 0), member: get32(b, 4), weight: get32(b, 8) } }
    fn encode(self, b: &mut [u8]) {
        b[..4].copy_from_slice(&self.position.to_le_bytes()); b[4..8].copy_from_slice(&self.member.to_le_bytes());
        b[8..12].copy_from_slice(&self.weight.to_le_bytes());
    }
    fn key(self) -> (u64, u32) { (u64::from(self.position), 0) }
}
pub(super) fn wire_bytes(kind: u32) -> Option<usize> { [24, 24, 12, 16, 8, 12].get(kind as usize).copied() }
