use super::*;
use crate::{app::Action, preferences::Orientation, time_zone::TimeZone};

#[test]
fn choice_failure_closes_zone_list_and_keeps_failed_zone_row() {
    let facts = Facts { zone: TimeZone::India, ..Facts::default() };
    let mut state = State::default();
    enter_step(&mut state, &facts, Step::TimeZone);
    assert_eq!(dispatch(&mut state, &facts, Action::Activate), Intent::Repaint);
    assert_eq!(state.focus, 11);
    assert_eq!(dispatch(&mut state, &facts, Action::Previous), Intent::Repaint);
    assert!(!saved(&mut state, &facts, false));
    assert_eq!(state, State { step: Step::TimeZone, notice: Notice::ZoneSaveFailed, ..State::default() });
    assert_eq!(dispatch(&mut state, &facts, Action::Activate), Intent::Repaint);
    assert_eq!(state.focus, 11);
    assert!(saved(&mut state, &facts, true));
    assert_eq!(state, State { step: Step::Wifi, ..State::default() });
}

#[test]
fn reading_default_and_back_use_acknowledged_facts() {
    let facts = Facts { orientation: Orientation::LandscapeCcw, ..Facts::default() };
    let mut state = State { from_settings: true, ..State::default() };
    assert_eq!(dispatch(&mut state, &facts, Action::Activate), Intent::Moved);
    assert_eq!(orientation(&state), Orientation::LandscapeCcw);
    assert_eq!(dispatch(&mut state, &facts, Action::Activate), Intent::SaveOrientation);
    assert!(!saved(&mut state, &facts, false));
    assert_eq!(state.notice, Notice::SaveFailed);
    assert_eq!(state.step, Step::Reading);
    assert!(saved(&mut state, &facts, true));
    assert_eq!(state.step, Step::TimeZone);
    assert_eq!(dispatch(&mut state, &facts, Action::Back), Intent::Moved);
    assert_eq!(state.focus, 1);
    assert_eq!(state.notice, Notice::None);
    assert!(state.from_settings);
}

#[test]
fn skipped_run_and_settings_exit_keep_distinct_intents() {
    let facts = Facts::default();
    let mut state = State::default();
    assert_eq!(dispatch(&mut state, &facts, Action::Back), Intent::None);
    state.from_settings = true;
    assert_eq!(dispatch(&mut state, &facts, Action::Back), Intent::Exit);
    dispatch(&mut state, &facts, Action::Next);
    assert_eq!(dispatch(&mut state, &facts, Action::Activate), Intent::Finish);
    enter_step(&mut state, &facts, Step::Reading);
    for _ in 0..3 { dispatch(&mut state, &facts, Action::Next); }
    assert_eq!(dispatch(&mut state, &facts, Action::Activate), Intent::Moved);
    assert_eq!(state.step, Step::TimeZone);
    dispatch(&mut state, &facts, Action::Next);
    dispatch(&mut state, &facts, Action::Activate);
    assert_eq!(dispatch(&mut state, &facts, Action::Activate), Intent::SetUpWifi);
    dispatch(&mut state, &facts, Action::Next);
    assert_eq!(dispatch(&mut state, &facts, Action::Activate), Intent::Moved);
    assert_eq!(dispatch(&mut state, &facts, Action::Activate), Intent::Moved);
    assert_eq!(dispatch(&mut state, &facts, Action::Activate), Intent::Finish);
    assert_eq!(state.step, Step::Done);
}

#[test]
fn reading_cards_choose_each_orientation_and_reopen_on_the_saved_one() {
    for (row, chosen) in Orientation::ALL.into_iter().enumerate() {
        let mut state = State::default();
        enter_step(&mut state, &Facts::default(), Step::Reading);
        for _ in 0..row { dispatch(&mut state, &Facts::default(), Action::Next); }
        assert_eq!(dispatch(&mut state, &Facts::default(), Action::Activate), Intent::SaveOrientation);
        assert_eq!(orientation(&state), chosen);
        let facts = Facts { orientation: chosen, ..Facts::default() };
        enter_step(&mut state, &facts, Step::Reading);
        assert_eq!(usize::from(state.focus), row);
    }
}
