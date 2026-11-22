// SPDX-License-Identifier: MIT
//! Bounded reading-session tracker and RAM midnight mark. MIT; see LICENSE.
use crate::progress::{Locator, Progress};
use crate::stats_clock::{local_stats_second, next_stats_midnight};
use crate::time_zone::TimeZone;

pub const READING_INACTIVITY_MS: u32 = 300000;
const PAGE_DWELL: u32 = 2000;
const SESSION: u32 = 60000;
const HALF_RANGE: u32 = 0x80000000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadingDirection {
    Forward,
    Backward,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShownPage {
    pub locator: Locator,
    pub last: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum DayMarkState {
    #[default]
    Empty,
    Armed,
    Crossed,
    Lost,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReadingDayMark {
    pub milliseconds: u64,
    pub pages: u32,
    pub sessions: u32,
    pub midnight_at: u32,
    pub expires_at: u32,
    pub midnight_utc: u32,
    pub day: u16,
    pub zone: TimeZone,
    pub state: DayMarkState,
}

impl ReadingDayMark {
    pub fn anchor(&mut self, utc: u32, now: u32, published_zone: TimeZone) {
        *self = Self::default();
        let clock = local_stats_second(utc, published_zone);
        if !clock.trusted() {
            return;
        }
        let midnight = next_stats_midnight(utc, published_zone);
        self.midnight_utc = midnight;
        self.midnight_at = now.wrapping_add(midnight.wrapping_sub(utc).wrapping_mul(1000));
        self.expires_at = now.wrapping_add(
            next_stats_midnight(midnight, published_zone)
                .wrapping_sub(utc)
                .wrapping_mul(1000),
        );
        self.day = clock.day;
        self.zone = published_zone;
        self.state = DayMarkState::Armed;
    }

    pub fn matches(&self, utc: u32, now: u32, published_zone: TimeZone) -> bool {
        if !matches!(self.state, DayMarkState::Armed | DayMarkState::Crossed)
            || self.zone != published_zone
            || now.wrapping_sub(self.expires_at) < HALF_RANGE
            || utc == 0
        {
            return false;
        }
        // Signed truncating division on elapsed ms, then wrapping u32 addition.
        let seconds = (now.wrapping_sub(self.midnight_at) as i32) / 1000;
        let projected = self.midnight_utc.wrapping_add(seconds as u32);
        projected.abs_diff(utc) <= 10
    }

    pub fn observe(&mut self, before: &Progress, now: u32, interval: u32) {
        if matches!(self.state, DayMarkState::Empty | DayMarkState::Lost) {
            return;
        }
        if now.wrapping_sub(self.expires_at) < HALF_RANGE {
            self.state = DayMarkState::Lost;
            return;
        }
        if self.state != DayMarkState::Armed || now.wrapping_sub(self.midnight_at) >= HALF_RANGE {
            return;
        }
        let after = now.wrapping_sub(self.midnight_at);
        let credit = interval.saturating_sub(after);
        self.milliseconds = before.active_milliseconds.saturating_add(u64::from(credit));
        self.pages = before.forward_pages;
        self.sessions = before.qualified_sessions;
        // A later qualification still belongs to a session's start day.
        if (before.session_milliseconds != 0 || credit != 0)
            && before.session_milliseconds < SESSION
        {
            self.sessions = self.sessions.saturating_add(1);
        }
        self.state = DayMarkState::Crossed;
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum ReadingEnd {
    #[default]
    Unseen,
    Reached,
    Forgotten,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReadingStatsMarks {
    pub day: ReadingDayMark,
    pub folded_at: u32,
    pub lease_checked: bool,
    pub folded_once: bool,
    pub import_checked: bool,
    pub end: ReadingEnd,
}

pub struct ReadingTracker {
    value: Progress,
    reading: bool,
    interval_start: u32,
    dwell: u32,
    marks: ReadingStatsMarks,
}

impl ReadingTracker {
    pub fn new(initial: Progress) -> Self {
        Self {
            value: initial,
            reading: false,
            interval_start: 0,
            dwell: 0,
            marks: ReadingStatsMarks::default(),
        }
    }

    pub fn enter(&mut self, now: u32, page: ShownPage) {
        if self.reading {
            return;
        }
        self.marks.day.observe(&self.value, now, 0);
        self.take(page);
        self.reading = true;
        self.interval_start = now;
        self.dwell = 0;
    }

    pub fn navigate(&mut self, now: u32, page: ShownPage, direction: ReadingDirection) {
        if !self.reading {
            return;
        }
        self.close(now);
        if self.value.locator != page.locator {
            if direction == ReadingDirection::Forward && self.dwell >= PAGE_DWELL {
                self.value.forward_pages = self.value.forward_pages.saturating_add(1);
            }
            self.dwell = 0;
        }
        self.take(page);
        self.reading = true;
        self.interval_start = now;
    }

    pub fn pause(&mut self, now: u32) {
        self.close(now);
    }

    pub fn leave(&mut self, now: u32) {
        self.close(now);
        self.value.session_milliseconds = 0;
    }

    pub fn relocate(&mut self, page: ShownPage) {
        self.take(page);
        self.dwell = 0;
    }

    /// Closed intervals only; `now` is deliberately unused.
    pub fn snapshot(&self, _now: u32) -> &Progress {
        &self.value
    }

    pub fn reading(&self) -> bool {
        self.reading
    }

    pub fn open_interval(&self, now: u32) -> u32 {
        if self.reading {
            now.wrapping_sub(self.interval_start)
        } else {
            0
        }
    }

    pub fn stats_marks(&self) -> &ReadingStatsMarks {
        &self.marks
    }

    /// The owner controls fold pacing, anchoring, and explicit end forgetting.
    pub fn stats_marks_mut(&mut self) -> &mut ReadingStatsMarks {
        &mut self.marks
    }

    fn take(&mut self, page: ShownPage) {
        self.value.locator = page.locator;
        if page.last && self.marks.end == ReadingEnd::Unseen {
            self.marks.end = ReadingEnd::Reached;
        }
    }

    fn close(&mut self, now: u32) {
        if !self.reading {
            return;
        }
        self.reading = false;
        let elapsed = now.wrapping_sub(self.interval_start);
        self.marks.day.observe(
            &self.value,
            now,
            if elapsed <= READING_INACTIVITY_MS {
                elapsed
            } else {
                0
            },
        );
        if elapsed >= HALF_RANGE {
            self.dwell = 0;
            self.value.session_milliseconds = 0;
            return;
        }
        self.dwell = if elapsed < PAGE_DWELL - self.dwell {
            self.dwell + elapsed
        } else {
            PAGE_DWELL
        };
        if elapsed > READING_INACTIVITY_MS {
            self.value.session_milliseconds = 0;
            return;
        }
        self.value.active_milliseconds = self
            .value
            .active_milliseconds
            .saturating_add(u64::from(elapsed));
        let session = &mut self.value.session_milliseconds;
        if *session >= SESSION {
            return;
        }
        *session = if elapsed < SESSION - *session {
            *session + elapsed
        } else {
            SESSION
        };
        if *session >= SESSION {
            self.value.qualified_sessions = self.value.qualified_sessions.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page(character: u32) -> ShownPage {
        ShownPage {
            locator: Locator {
                spine: 0,
                character,
            },
            last: false,
        }
    }

    #[test]
    fn same_locator_dwell_and_closed_snapshots() {
        let mut t = ReadingTracker::new(Progress::default());
        t.enter(0, page(0));
        assert_eq!(t.snapshot(60000).active_milliseconds, 0);
        t.navigate(1000, page(0), ReadingDirection::Forward);
        t.navigate(2000, page(1), ReadingDirection::Forward);
        assert_eq!(t.snapshot(0).forward_pages, 1);
        assert_eq!(t.open_interval(2500), 500);
    }

    #[test]
    fn pause_continues_leave_ends_and_rebuild_carries_session() {
        let mut t = ReadingTracker::new(Progress::default());
        t.enter(0, page(0));
        t.pause(30000);
        let mut t = ReadingTracker::new(*t.snapshot(0));
        t.enter(100000, page(0));
        t.pause(130000);
        assert_eq!(t.snapshot(0).qualified_sessions, 1);
        t.enter(140000, page(0));
        t.leave(150000);
        assert_eq!(t.snapshot(0).session_milliseconds, 0);
        t.enter(150000, page(0));
        t.pause(210000);
        assert_eq!(t.snapshot(0).qualified_sessions, 2);
    }

    #[test]
    fn wrap_expiry_and_forgotten_end() {
        let mut t = ReadingTracker::new(Progress::default());
        t.enter(u32::MAX - 999, page(0));
        t.pause(1000);
        assert_eq!(t.snapshot(0).active_milliseconds, 2000);
        t.stats_marks_mut().end = ReadingEnd::Forgotten;
        t.relocate(ShownPage {
            last: true,
            ..page(9)
        });
        assert_eq!(t.stats_marks().end, ReadingEnd::Forgotten);
        t.enter(1000, page(0));
        t.navigate(301001, page(1), ReadingDirection::Forward);
        assert_eq!(t.snapshot(0).active_milliseconds, 2000);
        assert_eq!(t.snapshot(0).forward_pages, 1);
        t.navigate(
            301001u32.wrapping_add(HALF_RANGE),
            page(2),
            ReadingDirection::Forward,
        );
        assert_eq!(t.snapshot(0).forward_pages, 1);
    }

    #[test]
    fn midnight_split_keeps_old_pages_and_start_day_session() {
        let mut mark = ReadingDayMark::default();
        // 2026-01-01 at noon UTC, with wrap of the monotonic counter.
        let noon = 9497 * 86400 + 12 * 3600;
        let now = u32::MAX - 500;
        mark.anchor(noon, now, TimeZone::Utc);
        assert!(mark.matches(noon, now, TimeZone::Utc));
        let before = Progress {
            active_milliseconds: 123,
            forward_pages: 7,
            qualified_sessions: 2,
            session_milliseconds: 30000,
            ..Progress::default()
        };
        mark.observe(&before, mark.midnight_at.wrapping_add(1000), 2000);
        assert_eq!((mark.milliseconds, mark.pages, mark.sessions), (1123, 7, 3));
        assert_eq!(mark.state, DayMarkState::Crossed);
        mark.observe(&Progress::default(), mark.expires_at.wrapping_sub(1), 0);
        assert_eq!(mark.milliseconds, 1123);
        mark.observe(&before, mark.expires_at, 0);
        assert_eq!(mark.state, DayMarkState::Lost);
        assert!(!mark.matches(noon, now, TimeZone::Utc));
    }

    #[test]
    fn midnight_uses_actual_dst_day_lengths() {
        use crate::stats_clock::stats_clock;
        use crate::time_zone::ClockTime;
        for (month, day, hour, hours) in [(3, 8, 5, 23), (11, 1, 4, 25)] {
            let date = ClockTime {
                year: 2026,
                month,
                day,
                hour,
                minute: 0,
            };
            let utc = u32::from(stats_clock(date).day) * 86400 + u32::from(hour) * 3600;
            let mut mark = ReadingDayMark::default();
            mark.anchor(utc, 100, TimeZone::UsEastern);
            assert_eq!(mark.midnight_at, 100 + hours * 3600000);
            assert_eq!(mark.expires_at, 100 + (hours + 24) * 3600000);
        }
    }
}
