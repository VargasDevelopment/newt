// SPDX-License-Identifier: MIT
//! Pure Quick Start transition policy. MIT; see LICENSE.
use super::Step;
use crate::{app::Action, preferences::Orientation, time_zone::{TimeZone, TIME_ZONE_COUNT}};

pub const ORDER: [Step; 6] = [Step::Welcome, Step::Reading, Step::TimeZone, Step::Wifi, Step::Books, Step::Done];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum Notice {
    #[default]
    None,
    SaveFailed,
    ZoneSaveFailed,
}

/// Same five bytes as QuickStartState; the owner lends it for each transition.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct State {
    pub step: Step,
    pub focus: u8,
    pub zone_list_open: bool,
    pub notice: Notice,
    pub from_settings: bool,
}
const _: () = assert!(core::mem::size_of::<State>() == 5);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Facts {
    pub orientation: Orientation,
    pub zone: TimeZone,
    pub wifi_saved: bool,
    pub clock_set: bool,
}
impl Default for Facts {
    fn default() -> Self {
        Self { orientation: Orientation::Portrait, zone: TimeZone::Utc, wifi_saved: false, clock_set: false }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Intent { None, Repaint, Moved, SaveOrientation, SaveZone, SetUpWifi, Finish, Exit }

pub fn enter_step(state: &mut State, facts: &Facts, step: Step) {
    *state = State {
        step,
        from_settings: state.from_settings,
        // The Reading step's cards follow the orientation's stored byte.
        focus: if step == Step::Reading { facts.orientation as u8 } else { 0 },
        ..State::default()
    };
}

pub fn orientation(state: &State) -> Orientation {
    Orientation::from_byte(state.focus).unwrap_or(Orientation::Portrait)
}

fn advance(state: &mut State, facts: &Facts) -> Intent {
    let next = match state.step {
        Step::Welcome => Step::Reading,
        Step::Reading => Step::TimeZone,
        Step::TimeZone => Step::Wifi,
        Step::Wifi => Step::Books,
        Step::Books | Step::Done => Step::Done,
    };
    enter_step(state, facts, next);
    Intent::Moved
}

/// Call for SaveOrientation/SaveZone after the owner's verified choice commit.
/// False confirmation is also used to close an overlay after a record failure.
pub fn saved(state: &mut State, facts: &Facts, verified: bool) -> bool {
    let zone = state.zone_list_open;
    if zone { state.focus = 0; }
    state.zone_list_open = false;
    if !verified {
        state.notice = if zone { Notice::ZoneSaveFailed } else { Notice::SaveFailed };
        return false;
    }
    advance(state, facts);
    true
}

pub fn dispatch(state: &mut State, facts: &Facts, action: Action) -> Intent {
    if state.zone_list_open {
        // The source's list dispatcher clamps even an unhandled press.
        state.focus = state.focus.min((TIME_ZONE_COUNT - 1) as u8);
        return match action {
            Action::Previous if state.focus > 0 => { state.focus -= 1; Intent::Repaint }
            Action::Next if u32::from(state.focus) + 1 < TIME_ZONE_COUNT => { state.focus += 1; Intent::Repaint }
            Action::Back => { state.zone_list_open = false; state.focus = 0; Intent::Repaint }
            Action::Activate => Intent::SaveZone,
            _ => Intent::None,
        };
    }
    let count = match state.step {
        Step::Reading => 4,
        Step::Books | Step::Done => 1,
        _ => 2,
    };
    match action {
        Action::Previous if state.focus > 0 => { state.focus -= 1; return Intent::Repaint; }
        Action::Next if u16::from(state.focus) + 1 < count => { state.focus += 1; return Intent::Repaint; }
        Action::Back => {
            let previous = match state.step {
                Step::Welcome => return if state.from_settings { Intent::Exit } else { Intent::None },
                Step::Reading => Step::Welcome,
                Step::TimeZone => Step::Reading,
                Step::Wifi => Step::TimeZone,
                Step::Books => Step::Wifi,
                Step::Done => Step::Books,
            };
            enter_step(state, facts, previous);
            return Intent::Moved;
        }
        Action::Activate => {}
        _ => return Intent::None,
    }
    let row = if u16::from(state.focus) < count { state.focus } else { 0 };
    match state.step {
        Step::Welcome if row == 1 => Intent::Finish,
        Step::Welcome | Step::Books => advance(state, facts),
        Step::Reading if row < 3 => Intent::SaveOrientation,
        Step::Reading => advance(state, facts),
        Step::TimeZone if row == 0 => {
            state.zone_list_open = true;
            state.focus = facts.zone as u8;
            Intent::Repaint
        }
        Step::TimeZone => advance(state, facts),
        Step::Wifi if (facts.wifi_saved && row == 1) || (!facts.wifi_saved && row == 0) => Intent::SetUpWifi,
        Step::Wifi => advance(state, facts),
        Step::Done => Intent::Finish,
    }
}

#[cfg(test)]
#[path = "quick_start_policy_tests.rs"]
mod tests;
