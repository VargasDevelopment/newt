// SPDX-License-Identifier: MIT
//! Concrete `QuickStartPorts` over the Home runtime owners. No storage adapter
//! or held state.
#![forbid(unsafe_code)]
use crate::{
    home_startup::{CardPort, HomeChoice, HomeRuntime},
    nvs,
    quick_start_owner::Ports,
};
use core::fmt::Write;
use newt_core::{
    durable::Commit,
    preferences::{Orientation, ViewSettings},
    quick_start::Record,
    time_zone::TimeZone,
};

struct HomePorts<'op, 'sdk, L, N, P> {
    home: &'op mut HomeRuntime<'sdk, L, N, P>,
    effect: &'op mut dyn FnMut(&mut HomeRuntime<'sdk, L, N, P>, Effect) -> bool,
}

/// One sequential reader handoff avoids borrowing that same owner into two
/// simultaneous closures. Preferences' return is its actual saved outcome;
/// only EnterWifi consumes the callback result as an entry decision.
pub enum Effect { Preferences { saved: bool, settings: ViewSettings }, EnterWifi }
impl<L: Write, N: nvs::Native, P: CardPort> Ports for HomePorts<'_, '_, L, N, P> {
    fn save_orientation(&mut self, orientation: Orientation) -> Commit {
        let saved = self.home.preferences
            .save_explicit_orientation(&mut self.home.nvs, orientation);
        // Root updates recovery availability, and only on true publishes the
        // reader draft/settings. This callback cannot manufacture confirmation.
        let settings = self.home.preferences.settings();
        (self.effect)(self.home, Effect::Preferences { saved, settings });
        if saved {
            Commit::Saved
        } else {
            Commit::Unconfirmed
        }
    }
    fn save_zone(&mut self, zone: TimeZone) -> Commit {
        self.home.commit(HomeChoice::Zone(zone))
    }
    fn record(&mut self, record: Record) -> Commit {
        self.home.commit_quick_start(record)
    }
    fn set_up_wifi(&mut self) -> bool {
        // Required Root reader-save/request/restart guard, with the same Home
        // owners reborrowed only for this call after every store loan ended.
        (self.effect)(self.home, Effect::EnterWifi)
    }
}

/// Lend required operations for one synchronous owner press. No port, backend
/// or Home loan can escape through R. Neither callback has a success default.
pub fn with_ports<'sdk, L: Write, N: nvs::Native, P: CardPort, R>(
    home: &mut HomeRuntime<'sdk, L, N, P>,
    preferences_result: &mut dyn FnMut(bool, ViewSettings),
    wifi_entry: &mut dyn FnMut(&mut HomeRuntime<'sdk, L, N, P>) -> bool,
    operation: impl FnOnce(&mut dyn Ports) -> R,
) -> R {
    with_effect(home, &mut |home, effect| match effect {
        Effect::Preferences { saved, settings } => { preferences_result(saved, settings); saved }
        Effect::EnterWifi => wifi_entry(home),
    }, operation)
}

pub fn with_effect<'sdk, L: Write, N: nvs::Native, P: CardPort, R>(
    home: &mut HomeRuntime<'sdk, L, N, P>,
    effect: &mut dyn FnMut(&mut HomeRuntime<'sdk, L, N, P>, Effect) -> bool,
    operation: impl FnOnce(&mut dyn Ports) -> R,
) -> R { operation(&mut HomePorts { home, effect }) }

/// Concrete Quick Start branch before RestApp::dispatch. A None result ends
/// every Home/port loan before the caller borrows its existing pause/settings
/// store. Reader/library/sleep work still returns through the ordinary driver.
/// Root lends truthful Wi-Fi facts only on Wifi/Done, and its real required
/// callbacks. No store and Home mutable aliases coexist across the call.
pub fn dispatch<'sdk, L: Write, N: nvs::Native, P: CardPort>(
    home: &mut HomeRuntime<'sdk, L, N, P>,
    app: &mut crate::rest_app::RestApp<'_, '_>,
    shown: &mut newt_core::quick_start::State,
    facts: &newt_core::quick_start::Facts,
    action: newt_core::app::Action,
    settings_quick_start: bool,
    preferences_result: &mut dyn FnMut(bool, ViewSettings),
    wifi_entry: &mut dyn FnMut(&mut HomeRuntime<'sdk, L, N, P>) -> bool,
) -> Option<crate::rest_app::Dispatch> {
    dispatch_with(home, app, shown, facts, action, settings_quick_start,
        &mut |home, effect| match effect {
            Effect::Preferences { saved, settings } => { preferences_result(saved, settings); saved }
            Effect::EnterWifi => wifi_entry(home),
        })
}

pub fn dispatch_with<'sdk, L: Write, N: nvs::Native, P: CardPort>(
    home: &mut HomeRuntime<'sdk, L, N, P>, app: &mut crate::rest_app::RestApp<'_, '_>,
    shown: &mut newt_core::quick_start::State, facts: &newt_core::quick_start::Facts,
    action: newt_core::app::Action, settings_quick_start: bool,
    effect: &mut dyn FnMut(&mut HomeRuntime<'sdk, L, N, P>, Effect) -> bool,
) -> Option<crate::rest_app::Dispatch> {
    use newt_core::quick_start::Step;
    if !app.quick_start_press(action, settings_quick_start) { return None; }
    let current = newt_core::quick_start::Facts {
        orientation: home.preferences.settings().orientation,
        zone: home.clock.zone(),
        wifi_saved: matches!(shown.step, Step::Wifi | Step::Done) && facts.wifi_saved,
        clock_set: false,
    };
    with_effect(home, effect, |ports| {
        app.dispatch_quick_start(shown, &current, action, settings_quick_start, ports)
    })
}
