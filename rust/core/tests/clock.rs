//! Proposed rust/core/tests/clock.rs. Focused assertions; not executed here.
use core::ffi::CStr;
use newt_core::{clock_format::*, time_zone::*};

#[test]
fn display_fallback_does_not_accept_an_unknown_durable_id() {
    for value in 0..=255u8 {
        let id = u32::from(value);
        assert_eq!(TimeZone::from_id(id).is_some(), known_time_zone(id));
        assert_eq!(ClockFormat::from_id(id).is_some(), known_clock_format(id));
        if !known_time_zone(id) {
            let zone = TimeZone::from_raw_or_utc(value);
            assert_eq!(zone, TimeZone::Utc);
            assert_eq!(time_zone_label(zone), "UTC");
            assert_eq!(time_zone_id(zone), "utc");
            assert_eq!(
                local_clock_time(ClockTime::default(), zone),
                ClockTime::default()
            );
        }
        if !known_clock_format(id) {
            assert_eq!(
                ClockFormat::from_raw_or_default(value),
                ClockFormat::TwentyFourHour
            );
        }
    }
    for id in [256, 65_536, u32::MAX] {
        assert_eq!(TimeZone::from_id(id), None);
        assert_eq!(ClockFormat::from_id(id), None);
    }
}

#[test]
fn invalid_text_is_borrowed_and_never_touches_output() {
    let non_utf8 = CStr::from_bytes_with_nul(b"\xff\0").unwrap();
    let invalid = [
        c"", c"--:--", c"24:00", c"12:60", c"1:05", c"12:05 ", non_utf8,
    ];
    for format in [ClockFormat::TwentyFourHour, ClockFormat::TwelveHour] {
        let mut out = [0xa5; CLOCK_TEXT_BYTES];
        assert_eq!(format_clock_text(None, format, &mut out), None);
        assert_eq!(out, [0xa5; CLOCK_TEXT_BYTES]);
        for input in invalid {
            let mut out = [0xa5; CLOCK_TEXT_BYTES];
            let shown = format_clock_text(Some(input), format, &mut out).unwrap();
            assert_eq!(shown.as_ptr(), input.as_ptr());
            assert_eq!(out, [0xa5; CLOCK_TEXT_BYTES]);
        }
    }
    let mut out = [0xa5; CLOCK_TEXT_BYTES];
    let input = c"21:41";
    let shown = format_clock_text(Some(input), ClockFormat::TwentyFourHour, &mut out).unwrap();
    assert_eq!(shown.as_ptr(), input.as_ptr());
    assert_eq!(out, [0xa5; CLOCK_TEXT_BYTES]);
}

#[test]
fn formatted_text_is_terminated_and_preserves_spare_bytes() {
    for (input, expected) in [
        (c"00:05", c"12:05 AM"),
        (c"12:59", c"12:59 PM"),
        (c"21:41", c"9:41 PM"),
        (c"23:59", c"11:59 PM"),
    ] {
        let mut out = [0xa5; CLOCK_TEXT_BYTES];
        let shown = format_clock_text(Some(input), ClockFormat::TwelveHour, &mut out).unwrap();
        assert_eq!(shown, expected);
        assert_ne!(shown.as_ptr(), input.as_ptr());
        let used = shown.to_bytes_with_nul().len();
        assert_eq!(&out[..used], expected.to_bytes_with_nul());
        assert!(out[used..].iter().all(|&byte| byte == 0xa5));
    }
}
