// SPDX-License-Identifier: MIT
//! Pure reading-statistics graph aggregation.
#![forbid(unsafe_code)]
use crate::crossink_stats::{
    CrossInkFile, GLOBAL_BYTES, classify_cross_ink_global, valid_cross_ink_book,
};
use crate::reading_stats::{
    CROSS_INK_HISTORY_DAYS, CrossInkHistory, DAY_UNSET, FIRST_DAY, LAST_DAY, MONTH_NONE,
    StatsLedger, StatsMonth, empty_stats_month,
};
use crate::stats_clock::{stats_clock_for_day, stats_date};
use crate::wire::get32;
pub const STATS_GRAPH_MAX_DAYS: usize = 42;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum StatsDayKind {
    #[default]
    Unrecorded,
    Timed,
    Untimed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatsDaysGraph {
    pub last_day: u16,
    pub count: u8,
    pub kinds: [StatsDayKind; STATS_GRAPH_MAX_DAYS],
    pub seconds: [u32; STATS_GRAPH_MAX_DAYS],
}
impl Default for StatsDaysGraph {
    fn default() -> Self {
        Self {
            last_day: DAY_UNSET,
            count: 0,
            kinds: [StatsDayKind::Unrecorded; STATS_GRAPH_MAX_DAYS],
            seconds: [0; STATS_GRAPH_MAX_DAYS],
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatsPeriodsGraph {
    pub seconds: [u32; 4],
    pub unknown_seconds: u32,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatsGraphs {
    pub days: StatsDaysGraph,
    pub periods: StatsPeriodsGraph,
}
/// Mutable output preserves the reference callback contract, including wrong-month payloads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphMonthRead {
    Loaded,
    Absent,
    Unreadable,
}
pub fn stats_unknown_seconds(periods: &[u32; 4], total_seconds: u64) -> u32 {
    let split: u64 = periods.iter().map(|&v| u64::from(v)).sum();
    let rest = total_seconds.saturating_sub(split);
    if rest < 60 {
        0
    } else {
        rest.min(u64::from(u32::MAX)) as u32
    }
}
pub fn stats_window_end(newest: u16, oldest: u16, count: u32, back: u32) -> u16 {
    if count == 0 {
        return newest;
    }
    let windows = if oldest != 0 && oldest <= newest {
        u32::from(newest - oldest) / count + 1
    } else {
        1
    };
    u32::from(newest).wrapping_sub((back % windows).wrapping_mul(count)) as u16
}
pub const fn stats_weekday(day: u16) -> usize {
    (day as usize + 5) % 7
}
fn month_of(day: u16) -> u16 {
    let d = stats_date(day);
    d.year * 12 + u16::from(d.month) - 1
}
fn first_of_month(month: u16) -> u16 {
    // statsClock rejects years outside 2026..2099, including the zero sentinel.
    if !(24312..=25199).contains(&month) {
        return 0;
    }
    // Civil arithmetic shared with the clock, then the same supported-day validation.
    let day = crate::stats_clock::days_from_civil(month / 12, (month % 12 + 1) as u8, 1) - 10957;
    stats_clock_for_day(day as u16, 0).day
}
struct MonthReader<'a, F> {
    ledger: &'a StatsLedger,
    load: F,
    cached: StatsMonth,
}
impl<'a, F: FnMut(u16, &mut StatsMonth) -> GraphMonthRead> MonthReader<'a, F> {
    fn new(ledger: &'a StatsLedger, load: F) -> Self {
        Self {
            ledger,
            load,
            cached: StatsMonth::default(),
        }
    }
    fn month(&mut self, month: u16) -> &StatsMonth {
        if month == self.ledger.month().month {
            return self.ledger.month();
        }
        if self.cached.month != month {
            self.cached = empty_stats_month(month);
            if (self.load)(month, &mut self.cached) != GraphMonthRead::Loaded {
                self.cached = empty_stats_month(month);
            }
            self.cached.month = month;
        }
        &self.cached
    }
    fn first_dated_day(&mut self) -> u16 {
        let oldest = self.ledger.oldest_month();
        if oldest == MONTH_NONE {
            return DAY_UNSET;
        }
        let first = first_of_month(oldest);
        for (i, &s) in self.month(oldest).day_seconds.iter().enumerate() {
            if s != 0 {
                return first.wrapping_add(i as u16);
            }
        }
        first
    }
}
pub fn library_oldest_day(ledger: &StatsLedger, cross_ink: &CrossInkHistory) -> u16 {
    let mut oldest = if ledger.oldest_month() == MONTH_NONE {
        0
    } else {
        first_of_month(ledger.oldest_month())
    };
    if !cross_ink.empty() && cross_ink.newest_day >= u32::from(FIRST_DAY) {
        let first = cross_ink
            .newest_day
            .wrapping_sub((CROSS_INK_HISTORY_DAYS - 1) as u32)
            .max(u32::from(FIRST_DAY)) as u16;
        if oldest == 0 || first < oldest {
            oldest = first;
        }
    }
    oldest
}
pub fn library_days_graph(
    ledger: &StatsLedger,
    load: impl FnMut(u16, &mut StatsMonth) -> GraphMonthRead,
    cross_ink: &CrossInkHistory,
    weekdays: &[u32; 7],
    last_day: u16,
    count: u32,
) -> StatsDaysGraph {
    let mut graph = StatsDaysGraph::default();
    if !(FIRST_DAY..=LAST_DAY).contains(&last_day) {
        return graph;
    }
    graph.last_day = last_day;
    graph.count = count.min(STATS_GRAPH_MAX_DAYS as u32) as u8;
    let mut read_on = [0u16; 7];
    if !cross_ink.empty() {
        for back in 0..CROSS_INK_HISTORY_DAYS as u32 {
            if back > cross_ink.newest_day {
                break;
            }
            let day = cross_ink.newest_day - back;
            if cross_ink.read(day) {
                read_on[stats_weekday(day as u16)] += 1;
            }
        }
    }
    let mut months = MonthReader::new(ledger, load);
    let first = months.first_dated_day();
    for slot in 0..usize::from(graph.count) {
        let day = last_day.wrapping_sub(u16::from(graph.count) - 1 - slot as u16);
        let month = month_of(day);
        let known = first != DAY_UNSET && day >= first;
        let mut seconds = 0;
        if known && month <= ledger.newest_month() {
            seconds =
                u32::from(months.month(month).day_seconds[usize::from(stats_date(day).day - 1)]);
        }
        let cross_known = !cross_ink.empty()
            && u32::from(day) <= cross_ink.newest_day
            && cross_ink.newest_day - u32::from(day) < CROSS_INK_HISTORY_DAYS as u32;
        if seconds == 0 && cross_ink.read(u32::from(day)) {
            graph.kinds[slot] = StatsDayKind::Untimed;
            let weekday = stats_weekday(day);
            if read_on[weekday] != 0 {
                seconds = (weekdays[weekday] / u32::from(read_on[weekday])).min(86400);
            }
        } else {
            graph.kinds[slot] = if known || cross_known {
                StatsDayKind::Timed
            } else {
                StatsDayKind::Unrecorded
            };
        }
        graph.seconds[slot] = seconds;
    }
    graph
}
pub fn library_periods_graph(
    ledger: &StatsLedger,
    load: impl FnMut(u16, &mut StatsMonth) -> GraphMonthRead,
    cross_ink: &[u32; 4],
    total_seconds: u32,
) -> StatsPeriodsGraph {
    let mut graph = StatsPeriodsGraph {
        seconds: *cross_ink,
        unknown_seconds: 0,
    };
    if ledger.oldest_month() != MONTH_NONE {
        let mut months = MonthReader::new(ledger, load);
        for month in ledger.oldest_month()..=ledger.newest_month() {
            for (out, &s) in graph
                .seconds
                .iter_mut()
                .zip(&months.month(month).bucket_seconds)
            {
                *out = out.wrapping_add(s);
            }
        }
    }
    graph.unknown_seconds = stats_unknown_seconds(&graph.seconds, u64::from(total_seconds));
    graph
}
pub fn cross_ink_global_periods(bytes: &[u8], out: &mut [u32; 4]) -> bool {
    if bytes.len() != GLOBAL_BYTES || classify_cross_ink_global(bytes) != CrossInkFile::Valid {
        return false;
    }
    for (i, v) in out.iter_mut().enumerate() {
        *v = get32(bytes, 17 + 4 * i);
    }
    true
}
pub fn cross_ink_global_weekdays(bytes: &[u8], out: &mut [u32; 7]) -> bool {
    if bytes.len() != GLOBAL_BYTES || classify_cross_ink_global(bytes) != CrossInkFile::Valid {
        return false;
    }
    for (i, v) in out.iter_mut().enumerate() {
        *v = get32(bytes, 33 + 4 * i);
    }
    true
}
pub fn cross_ink_book_periods(bytes: &[u8], out: &mut [u32; 4]) -> bool {
    if bytes.len() < 69 || !valid_cross_ink_book(bytes) {
        return false;
    }
    for (i, v) in out.iter_mut().enumerate() {
        *v = get32(bytes, 25 + 4 * i);
    }
    true
}
#[cfg(test)]
#[path = "book_graph_tests.rs"]
mod tests;
