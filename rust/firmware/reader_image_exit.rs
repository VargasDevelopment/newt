// SPDX-License-Identifier: MIT
//! The live physical exit loan. The checkpoint child owns record publication.
use super::*;
use crate::{app_image::Home, home_startup::CardPort, reader_book,
    reader_pages::ReaderPages, sd_revision::SdRevisionStorage};
use newt_c3_board::Error;
use newt_core::{selection_authority::flag, save_notice::{ReadingCommitResult, SaveOutcome}};
use newt_native_storage::{RouteState, chapter_cache::{ChapterCacheState, ChapterCacheStorage}};
use newt_phased_reader::imported_revision::RevisionPorts;
use newt_reader_ui::StrictSource;

impl Runtime {
    pub(super) fn publish_notice_image(&mut self, ui: &mut crate::app_image::Application<'_>) {
        self.saves.set_fold_owed(self.statistics.fold_owed);
        let notice = self.saves.publish();
        ui.info = core::mem::take(&mut ui.info).with_storage_notice(
            notice.storage.map(|text| text.to_bytes()), notice.page_save_unconfirmed);
    }

    pub(crate) fn finish_sleep_image(&mut self, home: &mut Home<'_>,
        ui: &mut crate::app_image::Application<'_>, logical: &mut newt_core::frame::Frame,
        events: &mut crate::app_image::Events<'_, '_>, now: u32,
    ) -> bool {
        let before = (ui.info.storage(), ui.info.page_save_unconfirmed());
        self.leave_for_exit_image(now);
        self.finish_verification_image(home, ui, logical, events, false);
        let _ = self.finish_paused_exit_image(home, millis(), true, events.action);
        self.save_picture_image(home, ui, logical, events);
        self.publish_notice_image(ui);
        before != (ui.info.storage(), ui.info.page_save_unconfirmed())
    }

    /// Finish the same exit/save/fold/destination operation as Back, while
    /// lending actual live owners. No parser/source/port loan survives return.
    pub(super) fn finish_exit_image(&mut self, home: &mut Home<'_>, now: u32,
        sleeping: bool, trace: crate::reader_action::Trace<'_>,
    ) -> Result<(), Error> {
        self.leave_for_exit_image(now);
        self.finish_paused_exit_image(home, now, sleeping, trace)
    }

    pub(super) fn leave_for_exit_image(&mut self, now: u32) {
        #[cfg(feature = "embedded-sample")]
        if let Some(memory) = self.memory.sample_mut() {
            memory.with_tracker(|tracker| tracker.leave(now));
        }
        if let Some(memory) = self.memory.external_mut() {
            memory.lend(|memory| {
                memory.runtime.owner.leave(now);
            });
        }
    }

    pub(super) fn refuse_checkpoint_image(&mut self, now: u32, accepted_open: bool) {
        let candidate = self.book().filter(|book| book.owner.state.has_progress())
            .map(|book| *book.owner.tracker.snapshot(now));
        let state = self.session.as_ref().map(|session| session.verification.state());
        let changed = state == Some(crate::reader_verification::Verification::Changed);
        let accepted_pending = accepted_open
            && state == Some(crate::reader_verification::Verification::Pending);
        if let Some(result) = reader_book::PendingRecovery::blocked_checkpoint(&mut self.recovery,
            &mut self.checkpoint, candidate, self.settings, self.selection.digest(), changed, accepted_pending) {
            self.saves.checkpoint(ReadingCommitResult {
                progress: outcome(result.progress), destination: outcome(result.destination),
            });
        }
    }

    /// The caller ended the old reading interval before verification. A changed
    /// source may now own a newly opened tracker, so never leave it at old time.
    pub(super) fn finish_paused_exit_image(&mut self, home: &mut Home<'_>, now: u32,
        sleeping: bool, trace: crate::reader_action::Trace<'_>,
    ) -> Result<(), Error> {
        self.selection.retire_checkpoint_metadata();
        self.checkpoint.begin_operation();
        #[cfg(feature = "embedded-sample")]
        if self.memory.has_sample() {
            self.save_sample_image(home, now);
            return Ok(());
        }
        if !self.book().is_some_and(|book| book.owner.state.has_progress()) { return Ok(()); }
        if self.session.as_ref().is_some_and(|session|
            session.verification.state() != crate::reader_verification::Verification::Verified) {
            self.refuse_checkpoint_image(now, false);
            return Err(Error::Io);
        }
        let mut accepted = self.selection.flags() & flag::ACCEPTED_OPEN_OWED != 0;
        let digest = *self.selection.digest();
        let clock = home.bus.board.monotonic_clock();
        let Self { memory, reader_ui, source, session: context, checkpoint, statistics, recovery,
            saved, cadence, exit_metadata, settings, selection, .. } = self;
        let result = match (memory.external_mut(), source.as_mut()) {
            (Some(memory), Some(source)) => {
                memory.lend(|memory| home.with_reader_io(|scope| {
                    let mut source_native = scope.card.port(scope.io);
                    let mut receipt_native = scope.card.port(scope.io);
                    let mut pages_native = scope.card.port(scope.io);
                    let cancel = boot::NoCancel;
                    let yield_now = || {
                        cadence.service(clock.millis(), || crate::app_power::delay_ms(1),
                            || clock.millis());
                    };
                    let zone = scope.clock.zone();
                    let ports = super::drive::Ports { io: scope.io, panel: scope.bus.panel,
                        card: scope.card, preferences: scope.preferences, nvs: scope.nvs, facts: scope.facts };
                    let Some(context) = context.as_mut() else {
                        let Ok(filename) = CStr::from_bytes_until_nul(&selection.identity().filename) else { return Err(Error::Io); };
                        let Ok(filename) = filename.to_str() else { return Err(Error::Io); };
                        let mut key = [0; 64];
                        let mut hash = crate::reader_hash::PackageHasher::from_live_sdk();
                        let Some(key) = reader_book::route_key(filename, &mut hash, &mut key) else { return Err(Error::Io); };
                        let route_state = RouteState::new(reader_book::CACHE_ROOT, key);
                        let route = route_state.lend(&cancel, millis);
                        let mut pages_state = ChapterCacheState::new();
                        let result = {
                            let mut pages = ReaderPages(ChapterCacheStorage::lend(&mut pages_state,
                                &mut pages_native, &route).map_err(|_| Error::Io)?);
                            source.with_source(&mut source_native, Some(&cancel), |raw| {
                                let mut authorize = |raw: &mut dyn newt_core::zip_window::source::BookSource,
                                    expected: &[u8; 32], _scratch: &mut [u8], allowed: &dyn Fn() -> bool| {
                                    if !allowed() { return false; }
                                    let mut digest = [0; 32];
                                    // SAFETY: the same Home SDK and selected FD
                                    // remain live throughout this required capability.
                                    (unsafe { crate::reader_hash::hash_source(raw, &mut digest,
                                        &mut || yield_now(), None) }) && allowed() && digest == *expected
                                };
                                let strict = StrictSource::for_live_reader(raw, selection,
                                    &memory.runtime.owner, &mut memory.runtime.reader).ok_or(Error::Io)?;
                                let allowed = || !newt_native_storage::Authority::cancelled(&cancel);
                                let mut strict = strict.with_reopen_authorizer(&mut authorize, &allowed);
                                Ok(super::drive::RunLoan { memory, ui: reader_ui, checkpoint, statistics, recovery,
                                    accepted: &mut accepted, metadata: exit_metadata, flags: None, settings }
                                    .finish(ports, &mut pages, &mut strict, digest, zone, now, sleeping, trace))
                            }).map_err(|_| Error::Io)?
                        };
                        let closed = pages_state.close(&mut pages_native, &route);
                        return if closed.is_ok_and(|outcome| outcome.clean) { result } else { Err(Error::Io) };
                    };
                    let context = &mut **context;
                    let session::Session { route, receipt, pages, whole, block, scratch,
                        source: state, .. } = context;
                    let route = route.lend(&cancel, millis);
                    let mut receipt = SdRevisionStorage::lend(receipt, &mut receipt_native, &route)
                        .map_err(|_| Error::Io)?;
                    let mut pages = ReaderPages(ChapterCacheStorage::lend(pages,
                        &mut pages_native, &route).map_err(|_| Error::Io)?);
                    let mut whole = whole.borrow_with_yield(&yield_now);
                    let mut block = block.borrow_with_yield(&yield_now);
                    source.with_source(&mut source_native, Some(&cancel), |source| {
                        session::with_source(state, RevisionPorts { source: Some(source),
                            store: &mut receipt, whole: &mut whole, block: Some(&mut block),
                            scratch }, |revision| {
                            super::drive::RunLoan { memory, ui: reader_ui, checkpoint, statistics, recovery,
                                accepted: &mut accepted, metadata: exit_metadata, flags: None, settings }
                                .finish(ports, &mut pages, revision, digest, zone, now, sleeping, trace)
                        }).map_err(|_| Error::Io)
                    }).map_err(|_| Error::Io)?
                }).and_then(|result| result))
            }
            _ => Err(Error::Io),
        };
        let result = result.map(|_| {
            if self.checkpoint.result().progress == reader_book::checkpoint::Outcome::Verified {
                if let Some(acknowledged) = self.checkpoint.acknowledged_for(&digest) { *saved = acknowledged; }
            }
        });
        // The exit fold may commit the finish (#319); Home and the sleep
        // screen are composed after this, so follow the row again here.
        self.home.set_finished(self.reader_ui.finish != newt_reader_ui::ReaderFinishRow::Mark);
        if accepted { *self.selection.flags_mut() |= flag::ACCEPTED_OPEN_OWED; }
        else { *self.selection.flags_mut() &= !flag::ACCEPTED_OPEN_OWED; }
        if result.is_err() { self.refuse_checkpoint_image(now, false); }
        else {
            let result_value = self.checkpoint.result();
            self.saves.checkpoint_to(sleeping, ReadingCommitResult {
                progress: outcome(result_value.progress), destination: outcome(result_value.destination),
            });
        }
        result
    }
}
pub(super) fn outcome(value: reader_book::checkpoint::Outcome) -> SaveOutcome {
    match value {
        reader_book::checkpoint::Outcome::Unchanged => SaveOutcome::Unchanged,
        reader_book::checkpoint::Outcome::Verified => SaveOutcome::Verified,
        reader_book::checkpoint::Outcome::Unconfirmed => SaveOutcome::Unconfirmed,
    }
}
