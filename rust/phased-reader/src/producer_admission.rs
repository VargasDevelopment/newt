//! Frozen ReaderRevisionSession::pageCacheStored, cachepolicy:103-172.
use super::*;
use crate::producer_probe::ProducerProbe;
use newt_core::{cache_names::{namespace::{PageCacheNeed, PAGE_SEGMENT_BATCH}, segment::{self, ChapterSegment}}, producer_visit::MissingProbe};
impl<C: ProducerProbe + CachePresence> PhasedChapterCache<'_, C> {
    fn producer_name(&mut self, digest: &Digest, spine: u32,
        name: &mut [u8; 128], generation: &mut u32,
    ) -> bool {
        match session_adapter::identity(self.settings, digest, spine, self.hash) {
            Ok((base, value)) => {
                name[..69].copy_from_slice(&base);
                name[69] = 0;
                *generation = value;
                true
            }
            Err(_) => false,
        }
    }
    pub fn producer_stored<B: ParserBackend + ?Sized, L: BuildLoan>(
        &mut self, source: &mut dyn BookSource, digest: &Digest, spine: u32,
        mut need: Option<&mut PageCacheNeed>, cancelled: &dyn Fn() -> bool,
        parsed: &mut bool,
        r: &mut OperationPhased<'_, '_, '_, '_, B, L>,
    ) -> MissingProbe {
        let info = r.reader.info();
        let spine = if spine == u32::MAX { info.spine } else { spine };
        let mut name = [0; 128];
        let mut generation = 0;
        if !self.producer_name(digest, spine, &mut name, &mut generation) {
            return MissingProbe::Denied;
        }
        let whole = self.store.producer_probe(&name[..69], cancelled);
        if whole == MissingProbe::Complete && spine != info.spine {
            if let Some(need) = need.as_deref_mut() {
                *parsed = true;
                need.short_chapter = r.storage.short_cached(r.lender, self.store,
                    &name[..69], generation, source, r.reader.source_bytes);
                if !self.store.release_for_admission() { return MissingProbe::Denied; }
                if cancelled() { return MissingProbe::Stopped; }
            }
        }
        let count = if r.reader.has_page() && info.spine == spine {
            ChapterSegment::count(info.page_count)
        } else { 0 };
        if whole == MissingProbe::Ready && spine != info.spine {
            if !segment::map_name(&mut name, &mut |b, d| self.hash.hash(b, d)) {
                return MissingProbe::Denied;
            }
            if let Some(need) = need.as_deref_mut() {
                need.files = 2;
                need.keep.add(&name[..69]);
            }
            let mapped = self.store.producer_probe(&name[..69], cancelled);
            if mapped != MissingProbe::Complete { return mapped; }
            *parsed = true;
            let Some(first) = self.locate(source, &name[..69], generation, 0, r) else {
                return MissingProbe::Ready;
            };
            if !self.producer_name(digest, spine, &mut name, &mut generation)
                || !segment::segment_name(first.segment, &mut name, &mut |b, d| self.hash.hash(b, d))
            { return MissingProbe::Denied; }
            if let Some(need) = need.as_deref_mut() { need.keep.add(&name[..69]); }
            return self.store.producer_probe(&name[..69], cancelled);
        }
        if whole != MissingProbe::Ready || count == 0 { return whole; }
        let mut work = MissingProbe::Complete;
        let mut skipped = false;
        if let Some(need) = need.as_deref_mut() { need.files = 0; }
        for k in 0..=count {
            if need.is_none() && work != MissingProbe::Complete { break; }
            let map = k == count;
            let current = ChapterSegment { index: k, pages: info.page_count, chars: info.total_chars };
            if !self.producer_name(digest, spine, &mut name, &mut generation)
                || !(if map { segment::map_name(&mut name, &mut |b, d| self.hash.hash(b, d)) }
                    else { segment::segment_name(current, &mut name, &mut |b, d| self.hash.hash(b, d)) })
            { return MissingProbe::Denied; }
            if let Some(need) = need.as_deref_mut() {
                need.keep.add(&name[..69]);
                if if map { skipped } else { need.files >= PAGE_SEGMENT_BATCH } {
                    skipped = true;
                    continue;
                }
            }
            let found = self.store.producer_probe(&name[..69], cancelled);
            if found != MissingProbe::Complete && found != MissingProbe::Ready { return found; }
            if found == MissingProbe::Ready {
                work = found;
                if let Some(need) = need.as_deref_mut() { need.files += 1; }
            }
        }
        work
    }
}

impl<C: ReadScope + CachePresence> PhasedChapterCache<'_, C> {
    pub(super) fn locate<B: ParserBackend + ?Sized, L: BuildLoan>(
        &mut self,
        source: &mut dyn BookSource,
        name: &[u8],
        generation: u32,
        target: u32,
        r: &mut OperationPhased<'_, '_, '_, '_, B, L>,
    ) -> Option<newt_core::cache_names::segment::SegmentAnchor> {
        use newt_core::{cache_names::segment, page_cache::CacheStorage};
        let bytes = r.reader.source_bytes;
        let opening = !r.reader.has_page();
        let hash = &mut *self.hash;
        let scope = &mut *self.store;
        let result = r.lender
            .with_parse(|parse| {
                // The map sits in the index; the verified query follows it.
                let (index, verified) = parse.split_at_mut_checked(INDEX_BYTES)?;
                let map = index.get_mut(..segment::MAX_BYTES)?;
                let mut located = segment::SegmentAnchor::default();
                let read = |inner: &mut dyn CacheStorage| {
                    let mut store = crate::phase_storage::ScopedRead(inner);
                    let size = store.file_size(name);
                    size > 0
                        && size <= segment::MAX_BYTES as i64
                        && store.read_at(name, 0, &mut map[..size as usize]) == size as i32
                        && segment::find(
                            &map[..size as usize],
                            generation,
                            target,
                            &mut located,
                            &mut |b, d| hash.hash(b, d),
                        )
                };
                let mut source_live = || validate(source, bytes) == Status::Ok;
                let (found, finished) = scope.query(name, opening, verified, &mut source_live, read).ok()?;
                if found && finished {
                    Some(located)
                } else {
                    None
                }
            });
        let located = match result {
            Ok(located) => located.filter(|_| validate(source, bytes) == Status::Ok),
            Err(_) => {
                admission::cache_miss(r.reader.on_yield, CacheMissReason::Lease);
                return None;
            }
        };
        if located.is_none() { admission::cache_miss(r.reader.on_yield, CacheMissReason::Map); }
        located
    }
}
