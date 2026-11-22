//! Frozen chapter_page_cache_segments.h:70-93. Index dies with the parse loan.
use super::*;
impl Storage<'_> {
    pub(crate) fn short_cached(
        &mut self, lender: &mut impl BuildLoan, store: &mut impl ReadScope,
        name: &[u8], generation: u32,
        source: &mut dyn crate::zip_window::source::BookSource, bytes: u64,
    ) -> bool {
        let result = lender.with_parse(|native| {
            let padding = native.as_ptr().align_offset(16);
            let end = padding.checked_add(BANK_BYTES).ok_or(Status::OutOfMemory)?;
            let parse = native.get_mut(padding..end).ok_or(Status::OutOfMemory)?;
            let (index, verified) = parse.split_at_mut(INDEX_BYTES);
            let mut index = Parts(index);
            let mut names = Arena::new(index.bytes(
                name.len().checked_add(1).ok_or(Status::OutOfMemory)?, 1)?);
            let mut index = IndexParts(index);
            let mut source_live = || crate::reader::validate(source, bytes) == Status::Ok;
            let (short, fresh) = store.query(name, false, verified, &mut source_live, |inner| {
                let mut guarded = ScopedRead(inner);
                let mut cached = newt_core::page_cache::PageCacheReader::default();
                match cached.open_allocated(&mut guarded, name, generation, &mut names, &mut index) {
                    Ok(memory) => crate::reader::cache_show::valid_index(&cached, &memory)
                        && cached.page_count() <= 3,
                    Err(_) => false,
                }
            })?;
            Ok::<bool, Status>(short && fresh)
        });
        matches!(result, Ok(Ok(true)))
            && crate::reader::validate(source, bytes) == Status::Ok
    }
}
