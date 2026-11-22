#[path = "../../target-probe/src/clock_probe.rs"]
mod clock_probe;
#[path = "../../target-probe/src/tracker_probe.rs"]
mod tracker_probe;
use newt_core::{progress::Progress, tracker::*};

#[test]
fn formatter_probe_observes_runtime_text_and_caller_output() {
    for (text, shown) in [
        (c"00:05", b"12:05 AM".as_slice()),
        (c"21:41", b"9:41 PM"),
        (c"12:59", b"12:59 PM"),
    ] {
        let mut output = [0xa5; 9];
        assert_eq!(
            clock_probe::newt_rust_clock_format_probe(Some(text), 1, &mut output),
            shown.len()
        );
        assert_eq!(&output[..shown.len()], shown);
        assert_eq!(output[shown.len()], 0);
        assert!(output[shown.len() + 1..].iter().all(|&b| b == 0xa5));
    }
    let mut output = [0xa5; 9];
    assert_eq!(
        clock_probe::newt_rust_clock_format_probe(Some(c"24:00"), 1, &mut output),
        5
    );
    assert_eq!(output, [0xa5; 9]);
}

#[test]
fn tracker_probe_changes_the_callers_tracker_before_later_operations() {
    let mut tracker = ReadingTracker::new(Progress {
        book: 777,
        ..Progress::default()
    });
    tracker_probe::newt_rust_tracker_probe(&mut tracker, 100, 0, 1, 2, 0);
    assert!(tracker.reading());
    assert_eq!(tracker.snapshot(100).locator.character, 2);
    tracker_probe::newt_rust_tracker_probe(&mut tracker, 999, 0, 9, 7, 5);
    assert_eq!(tracker.snapshot(999).book, 777);
    assert_eq!(tracker.snapshot(999).locator.character, 7);
    assert_eq!(tracker.open_interval(999), 899);
    tracker_probe::newt_rust_tracker_probe(&mut tracker, 999, 0, 0, 0, 10);
    assert_eq!(tracker.stats_marks().end, ReadingEnd::Forgotten);
    assert!(tracker.reading());
    tracker_probe::newt_rust_tracker_probe(&mut tracker, 2100, 0, 0, 0, 3);
    assert!(!tracker.reading());
    assert_eq!(tracker.snapshot(2100).active_milliseconds, 2000);
}
