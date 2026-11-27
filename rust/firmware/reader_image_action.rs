// SPDX-License-Identifier: MIT
//! One accepted reader action, its bounded source barrier and checked exit.
use super::*;
use crate::{app_image::{Application, Events, Home}, reader_book::{BookExit, BookStep}};
use newt_core::{app::{Action, Screen}, frame::Frame};
use newt_reader_ui::{ReaderPanel, ReaderUiIntent, ReaderUiNotice};

impl Runtime {
    /// Keep reader-action locals out of the shared work dispatch frame.
    #[inline(never)]
    pub(super) fn reader_action_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut Frame, events: &mut Events<'_, '_>, action: Action,
    ) -> bool {
        let Events { queue, control, task, action: trace } = events;
        control.with_open(millis(), |control, _owned| {
            let mut events = Events { queue, control, action: *trace, task: task.reborrow() };
            self.reader_action_armed(home, ui, logical, &mut events, action)
        })
    }

    /// Activating a Contents entry with a fragment must find its anchor by laying
    /// the chapter out (#188), unlike a page turn or a chapter-start entry.
    pub(super) fn starts_anchor_jump(&self, action: Action) -> bool {
        action == Action::Activate && self.book().is_some_and(|book| {
            let menu = book.reader.chapter_menu();
            let count = menu.info().count;
            book.ui.panel == ReaderPanel::Chapters && count > 0
                && menu.row(book.ui.selected.min(count - 1)).fragment
        })
    }

    fn reader_action_armed(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut Frame, events: &mut Events<'_, '_>, action: Action,
    ) -> bool {
        let before = self.book().map(|book| book.reader.info());
        #[cfg(feature = "embedded-sample")]
        let sample_result = self.memory.has_sample().then(|| self.sample_action_image(home, ui, logical, action));
        #[cfg(not(feature = "embedded-sample"))]
        let sample_result: Option<(Option<ReaderUiIntent>, u32)> = None;
        let (intent, at) = if let Some(result) = sample_result {
            if result.0 == Some(ReaderUiIntent::None) { return false; }
            result
        } else if self.memory.is_closed() {
            // SampleReader has no storage after failed admission, but main's
            // real menu/draft and Back/Sleep intents still exist.
            let at = millis();
            (Some(newt_reader_ui::dispatch_unavailable_reader_ui(&mut self.reader_ui,
                self.settings, action)), at)
        } else {
            if self.starts_anchor_jump(action) {
                // The jump may lay a long chapter out twice; say so before it starts.
                self.acknowledge_work_image(home, ui, logical, newt_reader_ui::SlowWork::Loading,
                    events.control, events.action);
            }
            match self.operate_image(home, ui, logical, events, None,
                Some(BookStep::Action(action)), false, None, None) {
                Ok(mut exit) => {
                    self.finish_pending_turn_image(home, ui, logical, events, action, before, &mut exit);
                    self.finish_pending_mark_image(home, ui, logical, events, &mut exit);
                    self.observe_operation(&exit);
                    self.update_home_image(home, events.action);
                    let after = self.book().map(|book| book.reader.info());
                    if before.map(|p| (p.spine, p.page)) != after.map(|p| (p.spine, p.page)) {
                        self.reflow_anchor = None;
                    }
                    if before != after {
                        self.reading_refresh.page_changed();
                    }
                    (exit.intent, exit.action_at)
                }
                Err(error) => {
                    self.open_failed(error);
                    self.publish_notice_image(ui);
                    return true;
                }
            }
        };
        match intent {
            Some(ReaderUiIntent::SaveSettings) => {
                self.save_reader_settings_image(home, ui, logical, events, at);
            }
            Some(ReaderUiIntent::Home) => {
                self.reflow_anchor = None;
                events.action.boundary(crate::reader_action::ActionStage::MarkClose as u8);
                let _exit = events.action.scope(crate::reader_action::ActionStage::READER_EXIT);
                self.defer_exit_metadata();
                if self.session.as_ref().is_some_and(|s| s.verification.state()
                    == crate::reader_verification::Verification::Pending) {
                    self.acknowledge_work_image(home, ui, logical, newt_reader_ui::SlowWork::Saving, events.control, events.action);
                }
                if self.has_progress() {
                    if self.memory.has_sample() { self.update_home_image(home, events.action); }
                    self.leave_for_exit_image(at);
                    self.finish_verification_image(home, ui, logical, events, false);
                    let _ = self.finish_paused_exit_image(home, millis(), false, events.action);
                    self.save_picture_image(home, ui, logical, events);
                }
                self.publish_notice_image(ui);
                let _release = events.action.scope(crate::reader_action::ActionStage::READER_RELEASE);
                if let Some(memory) = self.memory.external_mut() {
                    memory.lend(|memory| {
                        let book = &mut *memory.runtime;
                        book.owner.close(&mut book.reader);
                    });
                }
                self.reader_ui.panel = ReaderPanel::Page;
                self.reader_ui.selected = 0;
                self.reader_ui.notice = ReaderUiNotice::None;
                self.memory = ReaderBacking::Closed;
                self.standalone_status = newt_core::zip_container::Status::NotFound;
                self.restore_failure = newt_reader_ui::RestoreFailure::None;
                drop(_release);
                ui.app.screen = Screen::Home; ui.app.selected = 0;
            }
            Some(ReaderUiIntent::Sleep) => {
                if self.memory.has_sample() { self.update_home_image(home, events.action); }
                ui.app.resume_screen = Screen::EpubReading;
                ui.app.screen = Screen::Sleeping;
            }
            _ => {}
        }
        self.publish_notice_image(ui);
        true
    }

    fn finish_pending_turn_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut Frame, events: &mut Events<'_, '_>, action: Action,
        before: Option<newt_phased_reader::PageInfo>, exit: &mut BookExit,
    ) {
        if exit.intent.is_some() || !matches!(action, Action::Previous | Action::Next)
            || !self.session.as_ref().is_some_and(|session| session.verification.state()
                == crate::reader_verification::Verification::Pending) { return; }
        let Some(book) = self.book() else { return; };
        if !book.owner.state.opened() || book.ui.panel != ReaderPanel::Page { return; }
        if before != Some(book.reader.info()) { return; }
        let page = book.owner.state.position_info(book.reader.info(), book.reader.selection().spines).page;
        let boundary = if action == Action::Next {
            page.page.wrapping_add(1) >= page.page_count
                && page.spine.wrapping_add(1) >= book.owner.state.readable_spines()
        } else { page.page == 0 && page.spine == 0 };
        if boundary { return; }
        {
            let _verification = events.action.scope(crate::reader_action::ActionStage::SourceSelect);
            self.acknowledge_work_image(home, ui, logical, newt_reader_ui::SlowWork::Loading, events.control, events.action);
            if !self.finish_verification_image(home, ui, logical, events, true) {
                self.publish_notice_image(ui);
                return;
            }
        }
        match self.operate_image(home, ui, logical, events, None, Some(BookStep::Action(action)), false, None, None) {
            Ok(retried) => *exit = retried,
            Err(error) => self.open_failed(error),
        }
    }

    fn finish_pending_mark_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut Frame, events: &mut Events<'_, '_>, exit: &mut BookExit,
    ) {
        use newt_core::selection_authority::flag;
        if exit.intent != Some(ReaderUiIntent::MarkFinished) { return; }
        let before_digest = *self.selection.digest();
        // Capture the requested finish direction before prerequisite saves run:
        // a BookStep::Save lease on the last page can auto-finish and flip ui.finish.
        let requested_mark = if self.reader_ui.finish == newt_reader_ui::ReaderFinishRow::Undo {
            newt_core::reading_stats::StatsMark::NotFinished
        } else {
            newt_core::reading_stats::StatsMark::Finished
        };
        let pending = self.session.as_ref().is_some_and(|session|
            session.verification.state() == crate::reader_verification::Verification::Pending);
        // Capture explicit commit debt before verification runs: committing a
        // selection restores an empty store as a synthetic zero acknowledgement
        // (or loads an older checkpoint) without saving live progress or NWBR.
        let selection_commit_owed = self.selection.flags() & flag::COMMIT_OWED != 0;
        let accepted_open_owed = self.selection.flags() & flag::ACCEPTED_OPEN_OWED != 0;
        let needs_verify = pending
            || selection_commit_owed
            || self.session.as_ref().is_some_and(|session|
                session.verification.state() != crate::reader_verification::Verification::Verified);
        if pending {
            self.acknowledge_work_image(home, ui, logical, newt_reader_ui::SlowWork::Saving,
                events.control, events.action);
        }
        if needs_verify && !self.finish_verification_core(home, ui, logical, events, false, false) {
            let still_pending = self.session.as_ref().is_some_and(|session|
                session.verification.state() == crate::reader_verification::Verification::Pending);
            if self.reader_ui.notice == ReaderUiNotice::BookChanged {
                self.reader_ui.panel = ReaderPanel::Page;
                self.reader_ui.selected = 0;
            } else {
                self.selection.publish_selection_notice(&mut self.saves);
                self.reader_ui.notice = if still_pending {
                    ReaderUiNotice::FinishOpening
                } else if self.selection.flags() & flag::SAVE_FAILED != 0 {
                    ReaderUiNotice::FinishUnconfirmed
                } else {
                    ReaderUiNotice::FinishSaveFailed
                };
            }
            exit.intent = Some(ReaderUiIntent::Repaint);
            return;
        }
        if *self.selection.digest() != before_digest
            || self.reader_ui.notice == ReaderUiNotice::BookChanged
            || !self.opened() {
            if self.reader_ui.notice == ReaderUiNotice::BookChanged {
                self.reader_ui.panel = ReaderPanel::Page;
                self.reader_ui.selected = 0;
            }
            exit.intent = Some(ReaderUiIntent::Repaint);
            return;
        }
        let progress_owed = selection_commit_owed
            || accepted_open_owed
            || self.selection.flags() & flag::ACCEPTED_OPEN_OWED != 0
            || self.checkpoint.acknowledged_for(&before_digest).is_none()
            || self.checkpoint.page_unconfirmed()
            || self.saves.page_unconfirmed();
        if progress_owed {
            match self.operate_image(home, ui, logical, events, None,
                Some(BookStep::Save), false, None, None) {
                Ok(saved) => {
                    self.observe_operation(&saved);
                    let settled = saved.statistics.checkpoint.progress
                        == crate::reader_book::checkpoint::Outcome::Verified
                        && saved.statistics.checkpoint.destination
                            != crate::reader_book::checkpoint::Outcome::Unconfirmed;
                    if !settled {
                        *exit = saved;
                        self.reader_ui.notice = ReaderUiNotice::FinishUnconfirmed;
                        exit.intent = Some(ReaderUiIntent::Repaint);
                        return;
                    }
                }
                Err(error) => {
                    self.open_failed(error);
                    exit.intent = Some(ReaderUiIntent::Repaint);
                    return;
                }
            }
        }
        match self.operate_image(home, ui, logical, events, None,
            Some(BookStep::MarkFinished(requested_mark)), false, None, None) {
            Ok(marked) => *exit = marked,
            Err(error) => {
                self.open_failed(error);
                exit.intent = Some(ReaderUiIntent::Repaint);
            }
        }
    }
}
