// SPDX-License-Identifier: MIT
//! Pure reading-statistics ledger, wire codec and summary authority.
//! MIT, see the repository LICENSE.
use crate::progress::Progress;
use crate::stats_clock::{StatsClock, days_from_civil, days_in_month, stats_bucket, stats_date};
use crate::wire::{fnv1a, get32, put32};

pub const DAY_UNSET: u16 = 0;
pub const DAY_UNDATED: u16 = 0xfffe;
pub const DAY_UNKNOWN: u16 = 0xffff;
pub use crate::stats_clock::{STATS_FIRST_DAY as FIRST_DAY, STATS_LAST_DAY as LAST_DAY};
pub const MONTH_NONE: u16 = 0;
pub const FIRST_MONTH: u16 = 24312;
pub const LAST_MONTH: u16 = 25199;
pub const PERMILLE_UNKNOWN: u16 = 0xffff;
pub const MANUAL_FINISHED: u16 = 1;
pub const CROSS_INK_FINISHED: u16 = 2;
pub const STREAK_SECONDS: u16 = 60;
pub const LEDGER_BYTES: usize = 180;
pub const MONTH_BYTES: usize = 104;
pub const BOOK_BYTES: usize = 64;
pub const LEDGER_MAGIC: u32 = 0x5354_574e;
pub const MONTH_MAGIC: u32 = 0x4d54_574e;
pub const BOOK_MAGIC: u32 = 0x4254_574e;
pub type StatsDigest = [u8; 32];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatsCounters {
    pub milliseconds: u64,
    pub pages: u32,
    pub sessions: u32,
}
pub fn stats_counters(progress: &Progress) -> StatsCounters {
    StatsCounters {
        milliseconds: progress.active_milliseconds,
        pages: progress.forward_pages,
        sessions: progress.qualified_sessions,
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BookFacts {
    pub first_day: u16,
    pub last_day: u16,
    pub end_day: u16,
    pub manual_finish_day: u16,
    pub start_permille: u16,
    pub flags: u16,
}
impl Default for BookFacts {
    fn default() -> Self {
        Self {
            first_day: 0,
            last_day: 0,
            end_day: 0,
            manual_finish_day: 0,
            start_permille: PERMILLE_UNKNOWN,
            flags: 0,
        }
    }
}
pub fn unknown_book_facts() -> BookFacts {
    BookFacts {
        first_day: DAY_UNKNOWN,
        last_day: DAY_UNKNOWN,
        end_day: DAY_UNKNOWN,
        ..BookFacts::default()
    }
}
pub fn finished(facts: &BookFacts) -> bool {
    facts.end_day != DAY_UNSET || facts.flags & MANUAL_FINISHED != 0
}
pub fn counts_finished(facts: &BookFacts) -> bool {
    finished(facts) && facts.flags & CROSS_INK_FINISHED == 0
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BookFinish {
    No,
    Yes,
    Unknown,
}
pub fn book_finish(facts: &BookFacts) -> BookFinish {
    if facts.end_day == DAY_UNKNOWN {
        BookFinish::Unknown
    } else if finished(facts) {
        BookFinish::Yes
    } else {
        BookFinish::No
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatsMonth {
    pub month: u16,
    pub pages: u32,
    pub bucket_seconds: [u32; 4],
    pub day_seconds: [u16; 31],
}
pub fn empty_stats_month(month: u16) -> StatsMonth {
    StatsMonth {
        month,
        ..StatsMonth::default()
    }
}

/// Only seed, fold, handover and validated decode can construct or alter totals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatsLedger {
    seconds: u32,
    pages: u32,
    sessions: u32,
    books_finished: u16,
    oldest: u16,
    newest: u16,
    month: StatsMonth,
    lease: StatsDigest,
    watermark: StatsCounters,
    facts: BookFacts,
}
impl StatsLedger {
    pub fn total_seconds(&self) -> u32 {
        self.seconds
    }
    pub fn total_pages(&self) -> u32 {
        self.pages
    }
    pub fn total_sessions(&self) -> u32 {
        self.sessions
    }
    pub fn books_finished(&self) -> u16 {
        self.books_finished
    }
    pub fn oldest_month(&self) -> u16 {
        self.oldest
    }
    pub fn newest_month(&self) -> u16 {
        self.newest
    }
    pub fn month(&self) -> &StatsMonth {
        &self.month
    }
    pub fn leased(&self) -> bool {
        self.lease != [0; 32]
    }
    pub fn leases(&self, digest: &StatsDigest) -> bool {
        self.leased() && self.lease == *digest
    }
    pub fn lease_digest(&self) -> &StatsDigest {
        &self.lease
    }
    pub fn watermark(&self) -> &StatsCounters {
        &self.watermark
    }
    pub fn lease_facts(&self) -> &BookFacts {
        &self.facts
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatsLease {
    pub digest: StatsDigest,
    pub counters: StatsCounters,
    pub facts: BookFacts,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatsSeed {
    pub milliseconds: u64,
    pub pages: u64,
    pub sessions: u64,
    pub finished_books: u32,
    pub open: bool,
    pub lease: StatsLease,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StatsMark {
    #[default]
    None,
    Finished,
    NotFinished,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatsFold {
    pub counters: StatsCounters,
    pub clock: StatsClock,
    pub end_reached: bool,
    pub mark: StatsMark,
    pub permille: u16,
    pub cross_ink_finished: bool,
}
impl Default for StatsFold {
    fn default() -> Self {
        Self {
            counters: StatsCounters::default(),
            clock: StatsClock::default(),
            end_reached: false,
            mark: StatsMark::None,
            permille: PERMILLE_UNKNOWN,
            cross_ink_finished: false,
        }
    }
}
fn initial_facts(lease: &StatsLease) -> BookFacts {
    let mut facts = lease.facts;
    if facts.first_day == DAY_UNSET && lease.counters.sessions != 0 {
        facts.first_day = DAY_UNDATED;
    }
    facts
}
fn cap32(value: u64) -> u32 {
    value.min(u64::from(u32::MAX)) as u32
}
fn cap16(value: u64) -> u16 {
    value.min(u64::from(u16::MAX)) as u16
}
pub fn seed_ledger(seed: &StatsSeed) -> StatsLedger {
    let mut ledger = StatsLedger {
        seconds: cap32(seed.milliseconds / 1000),
        pages: cap32(seed.pages),
        sessions: cap32(seed.sessions),
        books_finished: cap16(u64::from(seed.finished_books)),
        oldest: 0,
        newest: 0,
        month: StatsMonth::default(),
        lease: [0; 32],
        watermark: StatsCounters::default(),
        facts: BookFacts::default(),
    };
    if seed.open && seed.lease.digest != [0; 32] {
        ledger.lease = seed.lease.digest;
        ledger.watermark = seed.lease.counters;
        ledger.facts = initial_facts(&seed.lease);
    }
    ledger
}
fn rollback(counters: StatsCounters, was: StatsCounters) -> bool {
    counters.milliseconds < was.milliseconds
        || counters.pages < was.pages
        || counters.sessions < was.sessions
}
pub fn fold_needs_month(current: &StatsLedger, fold: &StatsFold) -> bool {
    if !current.leased()
        || !fold.clock.trusted()
        || fold.clock.month == current.month.month
        || rollback(fold.counters, current.watermark)
    {
        return false;
    }
    fold.counters.milliseconds - current.watermark.milliseconds >= 1000
        || fold.counters.pages > current.watermark.pages
        || fold.counters.sessions > current.watermark.sessions
}
pub fn fold_ledger(
    current: &StatsLedger,
    fold: &StatsFold,
    target: Option<&StatsMonth>,
) -> StatsLedger {
    if !current.leased() {
        return *current;
    }
    let mut next = *current;
    let was = current.watermark;
    let counters = fold.counters;
    let clock = fold.clock;
    let mut credited = false;
    if rollback(counters, was) {
        next.watermark = counters;
    } else {
        let seconds = (counters.milliseconds - was.milliseconds) / 1000;
        let pages = counters.pages - was.pages;
        let sessions = counters.sessions - was.sessions;
        next.watermark = StatsCounters {
            milliseconds: was.milliseconds + seconds * 1000,
            pages: counters.pages,
            sessions: counters.sessions,
        };
        credited = seconds != 0 || pages != 0 || sessions != 0;
        next.seconds = cap32(u64::from(next.seconds) + seconds);
        next.pages = next.pages.saturating_add(pages);
        next.sessions = next.sessions.saturating_add(sessions);
        let dated = credited
            && clock.trusted()
            && (clock.month == next.month.month
                || target.is_some_and(|month| month.month == clock.month));
        if dated {
            if clock.month != next.month.month {
                next.month = *target.expect("matched target month");
            }
            if next.oldest == MONTH_NONE || clock.month < next.oldest {
                next.oldest = clock.month;
            }
            if clock.month > next.newest {
                next.newest = clock.month;
            }
            // The reference requires a clock returned by stats_clock[_for_day].
            let day = &mut next.month.day_seconds[usize::from(clock.day_of_month) - 1];
            *day = cap16(u64::from(*day) + seconds);
            let bucket = &mut next.month.bucket_seconds[stats_bucket(clock.hour)];
            *bucket = cap32(u64::from(*bucket) + seconds);
            next.month.pages = next.month.pages.saturating_add(pages);
        }
    }
    let today = if clock.trusted() {
        clock.day
    } else {
        DAY_UNDATED
    };
    let facts = &mut next.facts;
    if credited {
        if facts.first_day == DAY_UNSET {
            facts.first_day = today;
            facts.start_permille = fold.permille;
        }
        facts.last_day = today;
    }
    if fold.end_reached && facts.end_day == DAY_UNSET {
        facts.end_day = today;
    }
    if fold.mark == StatsMark::Finished && facts.flags & MANUAL_FINISHED == 0 {
        facts.flags |= MANUAL_FINISHED;
        facts.manual_finish_day = today;
    } else if fold.mark == StatsMark::NotFinished && book_finish(facts) != BookFinish::Unknown {
        facts.flags &= !MANUAL_FINISHED;
        facts.manual_finish_day = DAY_UNSET;
        facts.end_day = DAY_UNSET;
    }
    if fold.cross_ink_finished {
        facts.flags |= CROSS_INK_FINISHED;
    }
    let was_counted = counts_finished(&current.facts);
    let is_counted = counts_finished(facts);
    if !was_counted && is_counted {
        next.books_finished = next.books_finished.saturating_add(1);
    }
    if was_counted && !is_counted && next.books_finished != 0 {
        next.books_finished -= 1;
    }
    next
}
pub fn handover_ledger(
    current: &StatsLedger,
    final_fold: Option<&StatsFold>,
    target: Option<&StatsMonth>,
    next: &StatsLease,
) -> StatsLedger {
    let mut handed = final_fold.map_or(*current, |fold| fold_ledger(current, fold, target));
    handed.lease = next.digest;
    handed.watermark = next.counters;
    handed.facts = initial_facts(next);
    handed
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StatsState {
    #[default]
    Unseeded,
    Seeded(StatsLedger),
    Unsupported,
    Unavailable,
}
impl StatsState {
    pub fn seeded(&self) -> Option<&StatsLedger> {
        if let Self::Seeded(ledger) = self {
            Some(ledger)
        } else {
            None
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatsDecode {
    Valid,
    Corrupt,
    Unsupported,
}
pub fn classify_stats_record(bytes: &[u8], magic: u32) -> StatsDecode {
    if bytes.len() < 16
        || get32(bytes, 0) != magic
        || get32(bytes, bytes.len() - 4) != fnv1a(&bytes[..bytes.len() - 4])
    {
        return StatsDecode::Corrupt;
    }
    match get32(bytes, 4) {
        1 => StatsDecode::Valid,
        2.. => StatsDecode::Unsupported,
        _ => StatsDecode::Corrupt,
    }
}
fn get16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}
fn put16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn valid_day(day: u16) -> bool {
    day == DAY_UNSET || day >= DAY_UNDATED || (FIRST_DAY..=LAST_DAY).contains(&day)
}
fn valid_month(month: u16) -> bool {
    (FIRST_MONTH..=LAST_MONTH).contains(&month)
}
fn put_facts(bytes: &mut [u8], facts: &BookFacts) {
    for (i, value) in [
        facts.first_day,
        facts.last_day,
        facts.end_day,
        facts.manual_finish_day,
        facts.start_permille,
        facts.flags,
    ]
    .into_iter()
    .enumerate()
    {
        put16(bytes, i * 2, value);
    }
}
fn get_facts(bytes: &[u8]) -> Option<BookFacts> {
    let facts = BookFacts {
        first_day: get16(bytes, 0),
        last_day: get16(bytes, 2),
        end_day: get16(bytes, 4),
        manual_finish_day: get16(bytes, 6),
        start_permille: get16(bytes, 8),
        flags: get16(bytes, 10),
    };
    if ![
        facts.first_day,
        facts.last_day,
        facts.end_day,
        facts.manual_finish_day,
    ]
    .into_iter()
    .all(valid_day)
        || (facts.start_permille > 1000 && facts.start_permille != PERMILLE_UNKNOWN)
        || facts.flags & !(MANUAL_FINISHED | CROSS_INK_FINISHED) != 0
    {
        return None;
    }
    Some(facts)
}
fn put_month_body(bytes: &mut [u8], month: &StatsMonth) {
    put32(bytes, 0, month.pages);
    for (i, value) in month.bucket_seconds.iter().enumerate() {
        put32(bytes, 4 + 4 * i, *value);
    }
    for (i, value) in month.day_seconds.iter().enumerate() {
        put16(bytes, 20 + 2 * i, *value);
    }
}
fn get_month_body(bytes: &[u8], number: u16) -> Option<StatsMonth> {
    let mut month = empty_stats_month(number);
    month.pages = get32(bytes, 0);
    for (i, value) in month.bucket_seconds.iter_mut().enumerate() {
        *value = get32(bytes, 4 + 4 * i);
    }
    for (i, value) in month.day_seconds.iter_mut().enumerate() {
        *value = get16(bytes, 20 + 2 * i);
    }
    let days = if number == MONTH_NONE {
        0
    } else {
        days_in_month(number)
    };
    if month.day_seconds[days..].iter().any(|value| *value != 0)
        || (number == MONTH_NONE && month != empty_stats_month(MONTH_NONE))
    {
        return None;
    }
    Some(month)
}
fn seal(bytes: &mut [u8], magic: u32, generation: u32) {
    put32(bytes, 0, magic);
    put32(bytes, 4, 1);
    put32(bytes, 8, generation);
    let end = bytes.len() - 4;
    let checksum = fnv1a(&bytes[..end]);
    put32(bytes, end, checksum);
}
pub fn encode_ledger(ledger: &StatsLedger, generation: u32) -> [u8; LEDGER_BYTES] {
    let mut bytes = [0; LEDGER_BYTES];
    put16(&mut bytes, 12, ledger.oldest);
    put16(&mut bytes, 14, ledger.newest);
    put32(&mut bytes, 16, ledger.seconds);
    put32(&mut bytes, 20, ledger.pages);
    put32(&mut bytes, 24, ledger.sessions);
    put16(&mut bytes, 28, ledger.books_finished);
    put16(&mut bytes, 30, ledger.month.month);
    bytes[32..64].copy_from_slice(&ledger.lease);
    bytes[64..72].copy_from_slice(&ledger.watermark.milliseconds.to_le_bytes());
    put32(&mut bytes, 72, ledger.watermark.pages);
    put32(&mut bytes, 76, ledger.watermark.sessions);
    put_facts(&mut bytes[80..92], &ledger.facts);
    put_month_body(&mut bytes[92..174], &ledger.month);
    seal(&mut bytes, LEDGER_MAGIC, generation);
    bytes
}
/// Failed decoding leaves the caller's prior state unchanged.
pub fn decode_ledger(bytes: &[u8], state: &mut StatsState) -> bool {
    if bytes.len() != LEDGER_BYTES
        || classify_stats_record(bytes, LEDGER_MAGIC) != StatsDecode::Valid
        || get16(bytes, 174) != 0
    {
        return false;
    }
    let month = get16(bytes, 30);
    if month != MONTH_NONE && !valid_month(month) {
        return false;
    }
    let oldest = get16(bytes, 12);
    let newest = get16(bytes, 14);
    let undated = oldest == MONTH_NONE && newest == MONTH_NONE && month == MONTH_NONE;
    if !undated
        && (!valid_month(oldest) || !valid_month(newest) || month < oldest || month > newest)
    {
        return false;
    }
    let Some(facts) = get_facts(&bytes[80..92]) else {
        return false;
    };
    let Some(block) = get_month_body(&bytes[92..174], month) else {
        return false;
    };
    let mut lease = [0; 32];
    lease.copy_from_slice(&bytes[32..64]);
    let ledger = StatsLedger {
        seconds: get32(bytes, 16),
        pages: get32(bytes, 20),
        sessions: get32(bytes, 24),
        books_finished: get16(bytes, 28),
        oldest,
        newest,
        month: block,
        lease,
        watermark: StatsCounters {
            milliseconds: u64::from_le_bytes(
                bytes[64..72].try_into().expect("fixed ledger extent"),
            ),
            pages: get32(bytes, 72),
            sessions: get32(bytes, 76),
        },
        facts,
    };
    if !ledger.leased()
        && (ledger.watermark != StatsCounters::default() || ledger.facts != BookFacts::default())
    {
        return false;
    }
    *state = StatsState::Seeded(ledger);
    true
}
pub fn encode_month(month: &StatsMonth, generation: u32) -> [u8; MONTH_BYTES] {
    let mut bytes = [0; MONTH_BYTES];
    put16(&mut bytes, 12, month.month);
    put_month_body(&mut bytes[16..98], month);
    seal(&mut bytes, MONTH_MAGIC, generation);
    bytes
}
pub fn decode_month(bytes: &[u8]) -> Option<StatsMonth> {
    if bytes.len() != MONTH_BYTES
        || classify_stats_record(bytes, MONTH_MAGIC) != StatsDecode::Valid
        || get16(bytes, 14) != 0
        || get16(bytes, 98) != 0
    {
        return None;
    }
    let number = get16(bytes, 12);
    if !valid_month(number) {
        return None;
    }
    get_month_body(&bytes[16..98], number)
}
pub fn encode_book_facts(
    digest: &StatsDigest,
    facts: &BookFacts,
    generation: u32,
) -> [u8; BOOK_BYTES] {
    let mut bytes = [0; BOOK_BYTES];
    bytes[12..44].copy_from_slice(digest);
    put_facts(&mut bytes[44..56], facts);
    seal(&mut bytes, BOOK_MAGIC, generation);
    bytes
}
pub fn decode_book_facts(bytes: &[u8], digest: &StatsDigest) -> Option<BookFacts> {
    if bytes.len() != BOOK_BYTES
        || classify_stats_record(bytes, BOOK_MAGIC) != StatsDecode::Valid
        || get32(bytes, 56) != 0
        || *digest == [0; 32]
        || bytes[12..44] != digest[..]
    {
        return None;
    }
    get_facts(&bytes[44..56])
}

pub const CROSS_INK_HISTORY_DAYS: usize = 730;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CrossInkHistory {
    pub seconds: u32,
    pub pages: u32,
    pub sessions: u32,
    pub finished: u32,
    pub newest_day: u32,
    pub days: [u8; CROSS_INK_HISTORY_DAYS.div_ceil(8)],
    pub longest_streak: u16,
}
impl Default for CrossInkHistory {
    fn default() -> Self {
        Self {
            seconds: 0,
            pages: 0,
            sessions: 0,
            finished: 0,
            newest_day: 0,
            days: [0; CROSS_INK_HISTORY_DAYS.div_ceil(8)],
            longest_streak: 0,
        }
    }
}
impl CrossInkHistory {
    pub fn empty(&self) -> bool {
        self.newest_day == 0 && self.days[0] & 1 == 0
    }
    pub fn read(&self, day: u32) -> bool {
        let back = self.newest_day.wrapping_sub(day);
        !self.empty()
            && day <= self.newest_day
            && back < CROSS_INK_HISTORY_DAYS as u32
            && self.days[back as usize / 8] >> (back % 8) & 1 != 0
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatsSummary {
    pub total_seconds: u32,
    pub sessions: u32,
    pub pages: u32,
    pub books_finished: u16,
    pub dated: bool,
    pub cross_ink: bool,
    pub today_seconds: u32,
    pub week_seconds: u32,
    pub current_streak: u16,
    pub longest_streak: u16,
}
/// Missing and failed loads both supply zero Newt days, as in the reference.
/// The distinct outcomes let a storage owner retain its own diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MonthLoad {
    Loaded(StatsMonth),
    Missing,
    Unavailable,
}
/// At most one callback per archived month in the inclusive ledger range;
/// never a callback for the current month or CrossInk-only months. No retry.
/// A decoded ledger bounds this to 887 archived months (2026 through 2099).
pub fn summarize_stats(
    ledger: &StatsLedger,
    today: StatsClock,
    mut load: impl FnMut(u16) -> MonthLoad,
    cross_ink: Option<&CrossInkHistory>,
    recent: Option<&mut u8>,
) -> StatsSummary {
    let mut summary = StatsSummary {
        total_seconds: ledger.seconds,
        sessions: ledger.sessions,
        pages: ledger.pages,
        books_finished: ledger.books_finished,
        ..StatsSummary::default()
    };
    // Only the CrossInk overload resets this output in the original interface.
    let mut recent = if cross_ink.is_some() { recent } else { None };
    if let Some(bits) = recent.as_deref_mut() {
        *bits = 0;
    }
    if let Some(cross) = cross_ink {
        summary.cross_ink = true;
        summary.total_seconds = summary.total_seconds.saturating_add(cross.seconds);
        summary.sessions = summary.sessions.saturating_add(cross.sessions);
        summary.pages = summary.pages.saturating_add(cross.pages);
        summary.books_finished =
            cap16(u64::from(summary.books_finished) + u64::from(cross.finished));
    }
    if !today.trusted() {
        return summary;
    }
    summary.dated = true;
    let (mut oldest, mut newest) = (ledger.oldest, ledger.newest);
    let cross_days = cross_ink.filter(|cross| !cross.empty());
    if let Some(cross) = cross_ink {
        summary.longest_streak = cross.longest_streak;
    }
    if let Some(cross) = cross_days {
        let last = stats_date(cross.newest_day as u16);
        let first = stats_date(if cross.newest_day < CROSS_INK_HISTORY_DAYS as u32 {
            0
        } else {
            cross
                .newest_day
                .wrapping_sub(CROSS_INK_HISTORY_DAYS as u32 - 1) as u16
        });
        let from = first.year * 12 + u16::from(first.month) - 1;
        let to = last.year * 12 + u16::from(last.month) - 1;
        if oldest == MONTH_NONE || from < oldest {
            oldest = from;
        }
        if newest == MONTH_NONE || to > newest {
            newest = to;
        }
    }
    if oldest == MONTH_NONE {
        return summary;
    }
    let (mut visited, mut run) = (0u32, 0u32);
    let mut current_open = true;
    for month in (oldest..=newest.max(today.month)).rev() {
        let block = if month == ledger.month.month {
            ledger.month
        } else if month > ledger.newest || month < ledger.oldest {
            empty_stats_month(month)
        } else {
            match load(month) {
                MonthLoad::Loaded(block) if block.month == month => block,
                _ => empty_stats_month(month),
            }
        };
        let start = days_from_civil(month / 12, (month % 12 + 1) as u8, 1) - 10957;
        for day in (1..=days_in_month(month)).rev() {
            let seconds = block.day_seconds[day - 1];
            let streak_day = seconds >= STREAK_SECONDS
                || cross_days
                    .is_some_and(|cross| cross.read((start as u32).wrapping_add(day as u32 - 1)));
            run = if streak_day { run + 1 } else { 0 };
            if run > u32::from(summary.longest_streak) {
                summary.longest_streak = run as u16;
            }
            if month > today.month
                || (month == today.month && day > usize::from(today.day_of_month))
            {
                continue;
            }
            if visited == 0 {
                summary.today_seconds = u32::from(seconds);
            }
            if visited < 7 {
                summary.week_seconds += u32::from(seconds);
            }
            if visited < 8 && streak_day {
                if let Some(bits) = recent.as_deref_mut() {
                    *bits |= 1u8 << visited;
                }
            }
            if streak_day && current_open {
                summary.current_streak = summary.current_streak.wrapping_add(1);
            }
            if !streak_day && visited > 0 {
                current_open = false;
            }
            visited += 1;
        }
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats_clock::stats_clock_for_day;
    fn leased() -> StatsLedger {
        seed_ledger(&StatsSeed {
            open: true,
            lease: StatsLease {
                digest: [7; 32],
                ..StatsLease::default()
            },
            ..StatsSeed::default()
        })
    }
    #[test]
    fn subsecond_carry_rollback_and_missing_target_are_independent() {
        let first = StatsFold {
            counters: StatsCounters {
                milliseconds: 1999,
                pages: 3,
                sessions: 1,
            },
            clock: stats_clock_for_day(FIRST_DAY, 5),
            permille: 100,
            ..StatsFold::default()
        };
        assert!(fold_needs_month(&leased(), &first));
        let a = fold_ledger(&leased(), &first, None);
        assert_eq!(
            (
                a.total_seconds(),
                a.total_pages(),
                a.watermark().milliseconds
            ),
            (1, 3, 1000)
        );
        assert_eq!(a.month().month, MONTH_NONE);
        assert_eq!(a.lease_facts().first_day, FIRST_DAY);
        let rollback = StatsFold {
            counters: StatsCounters {
                milliseconds: 2000,
                pages: 2,
                sessions: 1,
            },
            ..first
        };
        assert!(!fold_needs_month(&a, &rollback));
        let b = fold_ledger(&a, &rollback, Some(&empty_stats_month(FIRST_MONTH)));
        assert_eq!(b.total_seconds(), 1);
        assert_eq!(b.watermark(), &rollback.counters);
        let c = fold_ledger(
            &b,
            &StatsFold {
                counters: StatsCounters {
                    milliseconds: 2999,
                    pages: 3,
                    sessions: 1,
                },
                ..first
            },
            Some(&empty_stats_month(FIRST_MONTH)),
        );
        assert_eq!(c.total_seconds(), 1);
        assert_eq!(c.month().pages, 1);
        assert_eq!(c.month().bucket_seconds, [0; 4]);
    }
    #[test]
    fn manual_marks_unknown_finish_and_import_count_once() {
        let mark = StatsFold {
            mark: StatsMark::Finished,
            ..StatsFold::default()
        };
        let a = fold_ledger(&leased(), &mark, None);
        let b = fold_ledger(&a, &mark, None);
        assert_eq!(b.books_finished(), 1);
        assert_eq!(b.lease_facts().manual_finish_day, DAY_UNDATED);
        let c = fold_ledger(
            &b,
            &StatsFold {
                cross_ink_finished: true,
                ..mark
            },
            None,
        );
        assert_eq!(c.books_finished(), 0);
        let unknown = handover_ledger(
            &a,
            None,
            None,
            &StatsLease {
                digest: [8; 32],
                facts: unknown_book_facts(),
                ..StatsLease::default()
            },
        );
        let untouched = fold_ledger(
            &unknown,
            &StatsFold {
                mark: StatsMark::NotFinished,
                ..StatsFold::default()
            },
            None,
        );
        assert_eq!(untouched.books_finished(), 1);
        assert_eq!(book_finish(untouched.lease_facts()), BookFinish::Unknown);
    }
    #[test]
    fn decoder_rejects_reserved_and_preserves_state() {
        let ledger = leased();
        let mut bytes = encode_ledger(&ledger, u32::MAX);
        let mut state = StatsState::Unavailable;
        assert!(decode_ledger(&bytes, &mut state));
        assert_eq!(state.seeded(), Some(&ledger));
        bytes[174] = 1;
        seal(&mut bytes, LEDGER_MAGIC, 0);
        assert!(!decode_ledger(&bytes, &mut state));
        assert_eq!(state.seeded(), Some(&ledger));
        put32(&mut bytes, 4, 2);
        let hash = fnv1a(&bytes[..176]);
        put32(&mut bytes, 176, hash);
        assert_eq!(
            classify_stats_record(&bytes, LEDGER_MAGIC),
            StatsDecode::Unsupported
        );
        assert!(!decode_ledger(&bytes, &mut state));
        assert_eq!(state.seeded(), Some(&ledger));
    }
    #[test]
    fn month_end_validation_and_zero_book_digest() {
        let mut feb = empty_stats_month(FIRST_MONTH + 1);
        feb.day_seconds[28] = 1;
        assert_eq!(decode_month(&encode_month(&feb, 0)), None);
        feb.month = 2028 * 12 + 1;
        assert_eq!(decode_month(&encode_month(&feb, 0)), Some(feb));
        assert_eq!(
            decode_book_facts(
                &encode_book_facts(&[0; 32], &BookFacts::default(), 1),
                &[0; 32]
            ),
            None
        );
        assert_eq!(fnv1a(b"hello"), 0x4f9f2cab);
    }
    #[test]
    fn failure_loads_are_bounded_and_future_days_only_extend_longest() {
        let january = empty_stats_month(FIRST_MONTH);
        let make = |day| StatsFold {
            counters: StatsCounters {
                milliseconds: 60000,
                ..StatsCounters::default()
            },
            clock: stats_clock_for_day(day, 17),
            ..StatsFold::default()
        };
        let a = fold_ledger(&leased(), &make(FIRST_DAY), Some(&january));
        let b = fold_ledger(
            &a,
            &StatsFold {
                counters: StatsCounters {
                    milliseconds: 120000,
                    ..StatsCounters::default()
                },
                ..make(FIRST_DAY + 31)
            },
            Some(&empty_stats_month(FIRST_MONTH + 1)),
        );
        let mut calls = 0;
        let s = summarize_stats(
            &b,
            stats_clock_for_day(FIRST_DAY + 30, 0),
            |month| {
                calls += 1;
                assert_eq!(month, FIRST_MONTH);
                MonthLoad::Unavailable
            },
            None,
            None,
        );
        assert_eq!(calls, 1);
        assert_eq!(
            (
                s.today_seconds,
                s.week_seconds,
                s.current_streak,
                s.longest_streak
            ),
            (0, 0, 0, 1)
        );
    }
}
