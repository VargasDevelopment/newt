//! Frozen chapter_cache_producer.h:289-334 inside the admitted SDK lease. MIT.
#![forbid(unsafe_code)]
use super::*;
use crate::{PageInfo, producer::Route};
use newt_core::cache_names::segment::{ChapterSegment, segment_name};

impl<C: ?Sized, S: CacheStorage, F: BookSource + newt_core::producer_policy::ProducerReceipt + ?Sized> Producer<'_, '_, '_, C, S, F> {
    /// Scheduler/Pages admission precedes this entry. Stage only the inactive
    /// selection; never replace the visible page, totals, plan or navigation.
    pub fn produce_next<B: ProducerBackend + ?Sized>(&mut self,
        reader: &mut PhasedReader<'_, '_>, spine: u32, backend: &mut B,
        breaker: &mut dyn LineBreak, route: &mut Route<'_>, prepared_pages: Option<&mut u32>,
    ) -> Status {
        let mut status = self.select_next(reader, spine, backend);
        if status == Status::Ok {
            self.state.language = reader.staged_selection().language;
            if !self.state.identify(route, spine) { status = Status::IoError; }
        }
        let mut pages = 0;
        let mut chars = 0;
        if status == Status::Ok {
            status = self.with_index(|owner, mut memory| {
                let mut last = NextPass { status: Status::Ok, pages: 0, chars: 0, restyle: false };
                while last.status == Status::Ok && owner.writer.cancellation().checkpoint() {
                    last = owner.next_pass(reader, spine, backend, breaker, true, &mut memory)?;
                    if last.status != Status::Ok || !last.restyle { break; }
                }
                pages = last.pages; chars = last.chars;
                let second = owner.writer.starts[1];
                let segmented = pages > PAGES;
                let mut status = last.status;
                if status == Status::Ok && pages != 0 && owner.writer.cancellation().checkpoint() {
                    owner.writer.segmented = segmented;
                    owner.writer.open = segmented;
                    status = owner.with_unbuffered(|_, mut a| {
                        if segmented && !segment_name(ChapterSegment { index: 0, pages, chars },
                            &mut a.state.name, &mut |b, d| route.hash.hash(b, d)) {
                            return Ok(Status::Unsupported);
                        }
                        Ok(if crate::producer_store::with_source_store(a.store, a.source, reader.source_bytes, true, a.guard_attempted.as_deref_mut(), |store| a.writer.cache.begin(store, &a.state.name[..69], a.state.generation, &memory)) {
                            Status::Ok
                        } else { Status::Unsupported })
                    })?;
                    if status == Status::Ok {
                        last = owner.next_pass(reader, spine, backend, breaker, false, &mut memory)?;
                        status = last.status;
                        if status == Status::Ok && (last.restyle || if segmented {
                            owner.writer.cache.page_count() != PAGES || owner.writer.starts[1] != second
                        } else { last.pages != pages || last.chars != chars }) { status = Status::Stale; }
                    }
                }
                if status == Status::Ok && owner.writer.cancellation().step(|| newt_core::zip_window::source::validate(owner.source, reader.source_bytes) == Status::Ok) {
                    owner.with_unbuffered(|_, mut a| {
                        a.writer.cache.set_total_chars(chars);
                        Ok(if pages == 0 || crate::producer_store::with_source_store(a.store, a.source, reader.source_bytes, true, a.guard_attempted.as_deref_mut(), |store| a.writer.cache.finish(store, &a.state.name[..69], &memory)) {
                            Status::Ok
                        } else { Status::IoError })
                    })
                } else { Ok(if status == Status::Ok { Status::IoError } else { status }) }
            }).unwrap_or_else(|status| status);
        }
        // The typed index view is dead. Map allocation reuses that exact region.
        if status == Status::Ok && pages > PAGES {
            let mut certified = Route { settings: route.settings, digest: route.digest,
                info: PageInfo { spine, page_count: pages, total_chars: chars, ..route.info },
                hash: &mut *route.hash };
            status = self.with_unbuffered(|_, mut a| Ok(a.publish_map_checked(&mut certified, spine, true)))
                .unwrap_or_else(|status| status);
        }
        self.writer.cancellation().result(status == Status::Ok);
        if status == Status::Ok {
            if let Some(out) = prepared_pages { *out = pages; }
        } else {
            let _ = self.with_unbuffered(|_, mut a| {
                crate::producer_store::with_source_store(a.store, a.source, reader.source_bytes, true, a.guard_attempted.as_deref_mut(), |store| store.remove(&a.state.name[..69])); Ok(()) });
        }
        self.drop_styles();
        status
    }
}
