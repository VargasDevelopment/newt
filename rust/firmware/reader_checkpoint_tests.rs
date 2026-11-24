use super::*;
use super::super::pending_recovery::PendingRecovery;
use crate::retained_runtime::record::{self, BootSnapshot, Recovery, SleepRecord};
use newt_core::app::{AppState, Screen};
use newt_core::progress::Locator;
use newt_reader_ui::RestorePolicy;

const DIGEST: [u8; 32] = [7; 32];
fn progress(character: u32) -> Progress {
    Progress { book: u64::from_le_bytes([7; 8]), sequence: 41,
        locator: Locator { spine: 2, character }, active_milliseconds: 32000,
        forward_pages: 8, qualified_sessions: 3, session_milliseconds: 0 }
}
fn settings() -> ViewSettings {
    ViewSettings { orientation: Orientation::LandscapeCcw, slim_progress_bar: true, refresh_pages: 10,
        ..ViewSettings::default() }
}
fn checkpoint(acknowledged: Progress) -> ReaderCheckpoint {
    let mut checkpoint = ReaderCheckpoint::new(CheckpointRestore::Latest);
    checkpoint.acknowledged = Some(acknowledged);
    checkpoint.acknowledged_digest = Some(DIGEST);
    checkpoint
}

#[test]
fn failed_checkpoint_survives_back_then_home_sleep() {
    let acknowledged = progress(10);
    let live = progress(29);
    let mut checkpoint = checkpoint(acknowledged);
    checkpoint.refuse();
    let mut pending = None;
    PendingRecovery::complete_checkpoint(&mut pending, &checkpoint, live, settings(), &DIGEST);
    assert_eq!(checkpoint.acknowledged_for(&DIGEST), Some(acknowledged));
    let pending = pending.expect("failed checkpoint must survive Back");
    assert_eq!(pending.progress(), &live);
    assert_eq!(pending.settings(), settings());
    assert_eq!(pending.policy(), RestorePolicy::ExactPage);
    // Back has already closed the live reader. A later Home sleep still keeps
    // the unconfirmed page in the actual RTC record and restore policy.
    let app = AppState { screen: Screen::Sleeping, resume_screen: Screen::Home, ..AppState::default() };
    let retained = record::make_record(&app, Some(pending.progress()), pending.settings(),
        false, Recovery::PendingExact, true);
    assert_eq!(record::validate(&retained, live.book, true), record::Status::Valid);
    assert_eq!(retained.resume_screen, Screen::Home as u32);
    assert_eq!(retained.character, 29);
    assert_eq!(retained.checkpoint_saved, 0);
    assert_eq!(retained.recovery_kind(), Some(Recovery::PendingExact));
    let snapshot = BootSnapshot { record: retained, reset: 0, cause: 0,
        deep_wake: true, software_reset: false, gpio_wake: true };
    let mut consumed = SleepRecord::default();
    let mut restored_app = AppState::default();
    let mut restored = Progress::default();
    let mut geometry = ViewSettings::default();
    let report = record::restore_boot(&snapshot, &mut consumed, &mut restored_app,
        Some(&mut restored), Some(&mut geometry), live.book, true);
    assert!(report.restored);
    assert_eq!(restored_app.screen, Screen::Home);
    assert_eq!(restored, live);
    assert_eq!(geometry, settings());
}

#[test]
fn verified_page_with_unconfirmed_destination_keeps_stamped_acknowledgement() {
    let candidate = progress(29);
    let acknowledged = Progress { sequence: 42, ..candidate };
    for restored in [false, true] {
        let mut checkpoint = checkpoint(acknowledged);
        if restored { checkpoint.restore_uncertainty(false, true); }
        checkpoint.observe(CheckpointResult { progress: Outcome::Verified,
            destination: if restored { Outcome::Unchanged } else { Outcome::Unconfirmed } });
        let mut pending = None;
        PendingRecovery::complete_checkpoint(&mut pending, &checkpoint, candidate, settings(), &DIGEST);
        let pending = pending.expect("destination uncertainty preserves the checkpoint");
        assert_eq!(pending.progress(), &acknowledged);
        assert_eq!(pending.policy(), RestorePolicy::ExactPage);
    }
}

#[test]
fn successful_checkpoint_releases_old_retry_geometry() {
    let candidate = progress(29);
    let mut checkpoint = checkpoint(candidate);
    checkpoint.observe(CheckpointResult { progress: Outcome::Verified, destination: Outcome::Verified });
    let mut pending = Some(PendingRecovery { progress: progress(10), settings: ViewSettings::default(),
        policy: RestorePolicy::Validated });
    PendingRecovery::complete_checkpoint(&mut pending, &checkpoint, candidate, settings(), &DIGEST);
    assert!(pending.is_none());
}

#[test]
fn operation_result_resets_without_losing_acknowledgement_or_warning() {
    let acknowledged = progress(29);
    let mut checkpoint = checkpoint(acknowledged);
    checkpoint.observe(CheckpointResult { progress: Outcome::Verified, destination: Outcome::Verified });
    checkpoint.begin_operation();
    assert_eq!(checkpoint.result(), CheckpointResult::default());
    assert_eq!(checkpoint.acknowledged_for(&DIGEST), Some(acknowledged));
    checkpoint.refuse();
    let warning = checkpoint.notice();
    checkpoint.begin_operation();
    assert_eq!(checkpoint.result(), CheckpointResult::default());
    assert_eq!(checkpoint.notice(), warning);
    assert!(checkpoint.page_unconfirmed());
    assert_eq!(checkpoint.acknowledged_for(&DIGEST), Some(acknowledged));
}

#[test]
fn blocked_checkpoint_preserves_frozen_exceptions() {
    for has_progress in [false, true] {
        for changed in [false, true] {
            for accepted_pending in [false, true] {
                let acknowledged = progress(10);
                let candidate = progress(29);
                let mut state = checkpoint(acknowledged);
                let previous = PendingRecovery { progress: progress(7), settings: ViewSettings::default(),
                    policy: RestorePolicy::Validated };
                let mut pending = Some(previous);
                let result = PendingRecovery::blocked_checkpoint(&mut pending, &mut state,
                    has_progress.then_some(candidate), settings(), &DIGEST, changed, accepted_pending);
                let attempted = has_progress && !changed && !accepted_pending;
                assert_eq!(result.is_some(), attempted);
                assert_eq!(state.acknowledged_for(&DIGEST), Some(acknowledged));
                let pending = pending.expect("previous or refused live candidate");
                if attempted {
                    assert_eq!(state.result().progress, Outcome::Unconfirmed);
                    assert_eq!(pending.progress(), &candidate);
                    assert_eq!(pending.settings(), settings());
                    assert_eq!(pending.policy(), RestorePolicy::ExactPage);
                } else {
                    assert_eq!(state.result(), CheckpointResult::default());
                    assert_eq!(state.notice(), None);
                    assert_eq!(pending.progress(), previous.progress());
                    assert_eq!(pending.settings(), previous.settings());
                    assert_eq!(pending.policy(), previous.policy());
                }
            }
        }
    }
}

#[test]
fn reading_time_ends_before_slow_verification() {
    use newt_core::tracker::{ReadingTracker, ShownPage};
    let mut tracker = ReadingTracker::new(progress(10));
    let page = ShownPage { locator: progress(10).locator, last: false };
    tracker.enter(100, page);
    tracker.leave(1100);
    let closed = *tracker.snapshot(1100);
    assert_eq!(closed.active_milliseconds, 33000);
    // Finishing verification much later cannot add time to this closed reader.
    assert_eq!(*tracker.snapshot(21100), closed);
    let mut reopened = ReadingTracker::new(closed);
    reopened.enter(21100, page);
    assert_eq!(*reopened.snapshot(21101), closed);
}

#[test]
fn settings_reflow_keeps_the_attempts_position_and_new_geometry() {
    use newt_core::preferences::Orientation;
    let live = progress(73);
    let acknowledged = Progress { sequence: 17, ..progress(19) };
    for saved in [false, true] {
        for orientation in [Orientation::Portrait, Orientation::LandscapeCcw] {
            let proposed = ViewSettings { orientation, refresh_pages: 10,
                slim_progress_bar: false, ..ViewSettings::default() };
            let pending = PendingRecovery::for_reflow(live, acknowledged, proposed, saved);
            assert_eq!(*pending.progress(), if saved { acknowledged } else { live });
            assert_eq!(pending.settings(), proposed);
            assert_eq!(pending.policy(), RestorePolicy::Validated);
        }
    }
}

// Controls observe valid values with clean exits. Assertion panics, malformed
// output, diagnostics or nonzero processes never count as semantic differences.
#[test]
fn protocol_rows() {
    let candidate = progress(29);
    let mut state = checkpoint(progress(10));
    state.refuse();
    let mut pending = None;
    PendingRecovery::complete_checkpoint(&mut pending, &state, candidate, settings(), &DIGEST);
    println!("R pending {} {} {}", u8::from(pending.is_some()),
        pending.map_or(0, |p| p.progress().locator.character),
        u8::from(pending.is_some_and(|p| p.policy().requires_exact_page())));

    state.observe(CheckpointResult { progress: Outcome::Verified, destination: Outcome::Verified });
    state.begin_operation();
    println!("R result {} {}", state.result().progress as u8, state.result().destination as u8);

    let acknowledged = Progress { sequence: 42, ..candidate };
    let mut state = checkpoint(acknowledged);
    state.observe(CheckpointResult { progress: Outcome::Verified, destination: Outcome::Unconfirmed });
    PendingRecovery::complete_checkpoint(&mut pending, &state, candidate, settings(), &DIGEST);
    println!("R stamped {} {}", u8::from(pending.is_some()), pending.map_or(0, |p| p.progress().sequence));
    state.observe(CheckpointResult { progress: Outcome::Verified, destination: Outcome::Verified });
    PendingRecovery::complete_checkpoint(&mut pending, &state, candidate, settings(), &DIGEST);
    println!("R cleared {}", u8::from(pending.is_none()));
    for (name, changed, accepted_pending) in [("ordinary", false, false),
        ("changed", true, false), ("deferred", false, true)] {
        let mut state = checkpoint(progress(10));
        let mut pending = None;
        let result = PendingRecovery::blocked_checkpoint(&mut pending, &mut state,
            Some(candidate), settings(), &DIGEST, changed, accepted_pending);
        println!("R blocked_{} {} {}", name, u8::from(result.is_some()), u8::from(pending.is_some()));
    }
}
