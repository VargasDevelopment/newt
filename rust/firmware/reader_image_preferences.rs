// SPDX-License-Identifier: MIT
//! Frozen main's settings transaction runs after every physical reader loan.
use super::*;
use crate::{app_image::{Application, Events, Home}, reader_book::{self, BookStep, PendingRecovery}};
use newt_core::{frame::Frame, zip_container::Status};
use newt_reader_ui::{ReaderPanel, ReaderUiNotice, SlowWork};

impl Runtime {
    pub(super) fn save_reader_settings_image(&mut self, home: &mut Home<'_>,
        ui: &mut Application<'_>, logical: &mut Frame, events: &mut Events<'_, '_>, at: u32,
    ) {
        self.acknowledge_work_image(home, ui, logical, SlowWork::Applying, events.control, events.action);
        if self.recovery.is_some() && !self.opened() {
            self.open_selected_image(home, ui, logical, events, false);
            if self.opened() && self.saves.unconfirmed() {
                self.checkpoint_settings_image(home, ui, logical, events, BookStep::Save);
            }
            if self.recovery.is_some() || !self.opened() {
                self.reader_ui.notice = ReaderUiNotice::SaveFailed;
                return;
            }
            #[cfg(feature = "embedded-sample")]
            if let Some(memory) = self.memory.sample_mut() {
                memory.with_tracker(|tracker| tracker.pause(at));
            }
        if let Some(memory) = self.memory.external_mut() {
                memory.lend(|loan| loan.runtime.owner.pause(at));
            }
        }
        let proposed = self.reader_ui.draft;
        if !self.reader_ui.preferences_writable || !home.preferences.save(&mut home.nvs, proposed) {
            self.saves.set_reader_preference_unavailable(true);
            self.reader_ui.notice = ReaderUiNotice::SaveFailed;
            return;
        }
        self.saves.set_reader_preference_unavailable(false);
        let session_checkpoint = self.has_progress();
        if session_checkpoint {
            let saved = self.checkpoint_settings_image(home, ui, logical, events, BookStep::ReflowCheckpoint);
            self.saves.set_checkpoint_saved(saved);
            // armCheckpoint uses the acknowledgement only after a complete
            // save; otherwise the still-live candidate owns retry geometry.
            if let Some(reading) = self.reading() {
                let candidate = *reading.tracker.snapshot(at);
                self.recovery = Some(PendingRecovery::for_reflow(candidate, self.saved, proposed, saved));
            }
        }
        // setSettings closes the reader and its session before another
        // allocation can start. The selected physical FD remains selected.
        if self.close_for_reflow_image(home).is_err() { crate::app_power::stop(); }
        self.settings = proposed;
        self.reader_ui.panel = ReaderPanel::Page;
        self.reader_ui.selected = 0;
        self.reader_ui.notice = ReaderUiNotice::None;
        self.open_selected_image(home, ui, logical, events, session_checkpoint);
    }

    fn checkpoint_settings_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut Frame, events: &mut Events<'_, '_>, step: BookStep,
    ) -> bool {
        #[cfg(feature = "embedded-sample")]
        if self.memory.has_sample() { return self.save_sample_image(home, millis()); }
        // saveReaderProgress finishes actual deferred verification before
        // lending Pages/State. A failed barrier cannot acknowledge a checkpoint.
        if !self.finish_verification_image(home, ui, logical, events, false) {
            self.refuse_checkpoint_image(millis(), false);
            return false;
        }
        match self.operate_image(home, ui, logical, events, None, Some(step), false, None, None) {
            Ok(exit) => self.observe_operation(&exit),
            Err(error) => { self.open_failed(error); return false; },
        }
        self.checkpoint.result().progress == reader_book::checkpoint::Outcome::Verified
            && self.checkpoint.result().destination != reader_book::checkpoint::Outcome::Unconfirmed
    }

    fn close_for_reflow_image(&mut self, home: &mut Home<'_>) -> Result<(), newt_c3_board::Error> {
        use crate::home_startup::CardPort;
        if let Some(memory) = self.memory.external_mut() {
            memory.lend(|loan| {
                let book = &mut *loan.runtime;
                book.owner.close(&mut book.reader);
            });
        }
        self.memory = ReaderBacking::Closed;
        self.standalone_status = Status::NotFound;
        self.restore_failure = newt_reader_ui::RestoreFailure::None;
        if let Some(session) = self.session.as_mut() {
            home.with_storage_io(|home, io| {
                let mut native = home.card.port(io);
                if matches!(session.close(&mut native, &boot::NoCancel, millis), Ok(true)) {
                    Ok(())
                } else { Err(newt_c3_board::Error::Io) }
            })?
        } else { Ok(()) }
    }
}
