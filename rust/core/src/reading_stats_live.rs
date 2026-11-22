//! Pure unsavedStatsSeconds projection from firmware/reading_stats_runtime.h.
use crate::{reading_stats::StatsLedger, reading_stats_service::StatsBook,
    stats_clock::StatsClock, time_zone::TimeZone,
    tracker::{DayMarkState, READING_INACTIVITY_MS, ReadingTracker}};

/// `book` is the caller's actual openStatsBook result, including its source
/// and handover admission. This reads the live tracker/marks without folding.
pub fn unsaved_stats_seconds(
    ledger: &StatsLedger, today: StatsClock, utc: u32, now: u32, panel: bool,
    zone: TimeZone, book: Option<&StatsBook>, tracker: &ReadingTracker, fold_owed: bool,
) -> u32 {
    let Some(book) = book else { return 0; };
    if fold_owed { return 0; }
    let marks = tracker.stats_marks();
    let leased = ledger.leases(&book.digest);
    if !panel && (!marks.lease_checked || !leased) { return 0; }
    let interval = tracker.open_interval(now);
    let open = if interval <= READING_INACTIVITY_MS { u64::from(interval) } else { 0 };
    let mark = &marks.day;
    let crossed = matches!(mark.state, DayMarkState::Armed | DayMarkState::Crossed)
        && u32::from(today.day) == u32::from(mark.day) + 1 && mark.matches(utc, now, zone);
    if crossed && mark.state == DayMarkState::Armed {
        return (open.min(u64::from(now.wrapping_sub(mark.midnight_at))) / 1000) as u32;
    }
    let live = tracker.snapshot(now).active_milliseconds.wrapping_add(open);
    let mut base = if leased { ledger.watermark().milliseconds } else { book.counters.milliseconds };
    if crossed && mark.milliseconds > base { base = mark.milliseconds; }
    if live > base { ((live - base) / 1000) as u32 } else { 0 }
}
