// SPDX-License-Identifier: MIT
//! Reader-only reading-statistics lifecycle.
//! Stores scalar policy only; midnight/end/pacing marks stay in the tracker.
use super::StatsCondition;
use newt_core::{
    reading_stats::{BookFinish, StatsMark},
    reading_stats_once::Sha256,
    reading_stats_service::{
        StatsBook, StatsKind, StatsReport, StatsResult, fold_marked_stats, fold_stats, mark_stats,
        seed_stats,
    },
    stats_clock::StatsClock,
    stats_service_backend::NamespaceBackend,
    time_zone::TimeZone,
    tracker::{DayMarkState, ReadingEnd, ReadingStatsMarks},
};
use newt_reader_ui::{ReaderFinishRow, ReaderUiNotice, ReaderUiState};

pub const FOLD_MS: u32 = 5 * 60 * 1000;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReadingStatsRuntime {
    pub condition: StatsCondition,
    pub fold_owed: bool,
}

pub(super) fn show_finish(ui: &mut ReaderUiState, finish: BookFinish) {
    ui.finish = match finish {
        BookFinish::Unknown => ReaderFinishRow::Hidden,
        BookFinish::Yes => ReaderFinishRow::Undo,
        BookFinish::No => ReaderFinishRow::Mark,
    };
}

impl ReadingStatsRuntime {
    fn note(&mut self, report: StatsReport) {
        if report.result != StatsResult::Cancelled {
            self.condition = match report.result {
                StatsResult::Unseeded => StatsCondition::Unseeded,
                StatsResult::Unsupported => StatsCondition::Unsupported,
                StatsResult::Unavailable => StatsCondition::Unavailable,
                _ => StatsCondition::Seeded,
            };
        }
    }

    /// A successful fold consumes acknowledged counters only. Keep a midnight
    /// boundary until both acknowledged counters have caught up with it.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn fold<B: NamespaceBackend>(
        &mut self,
        store: &mut B,
        book: Option<&StatsBook>,
        marks: &mut ReadingStatsMarks,
        ui: &mut ReaderUiState,
        now: u32,
        zone: TimeZone,
        read_clock: impl FnOnce() -> (StatsClock, u32),
        cancelled: Option<&mut dyn FnMut() -> bool>,
        sha256: Sha256<'_>,
    ) -> bool {
        let Some(book) = book else { return true };
        let (clock, utc) = read_clock();
        let mark = &mut marks.day;
        let projected = clock.trusted() && mark.matches(utc, now, zone);
        if !projected && mark.state != DayMarkState::Empty {
            mark.state = DayMarkState::Lost;
        }
        let report = if projected {
            fold_marked_stats(store, book, clock, mark, cancelled, sha256)
        } else {
            fold_stats(store, book, clock, cancelled, sha256)
        };
        let carry = projected
            && u32::from(clock.day) == u32::from(mark.day) + 1
            && (mark.state == DayMarkState::Armed
                || (mark.state == DayMarkState::Crossed
                    && (book.counters.milliseconds < mark.milliseconds
                        || book.counters.pages < mark.pages)));
        if (report.result == StatsResult::Done && report.kind == StatsKind::Fold && !carry)
            || mark.state == DayMarkState::Empty
        {
            mark.anchor(utc, now, zone);
        }
        self.note(report);
        if report.result == StatsResult::Cancelled {
            return false;
        }
        if report.result != StatsResult::Unseeded {
            marks.folded_at = now;
            marks.folded_once = true;
        }
        if report.result == StatsResult::Done {
            show_finish(ui, report.finish);
        }
        match report.result {
            StatsResult::Done | StatsResult::Unseeded | StatsResult::Unsupported => {
                marks.lease_checked = true;
                self.fold_owed = false;
            }
            StatsResult::Unavailable | StatsResult::Waiting | StatsResult::Unconfirmed => {
                self.fold_owed = true;
            }
            StatsResult::Cancelled => {}
        }
        true
    }

    /// Seeding reads progress records, so a progress commit whose durability
    /// is unknown must settle before a seed can use those bytes.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn idle<B: NamespaceBackend>(
        &mut self,
        store: &mut B,
        book: Option<&StatsBook>,
        marks: &mut ReadingStatsMarks,
        ui: &mut ReaderUiState,
        now: u32,
        zone: TimeZone,
        ready: bool,
        page_unconfirmed: bool,
        read_clock: impl FnOnce() -> (StatsClock, u32),
        cancelled: Option<&mut dyn FnMut() -> bool>,
        sha256: Sha256<'_>,
    ) -> bool {
        if !ready
            || self.condition == StatsCondition::Unsupported
            || (marks.folded_once && now.wrapping_sub(marks.folded_at) < FOLD_MS)
        {
            return false;
        }
        let Some(book) = book else { return false };
        if self.condition == StatsCondition::Unseeded {
            if page_unconfirmed {
                return false;
            }
            self.note(seed_stats(store, Some(&book.digest), cancelled));
        } else {
            self.fold(
                store,
                Some(book),
                marks,
                ui,
                now,
                zone,
                read_clock,
                cancelled,
                sha256,
            );
        }
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn lease<B: NamespaceBackend>(
        &mut self,
        store: &mut B,
        book: Option<&StatsBook>,
        marks: &mut ReadingStatsMarks,
        ui: &mut ReaderUiState,
        now: u32,
        zone: TimeZone,
        read_clock: impl FnOnce() -> (StatsClock, u32),
        cancelled: Option<&mut dyn FnMut() -> bool>,
        sha256: Sha256<'_>,
    ) -> bool {
        marks.lease_checked
            || self.fold(
                store, book, marks, ui, now, zone, read_clock, cancelled, sha256,
            )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn mark<B: NamespaceBackend>(
        &mut self,
        store: &mut B,
        book: Option<&StatsBook>,
        marks: &mut ReadingStatsMarks,
        ui: &mut ReaderUiState,
        now: u32,
        zone: TimeZone,
        opening: bool,
        mut read_clock: impl FnMut() -> (StatsClock, u32),
        sha256: Sha256<'_>,
    ) {
        let finish = ui.finish != ReaderFinishRow::Undo;
        let Some(book) = book else {
            ui.notice = if opening {
                ReaderUiNotice::FinishOpening
            } else {
                ReaderUiNotice::FinishSaveFailed
            };
            return;
        };
        if !finish {
            marks.end = ReadingEnd::Forgotten;
        }
        let mut leased = *book;
        leased.end_reached = marks.end == ReadingEnd::Reached;
        self.lease(
            store,
            Some(&leased),
            marks,
            ui,
            now,
            zone,
            &mut read_clock,
            None,
            sha256,
        );
        let (clock, _) = read_clock();
        let report = mark_stats(
            store,
            &book.digest,
            clock,
            if finish {
                StatsMark::Finished
            } else {
                StatsMark::NotFinished
            },
            sha256,
        );
        self.note(report);
        if report.result == StatsResult::Done {
            show_finish(ui, report.finish);
        }
        ui.notice = match report.result {
            StatsResult::Done => ReaderUiNotice::None,
            StatsResult::Unconfirmed => ReaderUiNotice::FinishUnconfirmed,
            _ => ReaderUiNotice::FinishSaveFailed,
        };
    }
}
