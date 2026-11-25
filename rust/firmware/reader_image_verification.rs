// SPDX-License-Identifier: MIT
//! Bounded foreground/idle verification over the session's shared hash region.
use super::*;
use crate::{app_image::{Application, Events, Home}, home_startup::CardPort,
    reader_duty::ReaderDuty, reader_verification::Verification};
use newt_core::{frame::{self, Frame}, durable::Load,
    producer_visit::{Admission, Position, ProducerVisit}, selection_authority::{flag, CardObservation},
    save_notice::{ReadingCommitResult, ResumeIssue, SaveOutcome}, sleep_intent::SleepResumeStore};
use newt_native_storage::{Authority, Error as IoError, Native};
use newt_phased_reader::{imported_revision::RevisionHasher, phase_storage::BuildLoan};
use newt_reader_ui::ReaderUiNotice;

struct Cancel<'a>(&'a dyn Fn() -> bool);
impl Authority for Cancel<'_> { fn cancelled(&self) -> bool { !(self.0)() } }

impl Runtime {
    pub(super) fn finish_verification_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut Frame, events: &mut Events<'_, '_>, queue_turns: bool,
    ) -> bool {
        self.finish_verification_core(home, ui, logical, events, queue_turns, true)
    }

    pub(super) fn finish_verification_core(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut Frame, events: &mut Events<'_, '_>, queue_turns: bool,
        record_checkpoint_failure: bool,
    ) -> bool {
        if !self.selection.active() { return !self.selection.selection_blocked(); }
        let Events { queue, control, task, action: trace } = events;
        control.with_open(millis(), |control, _owned| {
            let mut events = Events { queue, control, action: *trace, task: task.reborrow() };
            self.verify_source_pass(home, logical, &events, false, queue_turns,
                ui.app.screen == newt_core::app::Screen::Sleeping);
            let mut state = self.session.as_ref().map_or(Verification::Verified, |s| s.verification.state());
            if state == Verification::Changed {
                self.reopen_changed_image(home, ui, logical, &mut events);
                state = self.session.as_ref().map_or(Verification::Verified, |s| s.verification.state());
                if !self.book().is_some_and(|book| book.owner.state.opened()) {
                    if record_checkpoint_failure {
                        self.saves.checkpoint(ReadingCommitResult { progress: SaveOutcome::Unconfirmed,
                            destination: SaveOutcome::Unchanged });
                    }
                    return false;
                }
            }
            if state != Verification::Verified {
                self.reader_ui.notice = if events.control.timed_out() && queue_turns {
                    ReaderUiNotice::VerificationTimedOut
                } else if state == Verification::Changed { ReaderUiNotice::BookChanged }
                else if state == Verification::Failed { ReaderUiNotice::VerificationFailed }
                else { self.reader_ui.notice };
                if record_checkpoint_failure {
                    self.saves.checkpoint(ReadingCommitResult { progress: SaveOutcome::Unconfirmed,
                        destination: SaveOutcome::Unchanged });
                }
                return false;
            }
            self.commit_verified_image(home, events.action)
        })
    }

    pub(super) fn verify_idle_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &Frame, events: &Events<'_, '_>,
        delay: &mut u32,
    ) -> bool {
        if !self.session.as_ref().is_some_and(|s| s.verification.state() == Verification::Pending) {
            self.invalidate_failed_index(home);
            return false;
        }
        // `due` deliberately ignores the battery gate: 30 s durable checkpoints
        // require a verified source (most needed on low battery against sudden
        // power-off), and this runs at most once per warm-open session.
        let due = millis().wrapping_sub(self.last_progress.get()) >= 30000;
        let optional = ui.displayed.valid
            && ((ui.info.charging_known && ui.info.charging)
                || (ui.info.battery_known && ui.info.battery_percent >= 20));
        if !due && !optional { return false; }
        self.verify_source_pass(home, logical, events, true, false, false);
        let pending = events.queue.pending() || {
            #[cfg(feature = "dev-usb")]
            { home.bus.log.available() > 0 }
            #[cfg(not(feature = "dev-usb"))]
            { false }
        };
        *delay = if pending { 0 } else { 1 };
        match self.session.as_ref().map(|s| s.verification.state()) {
            Some(Verification::Changed) => {
                self.reader_ui.notice = ReaderUiNotice::BookChanged;
                true
            }
            Some(Verification::Failed) => {
                self.reader_ui.notice = ReaderUiNotice::VerificationFailed;
                true
            }
            _ => false,
        }
    }

    fn verify_source_pass(&mut self, home: &mut Home<'_>, logical: &Frame,
        events: &Events<'_, '_>, idle: bool, queue_turns: bool, sleeping: bool,
    ) {
        let (Some(source), Some(session)) = (self.source.as_mut(), self.session.as_mut()) else { return; };
        if session.verification.state() != Verification::Pending { return; }
        let digest = *self.selection.digest();
        let clock = home.bus.board.monotonic_clock();
        let duty = ReaderDuty::new(events.control, &self.cadence, &clock, None, events.action);
        let yielded = || duty.yield_now();
        let result = home.with_reader_io(|scope| {
            let allowed = || {
                if duty.cancelled() { return false; }
                if !sleeping && queue_turns {
                    let back = events.control.back_button() as u8;
                    let stop = events.queue.peek_button().is_some_and(|button|
                        button == back
                        || button == newt_input_policy::Button::Power as u8 || button == 0x80);
                    if stop { return false; }
                } else if !sleeping && events.queue.pending() { return false; }
                #[cfg(feature = "dev-usb")]
                if !sleeping && !queue_turns && scope.bus.log.available() > 0 { return false; }
                true
            };
            let cancel = Cancel(&allowed);
            let mut native = scope.card.port(scope.io);
            source.with_source(&mut native, Some(&cancel), |raw| {
                scope.bus.panel.with_parse(|scratch| {
                    let began = clock.micros();
                    while session.verification.state() == Verification::Pending && allowed()
                        && (!idle || clock.micros().wrapping_sub(began) < 20000) {
                        let bytes = if idle { scratch.len().min(4096) } else { scratch.len() };
                        session.verification.step_with_update(raw, &digest, &mut scratch[..bytes],
                            &mut || allowed(), &mut |hash, bytes| hash.borrow_with_yield(&yielded).update(bytes));
                    }
                })
            })
        });
        if !matches!(result, Ok(Ok(Ok(())))) { session.verification.mark_failed(); }
        self.invalidate_failed_index(home);
        let Ok(native) = home.bus.board.frame() else { crate::app_power::stop(); };
        let Ok(native) = native.try_into() else { crate::app_power::stop(); };
        if !frame::restore_native_frame_prefix(logical, native,
            if idle { 4096 } else { newt_core::frame::FRAME_BYTES }, self.settings.orientation) {
            crate::app_power::stop();
        }
    }

    #[inline(never)]
    pub(super) fn invalidate_failed_index(&mut self,home:&mut Home<'_>) {
        let Some(session)=self.session.as_mut() else { return; };
        if !matches!(session.verification.state(),Verification::Changed|Verification::Failed) { return; }
        let digest=self.selection.digest();
        let _=home.with_reader_io(|scope| {
            let mut native=scope.card.port(scope.io);
            super::index_invalidation::invalidate_failed_index(session,&mut native,digest,&super::boot::NoCancel,millis)
        });
    }

    pub(super) fn commit_verified_image(&mut self, home: &mut Home<'_>, trace: crate::reader_action::Trace<'_>) -> bool {
        self.commit_verified_admitted_image(home, trace, None, &mut false, &mut false)
    }

    pub(super) fn commit_verified_admitted_image(&mut self, home: &mut Home<'_>,
        trace: crate::reader_action::Trace<'_>, pending: Option<&dyn Fn() -> bool>,
        deferred: &mut bool, entered: &mut bool,
    ) -> bool {
        let verified = self.session.as_ref().is_none_or(|session|
            session.verification.state() == Verification::Verified);
        let verification_pending = self.session.as_ref().is_some_and(|session|
            session.verification.state() == Verification::Pending);
        if !verified { return false; }
        self.with_store_io(home, |this, store, io, log, _board| {
            let mut ready = || pending.is_none_or(|pending| !pending())
                && {
                    #[cfg(feature = "dev-usb")]
                    { pending.is_none() || log.available() == 0 }
                    #[cfg(not(feature = "dev-usb"))]
                    { let _ = log; true }
                };
            let mut admission = newt_core::durable::Admission {
                quiet: Some(&mut ready), entered: false, deferred: false,
            };
            if !admission.ready() { *deferred = true; return false; }
            if this.selection.flags() & flag::COMMIT_OWED != 0 {
                let authority = SleepResumeStore::new(store).load();
                if !admission.ready() { *deferred = true; return false; }
                let protected = match authority {
                    Load::Loaded(intent) => intent.book,
                    Load::Empty => 0,
                    _ => { this.saves.set_resume(ResumeIssue::Unavailable); return false; }
                };
                this.saves.set_resume(ResumeIssue::None);
                let committed = {
                    let _commit = trace.scope(crate::reader_action::ActionStage::SelectionCommit);
                    this.selection.commit_selection(store, &mut || verified, protected,
                        pending.map(|_| &mut admission))
                };
                *entered = admission.entered;
                if !committed { *deferred = admission.deferred; return false; }
                // The same transition runs after deferred verification and
                // failed-commit retries, before either can checkpoint this D.
                this.checkpoint.selection_committed(store, this.selection.digest(),
                    this.saves.page_unconfirmed(), this.saves.destination_unconfirmed());
            }
            let source_exists = this.source.is_some();
            let generation = &mut this.cover_generation;
            let memory = &this.memory;
            let session = &mut this.session;
            let mut card = CardObservation {
                reading_state_unavailable: &mut |store| newt_core::reading_stats_store::StatsBackend::reading_state_unavailable(store),
                card_absent: &mut |_| source_exists && matches!(io.port().stat("/newt-sd/Newt/Books", false), Err(IoError::Unavailable)),
                count_denied_if_session: &mut |_| {
                    if let Some(session) = session.as_mut() {
                        let position = memory.external().and_then(|memory| {
                            let view = memory.view();
                            let reader = &view.reader;
                            reader.opened().then(|| Position { spine: reader.info().spine,
                                spines: reader.selection().spines, verified })
                        });
                        session.verification.with_producer_visit(|ledger, view| {
                            let mut visit = ProducerVisit::from_parts(ledger, *view, Admission::Denied, true);
                            visit.deny_admission(position);
                            *view = visit.into_parts().0;
                        });
                    }
                },
                home_cover_changed: &mut |_| *generation = generation.wrapping_add(1),
            };
            let ready = this.selection.reconcile_verified_selection(store, &mut || verified, &mut || verification_pending, &mut card);
            if !admission.ready() { *deferred = true; return false; }
            ready
        }).unwrap_or(false)
    }
}
