// SPDX-License-Identifier: MIT
//! HTTP Date parsing, build-date floor and network clock trust policy.
//! Unix seconds only; no chip writes, HTTP client, retained owner or allocation.
#![forbid(unsafe_code)]
use core::ffi::CStr;
use crate::{stats_clock::days_from_civil, time_zone::ClockTime};

pub const NETWORK_CLOCK_FLOOR: u32 = 1_767_225_600;
const DAY_SECONDS: u32 = 86_400;
const TRUST_SPAN: u32 = 631_152_000;
const CHIP_END: u32 = 4_102_444_800;
const WEEKDAYS: &[u8] = b"ThuFriSatSunMonTueWed";
const MONTHS: &[u8] = b"JanFebMarAprMayJunJulAugSepOctNovDec";

/// The first authenticated response's Date and its actual arrival clock.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct NetworkTime { pub seconds: u32, pub at_ms: u32 }
const _: () = assert!(core::mem::size_of::<NetworkTime>() == 8);

fn digits(text: &[u8]) -> Option<u32> {
    let mut value = 0;
    for &byte in text {
        if !byte.is_ascii_digit() { return None; }
        value = value * 10 + u32::from(byte - b'0');
    }
    Some(value)
}
fn named(text: &[u8], names: &[u8]) -> Option<usize> { names.chunks_exact(3).position(|name| name == text) }
fn month_days(year: u32, month: u32) -> u32 {
    const DAYS: [u8; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    u32::from(DAYS[(month - 1) as usize]) + u32::from(month == 2 && year % 4 == 0 && (year % 100 != 0 || year % 400 == 0))
}

pub fn parse_http_date(text: Option<&CStr>) -> u32 {
    let Some(text) = text else { return 0 };
    let text = text.to_bytes();
    if text.len() != 29 || &text[3..5] != b", " || text[7] != b' ' || text[11] != b' ' ||
        text[16] != b' ' || text[19] != b':' || text[22] != b':' || &text[25..] != b" GMT" { return 0; }
    let Some(weekday) = named(&text[..3], WEEKDAYS) else { return 0 };
    let Some(month) = named(&text[8..11], MONTHS).map(|month| month as u32 + 1) else { return 0 };
    let (Some(day), Some(year), Some(hour), Some(minute), Some(second)) =
        (digits(&text[5..7]), digits(&text[12..16]), digits(&text[17..19]), digits(&text[20..22]), digits(&text[23..25])) else { return 0 };
    if !(1970..=2099).contains(&year) || day < 1 || day > month_days(year, month) || hour > 23 || minute > 59 || second > 59 { return 0; }
    let utc = ClockTime { year: year as u16, month: month as u8, day: day as u8, hour: hour as u8, minute: minute as u8 };
    // This existing helper is Unix civil days. StatsClock.day/session seconds
    // use the separate 2000 epoch and cannot substitute for this calculation.
    let days = days_from_civil(utc.year, utc.month, utc.day) as u32;
    if days % 7 != weekday as u32 { return 0; }
    days * DAY_SECONDS + u32::from(utc.hour) * 3600 + u32::from(utc.minute) * 60 + second
}

pub fn network_clock_floor(build_date: Option<&CStr>) -> u32 {
    let Some(build_date) = build_date else { return NETWORK_CLOCK_FLOOR };
    let text = build_date.to_bytes();
    if text.len() != 10 || text[4] != b'-' || text[7] != b'-' { return NETWORK_CLOCK_FLOOR; }
    let (Some(year), Some(month), Some(day)) = (digits(&text[..4]), digits(&text[5..7]), digits(&text[8..10])) else { return NETWORK_CLOCK_FLOOR };
    if !(2026..=2099).contains(&year) || !(1..=12).contains(&month) || day < 1 || day > month_days(year, month) { return NETWORK_CLOCK_FLOOR; }
    (days_from_civil(year as u16, month as u8, day as u8) as u32 - 1) * DAY_SECONDS
}

pub fn trusted_network_time(seconds: u32, floor: u32) -> bool {
    seconds >= floor && seconds - floor <= TRUST_SPAN && seconds < CHIP_END
}

/// Unix seconds for a valid UTC calendar from 1970 onward using wrapping u32 arithmetic.
pub fn utc_seconds(utc: ClockTime, second: u32) -> u32 {
    (days_from_civil(utc.year, utc.month, utc.day) as u32).wrapping_mul(DAY_SECONDS)
        .wrapping_add(u32::from(utc.hour).wrapping_mul(3600))
        .wrapping_add(u32::from(utc.minute).wrapping_mul(60))
        .wrapping_add(second)
}

fn bcd(value: u32) -> u8 { ((value / 10) << 4 | value % 10) as u8 }

/// Encode every u32 Unix second into the existing seven caller bytes. No
/// trust check, century rejection or chip write is added by this pure helper.
pub fn encode_clock_registers(seconds: u32, registers: &mut [u8; 7]) {
    // u32::MAX /60 fits i32. Reuse the single inverse, including1970..1999;
    // the u16 statistics-day API is based at2000 and is not interchangeable.
    let utc = crate::time_zone::civil_from_minutes((seconds / 60) as i32);
    registers[0] = bcd(seconds % 60);
    registers[1] = bcd(u32::from(utc.minute));
    registers[2] = bcd(u32::from(utc.hour));
    registers[3] = ((seconds / DAY_SECONDS + 3) % 7 + 1) as u8;
    registers[4] = bcd(u32::from(utc.day));
    registers[5] = bcd(u32::from(utc.month));
    registers[6] = bcd(u32::from(utc.year) % 100);
}
