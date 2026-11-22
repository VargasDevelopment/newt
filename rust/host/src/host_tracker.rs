//! Host-only protocol adapter. Add `mod host_tracker;` to the parity host.
use newt_core::{
    progress::{Locator, Progress},
    stats_clock::*,
    time_zone::{ClockTime, TimeZone},
    tracker::*,
};

fn n(s: &str) -> u64 {
    s.parse().expect("tracker numeric field")
}
fn zone(s: &str) -> TimeZone {
    TimeZone::from_id(n(s) as u32).expect("known tracker protocol zone")
}
fn page(f: &[&str]) -> ShownPage {
    ShownPage {
        locator: Locator {
            spine: n(f[2]) as u32,
            character: n(f[3]) as u32,
        },
        last: n(f[4]) != 0,
    }
}

/// `tracker` + eight progress fields + comma-delimited event words.
pub fn tracker_command(f: &[&str]) -> String {
    let mut t = ReadingTracker::new(Progress {
        sequence: n(f[1]) as u32,
        book: n(f[2]),
        locator: Locator {
            spine: n(f[3]) as u32,
            character: n(f[4]) as u32,
        },
        active_milliseconds: n(f[5]),
        forward_pages: n(f[6]) as u32,
        qualified_sessions: n(f[7]) as u32,
        session_milliseconds: n(f[8]) as u32,
    });
    let mut rows = Vec::new();
    for event in &f[9..] {
        let e: Vec<_> = event.split(',').collect();
        let now = n(e[1]) as u32;
        let mut matched: i8 = -1;
        match e[0] {
            "E" => t.enter(now, page(&e)),
            "N" => t.navigate(
                now,
                page(&e),
                if n(e[5]) == 0 {
                    ReadingDirection::Forward
                } else {
                    ReadingDirection::Backward
                },
            ),
            "P" => t.pause(now),
            "L" => t.leave(now),
            "R" => t.relocate(page(&e)),
            "B" => t = ReadingTracker::new(*t.snapshot(now)),
            "Q" => {}
            "A" => t
                .stats_marks_mut()
                .day
                .anchor(n(e[2]) as u32, now, zone(e[3])),
            "M" => matched = i8::from(t.stats_marks().day.matches(n(e[2]) as u32, now, zone(e[3]))),
            "O" => {
                let before = *t.snapshot(now);
                t.stats_marks_mut()
                    .day
                    .observe(&before, now, n(e[2]) as u32);
            }
            "F" => t.stats_marks_mut().end = ReadingEnd::Forgotten,
            "S" => {
                let marks = t.stats_marks_mut();
                marks.folded_at = n(e[2]) as u32;
                marks.lease_checked = n(e[3]) != 0;
                marks.folded_once = n(e[4]) != 0;
                marks.import_checked = n(e[5]) != 0;
            }
            _ => panic!("unknown tracker event"),
        }
        let v = t.snapshot(now);
        let m = t.stats_marks();
        let d = &m.day;
        rows.push(format!(
            "{} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}",
            v.sequence,
            v.book,
            v.locator.spine,
            v.locator.character,
            v.active_milliseconds,
            v.forward_pages,
            v.qualified_sessions,
            v.session_milliseconds,
            u8::from(t.reading()),
            t.open_interval(now),
            d.milliseconds,
            d.pages,
            d.sessions,
            d.midnight_at,
            d.expires_at,
            d.midnight_utc,
            d.day,
            d.zone as u8,
            d.state as u8,
            m.folded_at,
            u8::from(m.lease_checked),
            u8::from(m.folded_once),
            u8::from(m.import_checked),
            m.end as u8,
            matched,
        ));
    }
    rows.join(" ; ")
}

pub fn clock_command(f: &[&str]) -> String {
    if f[1] == "D" {
        let date = stats_date(n(f[2]) as u16);
        return format!(
            "{} {} {} {} {}",
            date.year, date.month, date.day, date.hour, date.minute
        );
    }
    let clock = match f[1] {
        "C" => stats_clock(ClockTime {
            year: n(f[2]) as u16,
            month: n(f[3]) as u8,
            day: n(f[4]) as u8,
            hour: n(f[5]) as u8,
            minute: n(f[6]) as u8,
        }),
        "F" => stats_clock_for_day(n(f[2]) as u16, n(f[3]) as u8),
        _ => panic!("unknown tracker clock command"),
    };
    format!(
        "{} {} {} {} {}",
        clock.day,
        clock.month,
        clock.day_of_month,
        clock.hour,
        u8::from(clock.trusted())
    )
}
