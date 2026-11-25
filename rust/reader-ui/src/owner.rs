//! Scalar SampleReader authority, separate from PhasedReader's retained state.
use newt_core::{progress::Locator, tracker::ShownPage, zip_container::Status};
use newt_phased_reader::{PageInfo, PhasedReader};

/// The real open/turn/jump owner updates these only from completed operations.
/// No reader, source, Session, Action, parser or frame loan survives here.
#[derive(Debug)]
pub struct ReaderOwnerState {
    has_progress: bool,
    status: Status,
    readable_spines: u32,
}
impl Default for ReaderOwnerState {
    fn default() -> Self {
        Self {
            has_progress: false,
            status: Status::NotFound,
            readable_spines: 0,
        }
    }
}
impl ReaderOwnerState {
    /// Called after an explicit open has established storage/progress ownership,
    /// including failed opens that retain progress. Never inferred from raw opened().
    pub fn from_open(has_progress: bool, status: Status, readable_spines: u32) -> Self {
        Self {
            has_progress,
            status,
            readable_spines,
        }
    }
    pub fn readable_spines(&self) -> u32 {
        self.readable_spines
    }
    pub fn opened(&self) -> bool {
        self.has_progress && self.status == Status::Ok
    }
    pub fn has_progress(&self) -> bool {
        self.has_progress
    }
    pub fn status(&self) -> Status {
        self.status
    }

    /// A failed raw reader may remain open. Recovery needs an explicit open;
    /// ordinary paint/menu success must never erase the failure latch.
    pub fn note_failure(&mut self, status: Status) {
        if self.status == Status::Ok && status != Status::Ok {
            self.status = status;
        }
    }
    /// Actual turn/jump recovery may clear a latch; paint/menu callbacks may not.
    pub fn navigation_completed(&mut self, status: Status, readable_spines: u32) {
        self.status = status;
        self.readable_spines = readable_spines;
    }
    /// Only a too-small menu workspace is optional; every other menu failure latches.
    pub(crate) fn menu_result(&mut self, status: Status) {
        match status {
            Status::Unsupported | Status::OutOfMemory => {}
            Status::Ok
            | Status::IoError
            | Status::NotZip
            | Status::NotEpub
            | Status::Encrypted
            | Status::Truncated
            | Status::ParseError
            | Status::NotFound
            | Status::Stale
            | Status::MetadataTooLarge
            | Status::PathTooLong
            | Status::Damaged => self.note_failure(status),
        }
    }
    /// Whether the latched failure belongs to the chapter, so the reader shows
    /// the chapter notice and keeps the place. Exhaustive so a new status needs
    /// a decision here.
    pub fn chapter_notice(&self) -> bool {
        match self.status {
            Status::IoError
            | Status::Truncated
            | Status::ParseError
            | Status::Unsupported
            | Status::Damaged => true,
            Status::Ok
            | Status::NotZip
            | Status::NotEpub
            | Status::Encrypted
            | Status::OutOfMemory
            | Status::NotFound
            | Status::Stale
            | Status::MetadataTooLarge
            | Status::PathTooLong => false,
        }
    }

    /// One projection for UI and future Home/Sleep consumers, including ShownPage.
    /// Uses the owner's readable bound, never raw spine count alone.
    pub fn position(&self, reader: &PhasedReader<'_, '_>) -> ReaderPosition {
        self.position_info(reader.info(), reader.selection().spines)
    }
    /// Retained read-only views use the same has-progress guard as live loans.
    pub fn position_info(&self, info: PageInfo, spines: u32) -> ReaderPosition {
        if self.has_progress {
            self.project(info, spines)
        } else {
            self.project(PageInfo::default(), 0)
        }
    }
    pub fn project(&self, mut page: PageInfo, raw_spines: u32) -> ReaderPosition {
        let spines = raw_spines.min(self.readable_spines);
        let last = page.page_count != 0
            && page.page.wrapping_add(1) >= page.page_count
            && page.spine.wrapping_add(1) >= spines;
        if page.overall_known && last {
            page.overall_percent = 100;
        }
        ReaderPosition {
            page,
            shown: ShownPage {
                locator: Locator {
                    spine: page.spine,
                    character: page.char_start,
                },
                last,
            },
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReaderPosition {
    pub page: PageInfo,
    pub shown: ShownPage,
}

#[cfg(test)]
#[path = "owner_cases.rs"]
mod cases;

#[cfg(test)]
#[path = "owner_missing_member_cases.rs"]
mod missing_member_cases;
