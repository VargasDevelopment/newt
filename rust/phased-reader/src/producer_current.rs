//! Frozen chapter_cache_producer.h:388-459; existing scoped attempt owners.
#![forbid(unsafe_code)]
use super::*;
use crate::phase_storage::producer::Producer;
impl<C: ?Sized, S: CacheStorage, F: BookSource + newt_core::producer_policy::ProducerReceipt + ?Sized>
    Producer<'_, '_, '_, C, S, F>
{
    fn settle_current_selected<B: ProducerBackend + ?Sized>(&mut self,
        reader: &mut PhasedReader<'_, '_>, backend: &mut B, breaker: &mut dyn LineBreak,
        select: impl FnOnce(&mut Self, &mut PhasedReader<'_, '_>, u32, &mut B) -> Status,
    ) -> Status {
        let spine = reader.info.spine;
        self.state.staged = true;
        let status = select(self, reader, spine, backend);
        if status != Status::Ok { return status; }
        self.with_index(|owner, mut memory| {
            let mut last = super::NextPass { status: Status::Ok, pages: 0, chars: 0, restyle: false, layout_done: false };
            let mut completed = false;
            while last.status == Status::Ok && owner.writer.cancellation().checkpoint() {
                last = owner.next_pass(reader, spine, backend, breaker, true, &mut memory)?;
                if last.layout_done && !last.restyle {
                    completed = true;
                    break;
                }
                if last.status != Status::Ok || !last.restyle { break; }
            }
            if completed
                && (last.pages != reader.info.page_count || last.chars != reader.info.total_chars)
            {
                owner.writer.cancellation().fail_terminal();
                return Ok(Status::Stale);
            }
            if last.status != Status::Ok {
                return Ok(last.status);
            }
            if !completed {
                return Ok(Status::Stale);
            }
            let settled = owner.with_unbuffered(|phase, _| Ok(phase.ready(spine)
                && phase.edit_slot(|slot| slot.state == crate::styles::State::Settled)?))?;
            if !settled {
                owner.writer.cancellation().fail_terminal();
                return Ok(Status::Stale);
            }
            Ok(if !owner.writer.cancellation().checkpoint()
            { Status::Stale } else { Status::Ok })
        }).unwrap_or_else(|status| status)
    }
    /// Called only after real source/Pages admission. Nothing replaces the
    /// published selection, retained plan or reader totals.
    pub fn produce_current<B: ProducerBackend + ?Sized>(&mut self,
        reader: &mut PhasedReader<'_, '_>, backend: &mut B, breaker: &mut dyn LineBreak,
        route: &mut Route<'_>,
    ) -> CurrentOutcome {
        self.produce_current_selected(reader, backend, breaker, route,
            |owner, reader, spine, backend| owner.select_next(reader, spine, backend))
    }
    fn produce_current_selected<B: ProducerBackend + ?Sized>(&mut self,
        reader: &mut PhasedReader<'_, '_>, backend: &mut B, breaker: &mut dyn LineBreak,
        route: &mut Route<'_>,
        select: impl FnOnce(&mut Self, &mut PhasedReader<'_, '_>, u32, &mut B) -> Status,
    ) -> CurrentOutcome {
        if !reader.opened || !reader.has_page || reader.info.page_count == 0
            || reader.info.page_count > newt_core::cache_names::segment::MAX_PAGES
            || reader.plan.is_none() || !crate::reader::session_adapter::reader_settings_match(reader, route.settings)
        {
            self.writer.cancellation().fail_terminal();
            return CurrentOutcome::NoProgress(Status::Unsupported);
        }
        if !self.writer.cancellation().checkpoint() {
            return CurrentOutcome::NoProgress(Status::IoError);
        }
        let settled = self.with_unbuffered(|phase, _| Ok(phase.ready(reader.info.spine)
            && phase.edit_slot(|slot| slot.state == crate::styles::State::Settled)?))
            .unwrap_or(false);
        let settle = if settled { Status::Ok } else { self.settle_current_selected(reader, backend, breaker, select) };
        let outcome = if settle != Status::Ok {
            CurrentOutcome::NoProgress(settle)
        } else {
            self.state.language = reader.book_language();
            self.state.sealed = 0;
            self.state.set_start(reader.info.page / newt_core::cache_names::segment::PAGES);
            self.with_buffered(|phase, mut attempt| {
                if attempt.state.count != 0 {
                    attempt.writer.clear_starts();
                    attempt.writer.segmented = true;
                    attempt.writer.capture_points = true;
                    attempt.writer.restore_points = true;
                    let named = attempt.state.identify(route, reader.info.spine)
                        && newt_core::cache_names::segment::map_name(&mut attempt.state.name,
                            &mut |b, d| route.hash.hash(b, d));
                    let mapped = named && crate::producer_store::with_source_store(attempt.store,
                        attempt.source, reader.source_bytes, false,
                        attempt.guard_attempted.as_deref_mut(), |store| store.exists(&attempt.state.name[..69]));
                    attempt.writer.mapping = !mapped;
                    Ok(attempt.produce_segments_and_map_outcome(reader, phase, backend, breaker, route))
                } else {
                    let status = attempt.with_writer(|write| {
                        if !write.source.validate_source(reader.source_bytes)
                            || !write.state.identify(route, reader.info.spine)
                        { return Status::IoError; }
                        if !crate::producer_store::with_source_store(write.store, write.source,
                            reader.source_bytes, false, write.guard_attempted.as_deref_mut(),
                            |store| {
                                let ok = write.writer.cache.begin(store, &write.state.name[..69], write.state.generation, &write.memory);
                                #[cfg(feature = "page-turn-trace")]
                                if ok { store.trace_segment_begin(0); }
                                ok
                            })
                        { return Status::OutOfMemory; }
                        write.finish_current(reader, phase, backend, breaker, route)
                    })?;
                    let cancellation = attempt.writer.cancellation();
                    Ok(if status == Status::Ok
                        || (attempt.state.sealed > 0 && cancellation.interrupted() && !cancellation.terminal())
                    {
                        CurrentOutcome::Done
                    } else {
                        CurrentOutcome::NoProgress(status)
                    })
                }
            }).unwrap_or_else(CurrentOutcome::NoProgress)
        };
        if let CurrentOutcome::NoProgress(_) = outcome {
            self.writer.cancellation().result(false);
        }
        if !matches!(outcome, CurrentOutcome::Done) || self.writer.segmented {
            let _ = self.with_unbuffered(|_, mut attempt| {
                crate::producer_store::with_source_store(attempt.store, attempt.source,
                    reader.source_bytes, false, attempt.guard_attempted.as_deref_mut(),
                    |store| store.remove(&attempt.state.name[..69]));
                Ok(())
            });
        }
        if self.state.staged { self.drop_styles(); }
        outcome
    }
}

impl<C: ?Sized, S: crate::phase_storage::ReadScope, F: BookSource + newt_core::producer_policy::ProducerReceipt + ?Sized>
    Producer<'_, '_, '_, C, S, F>
{
    #[inline(always)]
    pub fn produce_current_indexed<B: ProducerBackend + ?Sized>(&mut self, reader: &mut PhasedReader<'_, '_>,
        backend: &mut B, breaker: &mut dyn LineBreak, route: &mut Route<'_>,
    ) -> CurrentOutcome {
        let digest = route.digest;
        self.produce_current_selected(reader, backend, breaker, route,
            |owner, reader, spine, backend| owner.select_next_indexed(reader, digest, spine, backend))
    }
}
