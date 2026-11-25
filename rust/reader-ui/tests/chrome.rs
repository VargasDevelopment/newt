use newt_core::frame::FRAME_BYTES;
use newt_reader_ui::*;
use newt_ui_canvas::{UiCanvas, text_width};

fn render_footer_frame(
    page: u32,
    pages: u32,
    orientation: Orientation,
    save_unconfirmed: bool,
) -> Frame {
    let settings = ViewSettings {
        orientation,
        ..ViewSettings::default()
    };
    let mut info = DeviceInfo::default().with_storage_notice(None, save_unconfirmed);
    info.battery_known = true;
    info.battery_percent = 100;
    let footer = ReaderFooter {
        spine: 0,
        page,
        pages,
        overall_known: true,
        overall_percent: 100,
        section_label: Some(b"A Long Chapter Heading"),
    };
    let mut frame: Frame = [0; FRAME_BYTES];
    render_reader_chrome(&footer, &info, settings, true, Some(&mut frame));
    frame
}

fn band_pixels(frame: &Frame, frame_w: i32, x0: i32, x1: i32, y0: i32, y1: i32) -> Vec<bool> {
    let mut out = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            let bit = (y as usize) * (frame_w as usize) + (x as usize);
            out.push(frame[bit / 8] & (0x80 >> (bit % 8)) != 0);
        }
    }
    out
}

#[test]
fn footer_page_counter_fits_4_digit_and_larger_counts_without_clipping() {
    for orientation in Orientation::ALL {
        let settings = ViewSettings {
            orientation,
            ..ViewSettings::default()
        };
        let g = newt_core::preferences::geometry(settings);
        let (w, m) = (i32::from(g.width), i32::from(g.margin));
        let battery_left = m + 24;
        let battery_right = battery_left + text_width(Some(b"100%"), 16, false);
        let percent_left = w - m - text_width(Some(b"100%"), 16, false);
        for (page, pages) in [
            (0, 1000),
            (999, 1000),
            (4096, 4250),
            (4249, 4250),
            (9998, 9999),
            (12344, 67890),
            (999_998, 999_999),
            (u32::MAX - 1, u32::MAX),
        ] {
            let text = format!("{}/{}", u64::from(page) + 1, pages);
            let slots = footer_slots(w, m, b"100%", text.as_bytes(), pages, b"100%");
            let page_slot_w = slots.page_right - slots.page_left;
            assert!(
                text_width(Some(text.as_bytes()), 14, false) <= page_slot_w,
                "{orientation:?} {text}: slot {page_slot_w} too narrow for {}px",
                text_width(Some(text.as_bytes()), 14, false)
            );
            // Strict 12 px minimum clearance between adjacent footer fields.
            assert!(battery_right + 12 <= slots.chapter_left, "{orientation:?} {text}");
            assert!(slots.battery_max_w >= text_width(Some(b"100%"), 16, false));
            assert!(
                slots.chapter_right - slots.chapter_left
                    >= text_width(Some(b"PG SAVE ERR"), 14, false),
                "{orientation:?} {text}: chapter slot too narrow for save warning"
            );
            assert!(slots.chapter_right + 12 <= slots.page_left, "{orientation:?} {text}");
            assert!(slots.page_right + 12 <= percent_left, "{orientation:?} {text}");
        }
    }
}

#[test]
fn footer_keeps_existing_1_to_3_digit_geometry_and_steady_4_digit_alignment() {
    let settings = ViewSettings {
        orientation: Orientation::Portrait,
        ..ViewSettings::default()
    };
    let g = newt_core::preferences::geometry(settings);
    let (w, m) = (i32::from(g.width), i32::from(g.margin));
    let expected_overall_left = w - m - text_width(Some(b"UNSAVED"), 16, false);
    let expected_page_right = expected_overall_left - 20;
    for (text, pages) in [(&b"--/--"[..], 0u32)] {
        let slots = footer_slots(w, m, b"73%", text, pages, b"11%");
        assert_eq!(
            (slots.chapter_left, slots.chapter_right, slots.page_left, slots.page_right),
            (w / 2 - 70, w / 2 + 70, w / 2 + 70 + 12, expected_page_right),
            "default geometry drifted for --/--"
        );
    }
    // Every 1-to-3-digit chapter (1..=999) keeps the exact original default geometry.
    for pages in 1..=999u32 {
        let text = format!("{pages}/{pages}");
        let slots = footer_slots(w, m, b"73%", text.as_bytes(), pages, b"11%");
        assert_eq!(
            (slots.chapter_left, slots.chapter_right, slots.page_left, slots.page_right),
            (w / 2 - 70, w / 2 + 70, w / 2 + 70 + 12, expected_page_right),
            "default geometry drifted for {text}"
        );
    }
    // Every percentage 0%..=100% fits within the 100% reserve so overall% cannot shift page_right.
    let hundred_w = text_width(Some(b"100%"), 16, false);
    for pct in 0..=100u8 {
        let text = format!("{pct}%");
        assert!(text_width(Some(text.as_bytes()), 16, false) <= hundred_w, "{text}");
    }
    // Every page in a 4-digit chapter shares the same steady page_right and chapter bounds.
    let s1 = footer_slots(w, m, b"73%", b"1/4250", 4250, b"1%");
    let s2 = footer_slots(w, m, b"73%", b"4097/4250", 4250, b"96%");
    let s3 = footer_slots(w, m, b"73%", b"4250/4250", 4250, b"100%");
    assert_eq!(s1, s2);
    assert_eq!(s2, s3);
}

#[test]
fn footer_2011_page_chapter_stays_steady_and_untruncated_across_adjacent_pages() {
    let settings = ViewSettings {
        orientation: Orientation::Portrait,
        ..ViewSettings::default()
    };
    let g = newt_core::preferences::geometry(settings);
    let (w, h, m, fy) = (
        i32::from(g.width),
        i32::from(g.height),
        i32::from(g.margin),
        i32::from(g.footer_y),
    );
    let default_slot =
        w - m - text_width(Some(b"UNSAVED"), 16, false) - 20 - (w / 2 + 70 + 12);
    // Negative control premise: in the pinned Inter atlas, '8' is narrower than
    // '0'/'4', so an '8'-based estimate fits the default slot while 2004/2011
    // overflows it (causing 2003 -> 2004 -> 2005 to jump 42 px and change size).
    assert!(
        text_width(Some(b"8888/2011"), 14, false) <= default_slot
            && text_width(Some(b"2004/2011"), 14, false) > default_slot,
        "test premise broken: Inter atlas digit widths changed"
    );
    // Every single page 1..=2011 must produce identical slots and fit untruncated.
    let expected_slots = footer_slots(w, m, b"100%", b"1/2011", 2011, b"100%");
    let slot_w = expected_slots.page_right - expected_slots.page_left;
    for page_num in 1..=2011u32 {
        let label = format!("{page_num}/2011");
        let slots = footer_slots(w, m, b"100%", label.as_bytes(), 2011, b"100%");
        assert_eq!(slots, expected_slots, "footer geometry jumped at {label}");
        assert!(
            text_width(Some(label.as_bytes()), 14, false) <= slot_w,
            "{label} overflows its slot ({slot_w}px)"
        );
    }
    // Check adjacent rendered pages 2003, 2004, and 2005 of 2011: identical font
    // size, identical slot geometry, exact untruncated pixels, and clean gutters.
    let mut chosen_size = None;
    let percent_left = w - m - text_width(Some(b"100%"), 16, false);
    for page_num in [2003u32, 2004, 2005] {
        let label = format!("{page_num}/2011");
        let mut p = 16u32;
        while p > 14 && text_width(Some(label.as_bytes()), p, false) > slot_w {
            p -= 1;
        }
        assert_eq!(
            *chosen_size.get_or_insert(p),
            p,
            "footer font size changed across adjacent pages at {label}"
        );
        let tw = text_width(Some(label.as_bytes()), p, false);
        assert!(tw <= slot_w, "{label} truncated at size {p}");
        let frame = render_footer_frame(page_num - 1, 2011, Orientation::Portrait, false);
        let mut expected: Frame = [0; FRAME_BYTES];
        let mut canvas = UiCanvas::new(Some(&mut expected), w, h);
        canvas.text(
            expected_slots.page_right - tw,
            fy + 13 - p as i32,
            Some(label.as_bytes()),
            p,
            false,
            slot_w,
            true,
        );
        let actual_band = band_pixels(
            &frame,
            w,
            expected_slots.page_left,
            expected_slots.page_right,
            fy - 4,
            h - m,
        );
        let expected_band = band_pixels(
            &expected,
            w,
            expected_slots.page_left,
            expected_slots.page_right,
            fy - 4,
            h - m,
        );
        assert_eq!(actual_band, expected_band, "{label} clipped or misaligned");
        let left_gutter = band_pixels(
            &frame,
            w,
            expected_slots.chapter_right,
            expected_slots.page_left,
            fy - 4,
            h - m,
        );
        assert!(!left_gutter.iter().any(|&px| px), "{label}: left gutter polluted");
        let right_gutter = band_pixels(
            &frame,
            w,
            expected_slots.page_right,
            percent_left,
            fy - 4,
            h - m,
        );
        assert!(!right_gutter.iter().any(|&px| px), "{label}: right gutter polluted");
    }
}

#[test]
fn footer_renders_untruncated_4097_of_4250_pixels_in_portrait() {
    let settings = ViewSettings {
        orientation: Orientation::Portrait,
        ..ViewSettings::default()
    };
    let g = newt_core::preferences::geometry(settings);
    let (w, h, m, fy) = (
        i32::from(g.width),
        i32::from(g.height),
        i32::from(g.margin),
        i32::from(g.footer_y),
    );
    let slots = footer_slots(w, m, b"100%", b"4097/4250", 4250, b"100%");
    let frame = render_footer_frame(4096, 4250, Orientation::Portrait, false);
    // Draw the expected untruncated string directly into an empty frame at the same slot.
    let mut expected: Frame = [0; FRAME_BYTES];
    let mut canvas = UiCanvas::new(Some(&mut expected), w, h);
    let mut p = 16u32;
    let slot_w = slots.page_right - slots.page_left;
    while p > 14 && text_width(Some(b"4097/4250"), p, false) > slot_w {
        p -= 1;
    }
    let tw = text_width(Some(b"4097/4250"), p, false);
    assert!(tw <= slot_w, "4097/4250 must fit without ellipsis");
    canvas.text(
        slots.page_right - tw,
        fy + 13 - p as i32,
        Some(b"4097/4250"),
        p,
        false,
        slot_w,
        true,
    );
    let actual_band = band_pixels(&frame, w, slots.page_left, slots.page_right, fy - 4, h - m);
    let expected_band = band_pixels(&expected, w, slots.page_left, slots.page_right, fy - 4, h - m);
    assert!(actual_band.iter().any(|&px| px), "page counter band must be non-empty");
    assert_eq!(actual_band, expected_band, "4097/4250 was clipped or overlapped");
    // Also verify the 12 px gutters on both sides of the page counter are completely blank.
    let left_gutter = band_pixels(&frame, w, slots.chapter_right, slots.page_left, fy - 4, h - m);
    assert!(!left_gutter.iter().any(|&px| px), "chapter leaked into page gutter");
    let percent_left = w - m - text_width(Some(b"100%"), 16, false);
    let right_gutter = band_pixels(&frame, w, slots.page_right, percent_left, fy - 4, h - m);
    assert!(!right_gutter.iter().any(|&px| px), "page leaked into percent gutter");
    // And with an active PG SAVE ERR notice + u32::MAX pages, neither warning nor counter clips.
    let warn_frame = render_footer_frame(u32::MAX - 1, u32::MAX, Orientation::Portrait, true);
    let warn_slots = footer_slots(
        w,
        m,
        b"100%",
        b"4294967295/4294967295",
        u32::MAX,
        b"100%",
    );
    let warn_gutter = band_pixels(
        &warn_frame,
        w,
        warn_slots.chapter_right,
        warn_slots.page_left,
        fy - 4,
        h - m,
    );
    assert!(!warn_gutter.iter().any(|&px| px), "warning leaked into u32::MAX page gutter");
}
