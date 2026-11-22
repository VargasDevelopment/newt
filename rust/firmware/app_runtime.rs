// SPDX-License-Identifier: MIT
//! Y application composition over Home's boot owners. Reader work returns to
//! Root, which lends the same HomeRuntime into its mounted reader scope.
//! No Board, catalog, frame, durable record or reader state is duplicated here.
#![forbid(unsafe_code)]
#[path = "app_runtime/paint.rs"]
mod paint;
pub use paint::PreparedSystemPaint;
#[path = "app_runtime/package.rs"]
pub mod package;
#[path = "app_runtime/home_refresh.rs"]
pub mod home_refresh;
#[path = "app_runtime/display.rs"]
pub mod display;
#[path = "app_runtime/home_cover.rs"]
pub mod home_cover;
#[path = "app_runtime/library.rs"]
mod library;
#[path = "app_runtime/platform.rs"]
pub(crate) mod platform;
#[path = "app_runtime/run_loop.rs"]
pub mod run_loop;
#[path = "app_runtime/info.rs"]
pub(crate) mod info;
#[cfg(feature = "dev-usb")]
#[path = "app_runtime/publication.rs"]
pub(crate) mod publication;
#[path = "app_runtime/restart.rs"]
pub(crate) mod restart;
#[cfg(feature = "dev-usb")]
#[path = "app_runtime/trace.rs"]
pub(crate) mod trace;

use crate::{
    device_lifecycle,
    home_startup::{CardPort, HomeRuntime},
    nvs, quick_start_ports,
    rest_app::{Dispatch, RestApp, Work},
};
use core::fmt::Write;
use newt_core::{
    app::{self, Action, Screen, SettingsNotice, SettingsRow},
    preferences::ViewSettings,
    quick_start::{Facts, State, Step},
};

/// One synchronous application loan, rebuilt from the existing boot owners.
/// Reader scopes never coexist with this loan's mutable Home operations.
pub struct AppRuntime<'op, 'message> {
    pub rest: RestApp<'op, 'message>,
    pub quick_start: &'op mut State,
}

/// Input facts belong to this press, not a second application state. Root
/// supplies its catalog's actual count and native random value.
#[derive(Clone, Copy)]
pub struct Press {
    pub action: Action,
    pub catalog_count: usize,
    pub random: u32,
    pub wifi_saved: bool,
}

impl AppRuntime<'_, '_> {
    /// After HomeRuntime::start and the actual retained/durable reopen decision.
    /// Pause loads from its frozen NVS authority before retained Tiny
    /// selection is applied by Root. Quick Start never overrides a reopened book.
    pub fn start<L: Write, N: nvs::Native, P: CardPort>(
        &mut self,
        home: &mut HomeRuntime<'_, L, N, P>,
        reader_reopened: bool,
    ) -> bool {
        self.rest.start(&mut nvs::NvsBlobBackend::new(&mut home.nvs));
        home.begin_quick_start(
            self.rest.app,
            self.quick_start,
            &mut Facts::default(),
            reader_reopened,
        )
    }

    /// Frozen order: Quick Start, Library/reader bypass, Settings owners, then
    /// the ordinary app/Pause tail. Each durable operation ends its Home loan
    /// before the next one starts. No callback can retain that loan.
    pub fn dispatch<'sdk, L: Write, N: nvs::Native, P: CardPort>(
        &mut self,
        home: &mut HomeRuntime<'sdk, L, N, P>,
        press: Press,
        platform: &mut dyn device_lifecycle::Platform<'sdk, L, N, P>,
        preferences_result: &mut dyn FnMut(bool, ViewSettings),
        wifi_entry: Option<&mut dyn FnMut(&mut HomeRuntime<'sdk, L, N, P>) -> bool>,
    ) -> Dispatch {
        if let Some(dispatched) = self.dispatch_quick(home, press, preferences_result, wifi_entry) {
            return dispatched;
        }
        self.dispatch_after_quick(home, press, platform)
    }

    /// End the reader preference callback loan before borrowing that same
    /// reader for Settings' exit/save capability. The original API above
    /// follows these two bodies in the identical frozen order.
    pub fn dispatch_quick<'sdk, L: Write, N: nvs::Native, P: CardPort>(
        &mut self, home: &mut HomeRuntime<'sdk, L, N, P>, press: Press,
        preferences_result: &mut dyn FnMut(bool, ViewSettings),
        mut wifi_entry: Option<&mut dyn FnMut(&mut HomeRuntime<'sdk, L, N, P>) -> bool>,
    ) -> Option<Dispatch> {
        self.dispatch_quick_with(home, press, &mut |home, effect| match effect {
            quick_start_ports::Effect::Preferences { saved, settings } => { preferences_result(saved, settings); saved }
            quick_start_ports::Effect::EnterWifi => wifi_entry.as_mut().is_some_and(|enter| enter(home)),
        })
    }

    pub fn dispatch_quick_with<'sdk, L: Write, N: nvs::Native, P: CardPort>(
        &mut self, home: &mut HomeRuntime<'sdk, L, N, P>, press: Press,
        effect: &mut dyn FnMut(&mut HomeRuntime<'sdk, L, N, P>, quick_start_ports::Effect) -> bool,
    ) -> Option<Dispatch> {
        let settings_quick_start = self.rest.app.settings_row == SettingsRow::QuickStart;
        let facts = Facts {
            orientation: home.preferences.settings().orientation,
            zone: home.clock.zone(),
            wifi_saved: matches!(self.quick_start.step, Step::Wifi | Step::Done)
                && press.wifi_saved,
            clock_set: false,
        };
        let quick = quick_start_ports::dispatch_with(
            home,
            &mut self.rest,
            self.quick_start,
            &facts,
            press.action,
            settings_quick_start,
            effect,
        );
        if let Some(mut dispatched) = quick {
            // The owner already cleared its borrowed State on either exit.
            // Consume every frozen outcome before the next paint.
            match dispatched.work {
                Work::QuickStartHome => app::leave_quick_start(self.rest.app, false),
                Work::QuickStartSettings => app::leave_quick_start(self.rest.app, true),
                Work::QuickStartOpeningUnconfirmed => {
                    self.rest.app.settings_notice = SettingsNotice::QuickStartSaveFailed;
                }
                _ => return Some(dispatched),
            }
            dispatched.work = Work::None;
            return Some(dispatched);
        }
        None
    }

    pub fn dispatch_after_quick<'sdk, L: Write, N: nvs::Native, P: CardPort>(
        &mut self, home: &mut HomeRuntime<'sdk, L, N, P>, press: Press,
        platform: &mut dyn device_lifecycle::Platform<'sdk, L, N, P>,
    ) -> Dispatch {
        // These paths must not even construct a state backend or invoke a
        // Settings capability. Reader settings/saves stay in the reader owner.
        if self.rest.app.screen == Screen::EpubReading {
            return Dispatch { changed: false, work: Work::ReaderAction(press.action) };
        }
        if self.rest.app.screen == Screen::Library {
            return self.rest.dispatch_library(press.action, press.catalog_count);
        }
        let changed = device_lifecycle::dispatch(home, self.rest.app, press.action, platform);
        self.rest.dispatch(
            &mut nvs::NvsBlobBackend::new(&mut home.nvs),
            press.action, press.catalog_count, press.random, changed,
        )
    }
}
