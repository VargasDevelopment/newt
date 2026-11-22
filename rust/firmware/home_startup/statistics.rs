// SPDX-License-Identifier: MIT
//! Read-only Stats screen composition from reading_stats_runtime.h:255–263,
//! 335–341 and stats_graphs_service.h:53–77. Nothing survives this paint.
use super::{CardPort, HomeRuntime};
use crate::{nvs, stats_clock::read_stats_session_clock};
use core::fmt::Write;
use newt_core::{
    app::{StatsGraphPosition, StatsGraphView},
    crossink_stats::{CrossInkGlobalCopy, CrossInkStatus, cross_ink_history},
    reading_stats::{CrossInkHistory, StatsState},
    reading_stats_service::{StatsResult, summarize_stored_stats},
    reading_stats_store::{StatsBackend, StatsLoad, StatsStore},
    stats_clock::StatsClock,
    stats_graph_data::{
        GraphMonthRead, StatsGraphs, cross_ink_global_periods, cross_ink_global_weekdays,
        library_days_graph, library_oldest_day, library_periods_graph, stats_window_end,
    },
};
use newt_device_ui::stats::{StatsAvailability, StatsScreenView};

impl<L: Write, N: nvs::Native, P: CardPort> HomeRuntime<'_, L, N, P> {
    /// Uses the sole State router. The callback is invoked only without a
    /// Board pixel/parser loan; the logical application frame is independent.
    pub fn read_stats_screen(&mut self, shown: StatsGraphPosition, count: u32) -> StatsScreenView {
        let mut view = StatsScreenView::default();
        let today = read_stats_session_clock(&mut self.bus.board, self.clock.zone(), None);
        let result = self.with_state_store(|_, store| {
            summarize_stored_stats(store, today, &mut view.page.summary)
        });
        view.page.availability = match result {
            StatsResult::Done => StatsAvailability::Ready,
            StatsResult::Unseeded => StatsAvailability::Counting,
            StatsResult::Unsupported => StatsAvailability::Newer,
            _ => StatsAvailability::Unavailable,
        };
        if shown.view == StatsGraphView::Summary || view.page.availability != StatsAvailability::Ready {
            return view;
        }
        // Frozen graph paints take their own second date observation.
        let today = read_stats_session_clock(&mut self.bus.board, self.clock.zone(), None);
        self.with_state_store(|_, store| {
            read_graphs(store, today, shown, count, view.page.summary.total_seconds, &mut view.graphs);
        });
        view
    }
}

fn read_graphs<B: StatsBackend>(
    store: &mut B, today: StatsClock, shown: StatsGraphPosition, count: u32,
    total_seconds: u32, graphs: &mut StatsGraphs,
) {
    let mut stats = StatsStore::new(store);
    let mut state = StatsState::default();
    if stats.load_ledger(&mut state) != StatsLoad::Loaded { return; }
    let Some(ledger) = state.seeded() else { return; };
    let mut copy = CrossInkGlobalCopy::default();
    let imported = stats.load_cross_ink_copy(&mut copy) == StatsLoad::Loaded
        && copy.status == CrossInkStatus::Imported;
    if shown.view == StatsGraphView::Periods {
        let mut periods = [0; 4];
        if imported { cross_ink_global_periods(&copy.bytes[..usize::from(copy.size)], &mut periods); }
        graphs.periods = library_periods_graph(ledger, |month, out| {
            if stats.load_month(month, out) == StatsLoad::Loaded { GraphMonthRead::Loaded }
            else { GraphMonthRead::Unreadable }
        }, &periods, total_seconds);
        return;
    }
    let mut history = CrossInkHistory::default();
    let mut weekdays = [0; 7];
    if imported {
        history = cross_ink_history(&copy);
        cross_ink_global_weekdays(&copy.bytes[..usize::from(copy.size)], &mut weekdays);
    }
    if !today.trusted() { return; }
    let last = stats_window_end(today.day, library_oldest_day(ledger, &history), count, u32::from(shown.back));
    graphs.days = library_days_graph(ledger, |month, out| {
        if stats.load_month(month, out) == StatsLoad::Loaded { GraphMonthRead::Loaded }
        else { GraphMonthRead::Unreadable }
    }, &history, &weekdays, last, count);
}
