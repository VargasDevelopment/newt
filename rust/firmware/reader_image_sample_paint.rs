// SPDX-License-Identifier: MIT
//! Sample composition uses its own ended parser/layout loans and shared UI.
use super::*;
use crate::app_image::Application;
use newt_core::{frame::Frame, zip_container::Status};
use newt_reader_ui::{ReaderFooter, ReaderOwnerState};

impl Runtime {
    pub(super) fn compose_sample_image(&mut self, ui: &Application<'_>, logical: &mut Frame,
        now: u32,
    ) -> Result<bool, Status> {
        let memory = self.memory.sample_mut().ok_or(Status::NotFound)?;
        super::sample::with_operation(memory, &mut self.standalone_status, |sample, chapters|
            sample.paint_ui(chapters, &mut self.reader_ui, &ui.info, now, logical))
    }

    pub(super) fn compose_sample_unavailable(&self, ui: &Application<'_>, logical: &mut Frame) {
        let Some(memory) = self.memory.sample() else { return; };
        let view = memory.view();
        let owner = ReaderOwnerState::from_open(true, self.standalone_status, view.spines);
        newt_reader_ui::compose_sample_reader_ui(newt_reader_ui::SamplePaintSource {
            reader: view.reader, title: view.title.unwrap_or(&[]), spines: view.spines, owner: &owner },
            newt_reader_ui::ReaderPaintContext { ui: &self.reader_ui, settings: self.settings,
                info: &ui.info, progress: view.tracker.snapshot(0), ready: false }, logical, None);
    }

    pub(super) fn compose_sample_backdrop(&mut self, ui: &Application<'_>, logical: &mut Frame,
        now: u32,
    ) -> Result<bool, Status> {
        let memory = self.memory.sample_mut().ok_or(Status::NotFound)?;
        super::sample::with_operation(memory, &mut self.standalone_status, |sample, chapters| {
            let ready = sample.paint(chapters, now, logical);
            let page = sample.position().page;
            let section = if sample.owner.opened() { sample.reader.current_section() }
                else { newt_reader_navigation::Section::default() };
            newt_reader_ui::render_reader_chrome(&ReaderFooter { spine: section.spine,
                page: page.page, pages: page.page_count, overall_known: page.overall_known,
                overall_percent: page.overall_percent, section_label: Some(section.label) },
                &ui.info, self.settings, ready, Some(logical));
            ready
        })
    }
}
