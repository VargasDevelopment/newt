// SPDX-License-Identifier: MIT
//! Borrowed projections of the actual current and staged reader selection.
use super::{ChapterMenuView, PageInfo, PhasedReader, Section, Selection};

pub(super) fn current_section(has_page: bool, info: PageInfo, selection: &Selection) -> Section<'_> {
    if has_page {
        selection.navigation.section(info.char_start)
    } else {
        Section::default()
    }
}

impl PhasedReader<'_, '_> {
    pub fn current_section(&self) -> Section<'_> {
        current_section(self.has_page, self.info, self.selection())
    }
    /// Frozen sectionAt answers only from the displayed chapter projection.
    pub fn section_at(&self, at: newt_core::progress::Locator) -> Section<'_> {
        if self.has_page && self.info.spine == at.spine {
            self.selection().navigation.section(at.character)
        } else {
            Section::default()
        }
    }
    pub fn progress_estimate(&self) -> newt_reader_navigation::Estimate {
        if self.opened {
            self.selection().navigation.progress_inputs().estimate(self.info.spine, self.info.total_chars)
        } else {
            newt_reader_navigation::Estimate::default()
        }
    }
    pub fn chapter_menu(&self) -> ChapterMenuView<'_> {
        ChapterMenuView {
            navigation: &self.staged_selection().navigation,
        }
    }
}
