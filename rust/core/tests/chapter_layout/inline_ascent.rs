//! Focused tests for #148: lines with oversized or vertically shifted inline runs
//! reserve vertical extent above and below the baseline so glyphs never overprint
//! adjacent lines or clip at the page top or bottom margins.
use super::*;

struct ScaledFont;
impl Font for ScaledFont {
    fn covers(&mut self, _: u32) -> bool {
        true
    }
    fn advance(&mut self, _: u32, s: u16, _: u8) -> i16 {
        (s / 2) as i16
    }
    fn line_height(&mut self, s: u16) -> i16 {
        (i32::from(s) * 6 / 5) as i16
    }
    fn ascent(&mut self, s: u16) -> i16 {
        s as i16
    }
}

struct SpaceBreak;
impl LineBreak for SpaceBreak {
    fn breaks(&mut self, t: &[u8], _: Option<&core::ffi::CStr>, o: &mut [u8]) {
        assert_eq!(t.len(), o.len());
        o.fill(2);
        for (i, &b) in t.iter().enumerate() {
            if b == b'\n' {
                o[i] = 0;
            } else if b == b' ' || (b == 0xad && i > 0 && t[i - 1] == 0xc2) {
                o[i] = 1;
            }
        }
        if let Some(last) = o.last_mut() {
            *last = 0;
        }
    }
}

fn run_scaled(
    params: Params<'_>,
    font: &mut dyn Font,
    body: impl FnOnce(&mut Engine<'_>),
) -> Capture {
    let mut sink = Capture::default();
    let mut backend = SplitBackend {
        font,
        sink: &mut sink,
    };
    let mut breaker = SpaceBreak;
    let mut images = FixtureImages;
    let mut text = [0; 512];
    let mut breaks = [0; 512];
    let mut levels = [0; 512];
    let mut spans = [Span::default(); 160];
    let mut lines = [Line::default(); 64];
    let mut elements = [Element::default(); 16];
    let mut runs = [Run::default(); 256];
    let mut imgs = [Image::default(); 2];
    let mut links = [Link::default(); 4];
    let mut page = [0; 4096];
    let mut style = [0; 128];
    let mut pl = [ParLink::default(); 4];
    let storage = Storage {
        text: &mut text,
        breaks: &mut breaks,
        levels: &mut levels,
        spans: &mut spans,
        lines: &mut lines,
        elements: &mut elements,
        runs: &mut runs,
        images: &mut imgs,
        links: &mut links,
        page: &mut page,
        style_text: &mut style,
        paragraph_links: &mut pl,
    };
    let mut engine = Engine::new(
        params,
        storage,
        &mut backend,
        &mut breaker,
        &mut images,
        None,
        CssStylesheetBuilder::default(),
    )
    .unwrap();
    engine.start(b"body", None, None, None, None, None);
    body(&mut engine);
    engine.end(b"body");
    assert!(engine.finish());
    sink
}

fn page_baselines(capture: &Capture) -> Vec<Vec<(i16, u16)>> {
    capture
        .pages
        .iter()
        .map(|(_, _, runs)| runs.iter().map(|&(_, _, baseline, size, _)| (baseline, size)).collect())
        .collect()
}

fn page_texts(capture: &Capture) -> Vec<Vec<String>> {
    capture
        .pages
        .iter()
        .map(|(_, _, runs)| {
            runs.iter()
                .map(|(text, _, _, _, _)| String::from_utf8(text.clone()).unwrap())
                .collect()
        })
        .collect()
}

#[test]
fn inline_ascent_reserves_height_at_page_top_mid_page_and_page_bottom() {
    let make = |inline_ascent, height| Params {
        width: 300,
        height,
        left: 16,
        right: 16,
        top: 16,
        bottom: 48,
        base_size: 30,
        inline_ascent,
        ..Params::default()
    };

    // 1. Page top: 250% span (75 px) reserves ascent 75 above baseline (16 + 75 = 91)
    //    and descent ceil(75/4)=19 below baseline (row height 75 + 19 = 94), so line 2
    //    starts at y = 16 + 94 = 110 -> baseline = 140.
    let fixed_top = page_baselines(&run_scaled(make(true, 400), &mut ScaledFont, |e| {
        e.start(b"p", None, None, None, None, None);
        e.start(b"span", None, Some(b"font-size: 3em"), None, None, None);
        e.text(b"A");
        e.end(b"span");
        e.text(b"t the top of the page this sentence wraps cleanly onto a second line.");
        e.end(b"p");
    }));
    assert_eq!(fixed_top.len(), 1);
    assert_eq!(fixed_top[0][0], (91, 75));
    assert_eq!(fixed_top[0][1], (91, 30));
    assert_eq!(fixed_top[0][2], (140, 30));

    // FreeInk parity default (inline_ascent = false) keeps baseline at top + ascent(30) = 46.
    let legacy_top = page_baselines(&run_scaled(make(false, 400), &mut ScaledFont, |e| {
        e.start(b"p", None, None, None, None, None);
        e.start(b"span", None, Some(b"font-size: 3em"), None, None, None);
        e.text(b"A");
        e.end(b"span");
        e.text(b"t top");
        e.end(b"p");
    }));
    assert_eq!(legacy_top[0][0], (46, 75));

    // 2. Mid-page: second paragraph's 75 px drop cap starts cleanly below paragraph 1.
    //    Para 1: y=16 -> baseline=46, y advances by lh(36) + after(12) = 64.
    //    Para 2: baseline = 64 + 75 = 139, so top (139 - 75 = 64) never overlaps para 1 (<= 52).
    let mid = page_baselines(&run_scaled(make(true, 400), &mut ScaledFont, |e| {
        e.start(b"p", None, None, None, None, None);
        e.text(b"Previous line.");
        e.end(b"p");
        e.start(b"p", None, None, None, None, None);
        e.start(b"span", None, Some(b"font-size: 3em"), None, None, None);
        e.text(b"W");
        e.end(b"span");
        e.text(b"hen.");
        e.end(b"p");
    }));
    assert_eq!(mid.len(), 1);
    assert_eq!(mid[0][0], (46, 30));
    assert_eq!(mid[0][1], (139, 75));
    assert_eq!(mid[0][2], (139, 30));
    assert!(mid[0][1].0 - mid[0][1].1 as i16 >= mid[0][0].0);

    // 3. Page bottom: content bottom = 148 - 48 = 100. Para 1 ends at y=64 (36 px left).
    //    Even with height = 200 (content bottom = 152, 88 px left > 81 px ascent-only height
    //    but < 94 px full ascent+descent height for a 75 px descender 'g'), para 2 moves to page 2.
    let bottom = page_baselines(&run_scaled(make(true, 200), &mut ScaledFont, |e| {
        e.start(b"p", None, None, None, None, None);
        e.text(b"First.");
        e.end(b"p");
        e.start(b"p", None, None, None, None, None);
        e.start(b"span", None, Some(b"font-size: 3em"), None, None, None);
        e.text(b"g");
        e.end(b"span");
        e.text(b"ottom.");
        e.end(b"p");
    }));
    assert_eq!(bottom.len(), 2);
    assert_eq!(bottom[0], vec![(46, 30)]);
    assert_eq!(bottom[1], vec![(91, 75), (91, 30)]);
}

#[test]
fn inline_ascent_reserves_raised_superscript_and_lowered_subscript_extents() {
    let make = |height| Params {
        width: 300,
        height,
        left: 16,
        right: 16,
        top: 16,
        bottom: 48,
        base_size: 30,
        inline_ascent: true,
        ..Params::default()
    };

    // 1. Oversized 250% (75 px) superscript at page top: SUP raises rb by 75*33/100 = 24 px,
    //    so line baseline shifts to 16 + 75 + 24 = 115 and emitted rb = 115 - 24 = 91 (>= 16 + 75).
    let sup_top = page_baselines(&run_scaled(make(400), &mut ScaledFont, |e| {
        e.start(b"p", None, None, None, None, None);
        e.start(b"span", None, Some(b"font-size: 250%; vertical-align: super"), None, None, None);
        e.text("É".as_bytes());
        e.end(b"span");
        e.text(b"cho");
        e.end(b"p");
    }));
    assert_eq!(sup_top.len(), 1);
    assert_eq!(sup_top[0][0], (91, 75));
    assert_eq!(sup_top[0][1], (115, 30));

    // 2. Smaller raised SUP (120% = 36 px -> 36 + 11 = 47 px above baseline) exceeds a larger
    //    unshifted span (140% = 42 px -> 42 px above baseline) on the same line.
    let sup_beats_bigger = page_baselines(&run_scaled(make(400), &mut ScaledFont, |e| {
        e.start(b"p", None, None, None, None, None);
        e.start(b"span", None, Some(b"font-size: 140%"), None, None, None);
        e.text(b"B");
        e.end(b"span");
        e.start(b"span", None, Some(b"font-size: 120%; vertical-align: super"), None, None, None);
        e.text(b"S");
        e.end(b"span");
        e.text(b"x");
        e.end(b"p");
    }));
    assert_eq!(sup_beats_bigger.len(), 1);
    assert_eq!(sup_beats_bigger[0][0], (63, 42));
    assert_eq!(sup_beats_bigger[0][1], (52, 36));
    assert_eq!(sup_beats_bigger[0][2], (63, 30));

    // 3. Oversized 250% (75 px) subscript lowers rb by 75*12/100 = 9 px and needs 19+9 = 28 px
    //    below baseline (above = 75-9 = 66, total = 94). With height = 200 (content bottom 152),
    //    para 1 ends at y=64 leaving 88 px < 94 px -> moves to page 2 without clipping at bottom.
    //    With height = 206 (content bottom 158, 94 px left), it fits with rb + 19 = 158 <= 158.
    let sub_moves = page_baselines(&run_scaled(make(200), &mut ScaledFont, |e| {
        e.start(b"p", None, None, None, None, None);
        e.text(b"Lead.");
        e.end(b"p");
        e.start(b"p", None, None, None, None, None);
        e.start(b"span", None, Some(b"font-size: 250%; vertical-align: sub"), None, None, None);
        e.text(b"g");
        e.end(b"span");
        e.text(b"ap");
        e.end(b"p");
    }));
    assert_eq!(sub_moves.len(), 2);
    assert_eq!(sub_moves[1][0], (91, 75)); // baseline 16+66=82, rb = 82+9 = 91
    assert_eq!(sub_moves[1][1], (82, 30));

    let sub_fits = page_baselines(&run_scaled(make(206), &mut ScaledFont, |e| {
        e.start(b"p", None, None, None, None, None);
        e.text(b"Lead.");
        e.end(b"p");
        e.start(b"p", None, None, None, None, None);
        e.start(b"span", None, Some(b"font-size: 250%; vertical-align: sub"), None, None, None);
        e.text(b"g");
        e.end(b"span");
        e.text(b"ap");
        e.end(b"p");
    }));
    assert_eq!(sub_fits.len(), 1);
    assert_eq!(sub_fits[0][1], (139, 75)); // y=64, baseline=130, rb=139, rb+19=158 == content_bottom

    // 4. Default HTML <sup> and <sub> (50% = 15 px in 30 px body) stay within normal row bounds
    //    and add 0 extra height above or below the baseline.
    let normal_scripts = page_baselines(&run_scaled(make(400), &mut ScaledFont, |e| {
        e.start(b"p", None, None, None, None, None);
        e.text(b"H");
        e.start(b"sub", None, None, None, None, None);
        e.text(b"2");
        e.end(b"sub");
        e.text(b"O");
        e.start(b"sup", None, None, None, None, None);
        e.text(b"1");
        e.end(b"sup");
        e.text(b" and wrap.");
        e.end(b"p");
    }));
    assert_eq!(normal_scripts.len(), 1);
    assert_eq!(normal_scripts[0][0], (46, 30));
    assert_eq!(normal_scripts[0][1], (47, 15));
    assert_eq!(normal_scripts[0][3], (42, 15));
    // With width 300 (content 268 px), wrap falls onto line 2 at 46 + 36 = 82 (0 extra above/below).
    assert!(normal_scripts[0].len() >= 5);
    assert_eq!(normal_scripts[0].last().unwrap().0, 46);
    let wrapped_scripts = page_baselines(&run_scaled(make(400), &mut ScaledFont, |e| {
        e.start(b"p", None, None, None, None, None);
        e.text(b"H");
        e.start(b"sub", None, None, None, None, None);
        e.text(b"2");
        e.end(b"sub");
        e.text(b"O");
        e.start(b"sup", None, None, None, None, None);
        e.text(b"1");
        e.end(b"sup");
        e.text(b" next line wraps here.");
        e.end(b"p");
    }));
    assert_eq!(wrapped_scripts.len(), 1);
    assert_eq!(wrapped_scripts[0].last().unwrap().0, 82); // 46 + 36: zero extra_above or extra_below
}

#[test]
fn inline_ascent_ignores_invisible_spans_but_counts_visible_break_hyphen() {
    let params = Params {
        width: 300,
        height: 400,
        left: 16,
        right: 16,
        top: 16,
        bottom: 48,
        base_size: 30,
        inline_ascent: true,
        ..Params::default()
    };

    // Unbroken soft hyphen (U+00AD), ASCII space, NBSP (U+00A0), and variation selector (U+FE0F)
    // inside oversized spans draw no ink and add zero extra height.
    let invisible = page_baselines(&run_scaled(params, &mut ScaledFont, |e| {
        e.start(b"p", None, None, None, None, None);
        e.text(b"Lead");
        e.start(b"span", None, Some(b"font-size: 250%"), None, None, None);
        e.text(" \u{00ad}\u{00a0}\u{fe0f}".as_bytes());
        e.end(b"span");
        e.text(b"tail");
        e.end(b"p");
    }));
    assert_eq!(invisible.len(), 1);
    assert!(invisible[0].iter().all(|&(b, _)| b == 46));

    // When a line actually breaks at a soft hyphen inside an oversized span, emission appends
    // a visible '-' at that span's size, so the line reserves extra height for the hyphen.
    let narrow = Params { width: 120, ..params }; // content width = 88 px
    let broken_capture = run_scaled(narrow, &mut ScaledFont, |e| {
        e.start(b"p", None, None, None, None, None);
        e.text(b"abcd"); // 4 * 15 = 60 px
        e.start(b"span", None, Some(b"font-size: 250%"), None, None, None);
        e.text("\u{00ad}".as_bytes()); // breaks here and emits '-' at 75 px
        e.end(b"span");
        e.text(b"efgh");
        e.end(b"p");
    });
    let texts = page_texts(&broken_capture);
    let baselines = page_baselines(&broken_capture);
    assert_eq!(texts[0][1], "-");
    assert_eq!(baselines[0][1], (91, 75));
    assert_eq!(baselines[0][0], (91, 30));
}

#[test]
fn inline_ascent_places_tall_line_on_short_empty_page_without_dropping_text() {
    // Available height = 60 (>= lh 36, < tall line_lh 94).
    let params = Params {
        width: 300,
        height: 60,
        left: 0,
        right: 0,
        top: 0,
        bottom: 0,
        base_size: 30,
        inline_ascent: true,
        ..Params::default()
    };
    let capture = run_scaled(params, &mut ScaledFont, |e| {
        e.start(b"p", None, None, None, None, None);
        e.start(b"span", None, Some(b"font-size: 3em"), None, None, None);
        e.text(b"A");
        e.end(b"span");
        e.text(b"fter");
        e.end(b"p");
    });
    assert_eq!(capture.pages.len(), 1);
    assert_eq!(capture.pages[0].2.len(), 2);
    assert_eq!(capture.pages[0].2[0].2, 75);
}

#[test]
fn inline_ascent_handles_segment_continuation_without_overflowing_bottom() {
    struct NarrowFont;
    impl Font for NarrowFont {
        fn covers(&mut self, _: u32) -> bool {
            true
        }
        fn advance(&mut self, _: u32, _: u16, _: u8) -> i16 {
            2
        }
        fn line_height(&mut self, s: u16) -> i16 {
            s as i16
        }
        fn ascent(&mut self, s: u16) -> i16 {
            s as i16
        }
    }
    // Single measured line (67 chars * 2 px = 134 px <= width 400) with 67 styled segments
    // splits at MAX_SEGMENTS = 64 into a 64-segment tall prefix (height 25+7 = 32 px) and a
    // 3-segment 10 px continuation ending with "Z". Height 38 fits two ordinary 10 px rows
    // (so slots = 38/10 = 3), not the tall prefix plus continuation (32 + 10 = 42 > 38),
    // proving the post-split bottom-fit guard moves the continuation cleanly to page 2.
    let params = Params {
        width: 400,
        height: 38,
        left: 0,
        right: 0,
        top: 0,
        bottom: 0,
        base_size: 10,
        inline_ascent: true,
        ..Params::default()
    };
    let capture = run_scaled(params, &mut NarrowFont, |e| {
        e.start(b"p", None, None, None, None, None);
        e.start(b"span", None, Some(b"font-size: 250%"), None, None, None);
        e.text(b"X");
        e.end(b"span");
        for i in 0..65 {
            let tag: &[u8] = if i % 2 == 0 { b"b" } else { b"i" };
            e.start(tag, None, None, None, None, None);
            e.text(if i % 2 == 0 { b"a" } else { b"b" });
            e.end(tag);
        }
        e.start(b"i", None, None, None, None, None);
        e.text(b"Z");
        e.end(b"i");
        e.end(b"p");
    });
    let texts = page_texts(&capture);
    let baselines = page_baselines(&capture);
    assert_eq!(capture.pages.len(), 2, "continuation must break onto page 2");
    assert_eq!(texts[0].len(), 64);
    assert_eq!(texts[0][0], "X");
    assert!(baselines[0].iter().all(|&(b, _)| b == 25));
    assert_eq!(texts[1], vec!["b", "a", "Z"]);
    assert!(baselines[1].iter().all(|&(b, _)| b == 10));
}

#[test]
fn inline_ascent_refuses_blank_line_overflow_after_tall_continuation() {
    struct NarrowFont;
    impl Font for NarrowFont {
        fn covers(&mut self, _: u32) -> bool {
            true
        }
        fn advance(&mut self, _: u32, _: u16, _: u8) -> i16 {
            2
        }
        fn line_height(&mut self, s: u16) -> i16 {
            s as i16
        }
        fn ascent(&mut self, s: u16) -> i16 {
            s as i16
        }
    }
    // Paragraph 1 has 2 measured lines: a 65-segment tall line (32 px prefix + 10 px continuation
    // = 42 px) followed by a blank <br> line (10 px). Height 50 fits 32 + 10 = 42 <= 50 so flush()
    // sets avail = 2 == remain and expands slots to 50/10 = 5. After the split leaves y = 42,
    // place_line() must refuse the blank row onto page 2 (42 + 10 > 50) instead of advancing y past 50.
    let params = Params {
        width: 400,
        height: 50,
        left: 0,
        right: 0,
        top: 0,
        bottom: 0,
        base_size: 10,
        orphan: 1,
        widow: 1,
        inline_ascent: true,
        ..Params::default()
    };
    let capture = run_scaled(params, &mut NarrowFont, |e| {
        e.start(b"p", None, Some(b"margin-bottom: 0"), None, None, None);
        e.start(b"span", None, Some(b"font-size: 250%"), None, None, None);
        e.text(b"X");
        e.end(b"span");
        for i in 0..64 {
            let tag: &[u8] = if i % 2 == 0 { b"b" } else { b"i" };
            e.start(tag, None, None, None, None, None);
            e.text(if i % 2 == 0 { b"a" } else { b"b" });
            e.end(tag);
        }
        e.start(b"br", None, None, None, None, None);
        e.end(b"br");
        e.start(b"br", None, None, None, None, None);
        e.end(b"br");
        e.end(b"p");
        e.start(b"p", None, Some(b"margin-bottom: 0"), None, None, None);
        e.text(b"End");
        e.end(b"p");
    });
    let texts = page_texts(&capture);
    let baselines = page_baselines(&capture);
    assert_eq!(capture.pages.len(), 2);
    assert_eq!(texts[0].len(), 65);
    assert_eq!(baselines[0][0], (25, 25));
    assert_eq!(baselines[0][64], (42, 10));
    assert_eq!(texts[1], vec!["End"]);
    assert_eq!(baselines[1], vec![(20, 10)]); // refused blank row lands at y=0..10 on page 2, "End" at 20
}

#[test]
fn inline_ascent_uses_ink_extents_for_tall_capitals_deep_descenders_and_ignorables() {
    struct AtlasLikeFont;
    impl Font for AtlasLikeFont {
        fn covers(&mut self, _: u32) -> bool {
            true
        }
        fn advance(&mut self, _: u32, s: u16, _: u8) -> i16 {
            (s / 2) as i16
        }
        fn line_height(&mut self, s: u16) -> i16 {
            (i32::from(s) * 6 / 5) as i16
        }
        fn ascent(&mut self, s: u16) -> i16 {
            s as i16
        }
        fn ink_extents(&mut self, cp: u32, s: u16, _: u8) -> Option<(i16, i16)> {
            match cp {
                0x01d9 => Some(((i32::from(s) * 32 + 15) / 30, 0)).map(|(a, b)| (a as i16, b)), // Ǚ: 80 at 75 px
                0x210a => Some((((i32::from(s) * 11 + 15) / 30) as i16, ((i32::from(s) * 12 + 15) / 30) as i16)), // ℊ: 30 below at 75 px
                0xfe0f | 0x20 => Some((0, 0)),
                _ => Some((((i32::from(s) * 22 + 15) / 30) as i16, 0)), // uppercase-like, 0 descender
            }
        }
    }
    let make = |height| Params {
        width: 300,
        height,
        left: 16,
        right: 16,
        top: 16,
        bottom: 48,
        base_size: 30,
        inline_ascent: true,
        ..Params::default()
    };

    // 1. 75 px superscript Ǚ (ink_above = 80 > ascent 75, SUP shift 24) places baseline at
    //    16 + 80 + 24 = 120 so emitted rb = 96 and rb - 80 = 16 == top margin (0 rows clipped).
    let tall_sup = page_baselines(&run_scaled(make(400), &mut AtlasLikeFont, |e| {
        e.start(b"p", None, None, None, None, None);
        e.start(b"span", None, Some(b"font-size: 250%; vertical-align: super"), None, None, None);
        e.text("\u{01d9}".as_bytes());
        e.end(b"span");
        e.text(b"x");
        e.end(b"p");
    }));
    assert_eq!(tall_sup, vec![vec![(96, 75), (120, 30)]]);

    // 2. 75 px ℊ (ink_below = 30 -> row height 75 + 30 = 105). Para 1 ends at y=64;
    //    height=210 gives content_bottom=162 (98 px left: > 94 px old 1/4-em guess, < 105 px true ink),
    //    so ℊ moves cleanly to page 2 instead of losing 11 px at the bottom boundary.
    //    At height=217 (content_bottom=169, 105 px left), it fits with baseline + 30 = 169 == content_bottom.
    let deep_moves = page_baselines(&run_scaled(make(210), &mut AtlasLikeFont, |e| {
        e.start(b"p", None, None, None, None, None);
        e.text(b"Lead.");
        e.end(b"p");
        e.start(b"p", None, None, None, None, None);
        e.start(b"span", None, Some(b"font-size: 250%"), None, None, None);
        e.text("\u{210a}".as_bytes());
        e.end(b"span");
        e.end(b"p");
    }));
    assert_eq!(deep_moves.len(), 2);
    assert_eq!(deep_moves[1], vec![(91, 75)]);

    let deep_fits = page_baselines(&run_scaled(make(217), &mut AtlasLikeFont, |e| {
        e.start(b"p", None, None, None, None, None);
        e.text(b"Lead.");
        e.end(b"p");
        e.start(b"p", None, None, None, None, None);
        e.start(b"span", None, Some(b"font-size: 250%"), None, None, None);
        e.text("\u{210a}".as_bytes());
        e.end(b"span");
        e.end(b"p");
    }));
    assert_eq!(deep_fits.len(), 1);
    assert_eq!(deep_fits[0][1], (139, 75)); // 139 + 30 == 169 == content_bottom

    // 3. Uppercase drop cap with ink_below = 0 keeps normal 36 px leading to line 2 (no excess gap).
    let upper = page_baselines(&run_scaled(make(400), &mut AtlasLikeFont, |e| {
        e.start(b"p", None, None, None, None, None);
        e.start(b"span", None, Some(b"font-size: 250%"), None, None, None);
        e.text(b"A");
        e.end(b"span");
        e.text(b"t the top of the page this sentence wraps cleanly.");
        e.end(b"p");
    }));
    assert_eq!(upper[0][0], (91, 75));
    assert_eq!(upper[0][2], (127, 30)); // 91 + 36 (no phantom descent gap under uppercase 'A')

    // 4. Oversized variation selector U+FE0F reports Some((0, 0)) and adds 0 extra height.
    let vs = page_baselines(&run_scaled(make(400), &mut AtlasLikeFont, |e| {
        e.start(b"p", None, None, None, None, None);
        e.text(b"A");
        e.start(b"span", None, Some(b"font-size: 250%"), None, None, None);
        e.text("\u{fe0f}".as_bytes());
        e.end(b"span");
        e.text(b"B");
        e.end(b"p");
    }));
    assert!(vs[0].iter().all(|&(b, _)| b == 46));

    // 5. Condensed line_spacing = 80 (lh = 28 < ascent = 30): ordinary lines keep 28 px leading,
    //    default 50% <sup> adds 0 extra, and 75 px ℊ still requires full 75 + 30 = 105 px
    //    (para 1 ends at y = 16 + 28 + 12 = 56; content_bottom = 160 leaves 104 px -> moves to page 2;
    //    content_bottom = 161 leaves 105 px -> fits exactly with baseline 131 + 30 == 161).
    let condensed = |height| Params {
        line_spacing: 80,
        ..make(height)
    };
    let ordinary_condensed = page_baselines(&run_scaled(condensed(400), &mut AtlasLikeFont, |e| {
        e.start(b"p", None, None, None, None, None);
        e.text(b"First");
        e.start(b"sup", None, None, None, None, None);
        e.text(b"1");
        e.end(b"sup");
        e.text(b" line wraps cleanly here.");
        e.end(b"p");
    }));
    assert_eq!(ordinary_condensed[0][0], (46, 30));
    assert_eq!(ordinary_condensed[0].last().unwrap().0, 74); // 46 + 28 condensed leading

    let condensed_short = page_baselines(&run_scaled(condensed(208), &mut AtlasLikeFont, |e| {
        e.start(b"p", None, None, None, None, None);
        e.text(b"Lead.");
        e.end(b"p");
        e.start(b"p", None, None, None, None, None);
        e.start(b"span", None, Some(b"font-size: 250%"), None, None, None);
        e.text("\u{210a}".as_bytes());
        e.end(b"span");
        e.end(b"p");
    }));
    assert_eq!(condensed_short.len(), 2); // 104 px left < 105 px needed
    assert_eq!(condensed_short[1], vec![(91, 75)]);

    let condensed_exact = page_baselines(&run_scaled(condensed(209), &mut AtlasLikeFont, |e| {
        e.start(b"p", None, None, None, None, None);
        e.text(b"Lead.");
        e.end(b"p");
        e.start(b"p", None, None, None, None, None);
        e.start(b"span", None, Some(b"font-size: 250%"), None, None, None);
        e.text("\u{210a}".as_bytes());
        e.end(b"span");
        e.end(b"p");
    }));
    assert_eq!(condensed_exact.len(), 1); // 105 px left == 105 px needed
    assert_eq!(condensed_exact[0][1], (131, 75)); // y=56, baseline=131, 131 + 30 == 161 == content_bottom
}
