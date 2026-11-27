// SPDX-License-Identifier: MIT
//! App work dispatch over the one persistent selected source and reader.
use super::*;
use crate::{app_image::{Application, Events, Home}, reader_book::{BookExit, BookStep},
    rest_app::Work};
use newt_core::{app::Screen, frame::Frame, selection_authority::flag,
    save_notice::{ReaderIssue, ReadingCommitResult, ResumeIssue}};
use newt_reader_ui::{ReaderPanel, ReaderUiNotice};

/// A replacement must not consume the original pending checkpoint.
const RECOVERY_PENDING: &[u8] = b"Position kept. Use Read on Home to resume.";

impl Runtime {
    pub(super) fn work_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        work: Work, logical: &mut Frame, events: &mut Events<'_, '_>,
    ) -> bool {
        if matches!(work, Work::OpenSelection(_) | Work::OpenSample) && self.recovery.is_some() {
            ui.library.message = Some(RECOVERY_PENDING);
            return true;
        }
        match work {
            Work::None => { self.publish_exit_metadata(home); false }
            Work::EnterLibrary => {
                // Frozen openLibrary: browse, scan, then note the card.
                if self.close_runtime(home).is_err() { crate::app_power::stop(); }
                if ui.scan_library(home, self, logical, events.control, events.action).is_err() { crate::app_power::stop(); }
                self.note_books_image(home, ui, events.action);
                if self.recovery.is_some() { ui.library.message = Some(RECOVERY_PENDING); }
                true
            }
            Work::OpenSelection(index) => {
                let Events { queue, control, task, action: trace } = events;
                control.with_open(millis(), |control, _owned| {
                    let mut events = Events { queue, control, action: *trace, task: task.reborrow() };
                    self.open_candidate_image(home, ui, logical, &mut events, index)
                })
            }
            Work::OpenReader => self.open_selected_image(home, ui, logical, events, false),
            Work::ReaderAction(action) => self.reader_action_image(home, ui, logical, events, action),
            Work::PrepareSleep => {
                // W1 quiesces input and paints the live scene before the
                // required finish_sleep callback performs exit effects.
                true
            }
            Work::OpenSample => {
                if self.close_runtime(home).is_err() { crate::app_power::stop(); }
                self.index_trim_owed = false;
                self.selection.reset(&mut || {}, &mut || self.title.fill(0));
                *self.selection.flags_mut() |= flag::SAMPLE_CHOSEN;
                ui.catalog = None;
                ui.app.screen = Screen::EpubReading;
                self.open_selected_image(home, ui, logical, events, false)
            }
            Work::QuickStartHome | Work::QuickStartSettings | Work::QuickStartOpeningUnconfirmed => false,
            Work::EnterApps => ui.enter_apps(home),
            Work::InstallApp => ui.install_app(home),
        }
    }

    pub(super) fn observe_operation(&mut self, exit: &BookExit) {
        self.saves.set_reader_preference_unavailable(exit.preferences_unavailable);
        self.saves.set_fold_owed(exit.statistics.lifecycle.fold_owed);
        self.saves.checkpoint(ReadingCommitResult {
            progress: super::exit::outcome(exit.statistics.checkpoint.progress),
            destination: super::exit::outcome(exit.statistics.checkpoint.destination),
        });
    }

    /// Keep book-switch rollback locals out of the shared work dispatch frame.
    #[inline(never)]
    fn open_candidate_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut Frame, events: &mut Events<'_, '_>, index: usize,
    ) -> bool {
        let _action = events.action.scope(crate::reader_action::ActionStage::SourceSelect);
        if self.selection.flags() & flag::TITLE_DEFERRED != 0 { return false; }
        let mut name = [0; newt_core::library_name::FILENAME_BYTES];
        let found = home.with_storage_io(|_, io| {
            let Some(catalog) = ui.catalog.as_mut() else { return false; };
            let mut files = newt_native_io::LibraryFiles::new(io);
            let Some(filename) = catalog.filename(&mut files, index) else { return false; };
            if filename.len() >= name.len() { return false; }
            name[..filename.len()].copy_from_slice(filename);
            true
        }).unwrap_or(false);
        if !found { ui.library.message = Some(b"This book could not be read."); return false; }
        let Ok(filename) = CStr::from_bytes_until_nul(&name) else { return false; };
        // Frozen openSdSelection: a damaged wake destination refuses the
        // switch before anything closes.
        let authority = self.with_store(home, |_, store|
            newt_core::sleep_intent::SleepResumeStore::new(store).load()).unwrap_or(Load::Failed);
        let ready = matches!(authority, Load::Loaded(_) | Load::Empty);
        self.saves.set_resume(if ready { ResumeIssue::None } else { ResumeIssue::Unavailable });
        self.publish_notice_image(ui);
        if !ready {
            ui.library.message = ResumeIssue::Unavailable.text().map(CStr::to_bytes);
            return false;
        }
        // The frozen openBook transaction protects the accepted selection and
        // its Home projection until a real replacement reader publishes a page.
        let previous_identity = *self.selection.identity();
        let previous_flags = self.selection.flags();
        let previous_saved = self.saved;
        let previous_home = core::mem::take(&mut *self.home);
        let previous_notice = self.saves;
        let previous_ready = self.progress_ready;
        let previous_settings = self.settings;
        let previous_ui = self.reader_ui;
        let previous_trim = self.index_trim_owed;
        let previous_recovery = self.recovery;
        // Only this attempt's reader issue may name why it failed.
        self.saves.set_reader(ReaderIssue::None);
        *self.selection.flags_mut() |= flag::TITLE_DEFERRED;
        if self.close_runtime(home).is_err() { crate::app_power::stop(); }
        self.selection.begin_selection();
        let work = if self.receipt_present_image(home, filename) { newt_reader_ui::SlowWork::Opening }
            else { newt_reader_ui::SlowWork::FirstOpen };
        self.acknowledge_and_restart_image(home, ui, logical, work, events.control, events.action);
        super::profile::with_profile(|profile| {
            let selected = self.prepare_image(home, ui, logical, events, filename, None, profile).is_ok_and(|digest| {
                self.selection.publish_candidate(filename, digest, &mut || {}, &mut || {})
            });
            if selected { ui.catalog = None; self.index_trim_owed = false; }
            let loaded = selected && self.with_store(home, |this, store| {
                let loaded = this.load_progress(store, events.action);
                matches!(loaded, Load::Loaded(()) | Load::Empty)
            }).unwrap_or(false);
            self.recovery = None;
            let prepared = selected.then(|| *self.selection.digest());
            let opened = loaded && self.open_selected_profile(home, ui, logical, events, profile, prepared, false)
                && self.book().is_some_and(|book| book.owner.state.opened());
            if !opened {
                // Frozen openBook's message, taken before the rollback restores
                // the previous notice; a cancel or timeout names itself.
                let reason: &'static [u8] = if events.control.cancelled() {
                    if events.control.timed_out() { b"Opening timed out. Position kept." }
                    else { b"Opening canceled." }
                } else if !selected { b"This book could not be read." }
                else if !loaded { b"Saved position needs attention. It is kept." }
                else { ReaderIssue::text_raw(self.saves.reader())
                    .map_or(b"This book could not be opened.", CStr::to_bytes) };
                if self.close_runtime(home).is_err() { crate::app_power::stop(); }
                self.index_trim_owed = false;
                self.selection.reset(&mut || {}, &mut || {});
                // select released the catalog before reader admission. A
                // rejected transaction restores browsing through its real scan.
                if ui.scan_library(home, self, logical, events.control, events.action).is_err() { crate::app_power::stop(); }
                if previous_flags & flag::ACTIVE != 0 {
                    self.selection.restore_identity(&previous_identity, &mut || {}, &mut || {});
                }
                let rollback = flag::ACTIVE | flag::SAVED_PRESENT | flag::UNAVAILABLE
                    | flag::SAVE_FAILED | flag::UNCONFIRMED | flag::COMMIT_OWED
                    | flag::ACCEPTED_OPEN_OWED | flag::CHANGED_OPEN;
                *self.selection.flags_mut() = (self.selection.flags() & !(rollback | flag::TITLE_DEFERRED))
                    | (previous_flags & rollback);
                self.saved = previous_saved;
                *self.home = previous_home;
                self.saves = previous_notice;
                self.publish_notice_image(ui);
                self.progress_ready = previous_ready;
                self.settings = previous_settings;
                self.recovery = previous_recovery;
                self.reader_ui = previous_ui;
                self.index_trim_owed = previous_trim;
                if let Some(memory) = self.memory.external_mut() {
                    memory.lend(|memory| {
                        let book = &mut *memory.runtime;
                        book.owner.settings_committed(&mut book.reader, previous_settings);
                    });
                }
                ui.library.message = Some(reason);
                return false;
            }
            *self.selection.flags_mut() &= !flag::TITLE_DEFERRED;
            *self.selection.flags_mut() |= flag::COMMIT_OWED | flag::ACCEPTED_OPEN_OWED;
            self.update_home_image(home, events.action);
            if self.session.as_ref().is_some_and(|session| session.verification.state()
                == crate::reader_verification::Verification::Pending) {
                *self.selection.flags_mut() |= flag::UNCONFIRMED;
            }
            let committed = self.commit_verified_image(home, events.action);
            if committed {
                match self.operate_image(home, ui, logical, events, None, Some(BookStep::Save), false, None, None) {
                    Ok(exit) => self.observe_operation(&exit),
                    Err(error) => self.open_failed(error),
                }
            }
            self.selection.publish_selection_notice(&mut self.saves);
            self.reader_ui.panel = ReaderPanel::Page;
            self.reader_ui.selected = 0;
            self.reader_ui.notice = ReaderUiNotice::None;
            ui.catalog = None;
            ui.app.screen = Screen::EpubReading;
            // Action parsing may borrow the published logical frame; its next
            // use is a full composition in paint_image.
            let _ = logical;
            true
        })
    }

    pub(super) fn idle_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut Frame, events: &mut Events<'_, '_>,
        delay: &mut u32,
    ) -> bool {
        self.publish_exit_metadata(home);
        if ui.app.screen != Screen::EpubReading {
            if let Some(repaint) = self.service_home_reading_stats_image(home, ui, events) {
                return repaint;
            }
            return self.service_covers_image(home, ui, logical, events);
        }
        if ui.app.screen == Screen::EpubReading && !events.queue.pending()
            && {
                #[cfg(feature = "dev-usb")]
                { home.bus.log.available() == 0 }
                #[cfg(not(feature = "dev-usb"))]
                { true }
            }
            && self.reopen_owed() {
            events.action.begin();
            events.action.boundary(crate::reader_action::ActionStage::MarkOpen as u8);
            return self.open_selected_image(home, ui, logical, events, false);
        }
        if ui.app.screen != Screen::EpubReading || !self.opened() { return false; }
        let now = millis();
        if !self.idle.reading_ready(ui.app.screen, ui.displayed.generation, now) { return false; }
        if events.queue.pending() { return false; }
        #[cfg(feature = "dev-usb")]
        if home.bus.log.available() > 0 || !ui.bridge.background_work_ready() { return false; }
        #[cfg(feature = "embedded-sample")]
        if self.memory.has_sample() {
            if now.wrapping_sub(self.last_progress.get()) < 30000 { return false; }
            let before = (ui.info.storage(), ui.info.page_save_unconfirmed());
            self.save_sample_image(home, now);
            self.publish_notice_image(ui);
            return before != (ui.info.storage(), ui.info.page_save_unconfirmed());
        }
        if self.session.as_ref().is_some_and(|session| session.verification.state()
            == crate::reader_verification::Verification::Pending) {
            return self.verify_idle_image(home, ui, logical, events, delay);
        }
        let due = now.wrapping_sub(self.last_progress.get()) >= 30000;
        let mut admitted = false;
        if due {
            let mut deferred = false;
            let pending = || events.queue.pending() || {
                #[cfg(feature = "dev-usb")]
                { !ui.bridge.background_work_ready() }
                #[cfg(not(feature = "dev-usb"))]
                { false }
            };
            if !self.commit_verified_admitted_image(home, events.action, Some(&pending), &mut deferred, &mut admitted) {
                // Failed preparation gets the normal retry period; input did
                // not attempt storage and leaves that deadline unchanged.
                if deferred { return false; }
                let at = millis();
                let before = (ui.info.storage(), ui.info.page_save_unconfirmed());
                self.refuse_checkpoint_image(at, self.selection.flags() & flag::ACCEPTED_OPEN_OWED != 0);
                self.last_progress.set(at);
                self.publish_notice_image(ui);
                return before != (ui.info.storage(), ui.info.page_save_unconfirmed());
            }
        }
        if !due && self.service_metadata_image(home, ui, events) { return false; }
        let before = (self.book().map(|book| book.ui),
            ui.info.storage(), ui.info.page_save_unconfirmed(), ui.info.battery_known, ui.info.battery_percent);
        match self.operate_image(home, ui, logical, events, None,
            Some(BookStep::Idle { checkpoint: due, admitted }), false, None, None) {
            Ok(exit) => {
                if let Some(at) = exit.idle_quiet_from { self.idle.note_input(at); }
                self.observe_operation(&exit);
                self.publish_notice_image(ui);
                self.update_home_image(home, events.action);
                exit.intent == Some(newt_reader_ui::ReaderUiIntent::Repaint) || before != (self.book().map(|book| book.ui),
                    ui.info.storage(), ui.info.page_save_unconfirmed(), ui.info.battery_known, ui.info.battery_percent)
            }
            Err(error) => { self.open_failed(error); self.publish_notice_image(ui); true }
        }
    }

    /// Frozen noteBooks, after each Books scan. A card the scan could not list
    /// and the probe finds gone gets the no-card page, whose Open reads the
    /// sample, and Home calls it missing. A card back, with no recovery
    /// pending, restores its selection and Home reloads (loadReaderProgress).
    pub(super) fn note_books_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        trace: crate::reader_action::Trace<'_>,
    ) {
        use newt_core::selection_authority::CardObservation;
        use newt_native_storage::Native;
        let listed = ui.catalog.as_ref().is_some_and(|catalog|
            catalog.status() == newt_library_current::LibraryStatus::Ok);
        // scan_library's own failed mount already reads as no card (present()).
        let unmounted = ui.library.no_card;
        let generation = &mut self.cover_generation;
        let mut card = CardObservation {
            reading_state_unavailable: &mut |_| false,
            card_absent: &mut |_| unmounted || home.bus.board.with_optional_sd_io(|_, io| matches!(
                io.port().stat("/newt-sd/Newt/Books", false), Err(newt_native_storage::Error::Unavailable))),
            count_denied_if_session: &mut |_| {},
            home_cover_changed: &mut |_| *generation = generation.wrapping_add(1),
        };
        ui.library.no_card = !listed && self.selection.card_absent(&mut (), &mut card);
        if ui.library.no_card {
            *self.selection.flags_mut() |= flag::CARD_MISSING;
            if self.selection.active() { *self.selection.flags_mut() |= flag::UNCONFIRMED; }
            return;
        }
        if self.selection.flags() & flag::CARD_MISSING == 0 || self.recovery.is_some()
            || !self.resume_card_image(home) { return; }
        if self.selection.active() || self.selection.selection_blocked() {
            let loaded = self.with_store(home, |this, store| this.load_progress(store, trace))
                .unwrap_or(Load::Failed);
            self.saves.set_progress_unavailable(!self.progress_ready);
            let saved = self.saved;
            self.load_home_records(home, &saved, loaded == Load::Loaded(()));
        } else {
            #[cfg(feature = "embedded-sample")]
            self.load_sample_home(home);
        }
    }
}
