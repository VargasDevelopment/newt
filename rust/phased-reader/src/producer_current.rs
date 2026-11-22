//! Frozen chapter_cache_producer.h:388-459; existing scoped attempt owners.
#![forbid(unsafe_code)]
use super::*;
use crate::phase_storage::producer::Producer;
impl<C: ?Sized, S: CacheStorage, F: BookSource + newt_core::producer_policy::ProducerReceipt + ?Sized>
    Producer<'_, '_, '_, C, S, F>
{
    fn settle_current<B: ProducerBackend + ?Sized>(&mut self,
        reader: &mut PhasedReader<'_, '_>, backend: &mut B, breaker: &mut dyn LineBreak,
    ) -> Status {
        let spine = reader.info.spine;
        self.state.staged = true;
        let status = self.select_next(reader, spine, backend);
        if status != Status::Ok { return status; }
        self.with_index(|owner, mut memory| {
            let mut last = super::NextPass { status: Status::Ok, pages: 0, chars: 0, restyle: false };
            while last.status == Status::Ok && owner.writer.cancellation().checkpoint() {
                last = owner.next_pass(reader, spine, backend, breaker, true, &mut memory)?;
                if last.status != Status::Ok || !last.restyle { break; }
            }
            let settled = owner.with_unbuffered(|phase, _| Ok(phase.ready(spine)
                && phase.edit_slot(|slot| slot.state == crate::styles::State::Settled)?))?;
            Ok(if last.status == Status::Ok && (!owner.writer.cancellation().checkpoint()
                || last.pages != reader.info.page_count || last.chars != reader.info.total_chars || !settled)
            { Status::Stale } else { last.status })
        }).unwrap_or_else(|status| status)
    }
    /// Called only after real source/Pages admission. Nothing replaces the
    /// published selection, retained plan or reader totals.
    pub fn produce_current<B: ProducerBackend + ?Sized>(&mut self,
        reader: &mut PhasedReader<'_, '_>, backend: &mut B, breaker: &mut dyn LineBreak,
        route: &mut Route<'_>,
    ) -> Status {
        if !reader.opened || !reader.has_page || reader.info.page_count == 0
            || reader.plan.is_none() || !crate::reader::session_adapter::reader_settings_match(reader, route.settings)
        { return Status::Unsupported; }
        if !self.writer.cancellation().checkpoint() { return Status::IoError; }
        let settled = self.with_unbuffered(|phase, _| Ok(phase.ready(reader.info.spine)
            && phase.edit_slot(|slot| slot.state == crate::styles::State::Settled)?))
            .unwrap_or(false);
        let mut status = if settled { Status::Ok } else { self.settle_current(reader, backend, breaker) };
        if status == Status::Ok {
            status = self.with_buffered(|phase, mut attempt| {
                if attempt.state.count != 0 {
                    attempt.writer.segmented = true;
                    let named = attempt.state.identify(route, reader.info.spine)
                        && newt_core::cache_names::segment::map_name(&mut attempt.state.name,
                            &mut |b, d| route.hash.hash(b, d));
                    let mapped = named && crate::producer_store::with_source_store(attempt.store,
                        attempt.source, reader.source_bytes, false,
                        attempt.guard_attempted.as_deref_mut(), |store| store.exists(&attempt.state.name[..69]));
                    attempt.writer.mapping = !mapped;
                    Ok(attempt.produce_segments_and_map(reader, phase, backend, breaker, route))
                } else {
                    attempt.with_writer(|write| {
                        if !write.source.validate_source(reader.source_bytes)
                            || !write.state.identify(route, reader.info.spine)
                        { return Status::IoError; }
                        if !crate::producer_store::with_source_store(write.store, write.source,
                            reader.source_bytes, false, write.guard_attempted.as_deref_mut(),
                            |store| write.writer.cache.begin(store, &write.state.name[..69], write.state.generation, &write.memory))
                        { return Status::OutOfMemory; }
                        write.finish_current(reader, phase, backend, breaker, route)
                    })
                }
            }).unwrap_or_else(|status| status);
        }
        self.writer.cancellation().result(status == Status::Ok);
        if status != Status::Ok || self.writer.segmented {
            let _ = self.with_unbuffered(|_, mut attempt| {
                crate::producer_store::with_source_store(attempt.store, attempt.source,
                    reader.source_bytes, false, attempt.guard_attempted.as_deref_mut(),
                    |store| store.remove(&attempt.state.name[..69]));
                Ok(())
            });
        }
        if self.state.staged { self.drop_styles(); }
        status
    }
}
