// SPDX-License-Identifier: MIT
//! Day-clock and calendar helpers for reading statistics.
//! MIT; see LICENSE.
use crate::time_zone::{ClockTime, TimeZone, local_clock_time};

pub const STATS_FIRST_DAY: u16 = 9497;
pub const STATS_LAST_DAY: u16 = 36524;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatsClock {
    pub day: u16,
    pub month: u16,
    pub day_of_month: u8,
    pub hour: u8,
}

impl StatsClock {
    pub const fn trusted(self) -> bool {
        self.day != 0
    }
}

/// RTC session clock for reading statistics.
/// Only a complete, plausible, running UTC clock in 2026..2099 is trusted.
/// UTC seconds use the statistics epoch (2000-01-01); publish them before
/// judging the local date, which can cross either supported year boundary.
pub fn stats_session_clock(
    raw: Option<&[u8; 16]>,
    zone: TimeZone,
    mut utc_seconds: Option<&mut u32>,
) -> StatsClock {
    if let Some(seconds) = utc_seconds.as_deref_mut() {
        *seconds = 0;
    }
    let Some(raw) = raw else {
        return StatsClock::default();
    };
    let Some(utc) = crate::ds3231::valid_clock_utc(raw) else {
        return StatsClock::default();
    };
    if !(2026..=2099).contains(&utc.year) {
        return StatsClock::default();
    }
    if let Some(seconds) = utc_seconds {
        *seconds = u32::from(stats_clock(utc).day) * 86_400
            + u32::from(utc.hour) * 3600
            + u32::from(utc.minute) * 60
            + u32::from(crate::ds3231::clock_bcd(raw[0]));
    }
    stats_clock(local_clock_time(utc, zone))
}

// The input checks below bound this calculation to positive years/months.
pub(crate) fn days_from_civil(year: u16, month: u8, day: u8) -> i32 {
    let year = i32::from(year) - i32::from(month <= 2);
    let era = year / 400;
    let yoe = year - era * 400;
    let adjusted_month = i32::from(month) + if month > 2 { -3 } else { 9 };
    let doy = (153 * adjusted_month + 2) / 5 + i32::from(day) - 1;
    era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468
}

/// Preserve the reference's partial validation: minute is ignored and a day
/// beyond the month's end normalizes through civil arithmetic, even day 255.
pub fn stats_clock(local: ClockTime) -> StatsClock {
    if local.year < 2026
        || local.year > 2099
        || local.month < 1
        || local.month > 12
        || local.day < 1
        || local.hour > 23
    {
        return StatsClock::default();
    }
    let day = days_from_civil(local.year, local.month, local.day) - 10957;
    stats_clock_for_day(day as u16, local.hour)
}

pub fn stats_clock_for_day(day: u16, hour: u8) -> StatsClock {
    if !(STATS_FIRST_DAY..=STATS_LAST_DAY).contains(&day) || hour > 23 {
        return StatsClock::default();
    }
    let date = stats_date(day);
    StatsClock {
        day,
        month: date.year * 12 + u16::from(date.month) - 1,
        day_of_month: date.day,
        hour,
    }
}

/// Civil date for every u16 day since 2000-01-01, including undated IDs.
pub fn stats_date(day: u16) -> ClockTime {
    let z = i32::from(day) + 10957 + 719468;
    let era = z / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    ClockTime {
        year: (yoe as i32 + era * 400 + i32::from(month <= 2)) as u16,
        month,
        day: (doy - (153 * mp + 2) / 5 + 1) as u8,
        ..ClockTime::default()
    }
}

pub(crate) fn local_stats_second(utc: u32, zone: TimeZone) -> StatsClock {
    let mut date = stats_date((utc / 86400) as u16);
    date.hour = (utc % 86400 / 3600) as u8;
    date.minute = (utc % 3600 / 60) as u8;
    stats_clock(local_clock_time(date, zone))
}

// A fixed 26-hour bound covers all listed zones' 23/25-hour DST days.
pub(crate) fn next_stats_midnight(utc: u32, zone: TimeZone) -> u32 {
    let day = local_stats_second(utc, zone).day;
    let (mut low, mut high) = (1, 26 * 3600);
    while low < high {
        let middle = low + (high - low) / 2;
        if local_stats_second(utc.wrapping_add(middle), zone).day == day {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    utc.wrapping_add(low)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_date_validation_is_preserved() {
        let overflow = stats_clock(ClockTime {
            year: 2026,
            month: 2,
            day: 31,
            hour: 23,
            minute: 255,
        });
        assert_eq!(
            overflow,
            stats_clock(ClockTime {
                year: 2026,
                month: 3,
                day: 3,
                hour: 23,
                minute: 0,
            })
        );
        assert!(overflow.trusted());
        assert!(
            !stats_clock(ClockTime {
                year: 2099,
                month: 12,
                day: 32,
                ..ClockTime::default()
            })
            .trusted()
        );
        assert_eq!(stats_date(0), ClockTime::default());
        assert_eq!(stats_date(9497).year, 2026);
        assert_eq!(stats_date(36524).day, 31);
    }
}

// Calendar helpers for stats day counts. MIT; see LICENSE.
pub(crate) fn days_in_month(month: u16) -> usize {
    let year = month / 12;
    let index = usize::from(month % 12);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][index] + usize::from(index == 1 && leap)
}
pub fn stats_bucket(hour: u8) -> usize {
    match hour {
        5..=11 => 0,
        12..=16 => 1,
        17..=20 => 2,
        _ => 3,
    }
}
pub fn halloween(today: StatsClock) -> bool {
    today.trusted() && today.month % 12 == 9 && today.day_of_month == 31
}
pub fn christmas(today: StatsClock) -> bool {
    today.trusted() && today.month % 12 == 11 && matches!(today.day_of_month, 24 | 25)
}
/// US Thanksgiving: November's fourth Thursday always falls on the 22nd to the 28th.
/// Day 0 (2000-01-01) was a Saturday, so Thursdays are the days 5 mod 7.
pub fn thanksgiving(today: StatsClock) -> bool {
    today.trusted()
        && today.month % 12 == 10
        && (22..=28).contains(&today.day_of_month)
        && today.day % 7 == 5
}

#[cfg(test)]
mod thanksgiving_tests {
    use super::*;

    fn on(year: u16, month: u8, day: u8) -> bool {
        thanksgiving(stats_clock(ClockTime {
            year,
            month,
            day,
            hour: 12,
            ..ClockTime::default()
        }))
    }

    #[test]
    fn fourth_thursday_of_november_only() {
        // 2029 has the earliest date and 2030 the latest.
        for (year, day) in [(2026, 26), (2027, 25), (2028, 23), (2029, 22), (2030, 28), (2099, 26)] {
            assert!(on(year, 11, day), "{year}-11-{day}");
            assert!(!on(year, 11, day - 1), "the Wednesday before {year}-11-{day}");
            assert!(!on(year, 11, day + 1), "the Friday after {year}-11-{day}");
            assert!(!on(year, 11, day - 7), "the third Thursday of {year}");
            assert!(!on(year, 10, day), "October {year}");
        }
        // 2029's fifth Thursday is not Thanksgiving; neither is a Thursday outside November.
        assert!(!on(2029, 11, 29));
        assert!(!on(2026, 12, 24));
        assert!(!thanksgiving(StatsClock::default()), "an untrusted clock has no holiday");
    }
}

#[cfg(test)]
mod rtc_session_tests {
    use super::*;

    fn registers(second: u8, minute: u8, hour: u8, day: u8, month: u8, year: u8) -> [u8; 16] {
        let mut raw = [0; 16];
        raw[..7].copy_from_slice(&[second, minute, hour, 1, day, month, year]);
        raw
    }

    #[test]
    fn leap_day_uses_local_hour_and_utc_epoch_seconds() {
        let raw = registers(0x37, 0x45, 0x18, 0x29, 2, 0x28);
        let mut seconds = 77;
        let clock = stats_session_clock(Some(&raw), TimeZone::UsCentral, Some(&mut seconds));
        assert_eq!(
            clock,
            StatsClock {
                day: 10286,
                month: 24337,
                day_of_month: 29,
                hour: 12
            }
        );
        assert_eq!(
            seconds, 888777937,
            "UTC seconds use 2000, not Unix/local time"
        );
        assert_eq!(
            stats_session_clock(Some(&raw), TimeZone::UsCentral, None),
            clock
        );
    }

    #[test]
    fn missing_corrupt_or_stopped_clock_clears_the_optional_output() {
        let mut seconds = 77;
        assert_eq!(
            stats_session_clock(None, TimeZone::Utc, Some(&mut seconds)),
            StatsClock::default()
        );
        assert_eq!(seconds, 0, "failed read clears prior UTC timestamp");
        let raw = registers(0x56, 0x34, 0x12, 1, 7, 0x26);
        for (index, value) in [(15, 0x80), (0, 0x6a), (3, 0), (4, 0x32)] {
            let mut damaged = raw;
            damaged[index] = value;
            seconds = 77;
            assert_eq!(
                stats_session_clock(Some(&damaged), TimeZone::Utc, Some(&mut seconds)),
                StatsClock::default()
            );
            assert_eq!(seconds, 0, "invalid snapshot clears prior UTC timestamp");
        }
    }

    #[test]
    fn utc_year_is_validated_before_the_zone_can_move_it_into_range() {
        for (raw, zone) in [
            (
                registers(0x59, 0x59, 0x23, 0x31, 0x12, 0x25),
                TimeZone::JapanKorea,
            ),
            (registers(1, 0, 0, 1, 0x81, 0), TimeZone::UsCentral),
        ] {
            let mut seconds = 77;
            assert_eq!(
                stats_session_clock(Some(&raw), zone, Some(&mut seconds)),
                StatsClock::default()
            );
            assert_eq!(
                seconds, 0,
                "out-of-range UTC cannot become dated through a zone"
            );
        }
    }

    #[test]
    fn local_out_of_range_does_not_erase_valid_utc_seconds() {
        for (raw, zone, expected) in [
            (
                registers(0, 1, 0, 1, 1, 0x26),
                TimeZone::UsCentral,
                820540860,
            ),
            (
                registers(0x59, 0x59, 0x23, 0x31, 0x12, 0x99),
                TimeZone::JapanKorea,
                3155759999,
            ),
        ] {
            let mut seconds = 77;
            assert_eq!(
                stats_session_clock(Some(&raw), zone, Some(&mut seconds)),
                StatsClock::default()
            );
            assert_eq!(
                seconds, expected,
                "UTC timestamp survives unsupported local date"
            );
        }
    }

    #[test]
    fn last_trusted_utc_second_and_summer_offset_are_exact() {
        let last = registers(0x59, 0x59, 0x23, 0x31, 0x12, 0x99);
        let mut seconds = 77;
        assert_eq!(
            stats_session_clock(Some(&last), TimeZone::Utc, Some(&mut seconds)),
            StatsClock {
                day: 36524,
                month: 25199,
                day_of_month: 31,
                hour: 23
            }
        );
        assert_eq!(seconds, 3155759999);
        let summer = registers(0x56, 0x34, 0x12, 1, 7, 0x26);
        assert_eq!(
            stats_session_clock(Some(&summer), TimeZone::UsCentral, Some(&mut seconds)),
            StatsClock {
                day: 9678,
                month: 24318,
                day_of_month: 1,
                hour: 7
            }
        );
        assert_eq!(seconds, 836224496);
    }
}
