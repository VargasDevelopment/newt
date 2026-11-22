// SPDX-License-Identifier: MIT
//! Embedded SampleReader operations from frozen src/sample_reader.cpp. The
//! caller owns the one sample allocation and validates its checkpoint before
//! allocation; this view borrows its real reader, metadata, tracker and status.
use crate::{Action, ChapterJumpOutcome, DeviceInfo, Frame, ReaderOwnerState, ReaderPanel,
    ReaderUiIntent, ReaderUiResult, ReaderUiState, ReaderUiWork, RestoreFailure, RestorePolicy};
use newt_core::{tracker::{ReadingDirection, ReadingTracker}, zip_container::Status};
use newt_phased_reader::bounded::{Chapters, Metadata, Reader};
use newt_reader_navigation::NO_TOC;

pub struct SampleOperation<'op, 'book, 'storage> {
    pub reader: &'op mut Reader,
    pub metadata: &'op Metadata<'book, 'storage>,
    pub tracker: &'op mut ReadingTracker,
    pub owner: &'op mut ReaderOwnerState,
}

impl SampleOperation<'_, '_, '_> {
    /// Runs after successful indexing. Invalid strict checkpoints must already
    /// have been rejected before the caller allocated sample storage. On any
    /// failure the caller drops that allocation, as SampleReader::open does.
    pub fn restore_initial(&mut self, chapters: &mut impl Chapters,
        policy: RestorePolicy, now: u32, frame: &mut Frame,
    ) -> RestoreFailure {
        let locator = self.tracker.snapshot(0).locator;
        let mut status = self.reader.restore(self.metadata, chapters,
            locator.spine, locator.character, Some(frame));
        if status == Status::NotFound && policy == RestorePolicy::AllowInitialFallback {
            status = self.reader.show_page(self.metadata, chapters, 0, 0, Some(frame));
        }
        let exact_mismatch = status == Status::Ok && policy == RestorePolicy::ExactPage
            && (self.reader.info().spine != locator.spine
                || self.reader.info().char_start != locator.character);
        if exact_mismatch { status = Status::NotFound; }
        *self.owner = ReaderOwnerState::from_open(status == Status::Ok, status,
            self.metadata.package.spine_count as u32);
        if status != Status::Ok {
            return if exact_mismatch { RestoreFailure::InvalidCheckpoint }
                else { RestoreFailure::Unavailable };
        }
        self.tracker.enter(now, self.position().shown);
        RestoreFailure::None
    }

    pub fn position(&self) -> crate::ReaderPosition {
        self.owner.position_info(self.reader.info(), self.metadata.package.spine_count as u32)
    }

    /// Sample and external books use the same menu/settings/exit dispatch.
    /// This adapter borrows only this operation's real bounded menu projection.
    pub fn handle_ui(&mut self, chapters: &mut impl Chapters, ui: &mut crate::ReaderUiState,
        action: Action, now: u32,
    ) -> crate::ReaderUiResult {
        let settings = self.reader.settings();
        crate::lifecycle::handle_reader_ui_with(&mut SampleMenu {
            reader: self.reader, metadata: self.metadata, chapters,
        }, self.owner, ui, settings, action, now, self.tracker, None)
    }

    pub fn complete_ui(&mut self, chapters: &mut impl Chapters, ui: &mut ReaderUiState,
        result: ReaderUiResult, info: &DeviceInfo<'_>, now: u32, frame: &mut Frame,
    ) -> ReaderUiIntent {
        match result.work {
            ReaderUiWork::Intent(intent) => intent,
            ReaderUiWork::Turn(action) => {
                let changed = self.turn(chapters, action, now, frame);
                if changed || !self.owner.opened() { ReaderUiIntent::Repaint }
                else { ReaderUiIntent::None }
            }
            ReaderUiWork::Jump(row) => {
                let outcome = self.jump_to_row(chapters, row, now, frame);
                let intent = crate::settle_chapter_jump(ui, outcome);
                if outcome == ChapterJumpOutcome::Moved {
                    self.resume(now);
                } else {
                    // Resolution may have destroyed staging pixels. Replace
                    // them with a fresh page and Chapters rollback composition.
                    self.paint_ui(chapters, ui, info, now, frame);
                }
                intent
            }
        }
    }

    pub fn turn(&mut self, chapters: &mut impl Chapters, action: Action,
        now: u32, frame: &mut Frame,
    ) -> bool {
        if !matches!(action, Action::Previous | Action::Next) || !self.owner.opened() {
            return false;
        }
        let previous = self.reader.info();
        let status = match action {
            Action::Next if previous.page + 1 < previous.page_count => self.reader.show_page(
                self.metadata, chapters, previous.spine, previous.page + 1, Some(frame)),
            Action::Next if previous.spine + 1 < self.metadata.package.spine_count as u32 =>
                self.reader.show_page(self.metadata, chapters, previous.spine + 1, 0, Some(frame)),
            Action::Previous if previous.page > 0 => self.reader.show_page(
                self.metadata, chapters, previous.spine, previous.page - 1, Some(frame)),
            Action::Previous if previous.spine > 0 => self.reader.restore(
                self.metadata, chapters, previous.spine - 1, u32::MAX, Some(frame)),
            _ => return false,
        };
        self.owner.navigation_completed(status, self.metadata.package.spine_count as u32);
        if status != Status::Ok {
            self.tracker.pause(now);
            return false;
        }
        self.tracker.navigate(now, self.position().shown,
            if action == Action::Next { ReadingDirection::Forward } else { ReadingDirection::Backward });
        true
    }

    /// A parser failure latches unavailable until explicit reopen. A later
    /// paint cannot resume a tracker or silently clear the owner's failure.
    pub fn paint(&mut self, chapters: &mut impl Chapters, now: u32, frame: &mut Frame) -> bool {
        if !self.owner.opened() { return false; }
        let page = self.reader.info();
        let status = self.reader.show_page(self.metadata, chapters, page.spine, page.page, Some(frame));
        self.owner.note_failure(status);
        if status != Status::Ok { self.tracker.pause(now); }
        status == Status::Ok
    }

    /// The bounded sample paints the full page before lending layout workspace
    /// to its optional chapter menu, then uses the shared reader composition.
    pub fn paint_ui(&mut self, chapters: &mut impl Chapters, ui: &mut ReaderUiState,
        info: &DeviceInfo<'_>, now: u32, frame: &mut Frame,
    ) -> bool {
        let ready = self.paint(chapters, now, frame);
        if ready && ui.panel == ReaderPanel::Chapters && self.prepare_menu(chapters, ui.selected) {
            ui.selected = self.reader.chapter_menu().info().focus;
        }
        let title = self.metadata.package.metadata.title
            .and_then(|title| self.metadata.arena.c_string(title)).unwrap_or(b"");
        crate::compose_sample_reader_ui(crate::SamplePaintSource { reader: self.reader, title,
            spines: self.metadata.package.spine_count as u32, owner: self.owner },
            crate::ReaderPaintContext { ui, settings: self.reader.settings(), info,
                progress: self.tracker.snapshot(now), ready }, frame, None);
        ready
    }

    pub fn prepare_menu(&mut self, chapters: &mut impl Chapters, focus: u32) -> bool {
        self.owner.opened() && self.reader.prepare_chapter_menu(self.metadata, focus, chapters)
    }

    pub fn jump_to_spine(&mut self, chapters: &mut impl Chapters, spine: u32,
        now: u32, frame: &mut Frame,
    ) -> bool {
        if !self.owner.opened() { return false; }
        self.tracker.pause(now);
        if spine >= self.metadata.package.spine_count as u32 { return false; }
        let status = self.reader.show_page(self.metadata, chapters, spine, 0, Some(frame));
        self.owner.navigation_completed(status, self.metadata.package.spine_count as u32);
        if status != Status::Ok { return false; }
        self.tracker.relocate(self.position().shown);
        true
    }

    pub fn jump_to_row(&mut self, chapters: &mut impl Chapters, row: u32,
        now: u32, frame: &mut Frame,
    ) -> ChapterJumpOutcome {
        if !self.owner.opened() { return ChapterJumpOutcome::Failed; }
        if !self.reader.chapter_menu().row(row).valid && !self.prepare_menu(chapters, row) {
            return ChapterJumpOutcome::Unresolved;
        }
        let (spine, toc) = {
            let menu = self.reader.chapter_menu();
            let target = menu.row(row);
            if !target.valid || target.spine >= self.metadata.package.spine_count as u32 {
                return ChapterJumpOutcome::Unresolved;
            }
            (target.spine, if target.fragment { target.toc } else { NO_TOC })
        };
        let status = self.reader.show_chapter(self.metadata, chapters, spine, toc, Some(frame));
        if status != Status::Ok {
            if !self.reader.opened() {
                self.owner.navigation_completed(status, self.metadata.package.spine_count as u32);
                return ChapterJumpOutcome::Failed;
            }
            // The bounded reader's last good page/status stay live when an
            // optional chapter target cannot be resolved atomically.
            return if status == Status::NotFound { ChapterJumpOutcome::Unresolved }
                else { ChapterJumpOutcome::Failed };
        }
        self.owner.navigation_completed(Status::Ok, self.metadata.package.spine_count as u32);
        self.tracker.pause(now);
        self.tracker.relocate(self.position().shown);
        ChapterJumpOutcome::Moved
    }

    pub fn resume(&mut self, now: u32) {
        if self.owner.opened() { self.tracker.enter(now, self.position().shown); }
    }
}

struct SampleMenu<'op, 'book, 'storage, C> {
    reader: &'op mut Reader,
    metadata: &'op Metadata<'book, 'storage>,
    chapters: &'op mut C,
}
impl<C: Chapters> crate::lifecycle::MenuOperation for SampleMenu<'_, '_, '_, C> {
    fn menu(&self) -> newt_phased_reader::ChapterMenuView<'_> { self.reader.chapter_menu() }
    fn shown(&self, owner: &ReaderOwnerState) -> newt_core::tracker::ShownPage {
        owner.position_info(self.reader.info(), self.metadata.package.spine_count as u32).shown
    }
    fn prepare(&mut self, owner: &mut ReaderOwnerState, focus: u32) -> crate::MenuPreparation {
        // The sample's bool-returning SDK method never changes status_.
        let valid = self.reader.prepare_chapter_menu(self.metadata, focus, self.chapters);
        crate::MenuPreparation { status: owner.status(), valid }
    }
}

#[cfg(test)]
#[path = "sample_owner_cases.rs"]
mod cases;
