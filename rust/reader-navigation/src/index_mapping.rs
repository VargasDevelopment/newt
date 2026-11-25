//! Batch Contents projection over a schema-two index, using caller-owned scratch.
use crate::{BookView, NO_SPINE, declared_spine, usable_entry};
use newt_core::{book_index::{self, Reader, Unreadable}, page_cache::CacheStorage};

/// Three words avoid the padding of a (u64, u32) pair on 64-bit hosts.
#[derive(Clone, Copy, Default)]
pub struct HashRow([u32; 3]);
impl HashRow {
    fn new(hash: u64, row: usize) -> Self {
        Self([hash as u32, (hash >> 32) as u32, row as u32])
    }
    fn key(self) -> (u64, u32) {
        (u64::from(self.0[0]) | (u64::from(self.0[1]) << 32), self.0[2])
    }
}
const _: () = assert!(core::mem::size_of::<HashRow>() == 12);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MappingRoute { Batch, PerRow }

/// The caller reports PerRow in its trace. A short scratch loan retains the
/// oracle's rule rather than dropping Contents rows. Storage failure is fatal;
/// it must never publish a partially mapped menu.
pub fn declared_spines(
    book: &impl BookView,
    index: &Reader,
    storage: &mut (impl CacheStorage + ?Sized),
    mapping: &mut [u32],
    scratch: &mut [HashRow],
) -> Result<MappingRoute, Unreadable> {
    let count = book.toc_count();
    if mapping.len() < count { return Err(Unreadable); }
    let mapping = &mut mapping[..count];
    mapping.fill(NO_SPINE);
    if scratch.len() < count {
        for (row, mapped) in mapping.iter_mut().enumerate() {
            *mapped = declared_spine(book, row);
        }
        return Ok(MappingRoute::PerRow);
    }
    let mut used = 0;
    for row in 0..count {
        if usable_entry(book, row) {
            scratch[used] = HashRow::new(
                book_index::name_hash(book.toc_href(row).expect("usable declaration")), row);
            used += 1;
        }
    }
    let rows = &mut scratch[..used];
    rows.sort_unstable_by_key(|row| row.key());
    let mut cursor = 0;
    for member in 0..index.header().members {
        if cursor == rows.len() { break; }
        let member = index.member(storage, member).ok_or(Unreadable)?;
        while cursor < rows.len() && rows[cursor].key().0 < member.hash { cursor += 1; }
        while cursor < rows.len() && rows[cursor].key().0 == member.hash {
            mapping[rows[cursor].key().1 as usize] = member.contents_spine;
            cursor += 1;
        }
    }
    Ok(MappingRoute::Batch)
}
