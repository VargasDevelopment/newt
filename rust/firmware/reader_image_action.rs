// SPDX-License-Identifier: MIT
//! One accepted reader action, its bounded source barrier and checked exit.
use super::*;
use crate::{app_image::{Application, Events, Home}, reader_book::{BookExit, BookStep}};
use newt_core::{app::{Action, Screen}, frame::Frame};
use newt_reader_ui::{ReaderPanel, ReaderUiIntent, ReaderUiNotice};

impl Runtime {
    pub(super) fn reader_action_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut Frame, events: &mut Events<'_, '_>, action: Action,
    ) -> bool {
        let Events { queue, control, task, action: trace } = events;
        control.with_open(millis(), |control, _owned| {
            let mut events = Events { queue, control, action: *trace, task: task.reborrow() };
            self.reader_action_armed(home, ui, logical, &mut events, action)
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
            match self.operate_image(home, ui, logical, events, None,
                Some(BookStep::Action(action)), false, None, None) {
                Ok(mut exit) => {
                    self.finish_pending_turn_image(home, ui, logical, events, action, before, &mut exit);
                    self.observe_operation(&exit);
                    self.update_home_image(home, events.action);
                    if before != self.book().map(|book| book.reader.info()) {
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
}
