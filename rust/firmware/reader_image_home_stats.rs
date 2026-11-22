// SPDX-License-Identifier: MIT
//! Frozen reading_stats_runtime.h Home duties, before the one cover idle owner.
#![forbid(unsafe_code)]
use super::{Runtime, cover_idle::SETTLE_MS, millis};
use crate::{app_image::{Application, Events, Home}, reader_book::StatsCondition,
    serial_console::SerialConsole};
use newt_core::{
    app::{self, DevicePage, Screen},
    crossink_import::{ImportReport, ImportResult, import_cross_ink},
    reading_stats_service::{StatsKind, StatsReport, StatsResult, refresh_sleep_streak, seed_stats},
};

fn input_pending(events: &Events<'_, '_>, log: &SerialConsole) -> bool {
    if events.queue.pending() { return true; }
    #[cfg(feature = "dev-usb")]
    { log.available() > 0 }
    #[cfg(not(feature = "dev-usb"))]
    { let _ = log; false }
}

fn reading_stats_page(ui: &Application<'_>) -> bool {
    ui.app.screen == Screen::Device && app::device_page(&ui.app) == DevicePage::ReadingStats
}

impl Runtime {
    fn stats_quiet_image(&self, home: &Home<'_>, events: &Events<'_, '_>) -> bool {
        millis().wrapping_sub(self.idle.quiet_from) >= SETTLE_MS
            && !input_pending(events, &home.bus.log)
    }

    // Some claims this idle tick, independently of whether a repaint is due.
    // Nothing may pass through to streak work and then the cover producer.
    #[inline(never)]
    pub(super) fn service_home_reading_stats_image(&mut self, home: &mut Home<'_>,
        ui: &Application<'_>, events: &Events<'_, '_>,
    ) -> Option<bool> {
        if !matches!(ui.app.screen, Screen::Home | Screen::Device) { return None; }
        let condition = self.statistics.condition;
        if condition == StatsCondition::Seeded {
            return self.service_crossink_import_image(home, ui, events)
                .or_else(|| self.service_sleep_streak_image(home, events));
        }
        if !matches!(condition, StatsCondition::Unknown | StatsCondition::Unseeded)
            || self.saves.page_unconfirmed() || !self.stats_quiet_image(home, events) {
            return None;
        }
        let started = millis();
        let report = self.with_store_io(home, |_this, store, _io, log, _board| {
            let mut cancelled = || input_pending(events, log);
            seed_stats(store, None, Some(&mut cancelled))
        }).unwrap_or(StatsReport { kind: StatsKind::Seed, result: StatsResult::Unavailable,
            ..StatsReport::default() });
        if report.result != StatsResult::Cancelled {
            let condition = match report.result {
                StatsResult::Unseeded => StatsCondition::Unseeded,
                StatsResult::Unsupported => StatsCondition::Unsupported,
                StatsResult::Unavailable => StatsCondition::Unavailable,
                _ => StatsCondition::Seeded,
            };
            self.statistics.condition = condition;
        }
        #[cfg(feature = "dev-usb")]
        crate::app_runtime::trace::row(&home.bus.log, 128, format_args!(
            "STATS kind={} result={} seconds={} pages={} age=0 carry=0 ms={}\n",
            newt_core::reading_stats_service::stats_kind_name(report.kind),
            newt_core::reading_stats_service::stats_result_name(report.result),
            report.seconds, report.pages, millis().wrapping_sub(started)));
        #[cfg(not(feature = "dev-usb"))]
        let _ = started;
        self.idle.note_input(millis());
        Some(reading_stats_page(ui))
    }

    #[inline(never)]
    fn service_crossink_import_image(&mut self, home: &mut Home<'_>,
        ui: &Application<'_>, events: &Events<'_, '_>,
    ) -> Option<bool> {
        if self.saves.import_settled() || !self.stats_quiet_image(home, events) { return None; }
        let started = millis();
        let report = self.with_store_io(home, |_this, store, io, log, board| {
            let mut files = newt_native_io::CrossInkFiles::new(io);
            let mut cancelled = || input_pending(events, log);
            super::crossink_hash::with_factory(board, |hash| {
                import_cross_ink(store, &mut files, hash, c"/newt-sd", &mut cancelled,
                    &crate::reader_hash::package_hash)
            })
        }).unwrap_or(ImportReport { result: ImportResult::Unavailable, sources: 0, bound: 0 });
        if report.result != ImportResult::Cancelled { self.saves.set_import_settled(true); }
        if matches!(report.result, ImportResult::Done | ImportResult::Nothing) {
            self.saves.set_import_complete(true);
        }
        #[cfg(feature = "dev-usb")]
        crate::app_runtime::trace::row(&home.bus.log, 96, format_args!(
            "CROSSINK result={} sources={} bound={} ms={}\n", report.result.name(),
            report.sources, report.bound, millis().wrapping_sub(started)));
        #[cfg(not(feature = "dev-usb"))]
        let _ = started;
        if report.result == ImportResult::Nothing { return None; }
        self.idle.note_input(millis());
        Some(report.result == ImportResult::Done && reading_stats_page(ui))
    }

    #[inline(never)]
    fn service_sleep_streak_image(&mut self, home: &mut Home<'_>, events: &Events<'_, '_>) -> Option<bool> {
        if self.saves.streak_settled() || !home.clock.sleep_screen().week
            || !self.stats_quiet_image(home, events) { return None; }
        let today = crate::stats_clock::read_stats_session_clock(&mut home.bus.board, home.clock.zone(), None);
        let result = self.with_store_io(home, |_this, store, _io, log, _board| {
            let mut cancelled = || input_pending(events, log);
            refresh_sleep_streak(store, today, Some(&mut cancelled))
        }).unwrap_or(StatsResult::Unavailable);
        if result != StatsResult::Cancelled { self.saves.set_streak_settled(true); }
        self.idle.note_input(millis());
        Some(false)
    }
}
