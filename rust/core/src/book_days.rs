// SPDX-License-Identifier: MIT
//! NWTD/NWTE book-day wire records and pure reading folds.
#![forbid(unsafe_code)]
use crate::crossink_stats::valid_cross_ink_book;
use crate::reading_stats::{
    DAY_UNSET, FIRST_DAY, LAST_DAY, StatsCounters, StatsDecode, StatsDigest, StatsFold,
    classify_stats_record,
};
use crate::stats_clock::{StatsClock, stats_bucket};
use crate::stats_graph_data::{
    STATS_GRAPH_MAX_DAYS, StatsDayKind, StatsDaysGraph, StatsPeriodsGraph, cross_ink_book_periods,
    stats_window_end,
};
use crate::wire::{fnv1a, get32, put32};
pub const BOOK_BLOCK_DAYS: usize = 62;
pub const BOOK_DAYS_BYTES: usize = 232;
pub const BOOK_ARCHIVE_BYTES: usize = 176;
pub const BOOK_DAYS_MAGIC: u32 = 0x4454574e;
pub const BOOK_ARCHIVE_MAGIC: u32 = 0x4554574e;
pub const BOOK_IMPORT_CHECKED: u16 = 1;
pub const fn book_block(day: u16) -> u16 {
    day / BOOK_BLOCK_DAYS as u16
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BookDayBlock {
    pub block: u16,
    pub seconds: [u16; BOOK_BLOCK_DAYS],
}
impl Default for BookDayBlock {
    fn default() -> Self {
        Self {
            block: 0,
            seconds: [0; BOOK_BLOCK_DAYS],
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BookImported {
    pub seconds: u32,
    pub pages: u32,
    pub sessions: u32,
    pub periods: [u32; 4],
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BookDays {
    pub since_day: u16,
    pub flags: u16,
    pub watermark: u64,
    pub period_seconds: [u32; 4],
    pub imported: BookImported,
    pub current: BookDayBlock,
}
fn get16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
fn put16(b: &mut [u8], at: usize, n: u16) {
    b[at..at + 2].copy_from_slice(&n.to_le_bytes());
}
fn dated(day: u16) -> bool {
    (FIRST_DAY..=LAST_DAY).contains(&day)
}
fn valid_block(block: u16) -> bool {
    (book_block(FIRST_DAY)..=book_block(LAST_DAY)).contains(&block)
}
fn seal(b: &mut [u8], magic: u32, generation: u32) {
    put32(b, 0, magic);
    put32(b, 4, 1);
    put32(b, 8, generation);
    let n = b.len() - 4;
    let checksum = fnv1a(&b[..n]);
    put32(b, n, checksum);
}
fn same_digest(b: &[u8], digest: &StatsDigest) -> bool {
    b == digest && digest.iter().any(|&v| v != 0)
}
fn put_days(b: &mut [u8], at: usize, block: &BookDayBlock) {
    for (i, &s) in block.seconds.iter().enumerate() {
        put16(b, at + 2 * i, s);
    }
}
fn get_days(b: &[u8], at: usize, block: &mut BookDayBlock) {
    for (i, s) in block.seconds.iter_mut().enumerate() {
        *s = get16(b, at + 2 * i);
    }
}
pub fn encode_book_days(
    digest: &StatsDigest,
    days: &BookDays,
    generation: u32,
) -> [u8; BOOK_DAYS_BYTES] {
    let mut b = [0; BOOK_DAYS_BYTES];
    b[12..44].copy_from_slice(digest);
    put16(&mut b, 44, days.since_day);
    put16(&mut b, 46, days.current.block);
    put16(&mut b, 48, days.flags);
    b[52..60].copy_from_slice(&days.watermark.to_le_bytes());
    for (i, &s) in days.period_seconds.iter().enumerate() {
        put32(&mut b, 60 + 4 * i, s);
    }
    put32(&mut b, 76, days.imported.seconds);
    put32(&mut b, 80, days.imported.pages);
    put32(&mut b, 84, days.imported.sessions);
    for (i, &s) in days.imported.periods.iter().enumerate() {
        put32(&mut b, 88 + 4 * i, s);
    }
    put_days(&mut b, 104, &days.current);
    seal(&mut b, BOOK_DAYS_MAGIC, generation);
    b
}
pub fn decode_book_days(b: &[u8], digest: &StatsDigest, days: &mut BookDays) -> bool {
    if b.len() != BOOK_DAYS_BYTES
        || classify_stats_record(b, BOOK_DAYS_MAGIC) != StatsDecode::Valid
        || !same_digest(&b[12..44], digest)
        || get16(b, 50) != 0
    {
        return false;
    }
    let mut read = BookDays {
        since_day: get16(b, 44),
        flags: get16(b, 48),
        watermark: u64::from_le_bytes(b[52..60].try_into().expect("fixed extent")),
        ..BookDays::default()
    };
    read.current.block = get16(b, 46);
    for (i, s) in read.period_seconds.iter_mut().enumerate() {
        *s = get32(b, 60 + 4 * i);
    }
    read.imported.seconds = get32(b, 76);
    read.imported.pages = get32(b, 80);
    read.imported.sessions = get32(b, 84);
    for (i, s) in read.imported.periods.iter_mut().enumerate() {
        *s = get32(b, 88 + 4 * i);
    }
    get_days(b, 104, &mut read.current);
    if read.flags & !BOOK_IMPORT_CHECKED != 0 {
        return false;
    }
    if read.since_day == DAY_UNSET {
        if read.current.block != 0
            || read.current.seconds.iter().any(|&s| s != 0)
            || read.period_seconds != [0; 4]
        {
            return false;
        }
    } else {
        if !dated(read.since_day)
            || !valid_block(read.current.block)
            || read.current.block < book_block(read.since_day)
        {
            return false;
        }
        if read.current.block == book_block(read.since_day)
            && read.current.seconds[..usize::from(read.since_day) % BOOK_BLOCK_DAYS]
                .iter()
                .any(|&s| s != 0)
        {
            return false;
        }
    }
    *days = read;
    true
}
pub fn encode_book_archive(
    digest: &StatsDigest,
    block: &BookDayBlock,
    generation: u32,
) -> [u8; BOOK_ARCHIVE_BYTES] {
    let mut b = [0; BOOK_ARCHIVE_BYTES];
    b[12..44].copy_from_slice(digest);
    put16(&mut b, 44, block.block);
    put_days(&mut b, 48, block);
    seal(&mut b, BOOK_ARCHIVE_MAGIC, generation);
    b
}
pub fn decode_book_archive(
    b: &[u8],
    digest: &StatsDigest,
    block: u16,
    out: &mut BookDayBlock,
) -> bool {
    if b.len() != BOOK_ARCHIVE_BYTES
        || classify_stats_record(b, BOOK_ARCHIVE_MAGIC) != StatsDecode::Valid
        || !same_digest(&b[12..44], digest)
        || get16(b, 44) != block
        || !valid_block(block)
        || get16(b, 46) != 0
    {
        return false;
    }
    out.block = block;
    get_days(b, 48, out);
    true
}
fn credit(days: &BookDays, before: &StatsCounters, counters: &StatsCounters) -> (u64, u32) {
    let rollback = counters.milliseconds < before.milliseconds
        || counters.pages < before.pages
        || counters.sessions < before.sessions;
    if rollback {
        return (counters.milliseconds, 0);
    }
    let base = if counters.milliseconds < days.watermark {
        before.milliseconds
    } else {
        days.watermark.max(before.milliseconds)
    };
    let seconds = (counters.milliseconds - base) / 1000;
    (
        base + seconds * 1000,
        seconds.min(u64::from(u32::MAX)) as u32,
    )
}
fn dated_fold(days: &BookDays, clock: StatsClock) -> bool {
    clock.trusted() && (days.since_day == DAY_UNSET || clock.day >= days.since_day)
}
pub fn book_fold_block(days: &BookDays, before: &StatsCounters, fold: &StatsFold) -> u16 {
    if credit(days, before, &fold.counters).1 == 0 || !dated_fold(days, fold.clock) {
        return 0;
    }
    let block = book_block(fold.clock.day);
    if days.current.block != 0 && block != days.current.block {
        block
    } else {
        0
    }
}
pub fn fold_book_days(
    days: &mut BookDays,
    ledger_before: &StatsCounters,
    fold: &StatsFold,
    target: Option<&BookDayBlock>,
) -> bool {
    let before = *days;
    let (watermark, seconds) = credit(days, ledger_before, &fold.counters);
    days.watermark = watermark;
    let clock = fold.clock;
    if seconds != 0 && dated_fold(days, clock) {
        let block = book_block(clock.day);
        if days.current.block == 0 {
            days.current = BookDayBlock {
                block,
                ..BookDayBlock::default()
            };
        } else if block != days.current.block {
            if let Some(target) = target {
                if target.block == block {
                    days.current = *target;
                }
            }
        }
        if days.current.block == block {
            if days.since_day == DAY_UNSET {
                days.since_day = clock.day;
            }
            let day = &mut days.current.seconds[usize::from(clock.day) % BOOK_BLOCK_DAYS];
            *day = u64::from(*day)
                .saturating_add(u64::from(seconds))
                .min(u64::from(u16::MAX)) as u16;
            let period = &mut days.period_seconds[stats_bucket(clock.hour)];
            *period = period.saturating_add(seconds);
        }
    }
    *days != before
}
pub fn cross_ink_book_totals(b: &[u8], sum: &mut BookImported) -> bool {
    if !valid_cross_ink_book(b) {
        return false;
    }
    sum.sessions = sum.sessions.saturating_add(u32::from(get16(b, 1)));
    sum.seconds = sum.seconds.saturating_add(get32(b, 3));
    sum.pages = sum.pages.saturating_add(get32(b, 7));
    let mut periods = [0; 4];
    if cross_ink_book_periods(b, &mut periods) {
        for (out, s) in sum.periods.iter_mut().zip(periods) {
            *out = out.saturating_add(s);
        }
    }
    true
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BookBlockRead {
    Loaded,
    Absent,
    Unreadable,
}
pub fn book_days_graph(
    days: &BookDays,
    mut load: impl FnMut(u16, &mut BookDayBlock) -> BookBlockRead,
    last_day: u16,
    count: u32,
) -> StatsDaysGraph {
    let mut graph = StatsDaysGraph::default();
    if !dated(last_day) {
        return graph;
    }
    graph.last_day = last_day;
    graph.count = count.min(STATS_GRAPH_MAX_DAYS as u32) as u8;
    let mut held = BookDayBlock::default();
    let mut held_read = BookBlockRead::Unreadable;
    for slot in 0..usize::from(graph.count) {
        let day = last_day.wrapping_sub(u16::from(graph.count) - 1 - slot as u16);
        if days.since_day == DAY_UNSET || day < days.since_day {
            continue;
        }
        let block = book_block(day);
        let source = if block == days.current.block {
            &days.current
        } else {
            if held.block != block {
                held = BookDayBlock {
                    block,
                    ..BookDayBlock::default()
                };
                held_read = load(block, &mut held);
                if held_read != BookBlockRead::Loaded {
                    held = BookDayBlock {
                        block,
                        ..BookDayBlock::default()
                    };
                }
            }
            if held_read == BookBlockRead::Unreadable {
                continue;
            }
            &held
        };
        graph.kinds[slot] = StatsDayKind::Timed;
        graph.seconds[slot] = u32::from(source.seconds[usize::from(day) % BOOK_BLOCK_DAYS]);
    }
    graph
}
pub fn book_window_end(today: u16, last_read: u16, since_day: u16, count: u32, back: u32) -> u16 {
    let mut newest = if dated(today) { today } else { DAY_UNSET };
    if dated(last_read)
        && (newest == DAY_UNSET || u32::from(last_read).wrapping_add(count) <= u32::from(newest))
    {
        newest = last_read;
    }
    if newest == DAY_UNSET {
        return newest;
    }
    stats_window_end(
        newest,
        if since_day == DAY_UNSET {
            newest
        } else {
            since_day.min(newest)
        },
        count,
        back,
    )
}
pub fn book_periods_graph(days: &BookDays) -> StatsPeriodsGraph {
    let mut graph = StatsPeriodsGraph::default();
    for (i, s) in graph.seconds.iter_mut().enumerate() {
        *s = days.period_seconds[i].saturating_add(days.imported.periods[i]);
    }
    graph
}
