// SPDX-License-Identifier: MIT
//! Read-only reader statistics panel data loader. Nothing survives this paint.
use newt_core::{
    app::{StatsGraphPosition, StatsGraphView},
    book_days::{BookBlockRead, BookDays, book_days_graph, book_periods_graph, book_window_end},
    book_days_store::{load_book_days, load_day_block},
    progress::Progress,
    reading_stats::{DAY_UNSET, StatsDigest, StatsLedger, StatsState},
    reading_stats_service::{StatsBook, StatsResult, stored_book_facts},
    reading_stats_store::{StatsBackend, StatsLoad, StatsStore},
    stats_clock::{StatsClock, stats_bucket},
    stats_graph_data::StatsDayKind,
    time_zone::TimeZone,
    tracker::{DayMarkState, READING_INACTIVITY_MS, ReadingStatsMarks},
};
use newt_device_ui::stats::StatsAvailability;
use newt_reader_ui::BookStatsView;

pub(super) struct Session<'a> {
    pub book: Option<StatsBook>,
    pub progress: &'a Progress,
    pub marks: &'a ReadingStatsMarks,
    pub interval: u32,
    pub fold_owed: bool,
    pub now: u32,
    pub zone: TimeZone,
}

fn availability(result: StatsResult) -> StatsAvailability {
    match result {
        StatsResult::Done => StatsAvailability::Ready,
        StatsResult::Unseeded => StatsAvailability::Counting,
        StatsResult::Unsupported => StatsAvailability::Newer,
        _ => StatsAvailability::Unavailable,
    }
}

// The panel alone may show live time before leaseChecked. Subtract the
// acknowledged counters until the ledger leases this exact full digest.
fn unsaved(ledger: &StatsLedger, clock: StatsClock, utc: u32, session: &Session<'_>) -> u32 {
    let Some(book) = session.book.as_ref() else {
        return 0;
    };
    if session.fold_owed {
        return 0;
    }
    let open = if session.interval <= READING_INACTIVITY_MS {
        u64::from(session.interval)
    } else {
        0
    };
    let mark = &session.marks.day;
    let crossed = matches!(mark.state, DayMarkState::Armed | DayMarkState::Crossed)
        && u32::from(clock.day) == u32::from(mark.day) + 1
        && mark.matches(utc, session.now, session.zone);
    if crossed && mark.state == DayMarkState::Armed {
        return (open.min(u64::from(session.now.wrapping_sub(mark.midnight_at))) / 1000) as u32;
    }
    let live = session.progress.active_milliseconds.wrapping_add(open);
    let mut base = if ledger.leases(&book.digest) {
        ledger.watermark().milliseconds
    } else {
        book.counters.milliseconds
    };
    if crossed && mark.milliseconds > base {
        base = mark.milliseconds;
    }
    if live > base {
        ((live - base) / 1000) as u32
    } else {
        0
    }
}

pub(super) fn read<B: StatsBackend>(
    store: &mut B,
    digest: &StatsDigest,
    authorized: bool,
    shown: StatsGraphPosition,
    count: u32,
    session: &Session<'_>,
    read_clock: impl FnOnce() -> (StatsClock, u32),
) -> BookStatsView {
    let mut view = BookStatsView {
        history: StatsAvailability::Unavailable,
        ..BookStatsView::default()
    };
    if !authorized {
        return view;
    }
    view.dates_availability = availability(stored_book_facts(store, digest, &mut view.facts));
    let (clock, utc) = read_clock();
    let live = if shown.view == StatsGraphView::Summary || !clock.trusted() {
        0
    } else {
        let mut state = StatsState::default();
        if StatsStore::new(store).load_ledger(&mut state) == StatsLoad::Loaded {
            state
                .seeded()
                .map_or(0, |ledger| unsaved(ledger, clock, utc, session))
        } else {
            0
        }
    };
    let last_read = if view.dates_availability == StatsAvailability::Ready {
        view.facts.last_day
    } else {
        DAY_UNSET
    };
    view.history = history(
        store, digest, clock, last_read, shown, count, live, &mut view,
    );
    view
}

fn history<B: StatsBackend>(
    store: &mut B,
    digest: &StatsDigest,
    clock: StatsClock,
    last_read: u16,
    shown: StatsGraphPosition,
    count: u32,
    live: u32,
    view: &mut BookStatsView,
) -> StatsAvailability {
    let mut stats = StatsStore::new(store);
    let mut days = BookDays::default();
    match load_book_days(&mut stats, digest, &mut days) {
        StatsLoad::Unsupported => return StatsAvailability::Newer,
        StatsLoad::Loaded | StatsLoad::Absent => {}
        _ => return StatsAvailability::Unavailable,
    }
    view.seconds = days.imported.seconds;
    view.imported_seconds = days.imported.seconds;
    view.pages = days.imported.pages;
    view.sessions = days.imported.sessions;
    if shown.view == StatsGraphView::Periods {
        view.graphs.periods = book_periods_graph(&days);
        if clock.trusted() {
            let slot = stats_bucket(clock.hour);
            view.graphs.periods.seconds[slot] =
                view.graphs.periods.seconds[slot].wrapping_add(live);
        }
    } else if shown.view == StatsGraphView::Days {
        let newest = if clock.trusted() && live != 0 {
            clock.day
        } else {
            last_read
        };
        let last = book_window_end(
            clock.day,
            newest,
            days.since_day,
            count,
            u32::from(shown.back),
        );
        view.graphs.days = book_days_graph(
            &days,
            |block, out| match load_day_block(&mut stats, digest, block, out) {
                StatsLoad::Loaded => BookBlockRead::Loaded,
                StatsLoad::Absent => BookBlockRead::Absent,
                _ => BookBlockRead::Unreadable,
            },
            last,
            count,
        );
        let graph = &mut view.graphs.days;
        let slot = i32::from(clock.day) - (i32::from(graph.last_day) - i32::from(graph.count) + 1);
        if clock.trusted() && live != 0 && slot >= 0 && slot < i32::from(graph.count) {
            let slot = slot as usize;
            graph.kinds[slot] = StatsDayKind::Timed;
            graph.seconds[slot] = graph.seconds[slot].wrapping_add(live);
        }
    }
    StatsAvailability::Ready
}
