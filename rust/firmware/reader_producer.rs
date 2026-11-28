//! Frozen library_reader_cache_producer.h and reader_revision_cache_policy.h.
//! The real source, native Pages and one book ledger remain the sole owners.
use super::*;
use newt_core::zip_window::source::BookSource;
use newt_core::{cache_names::namespace::{PageCacheNeed, PAGE_CANDIDATE, PAGE_REQUIRED_UNITS},
    producer_policy::{Cancellation, Hooks, ProducerReceipt, Stop},
    producer_visit::{Admission, CurrentOutcome, MissingProbe, Position, ProducerVisit, quiet_after_attempt}};
use newt_native_storage::{ProducerPolicy, chapter_cache::ChapterCacheAccess, page_entries::PageEntries};
use newt_phased_reader::OperationPhased;

/// Only the State owners are lent while native Pages is suspended. The panel,
/// parser and source remain unavailable until their enclosing parse loan ends.
pub struct ProducerPause<'loan, 'op, 'io, 'route, N, P: CardPort> {
    pub io: &'io UiIo,
    pub card: &'loan mut P,
    pub nvs: &'loan mut N,
    pub facts: &'loan mut StateFacts,
    pub checkpoint: &'loan mut ReaderCheckpoint,
    pub pages: &'loan mut ChapterCacheAccess<'op, 'route, P::Port<'io>>,
    pub owner: &'loan mut ExternalOwner,
    pub ui: &'loan mut ReaderUiState,
    pub statistics: &'loan mut ReadingStatsRuntime,
    pub(crate) recovery: &'loan mut Option<PendingRecovery>,
    pub accepted_open: &'loan mut bool,
    pub(crate) metadata: Option<&'loan dyn super::metadata::MetadataReader>,
    pub permille: u16,
    pub settings: ViewSettings,
    pub digest: &'loan Digest,
}
pub type ProducerPauseFn<'callback, 'io, N, P> = dyn for<'loan, 'op, 'route>
    FnMut(ProducerPause<'loan, 'op, 'io, 'route, N, P>, &dyn Fn() -> bool) -> (Stop, bool)
    + 'callback;

/// Entry refusal keeps Power/Card denial distinct from a transient blocker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProducerEntry { Ready, Blocked, Denied }

/// Borrowed caller policy; foreground and producer yield share one Scheduler.
pub struct ProducerCallbacks<'a> {
    pub entry: &'a dyn Fn(u32) -> ProducerEntry,
    pub stop: &'a dyn Fn(u32) -> Stop,
    /// Some returns without I/O. None requests the suspended-State callback.
    pub pause_request: &'a dyn Fn(u32, u8) -> Option<Stop>,
    pub yield_now: newt_core::admission::Yield<'a>,
    pub wait: Option<fn()>,
    #[cfg(all(feature = "rust-app", feature = "dev-usb"))]
    pub trace: Option<&'a dyn Fn(ProducerEvent)>,
    #[cfg(feature = "page-turn-trace")]
    pub trace_micros: &'a dyn Fn() -> u32,
}
/// Only measured scalars cross the short DEV observation loan.
#[cfg(all(feature = "rust-app", feature = "dev-usb"))]
#[derive(Clone, Copy)]
pub enum ProducerEvent {
    Admission { result: Admission, units: u8 },
    Start { spine: u32, ahead: u32 },
    Finish { status: Status, interrupted: bool },
    #[cfg(feature = "page-turn-trace")]
    Grant { warm: bool, census_us: u32, files: u32, outcome: MissingProbe, units: u8, entries: u8 },
    #[cfg(feature = "page-turn-trace")]
    Segment(newt_core::page_turn_trace::SegmentTrace),
}
struct Poll<'a> {
    callbacks: &'a ProducerCallbacks<'a>,
    began: u32,
    pauses: core::cell::Cell<u8>,
    admission: core::cell::Cell<ProducerEntry>,
}
impl Poll<'_> {
    fn stop(&self) -> Stop { (self.callbacks.stop)(self.began) }
    fn admission_entry(&self) -> ProducerEntry {
        let entry = (self.callbacks.entry)(self.began);
        if entry == ProducerEntry::Denied || self.admission.get() == ProducerEntry::Ready {
            self.admission.set(entry);
        }
        self.admission.get()
    }
}
struct AttemptAuthority<'a, 'ctx> { cancel: &'a Cancellation<'ctx, Poll<'ctx>>, fail_terminal: bool }
impl Authority for AttemptAuthority<'_, '_> {
    fn cancelled(&self) -> bool { !self.cancel.checkpoint() }
    fn failure(&self) {
        if self.fail_terminal { self.cancel.fail_terminal(); }
        else { self.cancel.result(false); }
    }
}
fn position(reader: &PhasedReader<'_, '_>, source: &newt_reader_ui::ReaderSource<'_>,
    digest: &Digest,
) -> Option<Position> {
    reader.opened().then(|| Position { spine: reader.info().spine,
        spines: reader.selection().spines, verified: source.digest() == Some(*digest) })
}
fn pending(visit: &mut ProducerVisit<'_>, reader: &PhasedReader<'_, '_>,
    source: &newt_reader_ui::ReaderSource<'_>, digest: &Digest, eligible: bool,
) -> bool { visit.pending(position(reader, source, digest), eligible) }
fn interrupted(visit: &mut ProducerVisit<'_>, reader: &PhasedReader<'_, '_>,
    source: &newt_reader_ui::ReaderSource<'_>, digest: &Digest, target: u32,
) -> Admission {
    visit.count_try(position(reader, source, digest), false, target);
    Admission::Stopped
}
impl<'scope, 'sdk, 'route, 'bank, NativePort: Native + PageEntries>
    Resources<'_, 'scope, 'sdk, 'route, 'bank, NativePort>
{
    fn stored(&mut self, reader: &mut PhasedReader<'_, '_>,
        source: &mut newt_reader_ui::ReaderSource<'_>, digest: &Digest, target: u32,
        need: Option<&mut PageCacheNeed>, cancelled: &dyn Fn() -> bool, parsed: &mut bool,
    ) -> MissingProbe {
        let mut cache = PhasedChapterCache { store: &mut *self.pages,
            hash: &mut *self.hash, settings: self.settings };
        let mut operation = OperationPhased::new(reader, &mut self.storage,
            &mut *self.panel, &mut self.parser, &mut *self.breaker);
        cache.producer_stored(source, digest, target, need, cancelled, parsed, &mut operation)
    }
    fn admit(&mut self, reader: &mut PhasedReader<'_, '_>,
        source: &mut newt_reader_ui::ReaderSource<'_>, digest: &Digest,
        visit: &mut ProducerVisit<'_>, eligible: bool, poll: &Poll<'_>,
        clock: fn() -> u32, wait: Option<fn()>, parsed: &mut bool,
        #[cfg(all(feature = "rust-app", feature = "dev-usb"))] admitted_units: &mut u8,
    ) -> Admission {
        if !pending(visit, reader, source, digest, eligible) { return Admission::Denied; }
        if poll.admission_entry() != ProducerEntry::Ready || !pending(visit, reader, source, digest, eligible) {
            return Admission::Stopped;
        }
        let target = visit.target(position(reader, source, digest));
        let cancelled = || poll.admission_entry() != ProducerEntry::Ready;
        if target != reader.info().spine {
            let current = self.stored(reader, source, digest, u32::MAX, None, &cancelled, parsed);
            if current == MissingProbe::Stopped { return interrupted(visit, reader, source, digest, target); }
            if current != MissingProbe::Complete {
                visit.deny_current_admission();
                return Admission::Denied;
            }
        }
        let mut need = PageCacheNeed::default();
        let probe = self.stored(reader, source, digest, target, Some(&mut need), &cancelled, parsed);
        if probe == MissingProbe::Stopped { return interrupted(visit, reader, source, digest, target); }
        if probe == MissingProbe::Denied {
            visit.deny_admission(position(reader, source, digest)); return Admission::Denied;
        }
        if probe == MissingProbe::Complete {
            visit.complete(position(reader, source, digest), target, need.short_chapter);
            if target == reader.info().spine { visit.count_try(position(reader, source, digest), true, target); }
            visit.spent(); return Admission::Spent;
        }
        if visit.tries(position(reader, source, digest), target) >= 3 {
            visit.deny_admission(position(reader, source, digest)); return Admission::Denied;
        }
        if cancelled() || !pending(visit, reader, source, digest, eligible) {
            return interrupted(visit, reader, source, digest, target);
        }
        struct Check<'a>(&'a dyn Fn() -> bool);
        impl Authority for Check<'_> { fn cancelled(&self) -> bool { (self.0)() } }
        #[cfg(feature = "page-turn-trace")]
        let census_started = (poll.callbacks.trace_micros)();
        let grant = self.pages.0.census(&Check(&cancelled), Some(clock), wait, Some(&need));
        #[cfg(all(feature = "rust-app", feature = "page-turn-trace"))]
        if let Some(trace) = poll.callbacks.trace {
            trace(ProducerEvent::Grant {
                warm: false,
                census_us: (poll.callbacks.trace_micros)().wrapping_sub(census_started),
                files: grant.files,
                outcome: grant.outcome,
                units: grant.units,
                entries: grant.entries,
            });
        }
        if grant.outcome == MissingProbe::Stopped { return interrupted(visit, reader, source, digest, target); }
        if grant.outcome == MissingProbe::Denied {
            visit.deny_admission(position(reader, source, digest)); return Admission::Denied;
        }
        if cancelled() || !pending(visit, reader, source, digest, eligible) {
            return interrupted(visit, reader, source, digest, target);
        }
        let (status, fresh) = source.with_producer_read(|same| {
            if same.producer_receipt_live() { Status::Ok } else { Status::Stale }
        });
        if status != Status::Ok || !fresh || grant.units < PAGE_REQUIRED_UNITS {
            visit.deny_admission(position(reader, source, digest)); return Admission::Denied;
        }
        if cancelled() || !pending(visit, reader, source, digest, eligible) {
            return interrupted(visit, reader, source, digest, target);
        }
        if !self.pages.0.access().release_for_admission() {
            visit.deny_admission(position(reader, source, digest)); return Admission::Denied;
        }
        if cancelled() || !pending(visit, reader, source, digest, eligible) {
            return interrupted(visit, reader, source, digest, target);
        }
        self.pages.0.access().set_producer_allowance(grant.units, grant.entries);
        #[cfg(all(feature = "rust-app", feature = "dev-usb"))]
        { *admitted_units = grant.units; }
        visit.granted(); Admission::Granted
    }
    pub(super) fn producer_idle<'io, P, N: nvs::Native>(
        &mut self, io: &'io UiIo, card: &mut P, state: &mut MountedCheckpoint<'_, N>,
        duty: &mut ProducerPauseFn<'_, 'io, N, P>, visit: &mut ProducerVisit<'_>,
        reader: &mut PhasedReader<'_, '_>, source: &mut newt_reader_ui::ReaderSource<'_>,
        owner: &mut ExternalOwner, ui: &mut ReaderUiState, statistics: &mut ReadingStatsRuntime,
        accepted_open: &mut bool, digest: &Digest,
        request: &BookRequest<'_>, callbacks: &ProducerCallbacks<'_>,
    ) -> (bool, bool, Option<u32>)
    where P: CardPort<Port<'io> = NativePort> {
        let eligible = owner.state.opened() && reader.opened() && reader.has_page()
            && ui.panel == newt_reader_ui::ReaderPanel::Page && source.digest() == Some(*digest);
        if !eligible { return (false, false, None); }
        if !pending(visit, reader, source, digest, eligible) { return (false, false, None); }
        let poll = Poll { callbacks, began: (request.now)(), pauses: core::cell::Cell::new(0),
            admission: core::cell::Cell::new(ProducerEntry::Ready) };
        match poll.admission_entry() {
            ProducerEntry::Ready => {},
            ProducerEntry::Blocked => return (false, false, None),
            ProducerEntry::Denied => {
                visit.deny_current_admission();
                #[cfg(all(feature = "rust-app", feature = "dev-usb"))]
                if let Some(trace) = callbacks.trace {
                    trace(ProducerEvent::Admission { result: Admission::Denied, units: 0 });
                }
                return (false, false, None);
            }
        }
        let mut parsed = false;
        #[cfg(all(feature = "rust-app", feature = "dev-usb"))]
        let mut admitted_units = 0;
        let mut admitted = self.admit(reader, source, digest, visit, eligible, &poll,
            request.now, callbacks.wait, &mut parsed,
            #[cfg(all(feature = "rust-app", feature = "dev-usb"))] &mut admitted_units);
        let entry = poll.admission_entry();
        if entry == ProducerEntry::Denied {
            visit.stop_admission(position(reader, source, digest), eligible,
                || self.pages.0.access().set_producer_allowance(0, 0));
            visit.deny_current_admission();
            admitted = Admission::Denied;
        }
        #[cfg(all(feature = "rust-app", feature = "dev-usb"))]
        if let Some(trace) = callbacks.trace {
            trace(ProducerEvent::Admission { result: admitted, units: admitted_units });
        }
        let stopped = admitted == Admission::Granted && entry != ProducerEntry::Ready;
        if admitted != Admission::Granted || stopped {
            let p = position(reader, source, digest);
            visit.stop_admission(p, eligible, || self.pages.0.access().set_producer_allowance(0, 0));
            // Frozen ReadingCacheIdle renews quiet after Spent/Stopped too.
            let quiet_from = admitted.quiet_after_admission(stopped, request.now);
            return (parsed, false, quiet_from);
        }
        if !eligible || self.pages.0.access().producer_allowance() < u64::from(PAGE_CANDIDATE) {
            return (parsed, false, None);
        }
        visit.spent();
        let target = visit.target(position(reader, source, digest));
        let next = target != reader.info().spine;
        #[cfg(all(feature = "rust-app", feature = "dev-usb"))]
        if let Some(trace) = callbacks.trace {
            trace(ProducerEvent::Start { spine: reader.info().spine,
                ahead: target.wrapping_sub(reader.info().spine) });
        }
        let cancellation = Cancellation::new(Hooks { context: &poll,
            stop: Some(Poll::stop), yield_now: Some(|poll| {
                if let Some(callback) = poll.callbacks.yield_now { callback.yield_now(); }
            }), pause: None });
        let authority = AttemptAuthority { cancel: &cancellation, fail_terminal: !next };
        #[cfg(all(feature = "rust-app", feature = "page-turn-trace"))]
        let on_segment = |seg| {
            if let Some(trace) = callbacks.trace {
                trace(ProducerEvent::Segment(seg));
            }
        };
        #[cfg(all(feature = "rust-app", feature = "page-turn-trace"))]
        let policy = ProducerPolicy::new(&authority).with_trace(callbacks.trace_micros, &on_segment);
        #[cfg(not(all(feature = "rust-app", feature = "page-turn-trace")))]
        let policy = ProducerPolicy::new(&authority);
        let info = reader.info();
        let settings = self.settings;
        let permille = panel_statistics::position_permille(owner, reader);
        let bytes = source.size();
        let mut prepared = u32::MAX;
        let mut repaint = false;
        let recovery = &mut self.recovery;
        let deadline_failed = core::cell::Cell::new(false);
        let duty_terminal = core::cell::Cell::new(false);
        let mut service_duty = |pages: &mut ChapterCacheAccess<'_, 'route, NativePort>,
            metadata: Option<&dyn super::metadata::MetadataReader>, cancelled: &dyn Fn() -> Stop| {
            let producing = pages.producing();
            if producing {
                if !pages.suspend_producer_write() {
                    deadline_failed.set(true);
                    return if !policy.failed() && cancelled() == Stop::Interrupted {
                        Stop::Interrupted
                    } else {
                        duty_terminal.set(true);
                        Stop::Terminal
                    };
                }
            } else {
                // Finalization has detached the producer; State can now release
                // the normal Pages loan without suspending a nonexistent writer.
                pages.release_for_state();
            }
            let (mut reason, changed) = duty(ProducerPause { io, card: &mut *card,
                nvs: &mut *state.nvs, facts: &mut *state.facts, checkpoint: &mut *state.checkpoint,
                pages, owner: &mut *owner, ui: &mut *ui, statistics: &mut *statistics,
                recovery: &mut *recovery, accepted_open: &mut *accepted_open,
                metadata, permille, settings, digest },
                &|| cancelled() != Stop::None);
            repaint |= changed;
            if reason == Stop::None { reason = cancelled(); }
            if reason == Stop::None && producing {
                if pages.resume_producer_write() {
                    poll.pauses.set(poll.pauses.get() + 1);
                } else if !policy.failed() && cancelled() == Stop::Interrupted {
                    reason = Stop::Interrupted;
                } else {
                    reason = Stop::Terminal;
                }
            }
            if reason != Stop::None { deadline_failed.set(true); }
            if reason == Stop::Terminal { duty_terminal.set(true); }
            reason
        };
        let mut current_outcome = CurrentOutcome::NoProgress(Status::Unsupported);
        let result = self.pages.0.produce(&policy, |access| {
            let mut pause = |pages: &mut ChapterCacheAccess<'_, 'route, NativePort>, same: &Cancellation<'_, Poll<'_>>| {
                if let Some(reason) = (poll.callbacks.pause_request)(poll.began, poll.pauses.get()) {
                    return if next && same.interrupted() { Stop::Interrupted } else { reason };
                }
                let reason = service_duty(pages, None, &|| {
                    if !same.stopped() { Stop::None }
                    else if same.interrupted() { Stop::Interrupted } else { Stop::Terminal }
                });
                if next && same.interrupted() { Stop::Interrupted } else { reason }
            };
            let (mut status, fresh) = source.with_producer_read(|source| {
                parsed = true;
                let mut route = newt_phased_reader::producer::Route { settings, digest, info,
                    hash: &mut *self.hash };
                let status = self.storage.with_producer(&mut *self.panel, &cancellation, access,
                    source, info.page_count, Some(&mut pause), |producer| {
                        producer.with_source_guard(bytes, |producer| Ok(if next {
                            producer.produce_next_indexed(reader, target, &mut self.parser, &mut *self.breaker,
                                &mut route, Some(&mut prepared))
                        } else {
                            current_outcome = producer.produce_current_indexed(reader, &mut self.parser,
                                &mut *self.breaker, &mut route);
                            match current_outcome {
                                CurrentOutcome::Done | CurrentOutcome::Progress => Status::Ok,
                                CurrentOutcome::NoProgress(status) => status,
                            }
                        }))
                    }).unwrap_or_else(|status| {
                        if !next {
                            cancellation.fail_terminal();
                            current_outcome = CurrentOutcome::NoProgress(status);
                        }
                        status
                    });
                if next || matches!(current_outcome, CurrentOutcome::NoProgress(_)) {
                    cancellation.result(status == Status::Ok);
                }
                status
            });
            if !fresh {
                if !next {
                    cancellation.fail_terminal();
                    current_outcome = CurrentOutcome::NoProgress(Status::Stale);
                } else {
                    cancellation.result(false);
                }
                status = Status::Stale;
            }
            status
        });
        let status = match result {
            Some(status) => status,
            None => {
                if !next {
                    cancellation.fail_terminal();
                    current_outcome = CurrentOutcome::NoProgress(Status::Unsupported);
                }
                Status::Unsupported
            }
        };
        self.pages.0.access().set_producer_allowance(0, 0);
        // Prefetch keeps its original accounting and quiet-timestamp ordering
        // before detached duty runs; current-chapter accounting waits for the
        // final pause_request / detached duty decision while keeping the finish
        // trace before detached duty.
        let mut quiet_from = if next {
            if status == Status::Ok {
                visit.complete(position(reader, source, digest), target, prepared <= 3);
            }
            if status != Status::Ok {
                visit.count_try(position(reader, source, digest), !cancellation.interrupted(), target);
            }
            let interrupted = status != Status::Ok && cancellation.interrupted();
            let quiet_from = quiet_after_attempt(status == Status::Ok, interrupted, request.now);
            if status != Status::Ok {
                if interrupted {
                    visit.pending(position(reader, source, digest), eligible);
                } else {
                    visit.deny_admission(position(reader, source, digest));
                }
            }
            #[cfg(all(feature = "rust-app", feature = "dev-usb"))]
            if let Some(trace) = callbacks.trace {
                trace(ProducerEvent::Finish { status, interrupted });
            }
            quiet_from
        } else {
            #[cfg(all(feature = "rust-app", feature = "dev-usb"))]
            if let Some(trace) = callbacks.trace {
                let pre_terminal = cancellation.terminal() || policy.failed() || duty_terminal.get();
                let outcome = if pre_terminal {
                    match current_outcome {
                        CurrentOutcome::NoProgress(status) => CurrentOutcome::NoProgress(status),
                        CurrentOutcome::Done | CurrentOutcome::Progress => CurrentOutcome::NoProgress(Status::IoError),
                    }
                } else {
                    current_outcome
                };
                let interrupted = !pre_terminal && cancellation.interrupted();
                let trace_status = match outcome {
                    CurrentOutcome::Done | CurrentOutcome::Progress => Status::Ok,
                    CurrentOutcome::NoProgress(status) => status,
                };
                let trace_interrupted = match outcome {
                    CurrentOutcome::Done => false,
                    CurrentOutcome::Progress | CurrentOutcome::NoProgress(_) => interrupted,
                };
                trace(ProducerEvent::Finish { status: trace_status, interrupted: trace_interrupted });
            }
            None
        };
        // The same duty catches final CRC/sync deadlines only after producer,
        // parser and source loans have unwound and the finish trace is emitted.
        if !deadline_failed.get() {
            match (callbacks.pause_request)(poll.began, 0) {
                None => {
                    let mut access = self.pages.0.access();
                    let final_duty = service_duty(&mut access, Some(&*reader), &|| poll.stop());
                    if final_duty == Stop::Terminal {
                        duty_terminal.set(true);
                    }
                }
                Some(Stop::Terminal) => {
                    duty_terminal.set(true);
                }
                Some(_) => {}
            }
        } else if !next && matches!((callbacks.pause_request)(poll.began, 0), Some(Stop::Terminal)) {
            duty_terminal.set(true);
        }
        if !next {
            // Failure classification runs before leftover/progress rules: any
            // terminal stop, stale read, duty failure (including detached
            // finalization duty or terminal final pause_request), or native
            // storage fault overrides an earlier sealed segment or concurrent
            // interruption.
            let terminal = cancellation.terminal() || policy.failed() || duty_terminal.get();
            let outcome = if terminal {
                match current_outcome {
                    CurrentOutcome::NoProgress(status) => CurrentOutcome::NoProgress(status),
                    CurrentOutcome::Done | CurrentOutcome::Progress => CurrentOutcome::NoProgress(Status::IoError),
                }
            } else {
                current_outcome
            };
            let interrupted = !terminal && cancellation.interrupted();
            quiet_from = visit.finish_current(
                position(reader, source, digest), target, eligible, outcome, interrupted, request.now,
            );
        }
        (parsed, repaint, quiet_from)
    }
}
