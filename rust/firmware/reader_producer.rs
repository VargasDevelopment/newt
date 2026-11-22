//! Frozen library_reader_cache_producer.h and reader_revision_cache_policy.h.
//! The real source, native Pages and one book ledger remain the sole owners.
use super::*;
use newt_core::zip_window::source::BookSource;
use newt_core::{cache_names::namespace::{PageCacheNeed, PAGE_CANDIDATE, PAGE_REQUIRED_UNITS},
    producer_policy::{Cancellation, Hooks, ProducerReceipt, Stop},
    producer_visit::{Admission, MissingProbe, Position, ProducerVisit, quiet_after_attempt}};
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
}
/// Only measured scalars cross the short DEV observation loan.
#[cfg(all(feature = "rust-app", feature = "dev-usb"))]
#[derive(Clone, Copy)]
pub enum ProducerEvent {
    Admission { result: Admission, units: u8 },
    Start { spine: u32, ahead: u32 },
    Finish { status: Status, interrupted: bool },
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
struct AttemptAuthority<'a, 'ctx> { cancel: &'a Cancellation<'ctx, Poll<'ctx>> }
impl Authority for AttemptAuthority<'_, '_> {
    fn cancelled(&self) -> bool { !self.cancel.checkpoint() }
    fn failure(&self) { self.cancel.result(false); }
}
fn position(reader: &PhasedReader<'_, '_>, source: &newt_reader_ui::ReaderSource<'_>,
    digest: &Digest,
) -> Option<Position> {
    reader.opened().then(|| Position { spine: reader.info().spine,
        spines: reader.selection().spines, verified: source.digest() == *digest })
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
        let grant = self.pages.0.census(&Check(&cancelled), Some(clock), wait, Some(&need));
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
            && ui.panel == newt_reader_ui::ReaderPanel::Page && source.digest() == *digest;
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
        let authority = AttemptAuthority { cancel: &cancellation };
        let policy = ProducerPolicy::new(&authority);
        let info = reader.info();
        let settings = self.settings;
        let permille = panel_statistics::position_permille(owner, reader);
        let bytes = source.size();
        let mut prepared = u32::MAX;
        let mut repaint = false;
        let recovery = &mut self.recovery;
        let deadline_failed = core::cell::Cell::new(false);
        let mut service_duty = |pages: &mut ChapterCacheAccess<'_, 'route, NativePort>,
            metadata: Option<&dyn super::metadata::MetadataReader>, cancelled: &dyn Fn() -> Stop| {
            let producing = pages.producing();
            if producing {
                if !pages.suspend_producer_write() {
                    deadline_failed.set(true);
                    return Stop::Terminal;
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
                } else { reason = Stop::Terminal; }
            }
            if reason != Stop::None { deadline_failed.set(true); }
            reason
        };
        let result = self.pages.0.produce(&policy, |access| {
            let mut pause = |pages: &mut ChapterCacheAccess<'_, 'route, NativePort>, same: &Cancellation<'_, Poll<'_>>| {
                if let Some(reason) = (poll.callbacks.pause_request)(poll.began, poll.pauses.get()) {
                    return reason;
                }
                service_duty(pages, None, &|| {
                    if !same.stopped() { Stop::None }
                    else if same.interrupted() { Stop::Interrupted } else { Stop::Terminal }
                })
            };
            let (mut status, fresh) = source.with_producer_read(|source| {
                parsed = true;
                let mut route = newt_phased_reader::producer::Route { settings, digest, info,
                    hash: &mut *self.hash };
                let status = self.storage.with_producer(&mut *self.panel, &cancellation, access,
                    source, info.page_count, Some(&mut pause), |producer| {
                        producer.with_source_guard(bytes, |producer| Ok(if next {
                            producer.produce_next(reader, target, &mut self.parser, &mut *self.breaker,
                                &mut route, Some(&mut prepared))
                        } else { producer.produce_current(reader, &mut self.parser, &mut *self.breaker, &mut route) }))
                    }).unwrap_or_else(|status| status);
                cancellation.result(status == Status::Ok);
                status
            });
            if !fresh { cancellation.result(false); status = Status::Stale; }
            status
        });
        let status = result.unwrap_or(Status::Unsupported);
        self.pages.0.access().set_producer_allowance(0, 0);
        if status == Status::Ok {
            visit.complete(position(reader, source, digest), target, next && prepared <= 3);
        }
        if !next || status != Status::Ok {
            visit.count_try(position(reader, source, digest), status == Status::Ok || !cancellation.interrupted(), target);
        }
        let interrupted = status != Status::Ok && cancellation.interrupted();
        let quiet_from = quiet_after_attempt(status == Status::Ok, interrupted, request.now);
        if status != Status::Ok {
            if interrupted {
                visit.pending(position(reader, source, digest), eligible);
            }
            else { visit.deny_admission(position(reader, source, digest)); }
        }
        #[cfg(all(feature = "rust-app", feature = "dev-usb"))]
        if let Some(trace) = callbacks.trace {
            trace(ProducerEvent::Finish { status, interrupted });
        }
        // The same duty catches final CRC/sync deadlines only after producer,
        // parser and source loans have unwound and the finish trace is emitted.
        if !deadline_failed.get() && (callbacks.pause_request)(poll.began, 0).is_none() {
            let mut access = self.pages.0.access();
            service_duty(&mut access, Some(&*reader), &|| poll.stop());
        }
        (parsed, repaint, quiet_from)
    }
}
