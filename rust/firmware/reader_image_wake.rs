// SPDX-License-Identifier: MIT
//! A short observation of the real reader for the existing wake evidence owner.
use super::*;
use newt_wake_evidence::{Attempt, ReaderObservation};
use newt_reader_ui::{RestoreFailure, RestorePolicy, ViewSettings};
use newt_core::zip_container::Status;

struct Observation<'a> {
    reading: Option<ReadingView<'a>>,
    settings: &'a ViewSettings,
    status: Status,
    restore: RestoreFailure,
}
impl ReaderObservation for Observation<'_> {
    fn status(&self) -> u8 { self.status as u8 }
    fn restore_failure(&self) -> u8 {
        match self.restore {
            RestoreFailure::None => 0,
            RestoreFailure::Unavailable => 1,
            RestoreFailure::InvalidCheckpoint => 2,
        }
    }
    fn orientation(&self) -> u8 { self.settings.orientation as u8 }
    fn opened(&self) -> bool { self.reading.as_ref().is_some_and(|reading| reading.opened) }
    fn page(&self) -> (u32, u32) {
        self.reading.as_ref().map_or((0, 0), |reading| {
            let page = reading.page;
            (page.spine, page.char_start)
        })
    }
}
impl Runtime {
    pub(super) fn observe_wake_attempt(&mut self, outcome: Attempt, policy: RestorePolicy) {
        let status = self.status();
        let restore = self.book().map_or(self.restore_failure, |book| book.owner.restore_failure);
        let observation = Observation { reading: self.memory.reading(self.standalone_status),
            settings: &self.settings, status, restore };
        self.wake.initial_reader_attempt(outcome, &observation, policy as u8);
    }
}
