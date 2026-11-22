// SPDX-License-Identifier: MIT
//! DS3231 time and status snapshot decoding. MIT.
//!
//! Pure policy over registers 0x00..=0x0f as the native bridge read them. The
//! bounded read-only bus transaction stays in the board bridge
//! (native/clock_transaction.h, behind nb_clock_read and nb_clock_snapshot);
//! nothing here writes the chip, acknowledges OSF or retries.
use crate::time_zone::ClockTime;

/// Status register (0x0f) oscillator-stop flag: the time may be wrong.
pub const OSCILLATOR_STOPPED: u8 = 0x80;

pub const fn clock_bcd(value: u8) -> u8 {
    (value >> 4) * 10 + (value & 15)
}

pub const fn clock_bcd_valid(value: u8) -> bool {
    value & 15 < 10 && value >> 4 < 10
}

const fn clock_year(month: u8, year: u8) -> u16 {
    2000 + clock_bcd(year) as u16 + if month & 0x80 != 0 { 100 } else { 0 }
}

/// The 24-hour hour of a plausible snapshot, else None. Checks BCD digits,
/// 12/24-hour mode (12 AM is 0, 12 PM is 12), weekday 1..=7, seconds and
/// minutes, the month's reserved bits and the Gregorian day, century included.
pub fn plausible_clock(raw: &[u8; 16]) -> Option<u8> {
    let [second, minute, hours, weekday, day, month, year, ..] = *raw;
    if !clock_bcd_valid(second)
        || !clock_bcd_valid(minute)
        || !clock_bcd_valid(day)
        || !clock_bcd_valid(year)
    {
        return None;
    }
    if hours & 0x80 != 0
        || month & 0x60 != 0
        || !clock_bcd_valid(month & 0x1f)
        || !(1..=7).contains(&weekday)
        || clock_bcd(second) > 59
        || clock_bcd(minute) > 59
    {
        return None;
    }
    let hour = if hours & 0x40 != 0 {
        let twelve = clock_bcd(hours & 0x1f);
        if !clock_bcd_valid(hours & 0x1f) || !(1..=12).contains(&twelve) {
            return None;
        }
        twelve % 12 + if hours & 0x20 != 0 { 12 } else { 0 }
    } else {
        let hour = clock_bcd(hours);
        if !clock_bcd_valid(hours) || hour > 23 {
            return None;
        }
        hour
    };
    let (month_number, day_number) = (clock_bcd(month & 0x1f), clock_bcd(day));
    if !(1..=12).contains(&month_number) || day_number < 1 {
        return None;
    }
    let year = clock_year(month, year);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    const DAYS: [u8; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let last = DAYS[usize::from(month_number - 1)] + u8::from(month_number == 2 && leap);
    (day_number <= last).then_some(hour)
}

// The chip holds UTC; `raw` is plausible and `hour` is its 24-hour hour.
fn clock_utc(raw: &[u8; 16], hour: u8) -> ClockTime {
    ClockTime {
        year: clock_year(raw[5], raw[6]),
        month: clock_bcd(raw[5] & 0x1f),
        day: clock_bcd(raw[4]),
        hour,
        minute: clock_bcd(raw[1]),
    }
}

/// UTC of a plausible snapshot whose oscillator kept running, as Home and
/// reading statistics both trust it; None for any other snapshot.
pub fn valid_clock_utc(raw: &[u8; 16]) -> Option<ClockTime> {
    let hour = plausible_clock(raw)?;
    if raw[15] & OSCILLATOR_STOPPED != 0 {
        return None;
    }
    Some(clock_utc(raw, hour))
}
