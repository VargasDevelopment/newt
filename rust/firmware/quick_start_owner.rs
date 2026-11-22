// SPDX-License-Identifier: MIT
//! Quick Start press dispatch over `QuickStartPorts`.
//! The caller applies the returned screen/settings route; pure transitions stay
//! in Core. No startup, paint, reader handoff or update-entry guard lives here.
#![forbid(unsafe_code)]
use newt_core::{
    app::Action,
    durable::Commit,
    preferences::Orientation,
    quick_start::{self as quick, Facts, Intent, Notice, Record, State, Step},
    time_zone::TimeZone,
};

/// Required short operations on the existing owners. Implementations must
/// publish choices only after verification. `record` calls quick_start::commit
/// on the existing durable card backend; sharing one backend across these
/// synchronous calls must not require simultaneous mutable storage loans.
/// Wi-Fi entry is Root's real reader-save/request/restart guard, never a stub.
pub trait Ports {
    fn save_orientation(&mut self, orientation: Orientation) -> Commit;
    fn save_zone(&mut self, zone: TimeZone) -> Commit;
    fn record(&mut self, record: Record) -> Commit;
    fn set_up_wifi(&mut self) -> bool;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Route {
    Unchanged,
    Repaint,
    /// Enter QuickStart and clear SettingsNotice only after the record verifies.
    Enter,
    /// QuickStart state is already cleared; caller selects Home/Books.
    Home,
    /// QuickStart state is already cleared; caller selects Settings/QuickStart.
    Settings,
    /// Keep Settings and show its QuickStartSaveFailed notice (unconfirmed).
    OpeningUnconfirmed,
}

fn leave(shown: &mut State, to_settings: bool) -> Route {
    *shown = State::default();
    if to_settings { Route::Settings } else { Route::Home }
}

/// Caller uses the source's quickStartPress guard: opening is Select on
/// Settings/QuickStart; while showing, all presses except Sleep come here.
/// Opening/step/end records commit on a proposal before publishing it. A
/// choice may already be verified even when the subsequent step record is
/// unconfirmed; the old shown step stays until that record verifies.
pub fn press(
    shown: &mut State,
    facts: &Facts,
    action: Action,
    opening: bool,
    ports: &mut dyn Ports,
) -> Route {
    let mut next = *shown;
    let mut intent = if opening {
        next = State { from_settings: true, ..State::default() };
        quick::enter_step(&mut next, facts, Step::Welcome);
        Intent::Moved
    } else {
        quick::dispatch(&mut next, facts, action)
    };
    let choice_commit = match intent {
        Intent::SaveOrientation => Some(ports.save_orientation(quick::orientation(&next))),
        // Pure dispatch clamps the open zone list before returning SaveZone.
        Intent::SaveZone => Some(ports.save_zone(TimeZone::from_raw_or_utc(next.focus))),
        _ => None,
    };
    if let Some(committed) = choice_commit
        && !quick::saved(&mut next, facts, committed == Commit::Saved)
    {
        intent = Intent::Repaint;
    }
    match intent {
        Intent::None => return Route::Unchanged,
        Intent::Repaint => {
            *shown = next;
            return Route::Repaint;
        }
        Intent::SetUpWifi => {
            if !ports.set_up_wifi() { shown.notice = Notice::SaveFailed; }
            return Route::Repaint;
        }
        _ => {}
    }
    let finished = matches!(intent, Intent::Finish | Intent::Exit);
    let to_settings = intent != Intent::Finish && next.from_settings;
    let record = Record { finished, step: next.step, from_settings: next.from_settings };
    if ports.record(record) == Commit::Saved {
        if finished { return leave(shown, to_settings); }
        *shown = next;
        return if opening { Route::Enter } else { Route::Repaint };
    }
    if opening { return Route::OpeningUnconfirmed; }
    if shown.notice == Notice::None {
        quick::saved(shown, facts, false);
        shown.notice = Notice::SaveFailed;
        Route::Repaint
    } else {
        leave(shown, to_settings)
    }
}
