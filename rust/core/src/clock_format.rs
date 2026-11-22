// SPDX-License-Identifier: MIT
//! Clock display formatting policy (12-hour and 24-hour). MIT.
use core::ffi::CStr;

pub const CLOCK_FORMAT_COUNT: u32 = 2;
/// The longest displayed text, "12:59 PM", plus its NUL terminator.
pub const CLOCK_TEXT_BYTES: usize = 9;

/// Durable IDs: append only; never renumber existing variants.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum ClockFormat {
    #[default]
    TwentyFourHour = 0,
    TwelveHour = 1,
}

pub const fn known_clock_format(value: u32) -> bool {
    value < CLOCK_FORMAT_COUNT
}

impl ClockFormat {
    pub const fn from_id(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::TwentyFourHour),
            1 => Some(Self::TwelveHour),
            _ => None,
        }
    }

    /// Display fallback for unknown raw bytes; durable decoding stays strict.
    pub const fn from_raw_or_default(value: u8) -> Self {
        match Self::from_id(value as u32) {
            Some(format) => format,
            None => Self::TwentyFourHour,
        }
    }
}

pub const fn clock_format_label(format: ClockFormat) -> &'static str {
    match format {
        ClockFormat::TwentyFourHour => "24-hour",
        ClockFormat::TwelveHour => "12-hour",
    }
}

pub const fn clock_format_id(format: ClockFormat) -> &'static str {
    match format {
        ClockFormat::TwentyFourHour => "24h",
        ClockFormat::TwelveHour => "12h",
    }
}

/// Format a C string using caller-owned storage; allocate nothing.
///
/// `None` represents a missing clock string. A 24-hour choice or any invalid HH:MM
/// returns the original borrowed C string and leaves `out` untouched. Other
/// input bytes, including non-UTF-8 fallback text, are preserved exactly.
pub fn format_clock_text<'a>(
    hhmm: Option<&'a CStr>,
    format: ClockFormat,
    out: &'a mut [u8; CLOCK_TEXT_BYTES],
) -> Option<&'a CStr> {
    if format != ClockFormat::TwelveHour {
        return hhmm;
    }
    let input = hhmm?;
    let bytes = input.to_bytes();
    if bytes.len() != 5
        || !bytes[0].is_ascii_digit()
        || !bytes[1].is_ascii_digit()
        || bytes[2] != b':'
        || !bytes[3].is_ascii_digit()
        || !bytes[4].is_ascii_digit()
        || bytes[3] > b'5'
    {
        return hhmm;
    }
    let hour = (bytes[0] - b'0') * 10 + bytes[1] - b'0';
    if hour > 23 {
        return hhmm;
    }
    let shown = if hour % 12 == 0 { 12 } else { hour % 12 };
    let mut length = 0;
    if shown >= 10 {
        out[length] = b'1';
        length += 1;
    }
    out[length] = b'0' + shown % 10;
    out[length + 1] = b':';
    out[length + 2] = bytes[3];
    out[length + 3] = bytes[4];
    out[length + 4] = b' ';
    out[length + 5] = if hour < 12 { b'A' } else { b'P' };
    out[length + 6] = b'M';
    out[length + 7] = 0;
    Some(
        CStr::from_bytes_with_nul(&out[..length + 8])
            .expect("ASCII clock text has exactly one terminal NUL"),
    )
}
