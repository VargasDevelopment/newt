/// Caller-owned runtime state keeps all tracker operations observable under LTO.
/// This is target code-generation evidence, not firmware wiring.
#[unsafe(no_mangle)]
pub fn newt_rust_tracker_probe(
    tracker: &mut newt_core::tracker::ReadingTracker,
    now: u32,
    utc: u32,
    spine: u32,
    character: u32,
    flags: u8,
) -> u64 {
    use newt_core::{progress::Locator, stats_clock::*, time_zone::*, tracker::*};
    let page = ShownPage {
        locator: Locator { spine, character },
        last: flags & 128 != 0,
    };
    let zone = TimeZone::from_raw_or_utc((utc >> 24) as u8);
    let mut matched = false;
    match flags % 12 {
        0 => tracker.enter(now, page),
        1 => tracker.navigate(now, page, ReadingDirection::Forward),
        2 => tracker.navigate(now, page, ReadingDirection::Backward),
        3 => tracker.pause(now),
        4 => tracker.leave(now),
        5 => tracker.relocate(page),
        6 => tracker.stats_marks_mut().day.anchor(utc, now, zone),
        7 => matched = tracker.stats_marks().day.matches(utc, now, zone),
        8 => {
            let before = *tracker.snapshot(now);
            tracker.stats_marks_mut().day.observe(&before, now, utc);
        }
        9 => *tracker = ReadingTracker::new(*tracker.snapshot(now)),
        10 => tracker.stats_marks_mut().end = ReadingEnd::Forgotten,
        _ => {
            let mark = tracker.stats_marks_mut();
            mark.folded_at = utc;
            mark.folded_once = flags & 16 != 0;
            mark.lease_checked = flags & 32 != 0;
            mark.import_checked = flags & 64 != 0;
        }
    }
    let normalized = stats_clock(ClockTime {
        year: (utc >> 16) as u16,
        month: (utc >> 8) as u8,
        day: utc as u8,
        hour: now as u8,
        minute: (now >> 8) as u8,
    });
    let clock = stats_clock_for_day(utc as u16, now as u8);
    let date = stats_date(utc as u16);
    let value = tracker.snapshot(now);
    let marks = tracker.stats_marks();
    value.active_milliseconds
        ^ u64::from(value.forward_pages)
        ^ u64::from(value.qualified_sessions)
        ^ u64::from(value.session_milliseconds)
        ^ u64::from(tracker.open_interval(now))
        ^ u64::from(tracker.reading())
        ^ u64::from(matched)
        ^ marks.day.milliseconds
        ^ u64::from(marks.end as u8)
        ^ u64::from(marks.day.state as u8)
        ^ u64::from(normalized.day)
        ^ u64::from(clock.month)
        ^ u64::from(date.year)
        ^ u64::from(date.month) << 8
        ^ u64::from(date.day) << 16
}
