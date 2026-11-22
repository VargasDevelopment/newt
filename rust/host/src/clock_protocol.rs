//! Proposed rust/host/src/clock_protocol.rs; not wired into the host yet.
//! Host-only allocation is intentional. Core formatting itself allocates none.
use crate::{encoded, hex, number};
use newt_core::{clock_format, time_zone};
use std::ffi::CStr;

pub fn command(fields: &[&str]) -> Option<String> {
    match fields[0] {
        "zone_info" => {
            let raw = u8::try_from(number(fields[1])).expect("zone byte");
            let zone = time_zone::TimeZone::from_raw_or_utc(raw);
            Some(format!(
                "{} {} {}",
                u8::from(time_zone::known_time_zone(u32::from(raw))),
                encoded(time_zone::time_zone_label(zone).as_bytes()),
                encoded(time_zone::time_zone_id(zone).as_bytes()),
            ))
        }
        "clock_time" => {
            let raw = u8::try_from(number(fields[1])).expect("zone byte");
            let utc = time_zone::ClockTime {
                year: u16::try_from(number(fields[2])).expect("year extent"),
                month: u8::try_from(number(fields[3])).expect("month extent"),
                day: u8::try_from(number(fields[4])).expect("day extent"),
                hour: u8::try_from(number(fields[5])).expect("hour extent"),
                minute: u8::try_from(number(fields[6])).expect("minute extent"),
            };
            let local = time_zone::local_clock_time(utc, time_zone::TimeZone::from_raw_or_utc(raw));
            Some(format!(
                "{} {} {} {} {}",
                local.year, local.month, local.day, local.hour, local.minute,
            ))
        }
        "clock_format_info" => {
            let raw = u8::try_from(number(fields[1])).expect("format byte");
            let format = clock_format::ClockFormat::from_raw_or_default(raw);
            Some(format!(
                "{} {} {}",
                u8::from(clock_format::known_clock_format(u32::from(raw))),
                encoded(clock_format::clock_format_label(format).as_bytes()),
                encoded(clock_format::clock_format_id(format).as_bytes()),
            ))
        }
        "clock_text" => {
            let raw = u8::try_from(number(fields[1])).expect("format byte");
            let mut bytes = if fields[2] == "null" {
                Vec::new()
            } else {
                hex(fields[2])
            };
            bytes.push(0);
            let input = if fields[2] == "null" {
                None
            } else {
                Some(CStr::from_bytes_with_nul(&bytes).expect("no embedded NUL in text field"))
            };
            let mut output = [0xa5; clock_format::CLOCK_TEXT_BYTES];
            let shown = clock_format::format_clock_text(
                input,
                clock_format::ClockFormat::from_raw_or_default(raw),
                &mut output,
            );
            let (source, text) = match shown {
                None => ("null", "null".to_owned()),
                Some(text) => (
                    if input.is_some_and(|input| input.as_ptr() == text.as_ptr()) {
                        "input"
                    } else {
                        "output"
                    },
                    encoded(text.to_bytes()),
                ),
            };
            Some(format!("{source} {text} {}", encoded(&output)))
        }
        _ => None,
    }
}
