// SPDX-License-Identifier: MIT
//! Frozen exit, SleepSession preparation and deferred Home record publication.
use super::*;
use core::fmt::Write;
use crate::{app_image::{Application, Home},
    reader_book::checkpoint::{CheckpointResult, Outcome}, reader_exit_metadata::ExitMetadata,
    reader_pages::ReaderPages, reader_restore_policy::pending_recovery,
    retained_runtime::record::{self, Recovery, SleepRecord, Status}};
use newt_core::{app::Screen, durable::Backend, save_notice::SaveOutcome,
    sleep_intent::{SleepResumeIntent, progress_checkpoint}};
use newt_native_io::UiIo;
use newt_native_storage::{chapter_cache::{ChapterCacheState, ChapterCacheStorage}, RouteState};

impl Runtime {
    /// Only the frozen non-Sleep exit calls this before its verified checkpoint.
    pub(super) fn defer_exit_metadata(&mut self) {
        if self.selection.active() && self.exit_metadata.is_none() {
            self.exit_metadata = Fixed::try_new_with(ExitMetadata::new);
        }
    }

    pub(crate) fn save_exit(&mut self, home: &mut Home<'_>, now: u32, trace: crate::reader_action::Trace<'_>) -> SaveOutcome {
        if self.has_progress() {
            trace.boundary(crate::reader_action::ActionStage::MarkClose as u8);
            let _exit = trace.scope(crate::reader_action::ActionStage::READER_EXIT);
            self.defer_exit_metadata();
            // The physical seam runs the existing checkpoint/lease/fold bodies
            // and records real IO/loan refusal in checkpoint and save notice.
            let _ = self.finish_exit_image(home, now, false, trace);
        }
        self.saves.set_fold_owed(self.statistics.fold_owed);
        // Firmware switch has no later Home paint to publish this capture.
        self.publish_exit_metadata(home);
        self.saves.exit_saved()
    }

    /// Called after the reader action's actual exit/save/fold and Sleeping
    /// paint, once Root armed the physical wake source. Do not checkpoint twice.
    pub(crate) fn prepare_sleep(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut newt_core::frame::Frame, events: &mut crate::app_image::Events<'_, '_>, _now: u32,
    ) -> SleepRecord {
        let shown = ui.info.storage();
        let has_progress = self.has_progress();
        let opened = self.opened();
        let pending = self.recovery.as_ref().filter(|pending| pending.progress().book != 0);
        let checkpoint_saved = has_progress && self.saves.checkpoint_saved();
        let book_open = ui.app.resume_screen == Screen::EpubReading
            && (opened || pending.is_some() || (checkpoint_saved && has_progress));
        ui.app.resume_screen = if book_open { Screen::EpubReading } else { Screen::Home };
        if !book_open { ui.app.selected = 0; }
        let (progress, settings, saved, recovery, pending_attempted) = if let Some(pending) = pending {
            (Some(pending.progress()), pending.settings(), self.saves.checkpoint_saved(),
                pending_recovery(pending.policy()), Some(pending.attempted_sequence()))
        } else {
            (book_open.then_some(&self.saved), self.settings, checkpoint_saved, Recovery::Ordinary, None)
        };
        let mut retained = record::make_record(&ui.app, progress, settings, saved, recovery,
            self.selection.active());
        if !saved && let Some(progress) = progress {
            let mut attempted = self.checkpoint.attempted_sequence_for(progress);
            if let Some(pending_attempted) = pending_attempted
                && pending_attempted.wrapping_sub(attempted) < 0x8000_0000
            {
                attempted = pending_attempted;
            }
            retained.stamp_attempted(attempted);
        }
        retained.recovery |= self.saves.retained();
        retained.checksum = retained.checksum();

        // Like selection.retain: an inactive source clears ONLY its old magic.
        let sd = if self.selection.active() {
            home.write_retained_sd_identity(self.selection.identity())
        } else {
            home.bus.board.with_retained(|mut words| words.clear_sd_resume_magic())
        };
        if let Err(error) = sd { let _ = writeln!(home.bus.log, "Newt SD resume record error: {:?}", error); }

        if self.selection.active() || self.selection.selection_blocked() {
            let verified = self.selection.selection_verified(&mut || self.session.as_ref()
                .is_none_or(|session| session.verification.state()
                    == crate::reader_verification::Verification::Verified));
            let destination = if authorized_record(&retained, verified) {
                self.with_store(home, |_, store| checkpoint_destination(store, &retained))
                    .unwrap_or(SaveOutcome::Unconfirmed)
            } else { SaveOutcome::Unchanged };
            self.observe_destination(retained.resume_screen == Screen::EpubReading as u32, destination);
        }
        retained.recovery = (retained.recovery & !12) | self.saves.retained();
        retained.checksum = retained.checksum();
        self.saves.set_fold_owed(self.statistics.fold_owed);
        let notice = self.saves.publish();
        ui.info = core::mem::take(&mut ui.info).with_storage_notice(
            notice.storage.map(|text| text.to_bytes()), notice.page_save_unconfirmed);
        // Frozen enterSleep: a warning the destination commit raised repaints
        // the sleep image while the reader is still open, before close.
        if shown != ui.info.storage() {
            let _ = self.paint_image(home, ui, logical, events, true);
        }
        let expected = if self.selection.active() { self.selection.book_id() } else { self.unselected_book_id() };
        let status = record::validate(&retained, expected, self.selection.active());
        let _ = writeln!(home.bus.log,
            "Newt sleep request: resume={}, record={}, progress-ready={}, storage-warning={}",
            retained.resume_screen, status.name(), u8::from(self.progress_ready),
            u8::from(ui.info.storage().is_some()));
        // Snapshot, retain and diagnostic precede actual reader/session/source
        // closure. Root owns the later record write, panel, rails and sleep.
        if let Err(error) = self.close_runtime(home) {
            let _ = writeln!(home.bus.log, "Newt reader close error: {:?}", error);
            crate::app_power::stop();
        }
        retained
    }

    pub(crate) fn cancel_sleep(&mut self, home: &mut Home<'_>, now: u32) {
        // Frozen cancel clears only the two magics. Warning/retry state stays
        // live; W1 wakes the app route before resuming this same tracker.
        if let Err(error) = home.consume_retained_boot() {
            let _ = writeln!(home.bus.log, "Newt sleep cancel record error: {:?}", error);
        }
        #[cfg(feature = "embedded-sample")]
        if let Some(memory) = self.memory.sample_mut() {
            let view = memory.view();
            let owner = newt_reader_ui::ReaderOwnerState::from_open(true, self.standalone_status, view.spines);
            if owner.opened() {
                let shown = owner.position_info(view.reader.info(), view.spines).shown;
                memory.with_tracker(|tracker| tracker.enter(now, shown));
                if !self.saves.unconfirmed() { self.recovery = None; }
            }
        }
        if let Some(memory) = self.memory.external_mut() {
            memory.lend(|memory| {
                let book = &mut *memory.runtime;
                book.owner.resume(&book.reader, now);
                if book.owner.state.opened() && !self.saves.unconfirmed() { self.recovery = None; }
            });
        }
    }

    fn observe_destination(&mut self, reading: bool, destination: SaveOutcome) {
        let previous = self.checkpoint.result();
        self.checkpoint.observe(CheckpointResult { progress: previous.progress,
            destination: match destination {
                SaveOutcome::Unchanged => Outcome::Unchanged,
                SaveOutcome::Verified => Outcome::Verified,
                SaveOutcome::Unconfirmed => Outcome::Unconfirmed,
            } });
        self.saves.destination(reading, destination);
        if destination == SaveOutcome::Verified && self.saves.checkpoint_saved()
            && !self.saves.unconfirmed() { self.recovery = None; }
    }

    /// Move/drop the optional capture even on mismatch, mount or write failure.
    pub(crate) fn publish_exit_metadata(&mut self, home: &mut Home<'_>) {
        let Some(capture) = self.exit_metadata.take() else { return; };
        if !capture.identified || !self.selection.active()
            || capture.digest != *self.selection.digest() { return; }
        let Ok(filename) = CStr::from_bytes_until_nul(&self.selection.identity().filename) else { return; };
        let Ok(filename) = filename.to_str() else { return; };
        let _ = home.with_storage_io(|_, io| {
            with_ownerless_pages(io, filename, |pages| {
                crate::reader_metadata::save_checkpoint(pages, &capture.digest, capture.locator,
                    capture.section_at(capture.locator), &capture.peers,
                    |at| capture.section_at(at), &|| false)
            });
            if capture.estimated {
                with_ownerless_pages(io, filename, |pages| {
                    let saved = crate::reader_metadata::estimate::save(
                        pages, &capture.digest, capture.estimate, &|| false);
                    pages.0.access().close();
                    saved
                });
            }
        });
    }
}

fn authorized_record(record: &SleepRecord, selection_verified: bool) -> bool {
    if record.resume_screen != Screen::EpubReading as u32 { return true; }
    record.checkpoint_saved != 0 && selection_verified && record.rejected_sleep == 0
        && record::validate(record, record::join_words(record.book_low, record.book_high), true) == Status::Valid
}
fn checkpoint_destination<B: Backend>(store: &mut B, record: &SleepRecord) -> SaveOutcome {
    let intent = if record.resume_screen == Screen::EpubReading as u32 {
        let progress = record.progress();
        SleepResumeIntent { book: progress.book, checkpoint: progress_checkpoint(&progress),
            settings: record.reader_settings | if record.recovery_kind() == Some(Recovery::PendingAnchor) { 8 } else { 0 } }
    } else { SleepResumeIntent::default() };
    // Reuse the existing durable-save/readback body, with the original single
    // bounded retry of the SAME intended value. Unsynced SD readback cannot ack.
    if ReaderCheckpoint::store_destination(store, intent) || ReaderCheckpoint::store_destination(store, intent) {
        SaveOutcome::Verified
    } else { SaveOutcome::Unconfirmed }
}

pub(super) fn with_ownerless_pages<R>(io: &UiIo, filename: &str,
    work: impl FnOnce(&mut ReaderPages<'_, newt_native_io::OsNative<'_>>) -> R,
) -> Option<R> {
    let mut work = Some(work);
    let mut result = None;
    ownerless_pages(io, filename, &mut |pages| result = work.take().map(|work| work(pages)));
    result
}

// One copy of the route and loan for every caller (#317 flash budget).
#[inline(never)]
fn ownerless_pages(io: &UiIo, filename: &str,
    work: &mut dyn FnMut(&mut ReaderPages<'_, newt_native_io::OsNative<'_>>),
) {
    let mut key = [0; 64];
    let mut hash = crate::reader_hash::PackageHasher::from_live_sdk();
    let Some(key) = crate::reader_book::route_key(filename, &mut hash, &mut key) else { return; };
    let route_state = RouteState::new(crate::reader_book::CACHE_ROOT, key);
    // Frozen ownerless publication passes empty cancellation and stop hooks.
    let route = route_state.lend(&boot::NoCancel, millis);
    if !route.allowed() { return; }
    let mut state = ChapterCacheState::new();
    let mut native = io.port();
    if let Ok(storage) = ChapterCacheStorage::lend(&mut state, &mut native, &route) {
        work(&mut ReaderPages(storage));
    }
    // The actual outer destructor boundary, also attempted after a failed loan.
    // Identity closes before freeing scratch; scratch refusal owes only this.
    let _ = state.close(&mut native, &route);
}
