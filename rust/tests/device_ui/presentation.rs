//! Source-only expectations. No compiler/test execution accompanied this packet.
use newt_core::{
    frame::FRAME_BYTES,
    preferences::{Orientation, ViewSettings, geometry},
};
use newt_device_ui::stats::*;
use newt_device_ui::*;
fn portrait() -> Geometry {
    geometry(ViewSettings {
        orientation: Orientation::Portrait,
        ..Default::default()
    })
}
fn bytes(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|v| *v == 0).unwrap_or(b.len())]
}
#[test]
fn build_identity_vectors_preserve_tail() {
    for (id, date, expected) in [
        (
            Some(b"c83f64b673ec".as_slice()),
            Some(b"2026-10-27".as_slice()),
            Some("Build c83f64b673ec · Oct 27, 2026"),
        ),
        (
            Some(b"c83f64b673ec-dirty".as_slice()),
            Some(b"2026-10-27".as_slice()),
            Some("Build c83f64b673ec-dirty"),
        ),
        // The reference checks day 1..31, not calendar validity.
        (
            Some(b"012345abcdef".as_slice()),
            Some(b"0000-02-31".as_slice()),
            Some("Build 012345abcdef · Feb 31, 0"),
        ),
        (None, Some(b"2026-10-27".as_slice()), None),
        (
            Some(b"C83f64b673ec".as_slice()),
            Some(b"2026-10-27".as_slice()),
            None,
        ),
        (
            Some(b"c83f64b673ec-dirty".as_slice()),
            Some(b"2026-13-27".as_slice()),
            None,
        ),
        (Some(b"c83f64b673ec".as_slice()), None, None),
    ] {
        let mut out = [0xa5; 48];
        assert_eq!(
            format_build_line(BuildIdentity { id, date, ..Default::default() }, &mut out),
            expected.is_some()
        );
        if let Some(e) = expected {
            assert_eq!(bytes(&out), e.as_bytes());
            assert!(out[e.len() + 1..].iter().all(|v| *v == 0xa5));
        } else {
            assert_eq!(out[0], 0);
            assert!(out[1..].iter().all(|v| *v == 0xa5));
        }
    }
}
#[test]
fn formatting_is_bounded_and_null_capable() {
    let mut out = [0xa5; 32];
    format_stats_duration(4359, &mut out);
    assert_eq!(bytes(&out), b"1 h 12 min");
    assert_eq!(out[11], 0xa5);
    format_stats_duration(59, &mut out);
    assert_eq!(bytes(&out), b"0 min");
    let mut tiny = [0xa5; 4];
    format_stats_duration(3600, &mut tiny);
    assert_eq!(&tiny, b"1 h\0");
    format_stats_duration(0, &mut []);
    for (d, s) in [
        (0, b"Not recorded".as_slice()),
        (0xfffe, b"No date"),
        (0xffff, b"Unknown"),
    ] {
        format_stats_day(d, &mut out);
        assert_eq!(bytes(&out), s);
    }
}
#[test]
fn off_extras_leave_every_byte_untouched_and_invalid_geometry_is_rejected() {
    let mut frame = [0xa5; FRAME_BYTES];
    let home = HomeData::default();
    let e = SleepExtras::default();
    render_sleep_extras(&e, &home, Some(&mut frame), portrait(), false);
    assert!(frame.iter().all(|v| *v == 0xa5));
    assert!(!sleep_inverted(&e));
    render_sleep_extras(&e, &home, None, portrait(), true);
    let mut bad = portrait();
    bad.width = 0;
    render_sleep_ui(&DeviceInfo::default(), Some(&mut frame), bad, false);
    assert!(frame.iter().all(|v| *v == 0xa5));
}
#[test]
fn dots_have_independent_pixel_expectations_and_selector_complements() {
    let e = SleepExtras {
        week: true,
        days: 1,
        ..Default::default()
    };
    let mut dark = [0; FRAME_BYTES];
    let mut light = [255; FRAME_BYTES];
    render_sleep_extras(&e, &HomeData::default(), Some(&mut dark), portrait(), false);
    render_sleep_extras(&e, &HomeData::default(), Some(&mut light), portrait(), true);
    for (a, b) in dark.iter().zip(light) {
        assert_eq!(*a, !b);
    }
    // Portrait: top=(792-416)/2+420=608. No title: dot cy=608+11+8=627.
    let pixel = |x: usize, y: usize| dark[y * 66 + x / 8] & (0x80 >> (x % 8)) != 0;
    assert!(!pixel(186, 627)); // six days ago: hollow centre
    assert!(pixel(342, 627)); // today: filled centre
    assert!(pixel(342, 641)); // today's underline
    assert_eq!(&dark[..16 * 66], &[0; 16 * 66]);
}
#[test]
fn statistics_source_runs_once_only_when_its_page_is_selected_even_without_frame() {
    let mut calls = 0;
    let mut source = |shown: StatsGraphPosition, days: u32| {
        calls += 1;
        assert_eq!(days, 28);
        assert_eq!(shown.view, StatsGraphView::Days);
        assert_eq!(shown.back, 63);
        StatsScreenView::default()
    };
    let mut state = AppState {
        screen: Screen::Device,
        settings_row: SettingsRow::ReadingStats,
        row_open: true,
        list_focus: 253,
        ..Default::default()
    };
    render_device_ui(
        &state,
        &DeviceInfo::default(),
        &SystemBar::default(),
        None,
        newt_core::time_zone::TimeZone::Utc,
        None,
        Some(&mut source),
        FIRMWARE_BUILD,
        SleepScreenChoices::default(),
        Default::default(),
    );
    state.row_open = false;
    render_device_ui(
        &state,
        &DeviceInfo::default(),
        &SystemBar::default(),
        None,
        newt_core::time_zone::TimeZone::Utc,
        None,
        Some(&mut source),
        FIRMWARE_BUILD,
        SleepScreenChoices::default(),
        Default::default(),
    );
    assert_eq!(calls, 1);
}
#[test]
fn graph_selection_and_axis_vectors() {
    assert_eq!(stats_day_axis(0, 6), (1, 5));
    assert_eq!(stats_day_axis(3600, 6), (10, 6));
    assert_eq!(stats_graph_days(-1), 7);
    assert_eq!(stats_graph_days(428), 28);
    assert_eq!(stats_graph_days(10000), 42);
    let mut g = newt_core::stats_graph_data::StatsDaysGraph::default();
    g.last_day = 100;
    g.count = 7;
    assert_eq!(stats_graph_slot(&g, 94), 0);
    assert_eq!(stats_graph_slot(&g, 100), 6);
    assert_eq!(stats_graph_slot(&g, 93), -1);
    assert_eq!(stats_graph_slot(&g, 101), -1);
    assert_eq!(stats_graph_position(255).view, StatsGraphView::Summary);
}
#[test]
fn the_newt_dresses_for_each_holiday_on_its_dates_only() {
    use newt_core::{stats_clock::stats_clock, time_zone::ClockTime};
    let on = |year, month, day| {
        HolidayHat::on(stats_clock(ClockTime {
            year,
            month,
            day,
            hour: 9,
            ..ClockTime::default()
        }))
    };
    assert_eq!(on(2026, 10, 31), HolidayHat::Witch);
    assert_eq!(on(2026, 11, 26), HolidayHat::Thanksgiving);
    assert_eq!(on(2030, 11, 28), HolidayHat::Thanksgiving);
    assert_eq!(on(2026, 12, 25), HolidayHat::Christmas);
    for (year, month, day) in [(2026, 11, 25), (2026, 11, 27), (2026, 11, 19), (2030, 11, 21)] {
        assert_eq!(on(year, month, day), HolidayHat::None, "{year}-{month}-{day}");
    }
    // The outfit draws in both planes: the gray planes get its exact complement.
    let draw = |hat, selector| {
        let mut frame = [if selector { 255 } else { 0 }; FRAME_BYTES];
        let mut info = DeviceInfo::default();
        info.hat = hat;
        render_sleep_ui(&info, Some(&mut frame), portrait(), selector);
        frame
    };
    let dressed = draw(HolidayHat::Thanksgiving, false);
    assert_ne!(dressed, draw(HolidayHat::None, false));
    for (a, b) in dressed.iter().zip(draw(HolidayHat::Thanksgiving, true)) {
        assert_eq!(*a, !b);
    }
}
