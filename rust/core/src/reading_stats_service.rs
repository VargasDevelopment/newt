// SPDX-License-Identifier: MIT
//! Pure reading-statistics service operations over durable namespaces.
//! Frame boundaries express the source intention, not measured resource parity.
use crate::book_days_store::fold_stored_book_days;
use crate::durable::{BlobRead, Load, StorageOpen};
use crate::external_progress_store::ExternalProgressStore;
use crate::progress::Progress;
use crate::reading_stats::*;
use crate::reading_stats_once::{Sha256, count_once};
use crate::reading_stats_store::*;
use crate::sleep_stats::*;
use crate::stats_clock::{StatsClock, stats_clock_for_day};
use crate::stats_service_backend::NamespaceBackend;
use crate::tracker::{DayMarkState, ReadingDayMark};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatsBook {
    pub digest: StatsDigest,
    pub counters: StatsCounters,
    pub end_reached: bool,
    pub permille: u16,
}
impl Default for StatsBook {
    fn default() -> Self {
        Self {
            digest: [0; 32],
            counters: StatsCounters::default(),
            end_reached: false,
            permille: PERMILLE_UNKNOWN,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatsKind {
    Seed,
    Fold,
    Handover,
    Mark,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatsResult {
    Done,
    Unseeded,
    Unsupported,
    Unavailable,
    Waiting,
    Cancelled,
    Unconfirmed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatsReport {
    pub kind: StatsKind,
    pub result: StatsResult,
    pub finish: BookFinish,
    pub seconds: u32,
    pub pages: u32,
}
impl Default for StatsReport {
    fn default() -> Self {
        Self {
            kind: StatsKind::Fold,
            result: StatsResult::Unavailable,
            finish: BookFinish::No,
            seconds: 0,
            pages: 0,
        }
    }
}
pub fn stats_kind_name(kind: StatsKind) -> &'static str {
    match kind {
        StatsKind::Seed => "seed",
        StatsKind::Fold => "fold",
        StatsKind::Handover => "handover",
        StatsKind::Mark => "mark",
    }
}
pub fn stats_result_name(result: StatsResult) -> &'static str {
    match result {
        StatsResult::Done => "done",
        StatsResult::Unseeded => "unseeded",
        StatsResult::Unsupported => "unsupported",
        StatsResult::Unavailable => "unavailable",
        StatsResult::Waiting => "waiting",
        StatsResult::Cancelled => "cancelled",
        StatsResult::Unconfirmed => "unconfirmed",
    }
}

// Option<&mut dyn FnMut> is reborrowed at call boundaries; callbacks may have
// runtime-borrowed state, without interior mutability or retained references.
fn admitted<B: StatsBackend>(
    cancelled: &mut Option<&mut dyn FnMut() -> bool>,
) -> Option<StatsEffects> {
    match cancelled {
        Some(c) => StatsStore::<B>::begin_effects(Some(&mut **c)),
        None => StatsStore::<B>::begin_effects(None),
    }
}
pub fn committed(saved: StatsSave) -> StatsResult {
    match saved {
        StatsSave::Saved => StatsResult::Done,
        StatsSave::Unsupported => StatsResult::Unsupported,
        StatsSave::Unconfirmed => StatsResult::Unconfirmed,
        _ => StatsResult::Unavailable,
    }
}
fn credit(report: &mut StatsReport, before: &StatsLedger, after: &StatsLedger) {
    report.seconds = after.total_seconds().wrapping_sub(before.total_seconds());
    report.pages = after.total_pages().wrapping_sub(before.total_pages());
}
/// None is the source's undated target, including newer archive/target records.
fn month_target<B: StatsBackend>(
    stats: &mut StatsStore<'_, B>,
    effects: &StatsEffects,
    ledger: &StatsLedger,
    fold: &StatsFold,
) -> Result<Option<StatsMonth>, StatsResult> {
    if !fold_needs_month(ledger, fold) {
        return Ok(None);
    }
    if ledger.month().month != MONTH_NONE {
        let mut archived = stats.save_month(effects, ledger.month());
        if archived == StatsSave::Corrupt && stats.set_aside_month(effects, ledger.month().month) {
            archived = stats.save_month(effects, ledger.month());
        }
        match archived {
            StatsSave::Saved => {}
            StatsSave::Unsupported => return Ok(None),
            StatsSave::Unconfirmed => return Err(StatsResult::Unconfirmed),
            _ => return Err(StatsResult::Unavailable),
        }
    }
    let mut block = empty_stats_month(fold.clock.month);
    match stats.load_month(fold.clock.month, &mut block) {
        StatsLoad::Unavailable => return Err(StatsResult::Unavailable),
        StatsLoad::Unsupported => return Ok(None),
        StatsLoad::Corrupt => {
            if !stats.set_aside_month(effects, fold.clock.month) {
                return Err(StatsResult::Unavailable);
            }
            block = empty_stats_month(fold.clock.month);
        }
        _ => {}
    }
    Ok(Some(block))
}
#[inline(never)]
fn leased_counters<B: StatsBackend>(
    store: &mut B,
    digest: &StatsDigest,
    counters: &mut StatsCounters,
) -> Load<()> {
    let mut progress = Progress::default();
    let loaded = ExternalProgressStore::new(store, Some(digest)).load(&mut progress);
    if loaded == Load::Loaded(()) {
        *counters = stats_counters(&progress);
    }
    loaded
}
#[inline(never)]
fn store_final_facts<B: StatsBackend>(
    stats: &mut StatsStore<'_, B>,
    effects: &StatsEffects,
    ledger: &StatsLedger,
    final_fold: Option<&StatsFold>,
    target: Option<&StatsMonth>,
) -> StatsSave {
    let facts = final_fold.map_or(*ledger.lease_facts(), |f| {
        *fold_ledger(ledger, f, target).lease_facts()
    });
    stats.store_book_facts(effects, ledger.lease_digest(), &facts)
}
pub fn load_stats<B: StatsBackend>(
    stats: &mut StatsStore<'_, B>,
    state: &mut StatsState,
    mut cancelled: Option<&mut dyn FnMut() -> bool>,
) -> StatsResult {
    match stats.load_ledger(state) {
        StatsLoad::Loaded => StatsResult::Done,
        StatsLoad::Corrupt => {
            let Some(effects) = admitted::<B>(&mut cancelled) else {
                return StatsResult::Cancelled;
            };
            if stats.set_aside_ledger(&effects) {
                StatsResult::Unseeded
            } else {
                StatsResult::Unavailable
            }
        }
        StatsLoad::Absent => StatsResult::Unseeded,
        StatsLoad::Unsupported => StatsResult::Unsupported,
        StatsLoad::Unavailable => StatsResult::Unavailable,
    }
}
#[inline(never)]
pub fn handover_stats<B: StatsBackend>(
    stats: &mut StatsStore<'_, B>,
    ledger: &StatsLedger,
    open: &StatsBook,
    clock: StatsClock,
    mut cancelled: Option<&mut dyn FnMut() -> bool>,
) -> StatsReport {
    let mut report = StatsReport {
        kind: StatsKind::Handover,
        ..StatsReport::default()
    };
    let mut final_fold = StatsFold {
        clock,
        ..StatsFold::default()
    };
    let mut final_read = false;
    if ledger.leased() {
        let loaded = leased_counters(
            stats.store(),
            ledger.lease_digest(),
            &mut final_fold.counters,
        );
        if matches!(loaded, Load::Loaded(()) | Load::Empty) {
            final_read = true;
        } else if stats.store().reading_state_unavailable() {
            return report;
        }
    }
    let Some(effects) = admitted::<B>(&mut cancelled) else {
        report.result = StatsResult::Cancelled;
        return report;
    };
    let target = if final_read {
        match month_target(stats, &effects, ledger, &final_fold) {
            Ok(b) => b,
            Err(r) => {
                report.result = r;
                return report;
            }
        }
    } else {
        None
    };
    let mut lease = StatsLease {
        digest: open.digest,
        counters: open.counters,
        ..StatsLease::default()
    };
    if !stats.book_facts(&effects, &open.digest, &mut lease.facts) {
        report.result = if stats.store().reading_state_unavailable() {
            StatsResult::Unavailable
        } else {
            StatsResult::Waiting
        };
        return report;
    }
    if ledger.leased() {
        match store_final_facts(
            stats,
            &effects,
            ledger,
            if final_read { Some(&final_fold) } else { None },
            target.as_ref(),
        ) {
            StatsSave::Saved => {}
            StatsSave::Unconfirmed => {
                report.result = StatsResult::Unconfirmed;
                return report;
            }
            _ => {
                report.result = if stats.store().reading_state_unavailable() {
                    StatsResult::Unavailable
                } else {
                    StatsResult::Waiting
                };
                return report;
            }
        }
    }
    let next = handover_ledger(
        ledger,
        if final_read { Some(&final_fold) } else { None },
        target.as_ref(),
        &lease,
    );
    report.result = committed(stats.save_ledger(&effects, &next));
    if report.result == StatsResult::Done {
        credit(&mut report, ledger, &next);
        report.finish = book_finish(next.lease_facts());
    }
    report
}
#[inline(never)]
fn fold_leased<B: NamespaceBackend>(
    stats: &mut StatsStore<'_, B>,
    ledger: &StatsLedger,
    open: &StatsBook,
    clock: StatsClock,
    mut cancelled: Option<&mut dyn FnMut() -> bool>,
    sha256: Sha256<'_>,
) -> StatsReport {
    let mut report = StatsReport::default();
    let mut fold = StatsFold {
        counters: open.counters,
        clock,
        end_reached: open.end_reached,
        permille: open.permille,
        ..StatsFold::default()
    };
    let Some(effects) = admitted::<B>(&mut cancelled) else {
        report.result = StatsResult::Cancelled;
        return report;
    };
    let target = match month_target(stats, &effects, ledger, &fold) {
        Ok(b) => b,
        Err(r) => {
            report.result = r;
            return report;
        }
    };
    let mut next = fold_ledger(ledger, &fold, target.as_ref());
    if !count_once(stats, ledger, &mut fold, target.as_ref(), &mut next, sha256) {
        report.result = StatsResult::Unavailable;
        return report;
    }
    report.result = committed(stats.save_ledger(&effects, &next));
    if report.result != StatsResult::Done {
        return report;
    }
    credit(&mut report, ledger, &next);
    report.finish = book_finish(next.lease_facts());
    fold_stored_book_days(stats, &effects, &open.digest, ledger.watermark(), &fold);
    report
}
#[inline(never)]
pub fn fold_stats<B: NamespaceBackend>(
    store: &mut B,
    open: &StatsBook,
    clock: StatsClock,
    mut cancelled: Option<&mut dyn FnMut() -> bool>,
    sha256: Sha256<'_>,
) -> StatsReport {
    let mut stats = StatsStore::new(store);
    let mut state = StatsState::default();
    let result = match cancelled.as_mut() {
        Some(c) => load_stats(&mut stats, &mut state, Some(&mut **c)),
        None => load_stats(&mut stats, &mut state, None),
    };
    let Some(ledger) = state.seeded() else {
        return StatsReport {
            result,
            ..StatsReport::default()
        };
    };
    if ledger.leases(&open.digest) {
        fold_leased(&mut stats, ledger, open, clock, cancelled, sha256)
    } else {
        handover_stats(&mut stats, ledger, open, clock, cancelled)
    }
}
#[inline(never)]
pub fn mark_stats<B: NamespaceBackend>(
    store: &mut B,
    open: &StatsDigest,
    clock: StatsClock,
    mark: StatsMark,
    sha256: Sha256<'_>,
) -> StatsReport {
    let mut stats = StatsStore::new(store);
    let mut state = StatsState::default();
    let mut report = StatsReport {
        kind: StatsKind::Mark,
        result: load_stats(&mut stats, &mut state, None),
        ..StatsReport::default()
    };
    let Some(ledger) = state.seeded() else {
        return report;
    };
    if !ledger.leases(open) {
        report.result = StatsResult::Waiting;
        return report;
    }
    let mut fold = StatsFold {
        counters: *ledger.watermark(),
        clock,
        mark,
        ..StatsFold::default()
    };
    let effects = StatsStore::<B>::begin_effects(None).expect("no cancellation");
    let mut next = fold_ledger(ledger, &fold, None);
    if !count_once(&mut stats, ledger, &mut fold, None, &mut next, sha256) {
        report.result = StatsResult::Unavailable;
        return report;
    }
    report.result = committed(stats.save_ledger(&effects, &next));
    if report.result == StatsResult::Done {
        report.finish = book_finish(next.lease_facts());
    }
    report
}
#[inline(never)]
pub fn fold_before_midnight<B: NamespaceBackend>(
    store: &mut B,
    open: &StatsBook,
    mark: &ReadingDayMark,
    mut cancelled: Option<&mut dyn FnMut() -> bool>,
    sha256: Sha256<'_>,
) -> StatsReport {
    let mut stats = StatsStore::new(store);
    let mut state = StatsState::default();
    let result = match cancelled.as_mut() {
        Some(c) => load_stats(&mut stats, &mut state, Some(&mut **c)),
        None => load_stats(&mut stats, &mut state, None),
    };
    let Some(ledger) = state.seeded() else {
        return StatsReport {
            result,
            ..StatsReport::default()
        };
    };
    let done = StatsReport {
        result: StatsResult::Done,
        ..StatsReport::default()
    };
    let was = ledger.watermark();
    if !ledger.leases(&open.digest)
        || open.counters.milliseconds < was.milliseconds
        || open.counters.pages < was.pages
        || open.counters.sessions < was.sessions
    {
        return done;
    }
    if mark.state == DayMarkState::Armed {
        return fold_leased(
            &mut stats,
            ledger,
            open,
            stats_clock_for_day(mark.day, 23),
            cancelled,
            sha256,
        );
    }
    if mark.milliseconds < was.milliseconds || mark.pages < was.pages {
        return done;
    }
    let mut before = *open;
    before.counters.milliseconds = mark.milliseconds.min(open.counters.milliseconds);
    before.counters.pages = mark.pages.min(open.counters.pages);
    before.counters.sessions = mark.sessions.min(open.counters.sessions);
    if before.counters.sessions < was.sessions {
        return done;
    }
    before.end_reached = false;
    fold_leased(
        &mut stats,
        ledger,
        &before,
        stats_clock_for_day(mark.day, 23),
        cancelled,
        sha256,
    )
}
pub fn fold_marked_stats<B: NamespaceBackend>(
    store: &mut B,
    open: &StatsBook,
    clock: StatsClock,
    mark: &ReadingDayMark,
    mut cancelled: Option<&mut dyn FnMut() -> bool>,
    sha256: Sha256<'_>,
) -> StatsReport {
    let mut before = StatsReport {
        result: StatsResult::Done,
        ..StatsReport::default()
    };
    if matches!(mark.state, DayMarkState::Armed | DayMarkState::Crossed)
        && clock.trusted()
        && u32::from(clock.day) == u32::from(mark.day) + 1
    {
        before = match cancelled.as_mut() {
            Some(c) => fold_before_midnight(store, open, mark, Some(&mut **c), sha256),
            None => fold_before_midnight(store, open, mark, None, sha256),
        };
        if before.result != StatsResult::Done {
            return before;
        }
    }
    let mut report = fold_stats(store, open, clock, cancelled, sha256);
    report.seconds = report.seconds.wrapping_add(before.seconds);
    report.pages = report.pages.wrapping_add(before.pages);
    report
}

struct SeedScan<'a, 'c> {
    open: Option<&'a StatsDigest>,
    cancelled: Option<&'c mut dyn FnMut() -> bool>,
    seed: StatsSeed,
    open_facts: Option<Namespace>,
    failed: bool,
    stopped: bool,
}
impl SeedScan<'_, '_> {
    fn proceed(&mut self) -> bool {
        self.stopped = self.stopped || self.cancelled.as_mut().is_some_and(|c| c());
        !self.stopped
    }
    fn progress<B: NamespaceBackend>(&mut self, store: &mut B, name: &str) -> bool {
        if !self.proceed() {
            return false;
        }
        let mut digest = [0; 32];
        let mut read = BlobRead::Failed;
        if store.begin(name, true) == StorageOpen::Opened {
            read = store.read_blob("digest", &mut digest);
        }
        // Deliberately unconditional: the original calls end even on failed begin.
        store.end();
        if store.reading_state_unavailable() {
            self.failed = true;
            return false;
        }
        if read != BlobRead::Present(digest.len()) {
            return true;
        }
        let mut book = ExternalProgressStore::new(store, Some(&digest));
        if book.storage_namespace() != name {
            return true;
        }
        let is_open = self.open == Some(&digest);
        let mut progress = Progress::default();
        let loaded = book.load(&mut progress);
        if !matches!(loaded, Load::Loaded(()) | Load::Empty) {
            if is_open || store.reading_state_unavailable() {
                self.failed = true;
                return false;
            }
            return true;
        }
        let counters = if loaded == Load::Loaded(()) {
            stats_counters(&progress)
        } else {
            StatsCounters::default()
        };
        self.seed.milliseconds = self.seed.milliseconds.wrapping_add(counters.milliseconds);
        self.seed.pages = self.seed.pages.wrapping_add(u64::from(counters.pages));
        self.seed.sessions = self
            .seed
            .sessions
            .wrapping_add(u64::from(counters.sessions));
        if is_open {
            self.seed.lease.counters = counters;
        }
        true
    }
    fn facts<B: NamespaceBackend>(&mut self, store: &mut B, name: &str) -> bool {
        if !self.proceed() {
            return false;
        }
        let mut digest = [0; 32];
        let mut facts = BookFacts::default();
        let loaded = StatsStore::new(store).load_named_book(name, &mut digest, &mut facts);
        if loaded == StatsLoad::Unavailable {
            self.failed = true;
            return false;
        }
        let is_open = self.open_facts.as_ref().is_some_and(|n| n.as_str() == name);
        if loaded == StatsLoad::Loaded {
            if counts_finished(&facts) {
                self.seed.finished_books = self.seed.finished_books.saturating_add(1);
            }
            if is_open {
                self.seed.lease.facts = facts;
            }
        } else if is_open && loaded != StatsLoad::Absent {
            self.seed.lease.facts = unknown_book_facts();
        }
        true
    }
}
#[inline(never)]
pub fn seed_stats<B: NamespaceBackend>(
    store: &mut B,
    open: Option<&StatsDigest>,
    mut cancelled: Option<&mut dyn FnMut() -> bool>,
) -> StatsReport {
    let mut state = StatsState::default();
    let result = match cancelled.as_mut() {
        Some(c) => load_stats(&mut StatsStore::new(store), &mut state, Some(&mut **c)),
        None => load_stats(&mut StatsStore::new(store), &mut state, None),
    };
    let mut report = StatsReport {
        kind: StatsKind::Seed,
        result,
        ..StatsReport::default()
    };
    if result != StatsResult::Unseeded {
        return report;
    }
    let mut scan = SeedScan {
        open,
        cancelled,
        seed: StatsSeed::default(),
        open_facts: open.map(stats_book_namespace),
        failed: false,
        stopped: false,
    };
    if let Some(d) = open {
        scan.seed.open = true;
        scan.seed.lease.digest = *d;
    }
    let listed = store.for_each_namespace("nb", &mut |s, n| scan.progress(s, n))
        && store.for_each_namespace("sb", &mut |s, n| scan.facts(s, n));
    if !listed || scan.failed || scan.stopped {
        report.result = if scan.stopped {
            StatsResult::Cancelled
        } else {
            StatsResult::Unavailable
        };
        return report;
    }
    let Some(effects) = admitted::<B>(&mut scan.cancelled) else {
        report.result = StatsResult::Cancelled;
        return report;
    };
    let seeded = seed_ledger(&scan.seed);
    report.result = committed(StatsStore::new(store).save_ledger(&effects, &seeded));
    if report.result == StatsResult::Done {
        report.seconds = seeded.total_seconds();
        report.pages = seeded.total_pages();
    }
    report
}
fn read_result(loaded: StatsLoad) -> StatsResult {
    match loaded {
        StatsLoad::Loaded => StatsResult::Done,
        StatsLoad::Unsupported => StatsResult::Unsupported,
        StatsLoad::Unavailable => StatsResult::Unavailable,
        _ => StatsResult::Unseeded,
    }
}
fn summary_month<B: StatsBackend>(stats: &mut StatsStore<'_, B>, month: u16) -> MonthLoad {
    let mut out = StatsMonth::default();
    match stats.load_month(month, &mut out) {
        StatsLoad::Loaded => MonthLoad::Loaded(out),
        StatsLoad::Absent => MonthLoad::Missing,
        _ => MonthLoad::Unavailable,
    }
}
pub fn summarize_stored_stats<B: StatsBackend>(
    store: &mut B,
    today: StatsClock,
    summary: &mut StatsSummary,
) -> StatsResult {
    let mut stats = StatsStore::new(store);
    let mut state = StatsState::default();
    let loaded = stats.load_ledger(&mut state);
    if loaded != StatsLoad::Loaded {
        return read_result(loaded);
    }
    let mut cross = CrossInkHistory::default();
    let imported = stats.load_cross_ink(&mut cross, None);
    *summary = summarize_stats(
        state.seeded().expect("loaded"),
        today,
        |m| summary_month(&mut stats, m),
        if imported { Some(&cross) } else { None },
        None,
    );
    StatsResult::Done
}
pub fn stored_book_facts<B: StatsBackend>(
    store: &mut B,
    digest: &StatsDigest,
    facts: &mut BookFacts,
) -> StatsResult {
    let mut stats = StatsStore::new(store);
    let mut state = StatsState::default();
    let loaded = stats.load_ledger(&mut state);
    if loaded != StatsLoad::Loaded {
        return read_result(loaded);
    }
    let ledger = state.seeded().expect("loaded");
    if ledger.leases(digest) {
        *facts = *ledger.lease_facts();
        return StatsResult::Done;
    }
    *facts = BookFacts::default();
    match stats.load_book(digest, facts) {
        StatsLoad::Loaded | StatsLoad::Absent => StatsResult::Done,
        StatsLoad::Corrupt => {
            *facts = unknown_book_facts();
            StatsResult::Done
        }
        StatsLoad::Unsupported => StatsResult::Unsupported,
        StatsLoad::Unavailable => StatsResult::Unavailable,
    }
}
// These are the exact fields consumed/published by this service, not startup/UI.
#[derive(Clone, Copy, Default)]
pub struct SleepScreenChoices {
    pub habit: bool,
    pub week: bool,
}
#[derive(Clone, Copy, Default)]
pub struct SleepExtras {
    pub progress: bool,
    pub habit: ReadingHabit,
    pub week: bool,
    pub days: u8,
    pub streak: u16,
}
#[inline(never)]
pub fn read_sleep_stats<B: StatsBackend>(
    store: &mut B,
    today: StatsClock,
    choices: SleepScreenChoices,
    unsaved: impl FnOnce(&StatsLedger) -> u32,
    extras: &mut SleepExtras,
) {
    let mut stats = StatsStore::new(store);
    let mut state = StatsState::default();
    if !today.trusted() || stats.load_ledger(&mut state) != StatsLoad::Loaded {
        return;
    }
    let ledger = state.seeded().expect("loaded");
    if sleep_today_archived(ledger, today) {
        return;
    }
    let seconds = unsaved(ledger);
    let mut archive = StatsMonth::default();
    let month = sleep_archive_month(ledger, today, choices.habit);
    let loaded = if month == MONTH_NONE {
        StatsLoad::Unavailable
    } else {
        stats.load_month(month, &mut archive)
    };
    if loaded == StatsLoad::Absent {
        archive = empty_stats_month(month);
    }
    let read = if matches!(loaded, StatsLoad::Loaded | StatsLoad::Absent) {
        Some(&archive)
    } else {
        None
    };
    if choices.habit {
        extras.habit = reading_habit(ledger, read, today, seconds);
    }
    if !choices.week {
        return;
    }
    let mut cache = StatsStreak::default();
    if stats.load_streak(&mut cache) != StatsLoad::Loaded {
        cache = StatsStreak::default();
    }
    let week = sleep_week(ledger, &cache, read, today, seconds);
    extras.week = true;
    extras.days = week.days;
    extras.streak = week.streak;
}
#[inline(never)]
pub fn refresh_sleep_streak<B: StatsBackend>(
    store: &mut B,
    today: StatsClock,
    mut cancelled: Option<&mut dyn FnMut() -> bool>,
) -> StatsResult {
    if !today.trusted() {
        return StatsResult::Unavailable;
    }
    let mut stats = StatsStore::new(store);
    let mut state = StatsState::default();
    let result = match cancelled.as_mut() {
        Some(c) => load_stats(&mut stats, &mut state, Some(&mut **c)),
        None => load_stats(&mut stats, &mut state, None),
    };
    if result != StatsResult::Done {
        return result;
    }
    let mut incomplete = false;
    let mut cross = CrossInkHistory::default();
    if !stats.load_cross_ink(&mut cross, Some(&mut incomplete)) {
        cross = CrossInkHistory::default();
    }
    let ledger = state.seeded().expect("loaded");
    let mut recent = 0;
    let summary = summarize_stats(
        ledger,
        today,
        |m| {
            let mut out = StatsMonth::default();
            let loaded = stats.load_month(m, &mut out);
            incomplete |= !matches!(loaded, StatsLoad::Loaded | StatsLoad::Absent);
            if loaded == StatsLoad::Loaded {
                MonthLoad::Loaded(out)
            } else {
                MonthLoad::Missing
            }
        },
        Some(&cross),
        Some(&mut recent),
    );
    if incomplete {
        return StatsResult::Unavailable;
    }
    let streak = sleep_streak(ledger, &summary, recent, today);
    let mut stored = StatsStreak::default();
    let found = stats.load_streak(&mut stored);
    if matches!(found, StatsLoad::Unavailable | StatsLoad::Unsupported) {
        return StatsResult::Unavailable;
    }
    if found == StatsLoad::Loaded && stored == streak {
        return StatsResult::Done;
    }
    let Some(effects) = admitted::<B>(&mut cancelled) else {
        return StatsResult::Cancelled;
    };
    if found == StatsLoad::Corrupt && !stats.set_aside_streak(&effects) {
        return StatsResult::Unavailable;
    }
    committed(stats.save_streak(&effects, &streak))
}
