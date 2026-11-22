// SPDX-License-Identifier: MIT
//! Fixed Newt time-zone and DST rules; no platform TZ state or allocation. MIT.

pub const TIME_ZONE_COUNT: u32 = 15;

/// Durable IDs: append only; never renumber existing variants.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum TimeZone {
    #[default]
    Utc = 0,
    Hawaii = 1,
    Alaska = 2,
    UsPacific = 3,
    UsMountain = 4,
    Arizona = 5,
    UsCentral = 6,
    UsEastern = 7,
    UkIreland = 8,
    CentralEurope = 9,
    EasternEurope = 10,
    India = 11,
    ChinaSingapore = 12,
    JapanKorea = 13,
    AustraliaEastern = 14,
}

pub const fn known_time_zone(value: u32) -> bool {
    value < TIME_ZONE_COUNT
}

impl TimeZone {
    /// Strict validation for a durable ID; unknown IDs are not accepted.
    pub const fn from_id(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Utc),
            1 => Some(Self::Hawaii),
            2 => Some(Self::Alaska),
            3 => Some(Self::UsPacific),
            4 => Some(Self::UsMountain),
            5 => Some(Self::Arizona),
            6 => Some(Self::UsCentral),
            7 => Some(Self::UsEastern),
            8 => Some(Self::UkIreland),
            9 => Some(Self::CentralEurope),
            10 => Some(Self::EasternEurope),
            11 => Some(Self::India),
            12 => Some(Self::ChinaSingapore),
            13 => Some(Self::JapanKorea),
            14 => Some(Self::AustraliaEastern),
            _ => None,
        }
    }

    /// Display fallback for an unknown raw enum byte.
    /// This is not permission to accept an unknown durable record.
    pub const fn from_raw_or_utc(value: u8) -> Self {
        match Self::from_id(value as u32) {
            Some(zone) => zone,
            None => Self::Utc,
        }
    }
}

pub static TIME_ZONE_LABELS: [&str; TIME_ZONE_COUNT as usize] = [
    "UTC",
    "Hawaii",
    "Alaska",
    "US Pacific",
    "US Mountain",
    "Arizona",
    "US Central",
    "US Eastern",
    "UK/Ireland",
    "Central Europe",
    "Eastern Europe",
    "India",
    "China/Singapore",
    "Japan/Korea",
    "Australia Eastern",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
}

impl Default for ClockTime {
    fn default() -> Self {
        Self {
            year: 2000,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
        }
    }
}

// POSIX Mm.w.d/h: hour is local time before the transition; week 5 is last.
#[derive(Clone, Copy)]
struct Transition {
    month: u8,
    week: u8,
    weekday: u8,
    hour: u8,
}

#[derive(Clone, Copy)]
struct DstRule {
    start: Transition,
    end: Transition,
}

#[derive(Clone, Copy)]
enum RuleFamily {
    Us,
    Uk,
    Cet,
    Eet,
    Au,
}

static RULES: [DstRule; 5] = [
    DstRule {
        start: Transition {
            month: 3,
            week: 2,
            weekday: 0,
            hour: 2,
        },
        end: Transition {
            month: 11,
            week: 1,
            weekday: 0,
            hour: 2,
        },
    },
    DstRule {
        start: Transition {
            month: 3,
            week: 5,
            weekday: 0,
            hour: 1,
        },
        end: Transition {
            month: 10,
            week: 5,
            weekday: 0,
            hour: 2,
        },
    },
    DstRule {
        start: Transition {
            month: 3,
            week: 5,
            weekday: 0,
            hour: 2,
        },
        end: Transition {
            month: 10,
            week: 5,
            weekday: 0,
            hour: 3,
        },
    },
    DstRule {
        start: Transition {
            month: 3,
            week: 5,
            weekday: 0,
            hour: 3,
        },
        end: Transition {
            month: 10,
            week: 5,
            weekday: 0,
            hour: 4,
        },
    },
    DstRule {
        start: Transition {
            month: 10,
            week: 1,
            weekday: 0,
            hour: 2,
        },
        end: Transition {
            month: 4,
            week: 1,
            weekday: 0,
            hour: 3,
        },
    },
];

struct ZoneRule {
    id: &'static str,
    standard_minutes: i16,
    dst: Option<RuleFamily>,
}

static ZONES: [ZoneRule; TIME_ZONE_COUNT as usize] = [
    ZoneRule {
        id: "utc",
        standard_minutes: 0,
        dst: None,
    },
    ZoneRule {
        id: "hawaii",
        standard_minutes: -600,
        dst: None,
    },
    ZoneRule {
        id: "alaska",
        standard_minutes: -540,
        dst: Some(RuleFamily::Us),
    },
    ZoneRule {
        id: "us-pacific",
        standard_minutes: -480,
        dst: Some(RuleFamily::Us),
    },
    ZoneRule {
        id: "us-mountain",
        standard_minutes: -420,
        dst: Some(RuleFamily::Us),
    },
    ZoneRule {
        id: "arizona",
        standard_minutes: -420,
        dst: None,
    },
    ZoneRule {
        id: "us-central",
        standard_minutes: -360,
        dst: Some(RuleFamily::Us),
    },
    ZoneRule {
        id: "us-eastern",
        standard_minutes: -300,
        dst: Some(RuleFamily::Us),
    },
    ZoneRule {
        id: "uk-ireland",
        standard_minutes: 0,
        dst: Some(RuleFamily::Uk),
    },
    ZoneRule {
        id: "central-europe",
        standard_minutes: 60,
        dst: Some(RuleFamily::Cet),
    },
    ZoneRule {
        id: "eastern-europe",
        standard_minutes: 120,
        dst: Some(RuleFamily::Eet),
    },
    ZoneRule {
        id: "india",
        standard_minutes: 330,
        dst: None,
    },
    ZoneRule {
        id: "china-singapore",
        standard_minutes: 480,
        dst: None,
    },
    ZoneRule {
        id: "japan-korea",
        standard_minutes: 540,
        dst: None,
    },
    ZoneRule {
        id: "australia-eastern",
        standard_minutes: 600,
        dst: Some(RuleFamily::Au),
    },
];

pub fn time_zone_label(zone: TimeZone) -> &'static str {
    TIME_ZONE_LABELS[zone as usize]
}

pub fn time_zone_id(zone: TimeZone) -> &'static str {
    ZONES[zone as usize].id
}

// Hinnant's civil algorithms, as in the reference, for positive post-1970 days.
fn days_from_civil(year: u16, month: u8, day: u8) -> i32 {
    let y = i32::from(year) - if month <= 2 { 1 } else { 0 };
    let era = y / 400;
    let yoe = y - era * 400;
    let doy =
        (153 * (i32::from(month) + if month > 2 { -3 } else { 9 }) + 2) / 5 + i32::from(day) - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

pub(crate) fn civil_from_minutes(minutes: i32) -> ClockTime {
    let z = minutes / 1440 + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    ClockTime {
        year: (yoe + era * 400 + if month <= 2 { 1 } else { 0 }) as u16,
        month: month as u8,
        day: (doy - (153 * mp + 2) / 5 + 1) as u8,
        hour: (minutes % 1440 / 60) as u8,
        minute: (minutes % 60) as u8,
    }
}

// Offset is the local offset in effect before this transition.
fn transition_minute(year: u16, at: Transition, offset: i32) -> i32 {
    let first = days_from_civil(year, at.month, 1);
    let next = if at.month == 12 {
        days_from_civil(year + 1, 1, 1)
    } else {
        days_from_civil(year, at.month + 1, 1)
    };
    let length = next - first;
    let mut day =
        1 + (i32::from(at.weekday) + 7 - (first + 4) % 7) % 7 + 7 * (i32::from(at.week) - 1);
    while day > length {
        day -= 7;
    }
    days_from_civil(year, at.month, day as u8) * 1440 + i32::from(at.hour) * 60 - offset
}

/// Convert a valid Gregorian UTC calendar value in years 2000..=2199.
///
/// Month/day must exist; hour is 0..=23 and minute is 0..=59 (matching the
/// RTC chip input contract; malformed calendar values are unsupported).
/// Local output can cross into 1999 or 2200. No mutable TZ state is consulted.
pub fn local_clock_time(utc: ClockTime, zone: TimeZone) -> ClockTime {
    let selected = &ZONES[zone as usize];
    let now = days_from_civil(utc.year, utc.month, utc.day) * 1440
        + i32::from(utc.hour) * 60
        + i32::from(utc.minute);
    let mut offset = i32::from(selected.standard_minutes);
    if let Some(family) = selected.dst {
        let dst = &RULES[family as usize];
        // All listed changes are far from New Year, so the UTC year applies.
        let start = transition_minute(utc.year, dst.start, offset);
        let end = transition_minute(utc.year, dst.end, offset + 60);
        let active = if start < end {
            now >= start && now < end
        } else {
            now >= start || now < end
        };
        if active {
            offset += 60;
        }
    }
    civil_from_minutes(now + offset)
}
