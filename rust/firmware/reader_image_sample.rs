// SPDX-License-Identifier: MIT
//! Actual ROM sample admission and operations; no selected SD source/session.
use super::*;
use core::fmt::Write;
use crate::{app_image::{Application, Events, Home}, reader_book::BookFailure, sample_epub};
use newt_core::{frame::Frame, save_notice::{ReaderIssue, SourceIssue},
    zip_container::Status};
use newt_c3_board::sample_memory::SampleMemory;
use newt_phased_reader::phase_storage::bounded_chapters::Work;
use newt_reader_ui::{ReaderOwnerState, RestoreFailure, RestorePolicy, SampleOperation};
use newt_target_expat::Expat;

pub(super) fn with_operation<R>(memory: &mut SampleMemory, status: &mut Status,
    work: impl FnOnce(&mut SampleOperation<'_, '_, '_>, &mut Work<'_, Expat>) -> R,
) -> Result<R, Status> {
    let mut owner = ReaderOwnerState::from_open(true, *status, memory.view().spines);
    let result = memory.with(|loan| {
        let mut backend = Expat::default();
        let mut breaker = crate::reader_linebreak::NativeBreak;
        let mut chapters = Work::new(sample_epub::BYTES, loan.layout, loan.parse, &mut backend, &mut breaker);
        work(&mut SampleOperation { reader: loan.reader, metadata: loan.metadata,
            tracker: loan.tracker, owner: &mut owner }, &mut chapters)
    });
    *status = owner.status();
    result
}

impl Runtime {
    pub(super) fn load_sample_home(&mut self, home: &mut Home<'_>) {
        self.progress_ready = home.with_state_store(|_, store| sample_epub::load_home_progress(
            &mut newt_core::progress_store::ProgressStore::new(store), &mut self.saved, &mut self.home));
        self.home.set_finished(false);
        self.reader_ui.finish = newt_reader_ui::ReaderFinishRow::Hidden;
        self.saves.set_progress_unavailable(!self.progress_ready);
        self.title.fill(0);
        self.title[..newt_stage_home::SAMPLE_TITLE.len()].copy_from_slice(newt_stage_home::SAMPLE_TITLE);
    }

    // Keep sample-only admission locals out of the shared SD-open ancestor.
    #[inline(never)]
    pub(super) fn open_sample_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut Frame, events: &mut Events<'_, '_>, session_checkpoint: bool,
    ) -> bool {
        let policy = self.restore_policy_image();
        if policy != RestorePolicy::ExactPage && !session_checkpoint { self.load_sample_home(home); }
        if self.recovery.is_none() && policy != RestorePolicy::ExactPage
            && !session_checkpoint && !self.progress_ready {
            self.publish_notice_image(ui);
            return true;
        }
        let saved = self.recovery.as_ref().map_or(self.saved, |pending| pending.progress);
        self.settings = self.recovery.as_ref().map_or(self.settings, |pending| pending.settings);
        let Some(bytes) = SampleMemory::allocation_bytes() else { crate::app_power::stop(); };
        let free = match ui.hardware.free_heap_bytes() { Ok(value) => value, Err(_) => crate::app_power::stop() };
        let largest = match ui.hardware.largest_heap_block_bytes() { Ok(value) => value, Err(_) => crate::app_power::stop() };
        let required = bytes + 24576;
        let _ = writeln!(home.bus.log, "Reader admission: free={} largest={} required={} block={} credit=0",
            free, largest, required, bytes);
        let guard = if (largest as usize) < bytes { Some(newt_wake_evidence::Attempt::LargestBlockGuard) }
            else if (free as usize) < required { Some(newt_wake_evidence::Attempt::FreeHeapGuard) } else { None };
        if let Some(guard) = guard {
            self.saves.set_reader(ReaderIssue::Memory);
            self.observe_wake_attempt(guard, policy);
            let _ = writeln!(home.bus.log, "Newt sample unavailable: insufficient reader heap headroom");
            self.publish_notice_image(ui);
            return true;
        }
        // Heap admission observes the old sample, if any, just as attempt().
        // The old allocation ends before SampleReader::open allocates anew.
        self.memory = ReaderBacking::Closed;
        self.restore_failure = RestoreFailure::Unavailable;
        let now = millis();
        let opened = {
            let stage: &mut Frame = match home.bus.board.frame().and_then(|bytes| bytes.try_into()
                .map_err(|_| newt_c3_board::Error::Length)) {
                Ok(frame) => frame, Err(_) => crate::app_power::stop(),
            };
            stage.copy_from_slice(logical);
            sample_epub::open(saved, self.settings, policy, now, stage,
                &mut Expat::default(), &mut crate::reader_linebreak::NativeBreak)
        };
        match opened {
            Ok(memory) => {
                self.memory = ReaderBacking::Sample(memory);
                self.standalone_status = Status::Ok;
                self.restore_failure = RestoreFailure::None;
                self.saves.set_reader(ReaderIssue::None);
                self.saves.set_source(SourceIssue::None);
                self.observe_wake_attempt(newt_wake_evidence::Attempt::Succeeded, policy);
                if events.control.cancelled_at(millis()) {
                    self.memory = ReaderBacking::Closed;
                    self.standalone_status = Status::NotFound;
                } else {
                    match home.bus.board.frame() { Ok(stage) => logical.copy_from_slice(stage), Err(_) => crate::app_power::stop() }
                    self.last_progress.set(now);
                    self.home.set_finished(false);
                    self.reader_ui.finish = newt_reader_ui::ReaderFinishRow::Hidden;
                    if !self.saves.unconfirmed() { self.recovery = None; }
                }
            }
            Err(error) => {
                self.restore_failure = error.restore;
                self.open_failed(BookFailure::Open(error.status));
                self.observe_wake_attempt(newt_wake_evidence::Attempt::OpenFailed, policy);
                if error.restore == RestoreFailure::InvalidCheckpoint {
                    self.recovery = None;
                    self.settings = home.preferences.settings();
                    self.reader_ui.draft = self.settings;
                    self.restore_failure = RestoreFailure::None;
                }
            }
        }
        self.selection.publish_selection_notice(&mut self.saves);
        self.publish_notice_image(ui);
        true
    }

    pub(super) fn save_sample_image(&mut self, home: &mut Home<'_>, now: u32) -> bool {
        let Some(memory) = self.memory.sample() else { return false; };
        let candidate = *memory.view().tracker.snapshot(now);
        self.checkpoint.begin_operation();
        let result = home.with_state_store(|_, store|
            self.checkpoint.save_sample(store, candidate, self.progress_ready));
        if result.progress == crate::reader_book::checkpoint::Outcome::Verified {
            if let Some(saved) = self.checkpoint.acknowledged_sample(sample_epub::IDENTITY) { self.saved = saved; }
        }
        let committed = newt_core::save_notice::ReadingCommitResult {
            progress: super::exit::outcome(result.progress), destination: super::exit::outcome(result.destination),
        };
        let complete = self.saves.checkpoint(committed);
        if result.progress == crate::reader_book::checkpoint::Outcome::Unconfirmed || self.saves.unconfirmed() {
            self.recovery = Some(crate::reader_book::PendingRecovery {
                progress: if result.progress == crate::reader_book::checkpoint::Outcome::Verified { self.saved } else { candidate },
                settings: self.settings, policy: RestorePolicy::ExactPage,
            });
        } else { self.recovery = None; }
        self.last_progress.set(now);
        complete
    }
}
