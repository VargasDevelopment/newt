// SPDX-License-Identifier: MIT
//! Embedded action staging uses the idle SDK frame, as publishStagedFrame.
use super::*;
use crate::app_image::{Application, Home};
use newt_core::{app::Action, frame::Frame};
use newt_reader_ui::ReaderUiIntent;

impl Runtime {
    pub(super) fn sample_action_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut Frame, action: Action,
    ) -> (Option<ReaderUiIntent>, u32) {
        let at = millis();
        let before = self.reading().map(|view| (view.page.spine, view.page.page));
        let result = {
            let stage: &mut Frame = match home.bus.board.frame().and_then(|bytes| bytes.try_into()
                .map_err(|_| newt_c3_board::Error::Length)) {
                Ok(frame) => frame, Err(_) => crate::app_power::stop(),
            };
            stage.copy_from_slice(logical);
            let Some(memory) = self.memory.sample_mut() else { return (None, at); };
            super::sample::with_operation(memory, &mut self.standalone_status, |sample, chapters| {
                let result = sample.handle_ui(chapters, &mut self.reader_ui, action, at);
                sample.complete_ui(chapters, &mut self.reader_ui, result, &ui.info, at, stage)
            })
        };
        match result {
            Ok(intent) => {
                if self.opened() {
                    match home.bus.board.frame() { Ok(stage) => logical.copy_from_slice(stage), Err(_) => crate::app_power::stop() }
                }
                if self.opened() && self.reader_ui.panel == newt_reader_ui::ReaderPanel::Page
                    && before != self.reading().map(|view| (view.page.spine, view.page.page)) {
                    self.reflow_anchor = None;
                    self.reading_refresh.page_changed();
                }
                (Some(intent), at)
            }
            Err(status) => {
                self.open_failed(crate::reader_book::BookFailure::Open(status));
                (Some(ReaderUiIntent::Repaint), at)
            }
        }
    }
}
