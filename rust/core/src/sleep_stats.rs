// SPDX-License-Identifier: MIT
//! Pure Sleep screen reading-habit statistics.
use crate::reading_stats::{
    DAY_UNSET, MONTH_NONE, STREAK_SECONDS, StatsDecode, StatsLedger, StatsMonth, StatsSummary,
    classify_stats_record,
};
use crate::stats_clock::{STATS_FIRST_DAY, STATS_LAST_DAY, StatsClock, stats_bucket, stats_date};
use crate::wire::{fnv1a, get32, put32};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum ReadingHabit {
    #[default]
    None,
    Morning,
    Afternoon,
    Evening,
    Night,
}
pub const STREAK_MAGIC: u32 = 0x4b54574e;
pub const STREAK_BYTES: usize = 28;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatsStreak {
    pub day: u16,
    pub run: u16,
    pub days: u8,
    pub before: u32,
}
impl StatsStreak {
    pub fn stamped(&self) -> bool {
        self.day != DAY_UNSET
    }
    fn consistent(&self) -> bool {
        if !self.stamped() {
            return self.run == 0 && self.days == 0 && self.before == 0;
        }
        let mut trailing = 0;
        while trailing < 7 && (self.days >> (trailing + 1) & 1) != 0 {
            trailing += 1;
        }
        (STATS_FIRST_DAY..=STATS_LAST_DAY).contains(&self.day) && trailing == self.run.min(7)
    }
}
pub fn encode_streak(streak: &StatsStreak) -> [u8; STREAK_BYTES] {
    let mut b = [0; STREAK_BYTES];
    put32(&mut b, 0, STREAK_MAGIC);
    put32(&mut b, 4, 1);
    b[12..14].copy_from_slice(&streak.day.to_le_bytes());
    b[14..16].copy_from_slice(&streak.run.to_le_bytes());
    put32(&mut b, 16, streak.before);
    b[20] = streak.days;
    let sum = fnv1a(&b[..24]);
    put32(&mut b, 24, sum);
    b
}
pub fn decode_streak(b: &[u8]) -> Option<StatsStreak> {
    if b.len() != STREAK_BYTES
        || classify_stats_record(b, STREAK_MAGIC) != StatsDecode::Valid
        || b[21..24] != [0; 3]
    {
        return None;
    }
    let read = StatsStreak {
        day: u16::from_le_bytes([b[12], b[13]]),
        run: u16::from_le_bytes([b[14], b[15]]),
        before: get32(b, 16),
        days: b[20],
    };
    read.consistent().then_some(read)
}
fn month_of(day: u16) -> u16 {
    let date = stats_date(day);
    date.year * 12 + u16::from(date.month) - 1
}
fn block<'a>(
    ledger: &'a StatsLedger,
    archive: Option<&'a StatsMonth>,
    month: u16,
) -> Option<&'a StatsMonth> {
    if ledger.month().month == month {
        Some(ledger.month())
    } else {
        archive.filter(|b| b.month == month)
    }
}
pub fn sleep_seconds_before(
    ledger: &StatsLedger,
    archive: Option<&StatsMonth>,
    day: u16,
) -> Option<u32> {
    let first = month_of(day);
    let mut after = 0u32;
    if ledger.newest_month() != MONTH_NONE {
        for month in first.max(ledger.oldest_month())..=ledger.newest_month() {
            let b = block(ledger, archive, month)?;
            let start = if month == first {
                usize::from(stats_date(day).day - 1)
            } else {
                0
            };
            for &seconds in &b.day_seconds[start..] {
                after = after.wrapping_add(u32::from(seconds));
            }
        }
    }
    ledger.total_seconds().checked_sub(after)
}
pub fn sleep_streak(
    ledger: &StatsLedger,
    summary: &StatsSummary,
    recent_days: u8,
    today: StatsClock,
) -> StatsStreak {
    if !summary.dated || !today.trusted() {
        return StatsStreak::default();
    }
    let Some(before) = sleep_seconds_before(ledger, None, today.day) else {
        return StatsStreak::default();
    };
    let cache = StatsStreak {
        day: today.day,
        days: recent_days,
        before,
        run: summary
            .current_streak
            .wrapping_sub(u16::from(recent_days & 1)),
    };
    if cache.consistent() {
        cache
    } else {
        StatsStreak::default()
    }
}
pub fn sleep_archive_month(ledger: &StatsLedger, today: StatsClock, whole_month: bool) -> u16 {
    if !today.trusted() || (!whole_month && today.day_of_month > 6) {
        return MONTH_NONE;
    }
    let previous = today.month.wrapping_sub(1);
    if previous == ledger.month().month
        || previous < ledger.oldest_month()
        || previous > ledger.newest_month()
    {
        MONTH_NONE
    } else {
        previous
    }
}
pub fn sleep_today_archived(ledger: &StatsLedger, today: StatsClock) -> bool {
    today.month != ledger.month().month
        && today.month >= ledger.oldest_month()
        && today.month <= ledger.newest_month()
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SleepWeek {
    pub days: u8,
    pub streak: u16,
}
pub fn sleep_week(
    ledger: &StatsLedger,
    cache: &StatsStreak,
    archive: Option<&StatsMonth>,
    today: StatsClock,
    unsaved_today: u32,
) -> SleepWeek {
    let mut week = SleepWeek::default();
    if !today.trusted() {
        return week;
    }
    let stamped = cache.stamped() && cache.day <= today.day && today.day - cache.day < 7;
    for i in 0..7u16 {
        let day = today.day.wrapping_sub(i);
        let mut seconds = block(ledger, archive, month_of(day)).map_or(0, |b| {
            u32::from(b.day_seconds[usize::from(stats_date(day).day - 1)])
        });
        if i == 0 {
            seconds = seconds.wrapping_add(unsaved_today);
        }
        let cached = stamped
            && day <= cache.day
            && cache.day - day < 8
            && (cache.days >> (cache.day - day) & 1) != 0;
        if seconds >= u32::from(STREAK_SECONDS) || cached {
            week.days |= 1 << i;
        }
    }
    if week.days == 0x7f
        && stamped
        && sleep_seconds_before(ledger, archive, cache.day) == Some(cache.before)
    {
        week.streak = cache.run.wrapping_add(today.day - cache.day + 1);
    }
    week
}
pub fn reading_habit(
    ledger: &StatsLedger,
    archive: Option<&StatsMonth>,
    today: StatsClock,
    unsaved_today: u32,
) -> ReadingHabit {
    if !today.trusted() || sleep_today_archived(ledger, today) {
        return ReadingHabit::None;
    }
    let previous = today.month.wrapping_sub(1);
    let archived = sleep_archive_month(ledger, today, true);
    let last = archive.filter(|b| archived != MONTH_NONE && b.month == archived);
    if archived != MONTH_NONE && last.is_none() {
        return ReadingHabit::None;
    }
    let mut buckets = [0u32; 4];
    for b in [
        Some(ledger.month()).filter(|b| b.month == today.month || b.month == previous),
        last,
    ]
    .into_iter()
    .flatten()
    {
        for (sum, seconds) in buckets.iter_mut().zip(b.bucket_seconds) {
            *sum = sum.saturating_add(seconds);
        }
    }
    let now = &mut buckets[stats_bucket(today.hour)];
    *now = now.saturating_add(unsaved_today);
    let mut most = 0;
    for i in 1..4 {
        if buckets[i] > buckets[most] {
            most = i;
        }
    }
    if buckets[most] < 1800 {
        ReadingHabit::None
    } else {
        [
            ReadingHabit::Morning,
            ReadingHabit::Afternoon,
            ReadingHabit::Evening,
            ReadingHabit::Night,
        ][most]
    }
}
