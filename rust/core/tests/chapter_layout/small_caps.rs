use super::*;
use newt_core::chapter_layout::case;

struct StrictCaseFont {
    enable_ligature: bool,
    reject_cp: Option<u32>,
    kern_av: bool,
    kern_th: bool,
    kern_ah: bool,
    bold_hyphen_extra: bool,
    cross_size_kern_seen: bool,
}

impl Default for StrictCaseFont {
    fn default() -> Self {
        Self {
            enable_ligature: false,
            reject_cp: None,
            kern_av: false,
            kern_th: false,
            kern_ah: false,
            bold_hyphen_extra: false,
            cross_size_kern_seen: false,
        }
    }
}
impl Font for StrictCaseFont {
    fn covers(&mut self, cp: u32) -> bool {
        self.reject_cp != Some(cp)
    }
    fn advance(&mut self, cp: u32, size: u16, flags: u8) -> i16 {
        assert_eq!(
            flags & !STYLE_MASK,
            0,
            "layout-internal case bits leaked into Font::advance: {flags:#x}"
        );
        if cp == 32 {
            (size / 4).max(1) as i16
        } else if cp == 45 {
            let base = (size / 3).max(1) as i16;
            if self.bold_hyphen_extra && flags & BOLD != 0 {
                base + 1
            } else {
                base
            }
        } else {
            // Size-proportional width so 30 px and 24 px have distinct advances.
            (size / 2).max(1) as i16
        }
    }
    fn line_height(&mut self, size: u16) -> i16 {
        (i32::from(size) * 12 / 10) as i16
    }
    fn ascent(&mut self, size: u16) -> i16 {
        size as i16
    }
    fn ligature(&mut self, a: u32, b: u32, flags: u8) -> u32 {
        assert_eq!(
            flags & !STYLE_MASK,
            0,
            "layout-internal case bits leaked into Font::ligature: {flags:#x}"
        );
        if self.enable_ligature && a == u32::from(b'f') && b == u32::from(b'i') {
            0xfb01
        } else if self.enable_ligature && a == u32::from(b'f') && b == u32::from(b'l') {
            0xfb02
        } else {
            0
        }
    }
    fn kerning(&mut self, a: u32, b: u32, size: u16, flags: u8) -> i16 {
        assert_eq!(
            flags & !STYLE_MASK,
            0,
            "layout-internal case bits leaked into Font::kerning: {flags:#x}"
        );
        if self.kern_th && a == u32::from(b'T') && b == u32::from(b'H') {
            self.cross_size_kern_seen = true;
            return -3;
        }
        if self.kern_av && a == u32::from(b'A') && b == u32::from(b'V') {
            return if size >= 30 { -2 } else { -1 };
        }
        if self.kern_ah && a == u32::from(b'A') && b == u32::from(b'-') {
            return -2;
        }
        0
    }
}
fn painted_run_width(font: &mut StrictCaseFont, run: &(Vec<u8>, i16, i16, u16, u8)) -> i32 {
    let mut w = 0i32;
    let mut prev = 0u32;
    for ch in core::str::from_utf8(&run.0).unwrap().chars() {
        let cp = ch as u32;
        if prev != 0 {
            w += i32::from(font.kerning(prev, cp, run.3, run.4));
        }
        w += i32::from(font.advance(cp, run.3, run.4));
        prev = cp;
    }
    w
}

struct SpaceBreak;
impl LineBreak for SpaceBreak {
    fn breaks(&mut self, t: &[u8], _: Option<&core::ffi::CStr>, o: &mut [u8]) {
        assert_eq!(t.len(), o.len());
        o.fill(2);
        for (i, &b) in t.iter().enumerate() {
            if b == b' ' {
                o[i] = 1;
            } else if b == b'\n' {
                o[i] = 0;
            }
        }
        for (i, bytes) in t.windows(2).enumerate() {
            if bytes == [0xc2, 0xad] {
                o[i + 1] = 1;
            }
        }
        if let Some(last) = o.last_mut() {
            *last = 0;
        }
    }
}
#[derive(Default)]
struct AnchorCapture {
    inner: Capture,
    anchors: Vec<(u32, u32)>,
    links: Vec<(i16, u16)>,
}
impl Sink for AnchorCapture {
    fn anchor(&mut self, hash: u32, chars: u32) {
        self.anchors.push((hash, chars));
    }
    fn page(&mut self, p: Page<'_>) -> bool {
        for run in p.runs {
            assert_eq!(
                run.flags & !STYLE_MASK,
                0,
                "layout-internal case bits leaked into Run.flags: {:#x}",
                run.flags
            );
        }
        for link in p.links {
            self.links.push((link.x, link.width));
        }
        self.inner.page(p)
    }
}
fn run_case_layout(
    font: &mut StrictCaseFont,
    sink: &mut AnchorCapture,
    width: i16,
    height: i16,
    base_size: u16,
    css_text: &[u8],
    build: impl FnOnce(&mut Engine<'_>),
) {
    let mut text = vec![0; 2048];
    let mut breaks = vec![0; 2048];
    let mut levels = vec![0; 2048];
    let mut spans = vec![Span::default(); 256];
    let mut lines = vec![Line::default(); 128];
    let mut elements = vec![Element::default(); 32];
    let mut runs = vec![Run::default(); 304];
    let mut images = vec![Image::default(); 2];
    let mut links = vec![Link::default(); 16];
    let mut page = vec![0; 7 * 1024];
    let mut style = vec![0; 512];
    let mut pl = vec![ParLink::default(); 8];
    let mut sheet_rules = vec![CssRule::default(); 512];
    let mut sheet_builder = CssStylesheetBuilder::default();
    assert!(sheet_builder.begin(&mut sheet_rules));
    if !css_text.is_empty() {
        sheet_builder.add_text(css_text);
    }
    let sheet = sheet_builder.finish();
    let mut chapter_rules = vec![CssRule::default(); 512];
    let mut chapter_css = CssStylesheetBuilder::default();
    assert!(chapter_css.begin(&mut chapter_rules));

    let storage = Storage {
        text: &mut text,
        breaks: &mut breaks,
        levels: &mut levels,
        spans: &mut spans,
        lines: &mut lines,
        elements: &mut elements,
        runs: &mut runs,
        images: &mut images,
        links: &mut links,
        page: &mut page,
        style_text: &mut style,
        paragraph_links: &mut pl,
    };
    let mut backend = SplitBackend { font, sink };
    let mut breaker = SpaceBreak;
    let mut img = FixtureImages;
    let params = Params {
        width,
        height,
        left: 16,
        right: 16,
        top: 16,
        bottom: 16,
        base_size,
        stylesheet: if css_text.is_empty() { None } else { Some(sheet) },
        ..Params::default()
    };
    let mut engine = Engine::new(
        params,
        storage,
        &mut backend,
        &mut breaker,
        &mut img,
        None,
        chapter_css,
    )
    .unwrap();
    build(&mut engine);
    assert!(engine.finish(), "layout failed");
}

#[test]
fn case_tables_cover_scripts_and_never_grow_utf8() {
    assert_eq!(case::small_cap_px(24), 19);
    assert_eq!(case::small_cap_px(30), 24);
    assert_eq!(case::small_cap_px(36), 29);
    assert_eq!(case::small_cap_px(42), 34);
    assert_eq!(case::small_cap_px(10), 8);
    assert_eq!(case::small_cap_px(8), 8);

    // Pairs across Latin, Latin-1, Extended-A/B/Additional, Greek, Cyrillic.
    for (lo, up) in [
        ('a', 'A'),
        ('z', 'Z'),
        ('é', 'É'),
        ('ü', 'Ü'),
        ('ÿ', 'Ÿ'),
        ('ā', 'Ā'),
        ('ł', 'Ł'),
        ('š', 'Š'),
        ('ș', 'Ș'),
        ('ț', 'Ț'),
        ('ơ', 'Ơ'),
        ('ư', 'Ư'),
        ('ệ', 'Ệ'),
        ('α', 'Α'),
        ('ω', 'Ω'),
        ('ά', 'Ά'),
        ('ώ', 'Ώ'),
        ('а', 'А'),
        ('я', 'Я'),
        ('ё', 'Ё'),
        ('ї', 'Ї'),
    ] {
        assert_eq!(case::to_upper(lo as u32), up as u32, "upper {lo}");
        assert_eq!(case::to_lower(up as u32), lo as u32, "lower {up}");
        assert_eq!(case::to_upper(up as u32), up as u32);
        assert_eq!(case::to_lower(lo as u32), lo as u32);
    }
    // Special 1:1 edge cases and unicameral passthrough.
    assert_eq!(case::to_upper('ς' as u32), 'Σ' as u32);
    assert_eq!(case::to_upper('ı' as u32), 'I' as u32);
    assert_eq!(case::to_upper('ſ' as u32), 'S' as u32);
    assert_eq!(case::to_lower('İ' as u32), 'i' as u32);
    assert_eq!(case::to_lower('ẞ' as u32), 'ß' as u32);
    assert_eq!(case::to_upper('ß' as u32), 'ß' as u32);
    for unchanged in ['×', '÷', '0', '9', '!', '漢', 'あ', '한', 'ا', 'א'] {
        assert_eq!(case::to_upper(unchanged as u32), unchanged as u32);
        assert_eq!(case::to_lower(unchanged as u32), unchanged as u32);
    }

    // Exhaustive safety invariant: emit_segment's byte budget assumes case
    // mapping never increases a codepoint's UTF-8 byte length.
    for raw in 0..=0x10ffffu32 {
        let Some(ch) = char::from_u32(raw) else { continue };
        let orig = ch.len_utf8();
        let up = char::from_u32(case::to_upper(raw)).expect("valid scalar");
        let lo = char::from_u32(case::to_lower(raw)).expect("valid scalar");
        assert!(up.len_utf8() <= orig, "U+{raw:04X} grew on to_upper");
        assert!(lo.len_utf8() <= orig, "U+{raw:04X} grew on to_lower");
        if up != ch || lo != ch {
            assert!(
                case::is_word_char(raw),
                "mapped cased codepoint U+{raw:04X} missing from WORD_RANGES"
            );
        }
    }
}
#[test]
fn word_start_handles_contractions_quotes_hyphens_and_combining_marks() {
    let starts_after = |prefix: &str| {
        let full = format!("{prefix}x");
        let mut levels = vec![0u8; full.len()];
        case::mark_word_starts(full.as_bytes(), &mut levels, case::WS_SEP);
        levels[prefix.len()] & case::WORD_START_BIT != 0
    };
    assert!(starts_after(""));
    assert!(starts_after("hello "));
    assert!(starts_after("hello\u{00a0}"));
    assert!(starts_after("well-"));
    assert!(starts_after("one\u{2014}"));
    assert!(starts_after("("));
    assert!(starts_after("「"));
    assert!(starts_after("'"));
    assert!(starts_after("he said '"));
    assert!(starts_after("he said \u{2018}"));
    assert!(!starts_after("don'"));
    assert!(!starts_after("it\u{2019}"));
    assert!(!starts_after("l\u{2019}"));
    assert!(!starts_after("co\u{00ad}"));
    assert!(!starts_after("re\u{0301}"));
    assert!(!starts_after("a\u{1dc4}"));
    assert!(!starts_after("3"));
    assert!(!starts_after("a"));
    // Interior letters after arbitrary runs of combining marks or soft hyphens
    // must NOT manufacture a false word boundary.
    let mid_accents = format!("co{}", "\u{0301}".repeat(12));
    assert!(!starts_after(&mid_accents));
    let mid_shy = format!("co{}", "\u{00ad}".repeat(64));
    assert!(!starts_after(&mid_shy));
    // Leading soft hyphens before the first word still allow word start.
    let leading_shy = "\u{00ad}".repeat(64);
    assert!(starts_after(&leading_shy));

    // Mapped asymmetric cased letters (including U+00B5 micro sign) start their
    // own words under text-transform: capitalize rather than acting as separators.
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    run_case_layout(
        &mut font,
        &mut sink,
        400,
        200,
        24,
        b"p{text-transform:capitalize}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text("\u{00b5}m \u{017f}ong \u{01c6}ep".as_bytes());
            e.end(b"p");
            e.end(b"body");
        },
    );
    let rendered: String = sink.inner.pages[0]
        .2
        .iter()
        .map(|r| core::str::from_utf8(&r.0).unwrap())
        .collect();
    assert_eq!(rendered, "\u{039c}m Song \u{01c4}ep");
}

#[test]
fn small_caps_splits_runs_preserves_baseline_and_handles_acronyms_and_scripts() {
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    run_case_layout(
        &mut font,
        &mut sink,
        480,
        400,
        30,
        b".sc{font-variant:small-caps} .norm{font-variant-caps:normal}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", Some(b"sc"), None, None, None, None);
            e.text(b"Hello, NASA! ");
            e.start(b"span", Some(b"norm"), None, None, None, None);
            e.text(b"back");
            e.end(b"span");
            e.end(b"p");
            e.start(b"p", Some(b"sc"), None, None, None, None);
            e.text("Café ς Пр Việt 漢字 42".as_bytes());
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs = &sink.inner.pages[0].2;
    // First paragraph: H(30) ELLO(24) ", NASA!"(30) " back"(30) (collapsed
    // trailing space attaches to the start of the following span).
    let p1: Vec<_> = runs.iter().take(4).collect();
    assert_eq!(p1[0].0, b"H");
    assert_eq!(p1[0].3, 30);
    assert_eq!(p1[1].0, b"ELLO");
    assert_eq!(p1[1].3, 24);
    assert_eq!(p1[1].2, p1[0].2, "small caps must share full-cap baseline");
    assert_eq!(p1[2].0, b", NASA!");
    assert_eq!(p1[2].3, 30);
    assert_eq!(p1[3].0, b" back");
    assert_eq!(p1[3].3, 30);

    // Second paragraph: non-ASCII scripts uppercase at 24 px; CJK/digits stay 30 px.
    let p2: Vec<(&str, u16)> = runs[4..]
        .iter()
        .map(|r| (core::str::from_utf8(&r.0).unwrap(), r.3))
        .collect();
    assert_eq!(
        p2,
        vec![
            ("C", 30),
            ("AFÉ", 24),
            (" ", 30),
            ("Σ", 24),
            (" П", 30),
            ("Р", 24),
            (" V", 30),
            ("IỆT", 24),
            (" 漢字 42", 30),
        ]
    );
}

#[test]
fn small_caps_inhibits_ligatures_and_respects_font_coverage() {
    let mut font = StrictCaseFont {
        enable_ligature: true,
        reject_cp: Some('É' as u32),
        ..StrictCaseFont::default()
    };
    let mut sink = AnchorCapture::default();
    run_case_layout(
        &mut font,
        &mut sink,
        480,
        400,
        30,
        b".sc{font-variant:small-caps} .up{text-transform:uppercase} .lo{text-transform:lowercase}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(b"fi ");
            e.start(b"span", Some(b"sc"), None, None, None, None);
            e.text("fi fl é".as_bytes());
            e.end(b"span");
            e.text(b" ");
            e.start(b"span", Some(b"up"), None, None, None, None);
            e.text(b"fi");
            e.end(b"span");
            e.text(b" ");
            e.start(b"span", Some(b"lo"), None, None, None, None);
            e.text(b"FI");
            e.end(b"span");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs: Vec<(&str, u16)> = sink.inner.pages[0]
        .2
        .iter()
        .map(|r| (core::str::from_utf8(&r.0).unwrap(), r.3))
        .collect();
    assert_eq!(
        runs,
        vec![
            ("ﬁ", 30),
            (" ", 30),
            ("FI", 24),
            (" ", 30),
            ("FL", 24),
            (" é", 30), // É not covered -> keeps lowercase é at full size
            (" FI", 30), // uppercase inhibits fi ligature
            (" ﬁ", 30),  // lowercase FI -> f+i -> ligates naturally
        ]
    );
}

#[test]
fn text_transform_modes_cross_span_capitalize_and_small_caps_precedence() {
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    run_case_layout(
        &mut font,
        &mut sink,
        600,
        600,
        30,
        b".up{text-transform:uppercase} .lo{text-transform:lowercase} \
          .cap{text-transform:capitalize} .none{text-transform:none} \
          .sc{font-variant:small-caps}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", Some(b"up"), None, None, None, None);
            e.text(b"Hello ");
            e.start(b"span", Some(b"none"), None, None, None, None);
            e.text(b"Keep");
            e.end(b"span");
            e.end(b"p");

            e.start(b"p", Some(b"cap"), None, None, None, None);
            e.text("don't 'quote' well-known ".as_bytes());
            e.start(b"b", None, None, None, None, None);
            e.text(b"h");
            e.end(b"b");
            e.text(b"ello");
            e.end(b"p");

            // Precedence: uppercase + small-caps => all full size;
            // lowercase + small-caps => all small-cap size;
            // capitalize + small-caps => full initials + small-cap tails.
            e.start(b"p", Some(b"up sc"), None, None, None, None);
            e.text(b"Hello");
            e.end(b"p");
            e.start(b"p", Some(b"lo sc"), None, None, None, None);
            e.text(b"HELLO");
            e.end(b"p");
            e.start(b"p", Some(b"cap sc"), None, None, None, None);
            e.text(b"hello world");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs: Vec<(&str, u16, u8)> = sink.inner.pages[0]
        .2
        .iter()
        .map(|r| (core::str::from_utf8(&r.0).unwrap(), r.3, r.4))
        .collect();
    assert_eq!(
        runs,
        vec![
            ("HELLO", 30, 0),
            (" Keep", 30, 0),
            ("Don't 'Quote' Well-Known", 30, 0),
            (" H", 30, BOLD),
            ("ello", 30, 0),
            ("HELLO", 30, 0),
            ("HELLO", 24, 0),
            ("H", 30, 0),
            ("ELLO", 24, 0),
            (" W", 30, 0),
            ("ORLD", 24, 0),
        ]
    );
}

#[test]
fn justified_small_caps_aligns_to_right_margin_without_cross_size_kerning() {
    let mut font = StrictCaseFont {
        kern_av: true,
        kern_th: true,
        ..StrictCaseFont::default()
    };
    let mut sink = AnchorCapture::default();
    let width = 260i16;
    let right_edge = width - 16;
    run_case_layout(
        &mut font,
        &mut sink,
        width,
        500,
        30,
        b"p{font-variant:small-caps;text-align:justify}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            // Contains T+h (cross-size boundary -> must NOT kern T+H) and a+v
            // (same-size small-cap -> must kern A+V at 24 px), plus spaces.
            e.text(b"The brave aviator traveled far beyond a valley harbor.");
            e.end(b"p");
            e.end(b"body");
        },
    );
    assert!(
        !font.cross_size_kern_seen,
        "kerning must reset across small-cap size transitions (T 30px -> H 24px)"
    );
    let runs = &sink.inner.pages[0].2;
    // Group runs by baseline into lines.
    let mut lines: Vec<Vec<&(Vec<u8>, i16, i16, u16, u8)>> = Vec::new();
    for run in runs {
        if lines.last().is_some_and(|l| l[0].2 == run.2) {
            lines.last_mut().unwrap().push(run);
        } else {
            lines.push(vec![run]);
        }
    }
    assert!(lines.len() >= 3, "expected multi-line justified paragraph");
    // Every justified non-last line must flush cleanly to the right margin, and
    // adjacent words must have visible positive space gaps.
    let mut measure_font = StrictCaseFont {
        kern_av: true,
        kern_th: false,
        ..StrictCaseFont::default()
    };
    for line in &lines[..lines.len() - 1] {
        let last = line.last().unwrap();
        let mut w = 0i32;
        let mut prev = 0u32;
        for ch in core::str::from_utf8(&last.0).unwrap().chars() {
            let cp = ch as u32;
            if prev != 0 {
                w += i32::from(measure_font.kerning(prev, cp, last.3, last.4));
            }
            w += i32::from(measure_font.advance(cp, last.3, last.4));
            prev = cp;
        }
        assert_eq!(
            i32::from(last.1) + w,
            i32::from(right_edge),
            "justified small-caps line drifted from right margin: {line:?}"
        );
    }
}
#[test]
fn small_caps_sup_shares_baseline_and_preserves_char_offsets_and_anchors() {
    let run_with_css = |css_text: &[u8]| {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        let mut total_chars = 0;
        run_case_layout(&mut font, &mut sink, 200, 120, 30, css_text, |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", Some(b"sc"), None, Some(b"first"), None, None);
            e.text(b"Hello ");
            e.start(b"sup", None, None, None, None, None);
            e.text(b"Note");
            e.end(b"sup");
            e.end(b"p");
            e.start(b"p", Some(b"sc"), None, Some(b"second"), None, None);
            e.text(b"Another paragraph to reach page two cleanly.");
            e.end(b"p");
            e.end(b"body");
            total_chars = e.chars;
        });
        (sink, total_chars)
    };
    let (plain, plain_chars) = run_with_css(b"");
    let (sc, sc_chars) = run_with_css(b".sc{font-variant:small-caps}");
    assert_eq!(sc_chars, plain_chars);
    assert_eq!(sc.anchors, plain.anchors);
    // Page 2 starts cleanly at the source char offset of the first character on page 2.
    assert!(sc.inner.pages.len() >= 2, "expected multi-page layout");
    let full_source = "Hello NoteAnother paragraph to reach page two cleanly.";
    let p2_start = sc.inner.pages[1].1 as usize;
    let p2_first_run = core::str::from_utf8(&sc.inner.pages[1].2[0].0).unwrap();
    let expected_first = full_source
        .chars()
        .nth(p2_start)
        .unwrap()
        .to_ascii_uppercase();
    assert_eq!(
        p2_first_run.chars().next().unwrap(),
        expected_first,
        "page 2 char_start {p2_start} did not match painted text {p2_first_run:?}"
    );

    // Inside <sup>Note</sup>, leading space + 'N' (15 px) and 'OTE' (12 px) share one baseline.
    let sup_runs: Vec<_> = sc.inner.pages[0]
        .2
        .iter()
        .filter(|r| r.4 & SUP != 0)
        .collect();
    assert_eq!(sup_runs.len(), 2);
    assert_eq!(sup_runs[0].0, b" N");
    assert_eq!(sup_runs[0].3, 15);
    assert_eq!(sup_runs[1].0, b"OTE");
    assert_eq!(sup_runs[1].3, case::small_cap_px(15));
    assert_eq!(
        sup_runs[0].2, sup_runs[1].2,
        "superscript small-caps must share one raised baseline"
    );
}

#[test]
fn alternating_case_stress_stays_bounded_without_losing_text() {
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    let input = "aAbBcCdDeEfFgGhHiIjJkKlLmMnNoOpPqQrRsStTuUvVwWxXyYzZ".repeat(10);
    assert_eq!(input.len(), 520);
    run_case_layout(
        &mut font,
        &mut sink,
        800,
        480,
        24,
        b"p{font-variant:small-caps}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(input.as_bytes());
            e.end(b"p");
            e.end(b"body");
        },
    );
    let emitted: String = sink
        .inner
        .pages
        .iter()
        .flat_map(|(_, _, runs)| runs.iter())
        .map(|r| core::str::from_utf8(&r.0).unwrap())
        .collect();
    assert_eq!(emitted, input.to_ascii_uppercase());
    // Every emitted run sits strictly within the content width.
    // Every emitted run's full painted span stays strictly within the content width.
    let mut measure_font = StrictCaseFont::default();
    for (_, _, runs) in &sink.inner.pages {
        for r in runs {
            let right = i32::from(r.1) + painted_run_width(&mut measure_font, r);
            assert!(
                r.1 >= 16 && right <= 800 - 16,
                "run painted out of bounds: x={} right={} run={r:?}",
                r.1,
                right
            );
        }
    }
}
#[test]
fn justified_soft_hyphen_small_caps_uses_small_cap_hyphen_width() {
    let mut font = StrictCaseFont {
        kern_av: true,
        kern_ah: true,
        ..StrictCaseFont::default()
    };
    let mut sink = AnchorCapture::default();
    let width = 200i16;
    let right_edge = width - 16;
    run_case_layout(
        &mut font,
        &mut sink,
        width,
        400,
        30,
        b"p{font-variant:small-caps;text-align:justify}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            // Ends prefix before soft hyphen with 'a' so A- kerning is exercised too.
            e.text("We extra\u{00ad}ordinary quiet harbor.".as_bytes());
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs = &sink.inner.pages[0].2;
    let first_baseline = runs[0].2;
    let first_line: Vec<_> = runs.iter().filter(|r| r.2 == first_baseline).collect();
    let last = first_line.last().unwrap();
    assert!(
        last.0.ends_with(b"-"),
        "expected soft-hyphen break on first line, got {first_line:?}"
    );
    assert_eq!(
        last.3,
        case::small_cap_px(30),
        "soft-hyphenated small-cap run must emit the hyphen at small-cap size (24 px), got {last:?}"
    );
    let mut measure_font = StrictCaseFont {
        kern_av: true,
        kern_ah: true,
        ..StrictCaseFont::default()
    };
    assert_eq!(
        i32::from(last.1) + painted_run_width(&mut measure_font, last),
        i32::from(right_edge),
        "hyphenated justified small-caps line drifted from right margin: {first_line:?}"
    );

    // Unbroken soft hyphen inside small-caps must NOT split the run or disrupt A-V kerning.
    let measure_pair = |word: &str| {
        let mut f = StrictCaseFont {
            kern_av: true,
            ..StrictCaseFont::default()
        };
        let mut s = AnchorCapture::default();
        run_case_layout(&mut f, &mut s, 300, 200, 30, b"p{font-variant:small-caps}", |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(word.as_bytes());
            e.end(b"p");
            e.end(b"body");
        });
        s.inner.pages[0].2.clone()
    };
    assert_eq!(measure_pair("a\u{00ad}v"), measure_pair("av"));

    // Discretionary soft-hyphen break must budget the visible '-' so a barely-fitting
    // prefix does not overflow max when '-' is appended.
    let mut f2 = StrictCaseFont::default();
    let mut s2 = AnchorCapture::default();
    // Content width = 96 px (left=16, right=16 => width=128).
    // "aa " = 2*12 + 7 = 31 px; "bbbbbb" = 6*12 = 72 px (total 103 > 96).
    // "bbbbb" + shy = 31 + 5*12 = 91 px (fits without '-'), + '-' (8 px) = 99 > 96!
    // Engine must break at the earlier space instead of taking the overflowing shy.
    run_case_layout(&mut f2, &mut s2, 128, 300, 30, b"p{font-variant:small-caps}", |e| {
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, None, None, None, None);
        e.text("aa bbbbb\u{00ad}b".as_bytes());
        e.end(b"p");
        e.end(b"body");
    });
    let mut mf2 = StrictCaseFont::default();
    for r in &s2.inner.pages[0].2 {
        let right = i32::from(r.1) + painted_run_width(&mut mf2, r);
        assert!(right <= 128 - 16, "discretionary soft hyphen overflowed margin: {r:?} right={right}");
    }
}
#[test]
fn mixed_script_small_caps_matches_painted_right_edge_in_both_orders() {
    for sample in ["a漢a漢a漢a漢a漢", "漢a漢a漢a漢a漢a"] {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        // Tight width forces wrap across repeated small-cap <-> CJK transitions.
        let width = 180i16;
        let right_edge = width - 16;
        run_case_layout(
            &mut font,
            &mut sink,
            width,
            400,
            30,
            b"p{font-variant:small-caps}",
            |e| {
                e.start(b"body", None, None, None, None, None);
                e.start(b"p", None, None, None, None, None);
                e.text(sample.as_bytes());
                e.end(b"p");
                e.end(b"body");
            },
        );
        let mut mf = StrictCaseFont::default();
        let runs = &sink.inner.pages[0].2;
        assert!(!runs.is_empty());
        for r in runs {
            let right = i32::from(r.1) + painted_run_width(&mut mf, r);
            assert!(
                right <= i32::from(right_edge),
                "{sample}: mixed-script small-caps overflowed right edge ({right} > {right_edge}): {runs:?}"
            );
        }
    }
    // Justified mixed-script line in both orders must flush to the exact right margin.
    for sample in ["a漢 a漢 a漢 a漢 a漢", "漢a 漢a 漢a 漢a 漢a"] {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        let width = 200i16;
        let right_edge = width - 16;
        run_case_layout(
            &mut font,
            &mut sink,
            width,
            400,
            30,
            b"p{font-variant:small-caps;text-align:justify}",
            |e| {
                e.start(b"body", None, None, None, None, None);
                e.start(b"p", None, None, None, None, None);
                e.text(sample.as_bytes());
                e.end(b"p");
                e.end(b"body");
            },
        );
        let runs = &sink.inner.pages[0].2;
        let first_base = runs[0].2;
        let last = runs.iter().filter(|r| r.2 == first_base).last().unwrap();
        let mut mf = StrictCaseFont::default();
        assert_eq!(
            i32::from(last.1) + painted_run_width(&mut mf, last),
            i32::from(right_edge),
            "{sample}: justified mixed-script small-caps drifted from right edge"
        );
    }
}
#[test]
fn kerning_resets_across_case_only_span_and_focus_boundaries() {
    // Case-only span boundary: <span class="up">a</span>V creates two runs ("A", "V"),
    // so A-V kerning must NOT apply during measurement.
    let mut font = StrictCaseFont {
        kern_av: true,
        ..StrictCaseFont::default()
    };
    let mut sink = AnchorCapture::default();
    let width = 180i16;
    let right_edge = width - 16;
    run_case_layout(
        &mut font,
        &mut sink,
        width,
        400,
        30,
        b"p{text-align:justify} .up{text-transform:uppercase}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            for i in 0..8 {
                if i > 0 {
                    e.text(b" ");
                }
                e.start(b"span", Some(b"up"), None, None, None, None);
                e.text(b"a");
                e.end(b"span");
                e.text(b"V");
            }
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs = &sink.inner.pages[0].2;
    let first_base = runs[0].2;
    let last = runs.iter().filter(|r| r.2 == first_base).last().unwrap();
    let mut mf = StrictCaseFont {
        kern_av: true,
        ..StrictCaseFont::default()
    };
    assert_eq!(
        i32::from(last.1) + painted_run_width(&mut mf, last),
        i32::from(right_edge),
        "case-only span boundary kerning mismatch: {runs:?}"
    );

    // Focus bold boundary mid-word inside small caps: "av" (2 chars => 45% => 1 bold 'A',
    // 1 regular 'V') splits into two runs, so A-V kerning must NOT apply across the face split.
    let mut font2 = StrictCaseFont {
        kern_av: true,
        ..StrictCaseFont::default()
    };
    let mut sink2 = AnchorCapture::default();
    run_case_layout(
        &mut font2,
        &mut sink2,
        width,
        400,
        30,
        b"p{font-variant:small-caps;text-align:justify}",
        |e| {
            e.params.focus = true;
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(b"av av av av av av av av");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs2 = &sink2.inner.pages[0].2;
    let first_base2 = runs2[0].2;
    let last2 = runs2.iter().filter(|r| r.2 == first_base2).last().unwrap();
    assert_eq!(
        i32::from(last2.1) + painted_run_width(&mut mf, last2),
        i32::from(right_edge),
        "focus mid-word small-caps boundary kerning mismatch: {runs2:?}"
    );
}

#[test]
fn dictionary_hyphenation_prices_ending_span_size_and_hyphen_kerning() {
    use newt_core::hyphenator::Hyphenator;
    // Minimal valid FIBH v1 table with one pattern "ab" => break weight 1 between 'a' and 'b'.
    let mut fibh = Vec::new();
    fibh.extend_from_slice(b"FIBH");
    fibh.extend_from_slice(&[1, 0, 2, 0]); // version 1.0, max_len 2, pad
    fibh.extend_from_slice(&1u32.to_le_bytes()); // count = 1
    fibh.extend_from_slice(&0u32.to_le_bytes()); // offset[0] = 0
    fibh.extend_from_slice(&[2, b'a', b'b', 0, 1, 0]); // len=2, "ab", values [0, 1, 0]
    let mut hyph = Hyphenator::new();
    assert!(hyph.init(Some(&fibh)) && hyph.ready());

    // Word starts in normal span ("xx") and transitions into a small-cap span ("abcc"):
    // candidate split at "xxa|bcc" ends inside the small-cap span with 'A' + '-' kerning.
    struct TraceFont {
        inner: StrictCaseFont,
        advances: Vec<(u32, u16, u8)>,
    }
    impl Font for TraceFont {
        fn covers(&mut self, cp: u32) -> bool { self.inner.covers(cp) }
        fn advance(&mut self, cp: u32, size: u16, flags: u8) -> i16 {
            self.advances.push((cp, size, flags));
            self.inner.advance(cp, size, flags)
        }
        fn line_height(&mut self, size: u16) -> i16 { self.inner.line_height(size) }
        fn ascent(&mut self, size: u16) -> i16 { self.inner.ascent(size) }
        fn ligature(&mut self, a: u32, b: u32, flags: u8) -> u32 { self.inner.ligature(a, b, flags) }
        fn kerning(&mut self, a: u32, b: u32, size: u16, flags: u8) -> i16 { self.inner.kerning(a, b, size, flags) }
    }
    let mut font = TraceFont {
        inner: StrictCaseFont {
            kern_ah: true,
            ..StrictCaseFont::default()
        },
        advances: Vec::new(),
    };
    let mut sink = AnchorCapture::default();
    let width = 120i16; // content width = 88 px
    let right_edge = width - 16;
    let mut text = vec![0; 512];
    let mut breaks = vec![0; 512];
    let mut levels = vec![0; 512];
    let mut spans = vec![Span::default(); 64];
    let mut lines = vec![Line::default(); 32];
    let mut elements = vec![Element::default(); 16];
    let mut runs = vec![Run::default(); 64];
    let mut images = vec![Image::default(); 2];
    let mut links = vec![Link::default(); 8];
    let mut page = vec![0; 2048];
    let mut style = vec![0; 256];
    let mut pl = vec![ParLink::default(); 4];
    let mut sheet_rules = vec![CssRule::default(); 512];
    let mut sheet_builder = CssStylesheetBuilder::default();
    assert!(sheet_builder.begin(&mut sheet_rules));
    sheet_builder.add_text(b"p{text-align:justify} .sc{font-variant:small-caps}");
    let sheet = sheet_builder.finish();
    let mut ch_rules = vec![CssRule::default(); 512];
    let mut ch_css = CssStylesheetBuilder::default();
    assert!(ch_css.begin(&mut ch_rules));
    let storage = Storage {
        text: &mut text,
        breaks: &mut breaks,
        levels: &mut levels,
        spans: &mut spans,
        lines: &mut lines,
        elements: &mut elements,
        runs: &mut runs,
        images: &mut images,
        links: &mut links,
        page: &mut page,
        style_text: &mut style,
        paragraph_links: &mut pl,
    };
    let mut backend = SplitBackend {
        font: &mut font,
        sink: &mut sink,
    };
    let mut breaker = SpaceBreak;
    let mut img = FixtureImages;
    let params = Params {
        width,
        height: 400,
        left: 16,
        right: 16,
        top: 16,
        bottom: 16,
        base_size: 30,
        stylesheet: Some(sheet),
        ..Params::default()
    };
    let mut engine = Engine::new(
        params,
        storage,
        &mut backend,
        &mut breaker,
        &mut img,
        Some(&hyph),
        ch_css,
    )
    .unwrap();
    engine.start(b"body", None, None, None, None, None);
    engine.start(b"p", None, None, None, None, None);
    // "go " (37 px) + "xx" (30 px) + small-cap "A" (12 px) + small-cap "-" (8 px) + kern A- (-2 px) = 85 <= 88!
    engine.text(b"go xx");
    engine.start(b"span", Some(b"sc"), None, None, None, None);
    engine.text(b"abcc");
    engine.end(b"span");
    engine.text(b" done");
    engine.end(b"p");
    engine.end(b"body");
    assert!(engine.finish());
    drop(engine);
    let sc_sz = case::small_cap_px(30);
    assert!(
        font.advances.windows(6).any(|w| w == [
            (u32::from(b' '), 30, 0),
            (u32::from(b'-'), 30, 0),
            (u32::from(b'x'), 30, 0),
            (u32::from(b'x'), 30, 0),
            (u32::from(b'A'), sc_sz, 0),
            (u32::from(b'-'), sc_sz, 0),
        ]),
        "styled dictionary hyphenation must preserve #172's word-start hyphen probe before candidate probes: {:?}",
        font.advances
    );

    let runs = &sink.inner.pages[0].2;
    let first_base = runs[0].2;
    let line1: Vec<_> = runs.iter().filter(|r| r.2 == first_base).collect();
    let last = line1.last().unwrap();
    assert_eq!(last.0, b"A-", "expected hyphenated small-cap tail 'A-', got {line1:?}");
    assert_eq!(last.3, case::small_cap_px(30));
    let mut mf = StrictCaseFont {
        kern_ah: true,
        ..StrictCaseFont::default()
    };
    assert_eq!(
        i32::from(last.1) + painted_run_width(&mut mf, last),
        i32::from(right_edge),
        "dictionary-hyphenated mixed-span line drifted from right margin: {line1:?}"
    );

    // Direction A (plain first + styled middle + 200% plain ending): candidate
    // "xya|bcc" has prefix "go " (37) + "x" (15) + sc "Y" (12) + 60px "a" (30) = 94 px.
    // A 30 px hyphen would be 10 px (total 104 <= 110), while the actual 60 px ending
    // span's hyphen is 20 px (total 114 > 110). Budget = 110 (width = 142) MUST reject
    // the overflowing split so nothing paints past right_edge.
    let run_mixed_hyph = |width: i16, css: &[u8], build: &dyn Fn(&mut Engine<'_>)| {
        let mut f = StrictCaseFont {
            kern_ah: true,
            ..StrictCaseFont::default()
        };
        let mut s = AnchorCapture::default();
        let mut t = vec![0; 512];
        let mut br = vec![0; 512];
        let mut lv = vec![0; 512];
        let mut sp = vec![Span::default(); 64];
        let mut ln = vec![Line::default(); 32];
        let mut el = vec![Element::default(); 16];
        let mut rn = vec![Run::default(); 64];
        let mut im = vec![Image::default(); 2];
        let mut lk = vec![Link::default(); 8];
        let mut pg = vec![0; 2048];
        let mut st = vec![0; 256];
        let mut p_links = vec![ParLink::default(); 4];
        let mut sr = vec![CssRule::default(); 512];
        let mut sb = CssStylesheetBuilder::default();
        assert!(sb.begin(&mut sr));
        sb.add_text(css);
        let sh = sb.finish();
        let mut cr = vec![CssRule::default(); 512];
        let mut cc = CssStylesheetBuilder::default();
        assert!(cc.begin(&mut cr));
        let storage = Storage {
            text: &mut t,
            breaks: &mut br,
            levels: &mut lv,
            spans: &mut sp,
            lines: &mut ln,
            elements: &mut el,
            runs: &mut rn,
            images: &mut im,
            links: &mut lk,
            page: &mut pg,
            style_text: &mut st,
            paragraph_links: &mut p_links,
        };
        let mut backend = SplitBackend { font: &mut f, sink: &mut s };
        let mut breaker = SpaceBreak;
        let mut img = FixtureImages;
        let params = Params {
            width,
            height: 400,
            left: 16,
            right: 16,
            top: 16,
            bottom: 16,
            base_size: 30,
            stylesheet: Some(sh),
            ..Params::default()
        };
        let mut e = Engine::new(params, storage, &mut backend, &mut breaker, &mut img, Some(&hyph), cc).unwrap();
        build(&mut e);
        assert!(e.finish());
        s.inner.pages[0].2.clone()
    };
    let dir_a = run_mixed_hyph(
        142,
        b"p{text-align:justify} .sc{font-variant:small-caps} .big{font-size:200%}",
        &|e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(b"go x");
            e.start(b"span", Some(b"sc"), None, None, None, None);
            e.text(b"y");
            e.end(b"span");
            e.start(b"span", Some(b"big"), None, None, None, None);
            e.text(b"abcc");
            e.end(b"span");
            e.text(b" done");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let mut mf_a = StrictCaseFont { kern_ah: true, ..StrictCaseFont::default() };
    for r in &dir_a {
        let right = i32::from(r.1) + painted_run_width(&mut mf_a, r);
        assert!(right <= 142 - 16, "mixed plain-end hyphen overflowed margin: {r:?} right={right}");
    }

    // Direction B (styled start + plain capital 'A' tail): "go " (37) + sc "XY" (24)
    // + plain "A" (15) + plain "-" (10) + kern A- (-2) = 84 <= 85 (width = 117),
    // while unkerned would be 86 > 85. Tight 85 px budget verifies kerning is
    // priced in admission as well as emission and right-edge justification.
    let dir_b = run_mixed_hyph(
        117,
        b"p{text-align:justify} .sc{font-variant:small-caps}",
        &|e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(b"go ");
            e.start(b"span", Some(b"sc"), None, None, None, None);
            e.text(b"xy");
            e.end(b"span");
            e.text(b"Abcc done");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let b_base = dir_b[0].2;
    let b_line1: Vec<_> = dir_b.iter().filter(|r| r.2 == b_base).collect();
    let b_last = b_line1.last().unwrap();
    assert_eq!(b_last.0, b"A-", "expected plain-tail 'A-' run, got {b_line1:?}");
    assert_eq!(b_last.3, 30);
    assert_eq!(
        i32::from(b_last.1) + painted_run_width(&mut mf_a, b_last),
        117 - 16,
        "styled-start plain-tail justified hyphen drifted from right edge: {b_line1:?}"
    );
}

#[test]
fn justified_cjk_soft_hyphen_and_inline_margin_match_right_edge() {
    // Finding 1: unbroken soft hyphen between CJK characters in a justified
    // case-styled paragraph must count the same CJK gap in line_metrics as in
    // segment_line so spare width is never spent twice.
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    let width = 132i16; // content width = 100 px
    let right_edge = width - 16;
    run_case_layout(
        &mut font,
        &mut sink,
        width,
        400,
        30,
        b"p{font-variant:small-caps;text-align:justify}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            // "漢\u{00ad}字 x" = 15 + 15 + 7 + 12 = 49 px (fits easily), followed by a long word
            // that forces a wrap so line 1 is justified with both the CJK gap and space gap.
            e.text("漢\u{00ad}字 x longtrailingword".as_bytes());
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs = &sink.inner.pages[0].2;
    let base = runs[0].2;
    let line1: Vec<_> = runs.iter().filter(|r| r.2 == base).collect();
    assert_eq!(line1.len(), 3, "expected 漢, 字, X on line 1, got {line1:?}");
    let last = line1.last().unwrap();
    let mut mf = StrictCaseFont::default();
    assert_eq!(
        i32::from(last.1) + painted_run_width(&mut mf, last),
        i32::from(right_edge),
        "justified CJK + unbroken soft hyphen overshot right margin: {line1:?}"
    );

    // Finding 2: positive inline margin-left between small-cap Latin and CJK
    // must retain script adjacency during measurement so painted right edge
    // matches line_metrics exactly.
    let mut font2 = StrictCaseFont::default();
    let mut sink2 = AnchorCapture::default();
    let w2 = 160i16; // content width = 128 px
    run_case_layout(
        &mut font2,
        &mut sink2,
        w2,
        400,
        30,
        b"p{font-variant:small-caps;text-align:justify} .m{margin-left:1em}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(b"a");
            e.start(b"span", Some(b"m"), None, None, None, None);
            e.text("漢".as_bytes());
            e.end(b"span");
            e.text(b" bc defghijklm");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs2 = &sink2.inner.pages[0].2;
    let base2 = runs2[0].2;
    let line1_m: Vec<_> = runs2.iter().filter(|r| r.2 == base2).collect();
    let last_m = line1_m.last().unwrap();
    assert_eq!(
        i32::from(last_m.1) + painted_run_width(&mut mf, last_m),
        i32::from(w2 - 16),
        "inline margin mixed-script small-caps line drifted from right edge: {line1_m:?}"
    );
}

#[test]
fn soft_hyphen_after_small_cap_span_end_keeps_reduced_tail_and_budget() {
    // Finding 4: closing a small-cap span right before U+00AD must keep the
    // preceding reduced 'A' tail across the span boundary, emit a single 24 px
    // "EXTRA-" run with A- kerning when it fits, and reject the break when it
    // would overflow.
    let run_case = |width: i16| {
        let mut f = StrictCaseFont {
            kern_ah: true,
            ..StrictCaseFont::default()
        };
        let mut s = AnchorCapture::default();
        run_case_layout(
            &mut f,
            &mut s,
            width,
            400,
            30,
            b"p{text-align:justify} .sc{font-variant:small-caps}",
            |e| {
                e.start(b"body", None, None, None, None, None);
                e.start(b"p", None, None, None, None, None);
                e.text(b"go ");
                e.start(b"span", Some(b"sc"), None, None, None, None);
                e.text(b"extra");
                e.end(b"span");
                e.text("\u{00ad}ordinary".as_bytes());
                e.end(b"p");
                e.end(b"body");
            },
        );
        s.inner.pages[0].2.clone()
    };
    // "go " = 37 px; sc "EXTRA" = 5*12 = 60 px; sc "-" = 8 px + A- kern (-2) = 6 px.
    // Total with reduced hyphen = 103 px.
    // Case A: content width = 106 px (width = 138) fits reduced hyphen cleanly.
    let fits = run_case(138);
    let base_a = fits[0].2;
    let line1_a: Vec<_> = fits.iter().filter(|r| r.2 == base_a).collect();
    let last_a = line1_a.last().unwrap();
    assert_eq!(last_a.0, b"EXTRA-", "expected reduced small-cap hyphen tail, got {line1_a:?}");
    assert_eq!(last_a.3, case::small_cap_px(30));
    let mut mf = StrictCaseFont { kern_ah: true, ..StrictCaseFont::default() };
    assert_eq!(
        i32::from(last_a.1) + painted_run_width(&mut mf, last_a),
        138 - 16,
        "post-span soft hyphen justified line drifted: {line1_a:?}"
    );

    // Case B: content width = 100 px (width = 132) fits "go EXTRA" (97 px) without '-',
    // but NOT with reduced '-' (103 px). Must wrap at space before "extra" with zero overflow.
    let tight = run_case(132);
    for r in &tight {
        let right = i32::from(r.1) + painted_run_width(&mut mf, r);
        assert!(right <= 132 - 16, "post-span soft hyphen overflowed margin: {r:?} right={right}");
    }
    let base_b = tight[0].2;
    let line1_b: Vec<_> = tight.iter().filter(|r| r.2 == base_b).collect();
    assert_eq!(line1_b.len(), 1);
    assert_eq!(line1_b[0].0, b"go");
}

#[test]
fn plain_focused_hyphenated_link_keeps_bold_hyphen_width() {
    use newt_core::hyphenator::Hyphenator;
    let mut fibh = Vec::new();
    fibh.extend_from_slice(b"FIBH");
    fibh.extend_from_slice(&[1, 0, 2, 0]);
    fibh.extend_from_slice(&1u32.to_le_bytes());
    fibh.extend_from_slice(&0u32.to_le_bytes());
    fibh.extend_from_slice(&[2, b'a', b'b', 0, 1, 0]);
    let mut hyph = Hyphenator::new();
    assert!(hyph.init(Some(&fibh)) && hyph.ready());

    let mut font = StrictCaseFont {
        bold_hyphen_extra: true,
        ..StrictCaseFont::default()
    };
    let mut sink = AnchorCapture::default();
    let mut text = vec![0; 512];
    let mut breaks = vec![0; 512];
    let mut levels = vec![0; 512];
    let mut spans = vec![Span::default(); 64];
    let mut lines = vec![Line::default(); 32];
    let mut elements = vec![Element::default(); 16];
    let mut runs = vec![Run::default(); 64];
    let mut images = vec![Image::default(); 2];
    let mut links = vec![Link::default(); 8];
    let mut page = vec![0; 2048];
    let mut style = vec![0; 256];
    let mut pl = vec![ParLink::default(); 4];
    let mut cr = vec![CssRule::default(); 512];
    let mut cc = CssStylesheetBuilder::default();
    assert!(cc.begin(&mut cr));
    let storage = Storage {
        text: &mut text,
        breaks: &mut breaks,
        levels: &mut levels,
        spans: &mut spans,
        lines: &mut lines,
        elements: &mut elements,
        runs: &mut runs,
        images: &mut images,
        links: &mut links,
        page: &mut page,
        style_text: &mut style,
        paragraph_links: &mut pl,
    };
    let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
    let mut breaker = SpaceBreak;
    let mut img = FixtureImages;
    // Plain focus paragraph: "go " (37 px) + bold "xxa" (3*15=45 px) + bold '-' (11 px) = 93 px.
    // Content width = 96 px (width = 128) forces dictionary break at "xxa|bcc" inside focus-bold prefix.
    let params = Params {
        width: 128,
        height: 400,
        left: 16,
        right: 16,
        top: 16,
        bottom: 16,
        base_size: 30,
        focus: true,
        ..Params::default()
    };
    let mut e = Engine::new(params, storage, &mut backend, &mut breaker, &mut img, Some(&hyph), cc).unwrap();
    e.start(b"body", None, None, None, None, None);
    e.start(b"p", None, None, None, None, None);
    e.text(b"go ");
    e.start(b"a", None, None, None, Some(b"#target"), None);
    e.text(b"xxabcccccc");
    e.end(b"a");
    e.end(b"p");
    e.end(b"body");
    assert!(e.finish());

    let runs_out = &sink.inner.pages[0].2;
    let hy_run = runs_out.iter().find(|r| r.0 == b"xxa-").expect("expected focus-bold hyphenated run");
    assert_ne!(hy_run.4 & BOLD, 0, "expected focus-bold run flags");
    let mut mf = StrictCaseFont { bold_hyphen_extra: true, ..StrictCaseFont::default() };
    let expected_w = painted_run_width(&mut mf, hy_run) as i32;
    assert_eq!(expected_w, 3 * 15 + 11);
    assert_eq!(
        i32::from(sink.links[0].0) + i32::from(sink.links[0].1),
        i32::from(hy_run.1) + expected_w,
        "plain focused hyphenated link hitbox lost bold hyphen advance: links={:?}, run={hy_run:?}",
        sink.links
    );
}

#[test]
fn capitalize_survives_tiny_text_buffer_flushes_and_long_ignorable_runs() {
    let layout_with_cap = |cap: usize, input: &str| {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        let mut backend = SplitBackend {
            font: &mut font,
            sink: &mut sink,
        };
        with_text_capacity(&mut backend, 4096, cap, |e| {
            e.params.width = 480;
            e.params.height = 400;
            e.params.base_size = 24;
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, Some(b"text-transform:capitalize"), None, None, None);
            e.text(input.as_bytes());
            e.end(b"p");
            e.end(b"body");
            assert!(e.finish());
        });
        let mut out = String::new();
        for (_, _, runs) in &sink.inner.pages {
            for r in runs {
                out.push_str(core::str::from_utf8(&r.0).unwrap());
            }
        }
        // Capacity flushes force line breaks (which trim trailing line-end spaces),
        // so strip spaces to compare every non-space character's exact case 1:1.
        out
    };
    // Crosses tiny buffers mid-word ("harbor"), mid-contraction ("don't"),
    // and right across a multibyte UTF-8 letter ("café").
    let sample = "harbor don't café well-known 'quoted' re\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}sume";
    let large = layout_with_cap(256, sample);
    assert_eq!(
        large,
        "Harbor Don't Café Well-Known 'Quoted' Re\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}sume"
    );
    let large_nospace: String = large.chars().filter(|&c| c != ' ').collect();
    for tiny in [8, 9, 10, 12, 16] {
        let got: String = layout_with_cap(tiny, sample)
            .chars()
            .filter(|&c| c != ' ')
            .collect();
        assert_eq!(
            got,
            large_nospace,
            "tiny text_cap={tiny} altered capitalization across capacity flush"
        );
    }
}
#[test]
fn dictionary_hyphenation_keeps_prefix_script_gap_across_soft_hyphen() {
    use newt_core::hyphenator::Hyphenator;
    let mut fibh = Vec::new();
    fibh.extend_from_slice(b"FIBH");
    fibh.extend_from_slice(&[1, 0, 2, 0]);
    fibh.extend_from_slice(&1u32.to_le_bytes());
    fibh.extend_from_slice(&0u32.to_le_bytes());
    fibh.extend_from_slice(&[2, b'a', b'b', 0, 1, 0]);
    let mut hyph = Hyphenator::new();
    assert!(hyph.init(Some(&fibh)) && hyph.ready());

    // R3-1: in small-caps "漢\u{00ad}xxabcc" at 30 px (no kerning), "漢" = 15 px,
    // script gap 漢->X = 7 px, sc "XXA" = 36 px, sc "-" = 8 px => total 66 px.
    // Without the script gap it looked like 59 px <= 61 px and overflowed.
    // Content width = 61 px (width = 93) MUST take the earlier break after 漢
    // and keep every painted run strictly inside right_edge = 77.
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    let mut text = vec![0; 512];
    let mut breaks = vec![0; 512];
    let mut levels = vec![0; 512];
    let mut spans = vec![Span::default(); 64];
    let mut lines = vec![Line::default(); 32];
    let mut elements = vec![Element::default(); 16];
    let mut runs = vec![Run::default(); 64];
    let mut images = vec![Image::default(); 2];
    let mut links = vec![Link::default(); 8];
    let mut page = vec![0; 2048];
    let mut style = vec![0; 256];
    let mut pl = vec![ParLink::default(); 4];
    let mut sr = vec![CssRule::default(); 512];
    let mut sb = CssStylesheetBuilder::default();
    assert!(sb.begin(&mut sr));
    sb.add_text(b"p{font-variant:small-caps}");
    let sh = sb.finish();
    let mut cr = vec![CssRule::default(); 512];
    let mut cc = CssStylesheetBuilder::default();
    assert!(cc.begin(&mut cr));
    let storage = Storage {
        text: &mut text,
        breaks: &mut breaks,
        levels: &mut levels,
        spans: &mut spans,
        lines: &mut lines,
        elements: &mut elements,
        runs: &mut runs,
        images: &mut images,
        links: &mut links,
        page: &mut page,
        style_text: &mut style,
        paragraph_links: &mut pl,
    };
    let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
    let mut breaker = SpaceBreak;
    let mut img = FixtureImages;
    let width = 93i16; // content width = 61 px
    let params = Params {
        width,
        height: 400,
        left: 16,
        right: 16,
        top: 16,
        bottom: 16,
        base_size: 30,
        stylesheet: Some(sh),
        ..Params::default()
    };
    let mut e = Engine::new(params, storage, &mut backend, &mut breaker, &mut img, Some(&hyph), cc).unwrap();
    e.start(b"body", None, None, None, None, None);
    e.start(b"p", None, None, None, None, None);
    e.text("漢\u{00ad}xxabcc".as_bytes());
    e.end(b"p");
    e.end(b"body");
    assert!(e.finish());

    let runs_out = &sink.inner.pages[0].2;
    let mut mf = StrictCaseFont::default();
    for r in runs_out {
        let right = i32::from(r.1) + painted_run_width(&mut mf, r);
        assert!(right <= i32::from(width - 16), "dictionary candidate overflowed right margin: {r:?} right={right}");
    }
    let line1_base = runs_out[0].2;
    let line1: Vec<_> = runs_out.iter().filter(|r| r.2 == line1_base).collect();
    assert_eq!(line1.len(), 1);
    assert_eq!(line1[0].0, "漢-".as_bytes(), "expected earlier break after 漢, got {line1:?}");
}

#[test]
fn inline_margin_span_starting_with_soft_hyphen_preserves_margin_and_break_tail() {
    // R3-2a (unbroken): in left-aligned 30 px small caps, "a<span class='m'>\u{00ad}b</span>"
    // must keep the 30 px inline margin before B so B starts at x = 16 + 12 + 30 = 58.
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    run_case_layout(
        &mut font,
        &mut sink,
        300,
        300,
        30,
        b"p{font-variant:small-caps} .m{margin-left:1em}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(b"a");
            e.start(b"span", Some(b"m"), None, None, None, None);
            e.text("\u{00ad}b".as_bytes());
            e.end(b"span");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs = &sink.inner.pages[0].2;
    assert_eq!(runs.len(), 2, "expected A and B runs, got {runs:?}");
    assert_eq!(runs[0].0, b"A");
    assert_eq!(runs[0].1, 16);
    assert_eq!(runs[1].0, b"B");
    assert_eq!(runs[1].1, 16 + 12 + 30, "inline margin was lost on span starting with U+00AD: {runs:?}");

    // R3-2b (broken at the marker): content width = 52 px (width = 84) fits
    // A (12) + margin (30) + reduced hyphen (8) = 50 px, while + B (12) = 54 px overflows.
    // Line 1 breaks at U+00AD with reduced "A", 30 px margin, and reduced "-" hitting
    // the exact right edge (no cross-margin kerning); Line 2 places "B" at right edge.
    let mut font2 = StrictCaseFont { kern_ah: true, ..StrictCaseFont::default() };
    let mut sink2 = AnchorCapture::default();
    run_case_layout(
        &mut font2,
        &mut sink2,
        84,
        300,
        30,
        b"p{font-variant:small-caps;text-align:right} .m{margin-left:1em}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(b"a");
            e.start(b"span", Some(b"m"), None, None, None, None);
            e.text("\u{00ad}b".as_bytes());
            e.end(b"span");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs2 = &sink2.inner.pages[0].2;
    let base1_2 = runs2[0].2;
    let line1_2: Vec<_> = runs2.iter().filter(|r| r.2 == base1_2).collect();
    let line2_2: Vec<_> = runs2.iter().filter(|r| r.2 > base1_2).collect();
    assert_eq!(line1_2.len(), 2, "expected 'A' and '-' on line 1, got {runs2:?}");
    assert_eq!(line1_2[0].0, b"A");
    assert_eq!(line1_2[1].0, b"-");
    assert_eq!(line1_2[1].3, case::small_cap_px(30));
    assert_eq!(line1_2[1].1 - (line1_2[0].1 + 12), 30, "margin gap before hyphen: {line1_2:?}");
    let mut mf2 = StrictCaseFont { kern_ah: true, ..StrictCaseFont::default() };
    assert_eq!(
        i32::from(line1_2[1].1) + painted_run_width(&mut mf2, line1_2[1]),
        84 - 16,
        "right-aligned broken A + margin + '-' drifted from right edge: {runs2:?}"
    );
    assert_eq!(line2_2.len(), 1);
    assert_eq!(line2_2[0].0, b"B");
    assert_eq!(
        i32::from(line2_2[0].1) + painted_run_width(&mut mf2, line2_2[0]),
        84 - 16,
        "line 2 B drifted from right edge: {runs2:?}"
    );
}

#[test]
fn case_span_starting_with_soft_hyphen_does_not_alter_earlier_plain_run_kerning() {
    // R3-3: right-aligned "A\u{00ad}V<span class='sc'>\u{00ad}x</span>" at 30 px with
    // A-V kerning enabled. Paragraph-level par_case skips U+00AD uniformly across
    // measurement, emission, and the rendered run bytes so AV kerns consistently.
    let mut font = StrictCaseFont { kern_av: true, ..StrictCaseFont::default() };
    let mut sink = AnchorCapture::default();
    let width = 200i16;
    let right_edge = width - 16;
    run_case_layout(
        &mut font,
        &mut sink,
        width,
        300,
        30,
        b"p{text-align:right} .sc{font-variant:small-caps}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text("A\u{00ad}V".as_bytes());
            e.start(b"span", Some(b"sc"), None, None, None, None);
            e.text("\u{00ad}x".as_bytes());
            e.end(b"span");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs = &sink.inner.pages[0].2;
    assert_eq!(runs.len(), 2, "expected plain AV + sc X runs, got {runs:?}");
    assert_eq!(runs[0].0, b"AV");
    assert_eq!(runs[1].0, b"X");
    assert_eq!(runs[1].3, case::small_cap_px(30));
    let mut mf = StrictCaseFont { kern_av: true, ..StrictCaseFont::default() };
    assert_eq!(
        i32::from(runs[1].1) + painted_run_width(&mut mf, &runs[1]),
        i32::from(right_edge),
        "case paragraph plain run + styled U+00AD drifted from right edge: {runs:?}"
    );
    // Also check runs[0] ("AV") painted width agrees with runs[1].x (exact kerning consistency).
    assert_eq!(
        i32::from(runs[0].1) + painted_run_width(&mut mf, &runs[0]),
        i32::from(runs[1].1),
        "plain AV run width did not meet X origin: {runs:?}"
    );
}

#[test]
fn deferred_inline_margin_survives_break_into_plain_continuation() {
    // R4-1: left-aligned "<span class='sc'>aaa</span><span class='m'>\u{00ad}b</span>"
    // at 50 px content width (width = 82). AAA (36 px) + margin (30 px) overflows 50 px,
    // wrapping right before the margin-bearing soft hyphen; soft-wrap skip stops at the
    // margin-bearing marker so Line 2 paints plain 'b' at x = 16 + 30 = 46.
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    run_case_layout(
        &mut font,
        &mut sink,
        82,
        300,
        30,
        b".sc{font-variant:small-caps} .m{margin-left:1em}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.start(b"span", Some(b"sc"), None, None, None, None);
            e.text(b"aaa");
            e.end(b"span");
            e.start(b"span", Some(b"m"), None, None, None, None);
            e.text("\u{00ad}b".as_bytes());
            e.end(b"span");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs = &sink.inner.pages[0].2;
    assert_eq!(runs.len(), 2, "expected 2 lines ('AAA' and 'b'), got {runs:?}");
    assert_eq!(runs[0].0, b"AAA");
    assert_eq!(runs[0].3, case::small_cap_px(30));
    assert_eq!(runs[0].1, 16);
    assert_eq!(runs[1].0, b"b");
    assert_eq!(runs[1].3, 30);
    assert_eq!(runs[1].1, 16 + 30, "plain continuation lost 30 px margin: {runs:?}");
}

#[test]
fn marker_only_styled_span_between_plain_glyphs_resets_kerning() {
    // R4-2: right-aligned "A<span class='sc'>\u{00ad}</span>V" with A-V kerning enabled.
    // Segmentation splits A (span 0) and V (span 2) into two runs; measurement must
    // also reset kerning across the span change so V ends at the exact right edge.
    let mut font = StrictCaseFont { kern_av: true, ..StrictCaseFont::default() };
    let mut sink = AnchorCapture::default();
    let width = 200i16;
    let right_edge = width - 16;
    run_case_layout(
        &mut font,
        &mut sink,
        width,
        300,
        30,
        b"p{text-align:right} .sc{font-variant:small-caps}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(b"A");
            e.start(b"span", Some(b"sc"), None, None, None, None);
            e.text("\u{00ad}".as_bytes());
            e.end(b"span");
            e.text(b"V");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs = &sink.inner.pages[0].2;
    assert_eq!(runs.len(), 2, "expected separate A and V runs, got {runs:?}");
    assert_eq!(runs[0].0, b"A");
    assert_eq!(runs[1].0, b"V");
    let mut mf = StrictCaseFont { kern_av: true, ..StrictCaseFont::default() };
    assert_eq!(
        i32::from(runs[1].1) + painted_run_width(&mut mf, &runs[1]),
        i32::from(right_edge),
        "marker-only styled span allowed A-V kerning across emitted runs: {runs:?}"
    );
}

#[test]
fn leading_marker_only_margin_span_emits_no_empty_run_or_ghost_margin() {
    // R4-3: right-aligned "<span class='sc m'>\u{00ad}</span>B" at 30 px.
    // The invisible marker-only span must consume no run slot and must not add
    // its 30 px margin in placement when measurement skipped it.
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    let width = 200i16;
    let right_edge = width - 16;
    run_case_layout(
        &mut font,
        &mut sink,
        width,
        300,
        30,
        b"p{text-align:right} .sc{font-variant:small-caps} .m{margin-left:1em}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.start(b"span", Some(b"sc m"), None, None, None, None);
            e.text("\u{00ad}".as_bytes());
            e.end(b"span");
            e.text(b"B");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs = &sink.inner.pages[0].2;
    assert_eq!(runs.len(), 1, "leading marker-only span consumed a run slot: {runs:?}");
    assert_eq!(runs[0].0, b"B");
    let mut mf = StrictCaseFont::default();
    assert_eq!(
        i32::from(runs[0].1) + painted_run_width(&mut mf, &runs[0]),
        i32::from(right_edge),
        "leading marker-only span pushed B past right edge: {runs:?}"
    );
}

#[test]
fn justified_spaces_across_skipped_soft_hyphen_keep_gap_count_and_space_size() {
    // R5-1a: "A \u{00ad} B LONGWORD" at 60 px content width (width = 92) with
    // text-transform: uppercase; text-align: justify. Both spaces before and after
    // the skipped soft hyphen must contribute their advance and justification gap.
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    let w1 = 92i16; // content width = 60 px
    run_case_layout(
        &mut font,
        &mut sink,
        w1,
        300,
        30,
        b"p{text-transform:uppercase;text-align:justify}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text("A \u{00ad} B LONGWORD".as_bytes());
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs1 = &sink.inner.pages[0].2;
    let base1 = runs1[0].2;
    let line1: Vec<_> = runs1.iter().filter(|r| r.2 == base1).collect();
    let last1 = line1.last().unwrap();
    assert_eq!(last1.0, b"B", "expected line 1 to end at B, got {line1:?}");
    let mut mf = StrictCaseFont::default();
    assert_eq!(
        i32::from(last1.1) + painted_run_width(&mut mf, last1),
        i32::from(w1 - 16),
        "justified spaces separated by U+00AD lost a gap: {line1:?}"
    );

    // R5-1b: "A <span class='sc'>\u{00ad}</span><span class='big'>B</span> LONGWORD"
    // on a justified line. The space belongs to the 30 px span (7 px advance), NOT
    // the 60 px span (15 px advance) that follows the skipped marker.
    let mut font2 = StrictCaseFont::default();
    let mut sink2 = AnchorCapture::default();
    let w2 = 120i16; // content width = 88 px; A(15) + sp30(7) + B60(30) = 52 px
    run_case_layout(
        &mut font2,
        &mut sink2,
        w2,
        300,
        30,
        b"p{text-align:justify} .sc{font-variant:small-caps} .big{font-size:200%}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(b"A ");
            e.start(b"span", Some(b"sc"), None, None, None, None);
            e.text("\u{00ad}".as_bytes());
            e.end(b"span");
            e.start(b"span", Some(b"big"), None, None, None, None);
            e.text(b"B");
            e.end(b"span");
            e.text(b" LONGWORD");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs2 = &sink2.inner.pages[0].2;
    let base2 = runs2[0].2;
    let line2: Vec<_> = runs2.iter().filter(|r| r.2 == base2).collect();
    let last2 = line2.last().unwrap();
    assert_eq!(last2.0, b"B", "expected line 1 to end at 60px B, got {line2:?}");
    assert_eq!(last2.3, 60);
    assert_eq!(
        i32::from(last2.1) + painted_run_width(&mut mf, last2),
        i32::from(w2 - 16),
        "justified space before skipped marker was priced in B's 60 px span: {line2:?}"
    );
}

#[test]
fn plain_subpixel_margin_preserves_legacy_final_kerning_reset() {
    // R5-2: right-aligned plain "A<span style='margin-left:1%'>V</span>" with A-V
    // kerning enabled and no case styling. Stored margin=1 rounds to 0 px; legacy
    // final measurement still cleared prev on stored margin > 0 so V lands at right_edge.
    let mut font = StrictCaseFont { kern_av: true, ..StrictCaseFont::default() };
    let mut sink = AnchorCapture::default();
    let width = 200i16;
    let right_edge = width - 16;
    run_case_layout(
        &mut font,
        &mut sink,
        width,
        300,
        30,
        b"p{text-align:right}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(b"A");
            e.start(b"span", None, Some(b"margin-left:1%"), None, None, None);
            e.text(b"V");
            e.end(b"span");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs = &sink.inner.pages[0].2;
    assert_eq!(runs.len(), 2, "expected separate A and V runs, got {runs:?}");
    let mut mf = StrictCaseFont { kern_av: true, ..StrictCaseFont::default() };
    assert_eq!(
        i32::from(runs[1].1) + painted_run_width(&mut mf, &runs[1]),
        i32::from(right_edge),
        "plain subpixel margin lost legacy kerning reset: {runs:?}"
    );
}

#[test]
fn deferred_inline_margin_survives_exact_capacity_buffer_flush() {
    // R5-3: fill the bounded text buffer exactly through the complete U+00AD of
    // "<span class='sc'>aa</span><span class='m'>\u{00ad}b</span>" (text cap = 5
    // holds 4 bytes "aa\xc2\xad" + 1 NUL guard). Appending 'b' flushes buffer 1
    // with cut == end; buffer 2 must carry the deferred 30 px margin onto 'b'.
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
    with_text_capacity(&mut backend, 4096, 5, |e| {
        e.params.width = 200;
        e.params.height = 300;
        e.params.left = 16;
        e.params.right = 16;
        e.params.base_size = 30;
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, None, None, None, None);
        e.start(b"span", None, Some(b"font-variant:small-caps"), None, None, None);
        e.text(b"aa");
        e.end(b"span");
        e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
        e.text("\u{00ad}".as_bytes());
        e.text(b"b");
        e.end(b"span");
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
    });
    let runs = &sink.inner.pages[0].2;
    let b_run = runs.iter().find(|r| r.0 == b"b").expect("expected continuation run 'b'");
    assert_eq!(
        b_run.1,
        16 + 30,
        "deferred margin was lost across exact-capacity buffer flush: {runs:?}"
    );

    // R6-3: second marker-only capacity flush ("&shy;&shy;" in plain margin span)
    // before 'b' arrives must keep par_case active and keep carrying the 30 px margin.
    let mut font2 = StrictCaseFont::default();
    let mut sink2 = AnchorCapture::default();
    let mut backend2 = SplitBackend { font: &mut font2, sink: &mut sink2 };
    with_text_capacity(&mut backend2, 4096, 5, |e| {
        e.params.width = 200;
        e.params.height = 300;
        e.params.left = 16;
        e.params.right = 16;
        e.params.base_size = 30;
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, None, None, None, None);
        e.start(b"span", None, Some(b"font-variant:small-caps"), None, None, None);
        e.text(b"aa");
        e.end(b"span");
        e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
        e.text("\u{00ad}\u{00ad}\u{00ad}".as_bytes());
        e.text(b"b");
        e.end(b"span");
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
    });
    let runs2 = &sink2.inner.pages[0].2;
    let b_run2 = runs2.iter().find(|r| r.0 == b"b").expect("expected multi-flush run 'b'");
    assert_eq!(
        b_run2.1,
        16 + 30,
        "deferred margin was lost across second marker-only capacity flush: {runs2:?}"
    );

    // R7-2: span opening with margin-left:1em right at the exact capacity boundary
    // (start == cut, zero bytes consumed before flush) must carry its margin onto 'b'.
    let mut font3 = StrictCaseFont::default();
    let mut sink3 = AnchorCapture::default();
    let mut backend3 = SplitBackend { font: &mut font3, sink: &mut sink3 };
    with_text_capacity(&mut backend3, 4096, 5, |e| {
        e.params.width = 200;
        e.params.height = 300;
        e.params.left = 16;
        e.params.right = 16;
        e.params.base_size = 30;
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, None, None, None, None);
        e.start(b"span", None, Some(b"font-variant:small-caps"), None, None, None);
        e.text(b"aaaa");
        e.end(b"span");
        e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
        e.text(b"b");
        e.end(b"span");
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
    });
    let runs3 = &sink3.inner.pages[0].2;
    let b_run3 = runs3.iter().find(|r| r.0 == b"b").expect("expected boundary-span run 'b'");
    assert_eq!(
        b_run3.1,
        16 + 30,
        "margin on span starting at exact capacity boundary was lost: {runs3:?}"
    );
}

#[test]
fn rtl_justified_case_paragraph_places_gap_between_reordered_words() {
    // R6-1: 60 px content width (width = 92), uppercase + justify, "א ב LONGWORD".
    // Line 1 measures 37 px (15 + 7 + 15), leaving 23 px spare for the single gap
    // (30 px total separation). Visual order is bet (left) then alef (right edge).
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    let width = 92i16; // content width = 60 px
    run_case_layout(
        &mut font,
        &mut sink,
        width,
        300,
        30,
        b"p{text-transform:uppercase;text-align:justify}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text("א ב LONGWORD".as_bytes());
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs = &sink.inner.pages[0].2;
    let base = runs[0].2;
    let line1: Vec<_> = runs.iter().filter(|r| r.2 == base).collect();
    assert_eq!(line1.len(), 2, "expected reordered bet and alef on line 1, got {line1:?}");
    assert_eq!(line1[0].0, "ב".as_bytes());
    assert_eq!(line1[1].0, "א".as_bytes());
    assert_eq!(line1[0].1, 16, "bet should start at left margin: {line1:?}");
    assert_eq!(line1[1].1 - (line1[0].1 + 15), 30, "expected 30 px visual gap between bet and alef: {line1:?}");
    let mut mf = StrictCaseFont::default();
    assert_eq!(
        i32::from(line1[1].1) + painted_run_width(&mut mf, line1[1]),
        i32::from(width - 16),
        "RTL justified line drifted from right edge: {line1:?}"
    );
}

#[test]
fn discretionary_hyphen_after_justified_space_paints_at_right_edge() {
    // R6-2: uppercase + justify, 34 px content width (width = 66), "A \u{00ad}BBBB".
    // A(15) + space(7) + '-'(10) = 32 px <= 34 px (2 px expansion). Line 1 must
    // preserve the chosen hyphen after the justified space at the exact right edge.
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    let width = 66i16; // content width = 34 px
    run_case_layout(
        &mut font,
        &mut sink,
        width,
        300,
        30,
        b"p{text-transform:uppercase;text-align:justify}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text("A \u{00ad}BBBB".as_bytes());
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs = &sink.inner.pages[0].2;
    let base = runs[0].2;
    let line1: Vec<_> = runs.iter().filter(|r| r.2 == base).collect();
    assert_eq!(line1.len(), 2, "expected A and '-' runs on line 1, got {line1:?}");
    assert_eq!(line1[0].0, b"A");
    assert_eq!(line1[1].0, b"-");
    let mut mf = StrictCaseFont::default();
    assert_eq!(
        i32::from(line1[1].1) + painted_run_width(&mut mf, line1[1]),
        i32::from(width - 16),
        "discretionary hyphen after justified space missed right edge: {line1:?}"
    );
}

#[test]
fn rtl_mixed_direction_justified_line_reorders_gaps_with_boundaries() {
    // R7-1: 65 px content width (width = 97), uppercase + justify, "אa b LONGWORD".
    // Line 1 measures 52 px (15 + 15 + 7 + 15 => 13 px spare => 20 px total gap).
    // Visual order is A (16..31), 20 px gap, B (51..66), alef (66..81).
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    let width = 97i16; // content width = 65 px
    run_case_layout(
        &mut font,
        &mut sink,
        width,
        300,
        30,
        b"p{text-transform:uppercase;text-align:justify}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text("אa b LONGWORD".as_bytes());
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs = &sink.inner.pages[0].2;
    let base = runs[0].2;
    let line1: Vec<_> = runs.iter().filter(|r| r.2 == base).collect();
    assert_eq!(line1.len(), 3, "expected visual A, B, alef on line 1, got {line1:?}");
    assert_eq!(line1[0].0, b"A");
    assert_eq!(line1[1].0, b"B");
    assert_eq!(line1[2].0, "א".as_bytes());
    assert_eq!(line1[0].1, 16, "A x coordinate: {line1:?}");
    assert_eq!(line1[1].1, 51, "B must sit after the 20 px gap (16 + 15 + 20 = 51): {line1:?}");
    assert_eq!(line1[2].1, 66, "alef must sit immediately after B (51 + 15 = 66): {line1:?}");
    assert_eq!(line1[2].1 + 15, width - 16);
}

#[test]
fn body_inline_case_style_inherits_into_children_and_resets() {
    // R7-3: <body style="text-transform:uppercase"><p>abc <span style="text-transform:none">def</span></p></body>
    // works even without an external stylesheet, and explicit child resets override it.
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    run_case_layout(&mut font, &mut sink, 300, 300, 24, b"", |e| {
        e.start(b"body", None, Some(b"text-transform:uppercase"), None, None, None);
        e.start(b"p", None, None, None, None, None);
        e.text(b"abc ");
        e.start(b"span", None, Some(b"text-transform:none"), None, None, None);
        e.text(b"def");
        e.end(b"span");
        e.end(b"p");
        e.end(b"body");
    });
    let runs = &sink.inner.pages[0].2;
    let rendered: String = runs
        .iter()
        .map(|r| core::str::from_utf8(&r.0).unwrap())
        .collect();
    assert_eq!(rendered, "ABC def", "body inline text-transform failed to inherit/reset: {runs:?}");
}

#[test]
fn rtl_case_soft_hyphen_keeps_opportunity_and_matches_painted_advance() {
    // R8-1: uppercase RTL paragraph "א aa\u{00ad}bbbb" at 58 px content width (width = 90).
    // Placement suppresses trailing hyphens on RTL lines, so measurement, break
    // admission, and final line width must all price the suppressed hyphen at 0 px:
    // line 1 keeps "AA" and "א" (15 + 7 + 30 = 52 px <= 58 px) and aligns its
    // 52 px advance cleanly against the right edge (default RTL right-alignment).
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    let width = 90i16; // content width = 58 px
    run_case_layout(
        &mut font,
        &mut sink,
        width,
        300,
        30,
        b"p{text-transform:uppercase}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text("א aa\u{00ad}bbbb".as_bytes());
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs = &sink.inner.pages[0].2;
    let base1 = runs[0].2;
    let line1: Vec<_> = runs.iter().filter(|r| r.2 == base1).collect();
    assert_eq!(line1.len(), 2, "expected AA and ' א' on line 1 (no early wrap), got {line1:?}");
    assert_eq!(line1[0].0, b"AA");
    // Unjustified RTL run reverses "א " into visual " א" (space 7 px + alef 15 px = 22 px).
    assert_eq!(line1[1].0, " א".as_bytes());
    // Default left alignment flips to right alignment for RTL: x_start = 16 + (58 - 52) = 22.
    assert_eq!(line1[0].1, 22, "measured advance must match 52 px painted width: {line1:?}");
    assert_eq!(line1[1].1, 22 + 30, "' א' must start immediately after AA: {line1:?}");
    let mut mf = StrictCaseFont::default();
    assert_eq!(
        i32::from(line1[1].1) + painted_run_width(&mut mf, line1[1]),
        i32::from(width - 16),
        "RTL line 1 drifted from right edge: {line1:?}"
    );
}

#[test]
fn rtl_justified_space_before_suppressed_soft_hyphen_and_segment_cap() {
    // R9-1 part A: justified uppercase RTL "א ב \u{00ad}BBBB" at 50 px content width
    // (width = 82, left = 16, right = 16). Placement suppresses trailing RTL hyphens,
    // so trailing whitespace before the suppressed marker is trimmed consistently:
    // bet starts at x=16, alef starts at x=51 and ends flush at x=66 without a hyphen.
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    let width = 82i16; // content width = 50 px
    run_case_layout(
        &mut font,
        &mut sink,
        width,
        300,
        30,
        b"p{text-transform:uppercase;text-align:justify}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text("א ב \u{00ad}BBBB".as_bytes());
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs = &sink.inner.pages[0].2;
    let base1 = runs[0].2;
    let line1: Vec<_> = runs.iter().filter(|r| r.2 == base1).collect();
    assert_eq!(line1.len(), 2, "expected bet and alef on line 1, got {line1:?}");
    assert_eq!(line1[0].0, "ב".as_bytes());
    assert_eq!(line1[1].0, "א".as_bytes());
    assert_eq!(line1[0].1, 16, "bet must start at left edge: {line1:?}");
    assert_eq!(line1[1].1, 51, "alef must sit flush at right edge: {line1:?}");
    let mut mf = StrictCaseFont::default();
    assert_eq!(
        i32::from(line1[1].1) + painted_run_width(&mut mf, line1[1]),
        66,
        "alef right edge must hit 66 px: {line1:?}"
    );
    let line2: Vec<_> = runs.iter().filter(|r| r.2 > base1).collect();
    let tail: String = line2
        .iter()
        .map(|r| core::str::from_utf8(&r.0).unwrap())
        .collect();
    assert_eq!(tail, "BBBB");

    // R9-1 / R10-1 part B: LTR "a b \u{00ad} ccccc" at 50 px wraps before the marker,
    // discarding leading marker + space so CCC sits directly on row 2 (baseline + lh,
    // no blank row); at 53 px (width = 85), the post-marker space fits, forcing the
    // ordinary-line trailing trim to strip " \u{00ad} " so B hits exact right edge 69 px
    // and CCC sits directly on row 2.
    let lh_30 = font.line_height(30);
    for (w, expected_right) in [(width, 66i32), (85i16, 69i32)] {
        let mut font_ltr = StrictCaseFont::default();
        let mut sink_ltr = AnchorCapture::default();
        run_case_layout(
            &mut font_ltr,
            &mut sink_ltr,
            w,
            300,
            30,
            b"p{text-transform:uppercase;text-align:justify}",
            |e| {
                e.start(b"body", None, None, None, None, None);
                e.start(b"p", None, None, None, None, None);
                e.text("a b \u{00ad} ccccc".as_bytes());
                e.end(b"p");
                e.end(b"body");
            },
        );
        let ltr_runs = &sink_ltr.inner.pages[0].2;
        let ltr_base1 = ltr_runs[0].2;
        let ltr_line1: Vec<_> = ltr_runs.iter().filter(|r| r.2 == ltr_base1).collect();
        assert_eq!(ltr_line1.len(), 2, "width {w}: expected A and B on line 1, got {ltr_line1:?}");
        assert_eq!(ltr_line1[0].0, b"A");
        assert_eq!(ltr_line1[1].0, b"B");
        let mut mf_ltr = StrictCaseFont::default();
        assert_eq!(
            i32::from(ltr_line1[1].1) + painted_run_width(&mut mf_ltr, ltr_line1[1]),
            expected_right,
            "width {w}: LTR justified B must hit right edge {expected_right}: {ltr_line1:?}"
        );
        let ltr_after: Vec<_> = ltr_runs.iter().filter(|r| r.2 > ltr_base1).collect();
        assert_eq!(
            ltr_after[0].2 - ltr_base1,
            lh_30,
            "width {w}: first C must sit directly on row 2 without a blank row: {ltr_runs:?}"
        );
        assert_eq!(ltr_after[0].0, b"CCC");
        assert_eq!(ltr_after[0].1, 16, "width {w}: CCC must start at left margin 16: {ltr_runs:?}");
    }

    // R9-1 part C: 64 justified RTL words followed by " \u{00ad}BBBB" must fit on one row
    // and place BBBB on the immediately following row with no blank zero-segment row.
    let mut font_cap = StrictCaseFont::default();
    let mut sink_cap = AnchorCapture::default();
    let mut sixty_four = String::new();
    for i in 0..64 {
        if i > 0 {
            sixty_four.push(' ');
        }
        sixty_four.push('א');
    }
    sixty_four.push_str(" \u{00ad}BBBB");
    // 64 * 15 + 64 * 7 = 1408 px; width = 1480 => content width 1448 px, so the space +
    // suppressed marker fit, while BBBB (60 px) overflows onto row 2.
    let wide = 1480i16;
    run_case_layout(
        &mut font_cap,
        &mut sink_cap,
        wide,
        300,
        30,
        b"p{text-transform:uppercase;text-align:justify}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(sixty_four.as_bytes());
            e.end(b"p");
            e.end(b"body");
        },
    );
    let cap_runs = &sink_cap.inner.pages[0].2;
    let cap_base1 = cap_runs[0].2;
    let cap_line1: Vec<_> = cap_runs.iter().filter(|r| r.2 == cap_base1).collect();
    let cap_line2: Vec<_> = cap_runs.iter().filter(|r| r.2 > cap_base1).collect();
    assert_eq!(cap_line1.len(), 64, "all 64 RTL words stay on row 1");
    assert_eq!(cap_line2.len(), 1, "only BBBB on row 2, got {cap_line2:?}");
    assert_eq!(cap_line2[0].0, b"BBBB");
    let lh = font_cap.line_height(30);
    assert_eq!(
        cap_line2[0].2 - cap_base1,
        lh,
        "segment-cap split must not spend a blank row on a suppressed soft hyphen"
    );

    // R9-1 part D: 64 justified RTL words + " \u{00ad} ב BBBB" forces a 64-segment split
    // right before an interior soft hyphen and space; the continuation skips both
    // without a leading space or blank row, and BBBB follows on row 3.
    let mut font_split = StrictCaseFont::default();
    let mut sink_split = AnchorCapture::default();
    let mut sixty_five = String::new();
    for i in 0..64 {
        if i > 0 {
            sixty_five.push(' ');
        }
        sixty_five.push('א');
    }
    sixty_five.push_str(" \u{00ad} ב BBBB");
    run_case_layout(
        &mut font_split,
        &mut sink_split,
        wide,
        300,
        30,
        b"p{text-transform:uppercase;text-align:justify}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(sixty_five.as_bytes());
            e.end(b"p");
            e.end(b"body");
        },
    );
    let split_runs = &sink_split.inner.pages[0].2;
    let split_base1 = split_runs[0].2;
    let split_line1: Vec<_> = split_runs.iter().filter(|r| r.2 == split_base1).collect();
    let split_line2: Vec<_> = split_runs
        .iter()
        .filter(|r| r.2 == split_base1 + lh)
        .collect();
    let split_line3: Vec<_> = split_runs
        .iter()
        .filter(|r| r.2 == split_base1 + 2 * lh)
        .collect();
    assert_eq!(split_line1.len(), 64);
    assert_eq!(split_line2.len(), 1, "expected only bet on continuation row: {split_line2:?}");
    assert_eq!(split_line2[0].0, "ב".as_bytes());
    assert_eq!(
        split_line2[0].1, 16,
        "bet must start at left margin 16 with no leading space: {split_line2:?}"
    );
    assert_eq!(split_line3.len(), 1);
    assert_eq!(split_line3[0].0, b"BBBB");
}

#[test]
fn plain_body_inline_style_and_full_buffer_margin_preserve_parity() {
    // R9-2a: without case declarations, inline styles on <body> (e.g. font-size:200%)
    // must keep pre-#43 parity both without a stylesheet and with a stylesheet.
    for css_bytes in [b"".as_slice(), b"p{text-align:left}".as_slice()] {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        run_case_layout(&mut font, &mut sink, 300, 300, 30, css_bytes, |e| {
            e.start(b"body", None, Some(b"font-size:200%"), None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text("א".as_bytes());
            e.end(b"p");
            e.end(b"body");
        });
        let runs = &sink.inner.pages[0].2;
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].3, 30, "plain body inline font-size must not change run size");
    }
    // With a case declaration alongside font-size on <body>, only text_case applies.
    let mut font_case = StrictCaseFont::default();
    let mut sink_case = AnchorCapture::default();
    run_case_layout(&mut font_case, &mut sink_case, 300, 300, 30, b"", |e| {
        e.start(
            b"body",
            None,
            Some(b"font-size:200%;text-transform:uppercase"),
            None,
            None,
            None,
        );
        e.start(b"p", None, None, None, None, None);
        e.text(b"abc");
        e.end(b"p");
        e.end(b"body");
    });
    let case_runs = &sink_case.inner.pages[0].2;
    assert_eq!(case_runs.len(), 1);
    assert_eq!(case_runs[0].0, b"ABC");
    assert_eq!(case_runs[0].3, 30, "body inline font-size remains untouched alongside text_case");

    // R9-2b: plain RTL paragraph aligned right in a 5-byte text buffer with a margin span
    // starting at the exact cut preserves pre-#43 parity (gimel at x=16, not x=46).
    let mut font_buf = StrictCaseFont::default();
    let mut sink_buf = AnchorCapture::default();
    let mut backend_buf = SplitBackend {
        font: &mut font_buf,
        sink: &mut sink_buf,
    };
    with_text_capacity(&mut backend_buf, 4096, 5, |e| {
        e.params.width = 300;
        e.params.height = 300;
        e.params.left = 16;
        e.params.right = 16;
        e.params.base_size = 30;
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, Some(b"text-align:right"), None, None, None);
        e.text("אב".as_bytes()); // 4 UTF-8 payload bytes fills the 5-byte buffer (cap = len - 1)
        e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
        e.text("ג".as_bytes());
        e.end(b"span");
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
    });
    let buf_runs = &sink_buf.inner.pages[0].2;
    let gimel = buf_runs
        .iter()
        .find(|r| r.0 == "ג".as_bytes())
        .expect("missing gimel run");
    assert_eq!(
        gimel.1, 16,
        "plain full-buffer cut must preserve pre-#43 margin parity (x=16): {buf_runs:?}"
    );
}

#[test]
fn embedded_body_case_style_cascades_and_preserves_plain_parity() {
    // R10-2: default core Params (stylesheet: None, embedded_styles: true) collects
    // <head><style>body{...}</style></head> into chapter_css. Case rules on <body>
    // must cascade between supplied stylesheet and inline overrides while non-case
    // embedded body declarations (like font-size) preserve pre-#43 parity.
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    run_case_layout(&mut font, &mut sink, 300, 300, 30, b"", |e| {
        e.start(b"head", None, None, None, None, None);
        e.start(b"style", None, None, None, None, None);
        e.text(b"body{font-size:200%;text-transform:uppercase}");
        e.end(b"style");
        e.end(b"head");
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, None, None, None, None);
        e.text(b"abc");
        e.end(b"p");
        e.end(b"body");
    });
    let runs = &sink.inner.pages[0].2;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].0, b"ABC", "embedded <style> body case rule must apply: {runs:?}");
    assert_eq!(runs[0].3, 30, "embedded <style> body font-size must preserve plain parity");

    // Precedence: external stylesheet < embedded <style> < inline body override.
    // External sets uppercase + small-caps, embedded flips transform to lowercase,
    // and inline resets font-variant to normal so AB paints as 30 px lowercase "ab"
    // (distinguishing both overrides independently).
    let mut font2 = StrictCaseFont::default();
    let mut sink2 = AnchorCapture::default();
    run_case_layout(
        &mut font2,
        &mut sink2,
        300,
        300,
        30,
        b"body{text-transform:uppercase;font-variant:small-caps}",
        |e| {
            e.start(b"head", None, None, None, None, None);
            e.start(b"style", None, None, None, None, None);
            e.text(b"body{text-transform:lowercase}");
            e.end(b"style");
            e.end(b"head");
            e.start(b"body", None, Some(b"font-variant:normal"), None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(b"AB");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs2 = &sink2.inner.pages[0].2;
    assert_eq!(runs2.len(), 1);
    assert_eq!(runs2[0].0, b"ab", "embedded lowercase must override external uppercase");
    assert_eq!(runs2[0].3, 30, "inline font-variant:normal must override external small-caps");

    // Plain embedded <style>body{font-size:200%}</style> without case declarations stays inert.
    let mut font3 = StrictCaseFont::default();
    let mut sink3 = AnchorCapture::default();
    run_case_layout(&mut font3, &mut sink3, 300, 300, 30, b"", |e| {
        e.start(b"head", None, None, None, None, None);
        e.start(b"style", None, None, None, None, None);
        e.text(b"body{font-size:200%}");
        e.end(b"style");
        e.end(b"head");
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, None, None, None, None);
        e.text(b"abc");
        e.end(b"p");
        e.end(b"body");
    });
    let runs3 = &sink3.inner.pages[0].2;
    assert_eq!(runs3.len(), 1);
    assert_eq!(runs3[0].0, b"abc");
    assert_eq!(runs3[0].3, 30);
}

#[test]
fn emergency_wrap_after_br_skips_invisible_prefix_while_double_br_keeps_blank_row() {
    // R11-1: 52 px viewport with 16 px margins (20 px content), uppercase "a<br/>&shy; c".
    // Space (7) + C (15) overflows 20 px, so emergency wrap must discard the invisible
    // "&shy; " prefix without spending a blank row (C lands 36 px below A, not 72 px).
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    let lh = font.line_height(30);
    run_case_layout(
        &mut font,
        &mut sink,
        52,
        300,
        30,
        b"p{text-transform:uppercase}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(b"a");
            e.start(b"br", None, None, None, None, None);
            e.end(b"br");
            e.text("\u{00ad} c".as_bytes());
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs = &sink.inner.pages[0].2;
    assert_eq!(runs.len(), 2, "expected A and C runs, got {runs:?}");
    assert_eq!(runs[0].0, b"A");
    assert_eq!(runs[1].0, b"C");
    assert_eq!(
        runs[1].2 - runs[0].2,
        lh,
        "single <br/> followed by '&shy; c' must not insert a blank row: {runs:?}"
    );
    assert_eq!(runs[1].1, 16);

    // Control: two consecutive explicit <br/> tags preserve an intentional blank row (2 * lh).
    let mut font2 = StrictCaseFont::default();
    let mut sink2 = AnchorCapture::default();
    run_case_layout(
        &mut font2,
        &mut sink2,
        52,
        300,
        30,
        b"p{text-transform:uppercase}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(b"a");
            e.start(b"br", None, None, None, None, None);
            e.end(b"br");
            e.start(b"br", None, None, None, None, None);
            e.end(b"br");
            e.text(b"c");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs2 = &sink2.inner.pages[0].2;
    assert_eq!(runs2.len(), 2);
    assert_eq!(runs2[0].0, b"A");
    assert_eq!(runs2[1].0, b"C");
    assert_eq!(
        runs2[1].2 - runs2[0].2,
        2 * lh,
        "double <br/><br/> must retain its explicit blank row: {runs2:?}"
    );
}

#[test]
fn point_margin_soft_hyphen_preserves_spaces_children_wraps_and_many_spans() {
    // R14-1: pending space inside closed margin span ("a<span style='margin-left:1em'>&shy; </span>b")
    // keeps its 30 px margin: A(15) at 16, margin(30) + " B"(7+15) at 61 => B at 68.
    let mut font1 = StrictCaseFont::default();
    let mut sink1 = AnchorCapture::default();
    run_case_layout(
        &mut font1,
        &mut sink1,
        300,
        300,
        30,
        b"p{text-transform:uppercase;text-align:left}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(b"a");
            e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
            e.text("\u{00ad} ".as_bytes());
            e.end(b"span");
            e.text(b"b");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs1 = &sink1.inner.pages[0].2;
    assert_eq!(runs1.len(), 2, "expected A and ' B' runs, got {runs1:?}");
    assert_eq!(runs1[0].0, b"A");
    assert_eq!(runs1[0].1, 16);
    assert_eq!(runs1[1].0, b" B");
    assert_eq!(runs1[1].1, 61, "' B' must start at 16+15+30=61 (B visually at 68): {runs1:?}");

    // R14-2: styled child inside margin span starting with marker ("a<span style='margin-left:1em'>&shy;<b>b</b></span>")
    // preserves parent's 30 px margin across the style change: B starts at x=61.
    let mut font2 = StrictCaseFont::default();
    let mut sink2 = AnchorCapture::default();
    run_case_layout(
        &mut font2,
        &mut sink2,
        300,
        300,
        30,
        b"p{text-transform:uppercase;text-align:left}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(b"a");
            e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
            e.text("\u{00ad}".as_bytes());
            e.start(b"b", None, None, None, None, None);
            e.text(b"b");
            e.end(b"b");
            e.end(b"span");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs2 = &sink2.inner.pages[0].2;
    let b_run2 = runs2.iter().find(|r| r.0 == b"B").expect("missing bold B run");
    assert_eq!(b_run2.1, 61, "styled child B must start at x=61 (16+15+30): {runs2:?}");

    // R13-3: materialized inner space ("a<span style='margin-left:1em'>&shy; &shy;</span><b>b</b>")
    // retains 30 px margin: A(15) + margin(30) + space(7) => B at x=68.
    let mut font3 = StrictCaseFont::default();
    let mut sink3 = AnchorCapture::default();
    run_case_layout(
        &mut font3,
        &mut sink3,
        300,
        300,
        30,
        b"p{text-transform:uppercase;text-align:left}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(b"a");
            e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
            e.text("\u{00ad} \u{00ad}".as_bytes());
            e.end(b"span");
            e.start(b"b", None, None, None, None, None);
            e.text(b"b");
            e.end(b"b");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs3 = &sink3.inner.pages[0].2;
    let b_run3 = runs3.iter().find(|r| r.0 == b"B").expect("missing bold B run");
    assert_eq!(b_run3.1, 68, "materialized space + margin must place B at x=68: {runs3:?}");

    // R14-3: 12 repeated "<span style='margin-left:1em'>&shy;</span>c" spans in one
    // right-aligned line have no fixed 8-entry bookkeeping cap; measurement and placement
    // match to the pixel for every span (including 9th..12th).
    let mut font4 = StrictCaseFont::default();
    let mut sink4 = AnchorCapture::default();
    let wide = 800i16;
    run_case_layout(
        &mut font4,
        &mut sink4,
        wide,
        300,
        30,
        b"p{text-transform:uppercase;text-align:right}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(b"a");
            for _ in 0..12 {
                e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
                e.text("\u{00ad}".as_bytes());
                e.end(b"span");
                e.text(b"c");
            }
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs4 = &sink4.inner.pages[0].2;
    assert_eq!(runs4.len(), 13, "expected A + 12 C runs, got {}", runs4.len());
    let mut mf4 = StrictCaseFont::default();
    for k in 1..13 {
        assert_eq!(
            runs4[k].1 - (runs4[k - 1].1 + 15),
            30,
            "span {k} lost 30 px margin gap: {runs4:?}"
        );
    }
    assert_eq!(
        i32::from(runs4[12].1) + painted_run_width(&mut mf4, &runs4[12]),
        i32::from(wide - 16),
        "12-span line drifted from right edge: {runs4:?}"
    );

    // Mixed-script + margin-bearing soft hyphen ("漢<span style='margin-left:1em'>&shy;</span>a")
    // preserves both the 30 px margin and the 7 px CJK-Latin script gap (15+30+7+15 = 67 px).
    let mut font5 = StrictCaseFont::default();
    let mut sink5 = AnchorCapture::default();
    run_case_layout(
        &mut font5,
        &mut sink5,
        200,
        300,
        30,
        b"p{text-transform:uppercase;text-align:right}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text("漢".as_bytes());
            e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
            e.text("\u{00ad}".as_bytes());
            e.end(b"span");
            e.text(b"a");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs5 = &sink5.inner.pages[0].2;
    assert_eq!(runs5.len(), 2);
    assert_eq!(runs5[1].1 - (runs5[0].1 + 15), 37, "expected 30 px margin + 7 px script gap: {runs5:?}");
    let mut mf5 = StrictCaseFont::default();
    assert_eq!(
        i32::from(runs5[1].1) + painted_run_width(&mut mf5, &runs5[1]),
        200 - 16,
        "mixed-script margin-bearing soft hyphen drifted from right edge: {runs5:?}"
    );

    // Wrap skip discards margin-0 soft hyphen but stops at margin-bearing soft hyphen:
    // "a b &shy;<span style='margin-left:1em'>&shy;c</span>" at 50 px puts C on row 2 at x=46.
    let mut font6 = StrictCaseFont::default();
    let mut sink6 = AnchorCapture::default();
    let lh = font6.line_height(30);
    run_case_layout(
        &mut font6,
        &mut sink6,
        82,
        300,
        30,
        b"p{text-transform:uppercase;text-align:left}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text("a b \u{00ad}".as_bytes());
            e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
            e.text("\u{00ad}c".as_bytes());
            e.end(b"span");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs6 = &sink6.inner.pages[0].2;
    let base1_6 = runs6[0].2;
    let c_run6 = runs6.iter().find(|r| r.0 == b"C").expect("missing C run");
    assert_eq!(c_run6.2 - base1_6, lh, "C must sit on row 2: {runs6:?}");
    assert_eq!(c_run6.1, 46, "wrap skip must stop at margin-bearing marker so C lands at x=46: {runs6:?}");
}

#[test]
fn exact_buffer_cut_after_discarded_space_carries_surviving_margin() {
    // R12-2: 5-byte text buffer (4 usable bytes), uppercase "a<span style='margin-left:1em'>&shy; c</span>".
    // Chunk 1 holds "a\u{00ad} " and trims "&shy; " on flush; the open margin span was
    // never consumed in chunk 1, so C on chunk 2 must start at x=46 (16 + 30).
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    let mut backend = SplitBackend {
        font: &mut font,
        sink: &mut sink,
    };
    with_text_capacity(&mut backend, 4096, 5, |e| {
        e.params.width = 300;
        e.params.height = 300;
        e.params.left = 16;
        e.params.right = 16;
        e.params.base_size = 30;
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, Some(b"text-transform:uppercase;text-align:left"), None, None, None);
        e.text(b"a");
        e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
        e.text("\u{00ad} c".as_bytes());
        e.end(b"span");
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
    });
    let runs = &sink.inner.pages[0].2;
    let c_run = runs.iter().find(|r| r.0 == b"C").expect("missing C run");
    assert_eq!(
        c_run.1, 46,
        "surviving margin across discarded-space exact cut must place C at x=46: {runs:?}"
    );
}

#[test]
fn refused_64_segment_line_retries_identically_to_fresh_page() {
    // R12-3: 330-byte page pool, 128-byte text buffer, 1000 px content width (width=1032).
    // A right-aligned small-caps paragraph of 32x "aA" + <span style="margin-left:1em"> c</span>
    // hits 64 segments before the space. When a prior 10-letter run forces pool refusal and
    // page-top retry, the retried row must match a fresh-page placement byte-for-byte and
    // x-for-x (no 7 px shift from mutating span.start before refusal).
    let run_scenario = |with_prior: bool| -> Vec<(Vec<u8>, i16)> {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        let mut backend = SplitBackend {
            font: &mut font,
            sink: &mut sink,
        };
        let mut body_text = String::new();
        for _ in 0..32 {
            body_text.push_str("aA");
        }
        with_text_capacity(&mut backend, 330, 128, |e| {
            e.params.width = 1032;
            e.params.height = 400;
            e.params.left = 16;
            e.params.right = 16;
            e.params.base_size = 30;
            e.start(b"body", None, None, None, None, None);
            if with_prior {
                e.start(b"p", None, None, None, None, None);
                e.text(b"0123456789");
                e.end(b"p");
            }
            e.start(
                b"p",
                None,
                Some(b"font-variant:small-caps;text-align:right"),
                None,
                None,
                None,
            );
            e.text(body_text.as_bytes());
            e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
            e.text(b" c");
            e.end(b"span");
            e.end(b"p");
            e.end(b"body");
            assert!(e.finish());
        });
        let page = if with_prior {
            assert_eq!(sink.inner.pages.len(), 2, "expected refusal onto page 2");
            &sink.inner.pages[1]
        } else {
            assert_eq!(sink.inner.pages.len(), 1);
            &sink.inner.pages[0]
        };
        let base1 = page.2[0].2;
        page.2
            .iter()
            .filter(|r| r.2 == base1)
            .map(|r| (r.0.clone(), r.1))
            .collect()
    };
    let fresh = run_scenario(false);
    let retried = run_scenario(true);
    assert_eq!(fresh.len(), 64);
    assert_eq!(
        retried, fresh,
        "retried 64-segment row drifted from fresh-page placement"
    );
}

#[test]
fn buffer_cut_before_styled_child_carries_parent_margin_on_exact_and_partial_utf8() {
    // R15-1a: 4-byte text buffer (3 usable), uppercase "a<span style='margin-left:1em'>&shy;<b>b</b></span>".
    // Chunk 1 holds "a\u{00ad}" with bold span opening at offset 3 (margin 0); the cut
    // trims the marker back to offset 1 and must carry spans[1]'s unplaced 30 px margin
    // onto bold B (x = 46).
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    let mut backend = SplitBackend {
        font: &mut font,
        sink: &mut sink,
    };
    with_text_capacity(&mut backend, 4096, 4, |e| {
        e.params.width = 300;
        e.params.height = 300;
        e.params.left = 16;
        e.params.right = 16;
        e.params.base_size = 30;
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, Some(b"text-transform:uppercase;text-align:left"), None, None, None);
        e.text(b"a");
        e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
        e.text("\u{00ad}".as_bytes());
        e.start(b"b", None, None, None, None, None);
        e.text(b"b");
        e.end(b"b");
        e.end(b"span");
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
    });
    let runs = &sink.inner.pages[0].2;
    let b_run = runs.iter().find(|r| r.0 == b"B").expect("missing bold B run");
    assert_eq!(b_run.1, 46, "exact cut before bold child lost parent 30 px margin: {runs:?}");

    // R15-1b: 5-byte text buffer (4 usable) with multi-byte bold "é" straddling byte 3:
    // UTF-8 rewind cuts at 3 (bold span, margin 0) and must still carry spans[1]'s 30 px
    // margin onto bold É (x = 46).
    let mut font2 = StrictCaseFont::default();
    let mut sink2 = AnchorCapture::default();
    let mut backend2 = SplitBackend {
        font: &mut font2,
        sink: &mut sink2,
    };
    with_text_capacity(&mut backend2, 4096, 5, |e| {
        e.params.width = 300;
        e.params.height = 300;
        e.params.left = 16;
        e.params.right = 16;
        e.params.base_size = 30;
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, Some(b"text-transform:uppercase;text-align:left"), None, None, None);
        e.text(b"a");
        e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
        e.text("\u{00ad}".as_bytes());
        e.start(b"b", None, None, None, None, None);
        e.text("é".as_bytes());
        e.end(b"b");
        e.end(b"span");
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
    });
    let runs2 = &sink2.inner.pages[0].2;
    let e_run = runs2
        .iter()
        .find(|r| r.0 == "É".as_bytes())
        .expect("missing bold É run");
    assert_eq!(e_run.1, 46, "partial UTF-8 cut before bold child lost parent 30 px margin: {runs2:?}");
}

#[test]
fn rtl_unbroken_soft_hyphen_keeps_physical_left_margin_identical_to_plain() {
    // R15-2: compare "א<span style='margin-left:1em'>ב</span>" with
    // "א<span style='margin-left:1em'>&shy;ב</span>" at width 200, text-align:left,
    // uppercase. Both must place bet at x=154 and alef at x=169 (physical left margin
    // stays before bet, never reordered after bet as an inter-word gap).
    let layout_rtl = |with_shy: bool| -> Vec<(Vec<u8>, i16)> {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        run_case_layout(
            &mut font,
            &mut sink,
            200,
            300,
            30,
            b"p{text-align:left;text-transform:uppercase}",
            |e| {
                e.start(b"body", None, None, None, None, None);
                e.start(b"p", None, None, None, None, None);
                e.text("א".as_bytes());
                e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
                e.text(if with_shy { "\u{00ad}ב".as_bytes() } else { "ב".as_bytes() });
                e.end(b"span");
                e.end(b"p");
                e.end(b"body");
            },
        );
        sink.inner.pages[0]
            .2
            .iter()
            .map(|r| (r.0.clone(), r.1))
            .collect()
    };
    let without_shy = layout_rtl(false);
    let with_shy = layout_rtl(true);
    assert_eq!(
        without_shy,
        vec![("ב".as_bytes().to_vec(), 154), ("א".as_bytes().to_vec(), 169)]
    );
    assert_eq!(
        with_shy, without_shy,
        "unbroken soft hyphen moved RTL physical left margin"
    );
}

#[test]
fn trailing_margin_bearing_soft_hyphen_preserves_advance_in_ltr_and_rtl() {
    // R15-3: "<p style='text-align:right;text-transform:uppercase'>A<span style='margin-left:1em'>&shy;</span></p>"
    // at width 200 must keep the 45 px line width (15 + 30) so A sits at x=139 (30 px left
    // of 169), matching the plain base path while emitting only 1 run.
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    run_case_layout(
        &mut font,
        &mut sink,
        200,
        300,
        30,
        b"p{text-align:right;text-transform:uppercase}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(b"A");
            e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
            e.text("\u{00ad}".as_bytes());
            e.end(b"span");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs = &sink.inner.pages[0].2;
    assert_eq!(runs.len(), 1, "expected only A run (no empty marker run), got {runs:?}");
    assert_eq!(runs[0].0, b"A");
    assert_eq!(runs[0].1, 139, "trailing margin-bearing soft hyphen must keep A at x=139: {runs:?}");

    // RTL counterpart: default left alignment flips to right; trailing 30 px margin lands
    // on the visual left (139..169) and alef sits flush at the right edge (169..184).
    let mut font_rtl = StrictCaseFont::default();
    let mut sink_rtl = AnchorCapture::default();
    run_case_layout(
        &mut font_rtl,
        &mut sink_rtl,
        200,
        300,
        30,
        b"p{text-align:left;text-transform:uppercase}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text("א".as_bytes());
            e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
            e.text("\u{00ad}".as_bytes());
            e.end(b"span");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let rtl_runs = &sink_rtl.inner.pages[0].2;
    assert_eq!(rtl_runs.len(), 1, "expected only alef run, got {rtl_runs:?}");
    assert_eq!(rtl_runs[0].0, "א".as_bytes());
    assert_eq!(rtl_runs[0].1, 169, "RTL trailing margin-bearing soft hyphen must keep alef at x=169: {rtl_runs:?}");
}

#[test]
fn segment_cap_break_at_64th_run_keeps_reduced_hyphen_attached_to_visible_word() {
    // R15-4: width 482 (450 px content), p{font-size:50%;font-variant:small-caps;text-align:left},
    // 32x "Aa" (64 alternating runs = 416 px) + <span style="font-size:200%;margin-left:1em">&shy;B</span>.
    // The 64-run cap must move the 64th letter with the trailing discretionary hyphen onto
    // row 2 so the hyphen keeps its 12 px small-cap size rather than painting alone at 30 px.
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    let mut text_64 = String::new();
    for _ in 0..32 {
        text_64.push_str("Aa");
    }
    run_case_layout(
        &mut font,
        &mut sink,
        482,
        300,
        30,
        b"p{font-size:50%;font-variant:small-caps;text-align:left}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(text_64.as_bytes());
            e.start(
                b"span",
                None,
                Some(b"font-size:200%;margin-left:1em"),
                None,
                None,
                None,
            );
            e.text("\u{00ad}B".as_bytes());
            e.end(b"span");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs = &sink.inner.pages[0].2;
    let base1 = runs[0].2;
    let row1: Vec<_> = runs.iter().filter(|r| r.2 == base1).collect();
    let after1: Vec<_> = runs.iter().filter(|r| r.2 > base1).collect();
    let base2 = after1[0].2;
    let row2: Vec<_> = after1.iter().copied().filter(|r| r.2 == base2).collect();
    let row3: Vec<_> = after1.iter().copied().filter(|r| r.2 > base2).collect();
    assert_eq!(row1.len(), 63, "row 1 yields 64th letter to keep hyphen attached: {row1:?}");
    assert_eq!(row2.len(), 2, "row 2 must contain 64th letter 'A' and reduced '-', got {row2:?}");
    assert_eq!(row2[0].0, b"A");
    assert_eq!(row2[0].3, case::small_cap_px(15));
    assert_eq!(row2[1].0, b"-");
    assert_eq!(
        row2[1].3,
        case::small_cap_px(15),
        "hyphen must keep 12 px small-cap size, not 30 px span size: {row2:?}"
    );
    assert_eq!(row3.len(), 1);
    assert_eq!(row3[0].0, b"B");
    assert_eq!(row3[0].3, 30);

    // R28-1a: 32x "Aa" + U+0301 in one small-caps span (423 px <= 450 px content width).
    // The combining acute accent inherits the 64th letter's small-cap size and stays in
    // segment 64 on row 1 rather than starting a 65th segment on row 2.
    let mut font_a = StrictCaseFont::default();
    let mut sink_a = AnchorCapture::default();
    let mut text_acute = text_64.clone();
    text_acute.push('\u{0301}');
    run_case_layout(
        &mut font_a,
        &mut sink_a,
        482,
        300,
        30,
        b"p{font-size:50%;font-variant:small-caps;text-align:left}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(text_acute.as_bytes());
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs_a = &sink_a.inner.pages[0].2;
    assert_eq!(runs_a.len(), 64, "32x Aa + combining acute must fit in 64 segments: {runs_a:?}");
    let last_a = runs_a.last().unwrap();
    assert_eq!(last_a.0, "A\u{0301}".as_bytes());
    assert_eq!(last_a.3, case::small_cap_px(15));
    assert_eq!(
        last_a.2, runs_a[0].2,
        "combining acute after 64th small-cap letter must stay on row 1: {last_a:?}"
    );

    // R28-1b: Cross-span combining acute in a 65th segment (<b>\u{0301}</b>) after 32x "Aa"
    // moves the 64th base letter onto row 2 alongside the combining mark so both share a baseline.
    let mut font_b = StrictCaseFont::default();
    let mut sink_b = AnchorCapture::default();
    run_case_layout(
        &mut font_b,
        &mut sink_b,
        482,
        300,
        30,
        b"p{font-size:50%;font-variant:small-caps;text-align:left}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(text_64.as_bytes());
            e.start(b"b", None, None, None, None, None);
            e.text("\u{0301}".as_bytes());
            e.end(b"b");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs_b = &sink_b.inner.pages[0].2;
    let base_b1 = runs_b[0].2;
    let row_b1: Vec<_> = runs_b.iter().filter(|r| r.2 == base_b1).collect();
    let row_b2: Vec<_> = runs_b.iter().filter(|r| r.2 > base_b1).collect();
    assert_eq!(row_b1.len(), 63, "row 1 yields 64th letter for cross-span accent: {row_b1:?}");
    assert_eq!(row_b2.len(), 2, "row 2 must keep 64th letter and bold accent together: {row_b2:?}");
    assert_eq!(row_b2[0].0, b"A");
    assert_eq!(row_b2[0].3, case::small_cap_px(15));
    assert_eq!(row_b2[1].0, "\u{0301}".as_bytes());
    assert_eq!(row_b2[0].2, row_b2[1].2, "cross-span accent must share its base letter's baseline");

    // R29-1: Moving a discretionary hyphen onto row 2 when segment 64 is a cross-span
    // combining mark (31x "aA" + "a" + <b>\u{0301}</b> + <span 200% margin>&shy;B</span>)
    // must also carry the 63rd letter 'a' so base A, bold accent and '-' share row 2.
    let mut font_c = StrictCaseFont::default();
    let mut sink_c = AnchorCapture::default();
    let mut text_63 = String::new();
    for _ in 0..31 {
        text_63.push_str("aA");
    }
    text_63.push('a');
    run_case_layout(
        &mut font_c,
        &mut sink_c,
        484,
        300,
        30,
        b"p{font-size:50%;font-variant:small-caps;text-align:left}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(text_63.as_bytes());
            e.start(b"b", None, None, None, None, None);
            e.text("\u{0301}".as_bytes());
            e.end(b"b");
            e.start(
                b"span",
                None,
                Some(b"font-size:200%;margin-left:1em"),
                None,
                None,
                None,
            );
            e.text("\u{00ad}B".as_bytes());
            e.end(b"span");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs_c = &sink_c.inner.pages[0].2;
    let base_c1 = runs_c[0].2;
    let row_c1: Vec<_> = runs_c.iter().filter(|r| r.2 == base_c1).collect();
    let after_c1: Vec<_> = runs_c.iter().filter(|r| r.2 > base_c1).collect();
    let base_c2 = after_c1[0].2;
    let row_c2: Vec<_> = after_c1.iter().copied().filter(|r| r.2 == base_c2).collect();
    assert_eq!(row_c1.len(), 62, "row 1 must keep 62 letter runs: {row_c1:?}");
    assert_eq!(row_c2.len(), 3, "row 2 must keep base A, bold accent, and hyphen together: {row_c2:?}");
    assert_eq!(row_c2[0].0, b"A");
    assert_eq!(row_c2[0].3, case::small_cap_px(15));
    assert_eq!(row_c2[1].0, "\u{0301}".as_bytes());
    assert_eq!(row_c2[2].0, b"-");
    assert_eq!(row_c2[0].2, row_c2[1].2);
    assert_eq!(row_c2[1].2, row_c2[2].2);

    // R29-2: Emergency width break when 75 lowercase small-caps letters fill 450 px exactly
    // and U+0301 overflows must carry letter 75 onto row 2 with its accent in one 12 px run.
    let mut font_d = StrictCaseFont::default();
    let mut sink_d = AnchorCapture::default();
    let mut text_75 = "a".repeat(75);
    text_75.push('\u{0301}');
    run_case_layout(
        &mut font_d,
        &mut sink_d,
        482,
        300,
        30,
        b"p{font-size:50%;font-variant:small-caps;text-align:left}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(text_75.as_bytes());
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs_d = &sink_d.inner.pages[0].2;
    assert_eq!(runs_d.len(), 2, "expected 74-letter row 1 and 'A\\u{{0301}}' row 2: {runs_d:?}");
    assert_eq!(runs_d[0].0.len(), 74);
    assert_eq!(runs_d[0].3, case::small_cap_px(15));
    assert_eq!(runs_d[1].0, "A\u{0301}".as_bytes());
    assert_eq!(runs_d[1].3, case::small_cap_px(15));
    assert!(runs_d[1].2 > runs_d[0].2);

    // R30-1: 32x "Aa" + <span style="margin-left:1em">&shy;\u{0301}</span> on 500 px page
    // (468 px content). The retained margin-bearing soft hyphen at continuation start must not
    // hide U+0301 from combining-mark attachment: 63 runs stay on row 1, reduced A and U+0301
    // share row 2, and the 15 px left margin (1em at 50% of 30 px) is charged once between them.
    let mut font_e = StrictCaseFont::default();
    let mut sink_e = AnchorCapture::default();
    run_case_layout(
        &mut font_e,
        &mut sink_e,
        500,
        300,
        30,
        b"p{font-size:50%;font-variant:small-caps;text-align:left}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(text_64.as_bytes());
            e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
            e.text("\u{00ad}\u{0301}".as_bytes());
            e.end(b"span");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs_e = &sink_e.inner.pages[0].2;
    let base_e1 = runs_e[0].2;
    let row_e1: Vec<_> = runs_e.iter().filter(|r| r.2 == base_e1).collect();
    let row_e2: Vec<_> = runs_e.iter().filter(|r| r.2 > base_e1).collect();
    assert_eq!(row_e1.len(), 63, "row 1 must keep 63 runs: {row_e1:?}");
    assert_eq!(row_e2.len(), 2, "row 2 must keep reduced A and U+0301 together: {row_e2:?}");
    assert_eq!(row_e2[0].0, b"A");
    assert_eq!(row_e2[0].3, case::small_cap_px(15));
    assert_eq!(row_e2[1].0, "\u{0301}".as_bytes());
    assert_eq!(row_e2[1].3, 15, "cross-span combining accent must keep full 15 px size: {row_e2:?}");
    assert_eq!(row_e2[0].2, row_e2[1].2);
    // Margin charged once: A at x=16 (advance 6 px at 12 px) + 15 px (1em at 50% of 30 px) => x=37.
    assert_eq!(row_e2[0].1, 16);
    assert_eq!(row_e2[1].1, 16 + 6 + 15, "margin must be charged once: {row_e2:?}");
}

#[test]
fn trailing_margin_after_64_visible_runs_keeps_row_and_spends_no_blank_line() {
    // R16-1: width 482 (450 px content), right-aligned 50% small caps, 32x "Aa" (64 runs = 416 px)
    // + <span style="font-size:200%;margin-left:1em">&shy;</span><br/>Z.
    // Trailing 30 px margin stays on row 1 (446 px total => first A at x=20) and Z sits
    // directly one line height below A (no blank row from an empty marker continuation).
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    let lh = font.line_height(15);
    let mut text_64 = String::new();
    for _ in 0..32 {
        text_64.push_str("Aa");
    }
    run_case_layout(
        &mut font,
        &mut sink,
        482,
        300,
        30,
        b"p{font-size:50%;font-variant:small-caps;text-align:right}",
        |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(text_64.as_bytes());
            e.start(
                b"span",
                None,
                Some(b"font-size:200%;margin-left:1em"),
                None,
                None,
                None,
            );
            e.text("\u{00ad}".as_bytes());
            e.end(b"span");
            e.start(b"br", None, None, None, None, None);
            e.end(b"br");
            e.text(b"Z");
            e.end(b"p");
            e.end(b"body");
        },
    );
    let runs = &sink.inner.pages[0].2;
    let base1 = runs[0].2;
    let row1: Vec<_> = runs.iter().filter(|r| r.2 == base1).collect();
    let row2: Vec<_> = runs.iter().filter(|r| r.2 > base1).collect();
    assert_eq!(row1.len(), 64, "all 64 visible runs stay on row 1");
    assert_eq!(row1[0].1, 20, "446 px right-aligned row 1 must start A at x=20: {:?}", row1[0]);
    assert_eq!(row2.len(), 1, "only Z after <br/>, got {row2:?}");
    assert_eq!(row2[0].0, b"Z");
    assert_eq!(
        row2[0].2 - base1,
        lh,
        "Z must sit exactly one line height below A (no blank row): {:?}",
        row2[0]
    );
}

#[test]
fn rtl_marker_before_justified_space_matches_margin_on_space_directly() {
    // R16-2: at width 112 (80 px content), uppercase + justify, compare
    // "א<span style='margin-left:1em'> ב</span> גג" and
    // "א<span style='margin-left:1em'>&shy; ב</span> גג".
    // Both must place bet at x=16, 50 px justified gap, and alef at x=81.
    let layout_rtl = |with_shy: bool| -> Vec<(Vec<u8>, i16)> {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        run_case_layout(
            &mut font,
            &mut sink,
            112,
            300,
            30,
            b"p{text-transform:uppercase;text-align:justify}",
            |e| {
                e.start(b"body", None, None, None, None, None);
                e.start(b"p", None, None, None, None, None);
                e.text("א".as_bytes());
                e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
                e.text(if with_shy { "\u{00ad} ב".as_bytes() } else { " ב".as_bytes() });
                e.end(b"span");
                e.text(" גג".as_bytes());
                e.end(b"p");
                e.end(b"body");
            },
        );
        let runs = &sink.inner.pages[0].2;
        let base1 = runs[0].2;
        runs.iter()
            .filter(|r| r.2 == base1)
            .map(|r| (r.0.clone(), r.1))
            .collect()
    };
    let without_shy = layout_rtl(false);
    let with_shy = layout_rtl(true);
    assert_eq!(
        without_shy,
        vec![("ב".as_bytes().to_vec(), 16), ("א".as_bytes().to_vec(), 81)]
    );
    assert_eq!(
        with_shy, without_shy,
        "marker before justified RTL space shifted bet"
    );
}

#[test]
fn oversized_first_glyph_after_leading_soft_hyphen_keeps_its_margin() {
    // R16-3: width 72 (40 px content), left-aligned uppercase:
    // "<span style='margin-left:1em'>B</span>" and "<span style='margin-left:1em'>&shy;B</span>"
    // both exceed 40 px (30 + 15 = 45 px) on the first glyph and must both admit B at x=46.
    for with_shy in [false, true] {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        run_case_layout(
            &mut font,
            &mut sink,
            72,
            300,
            30,
            b"p{text-transform:uppercase;text-align:left}",
            |e| {
                e.start(b"body", None, None, None, None, None);
                e.start(b"p", None, None, None, None, None);
                e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
                e.text(if with_shy { "\u{00ad}B".as_bytes() } else { b"B" });
                e.end(b"span");
                e.end(b"p");
                e.end(b"body");
            },
        );
        let runs = &sink.inner.pages[0].2;
        assert_eq!(runs.len(), 1, "with_shy={with_shy}: expected 1 run, got {runs:?}");
        assert_eq!(runs[0].0, b"B");
        assert_eq!(
            runs[0].1, 46,
            "with_shy={with_shy}: oversized first glyph must keep 30 px margin (x=46): {runs:?}"
        );
    }
}
#[test]
fn buffer_cut_carries_individually_resolved_pixels_without_fraction_aggregation() {
    // R16-4: 26-byte text buffer (25 usable): "A" + twelve sibling spans each with
    // "&shy;" and a fractional margin-left, then "B".
    // - 12x 0.03em (30*3/100 = 0 px each): must carry 0 px (B at x=16, not 26).
    // - 12x 0.05em (30*5/100 = 1 px each): must carry 12 px (B at x=28, not 34).
    for (decl, expected_x) in [
        (b"margin-left:0.03em".as_slice(), 16i16),
        (b"margin-left:0.05em".as_slice(), 28i16),
    ] {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        let mut backend = SplitBackend {
            font: &mut font,
            sink: &mut sink,
        };
        with_text_capacity(&mut backend, 4096, 26, |e| {
            e.params.width = 300;
            e.params.height = 300;
            e.params.left = 16;
            e.params.right = 16;
            e.params.base_size = 30;
            e.start(b"body", None, None, None, None, None);
            e.start(
                b"p",
                None,
                Some(b"text-transform:uppercase;text-align:left"),
                None,
                None,
                None,
            );
            e.text(b"A");
            for _ in 0..12 {
                e.start(b"span", None, Some(decl), None, None, None);
                e.text("\u{00ad}".as_bytes());
                e.end(b"span");
            }
            e.text(b"B");
            e.end(b"p");
            e.end(b"body");
            assert!(e.finish());
        });
        let runs = &sink.inner.pages[0].2;
        let b_run = runs.iter().find(|r| r.0 == b"B").expect("missing B run");
        assert_eq!(
            b_run.1, expected_x,
            "decl {:?}: expected B at x={expected_x}, got {runs:?}",
            core::str::from_utf8(decl)
        );
    }
}

#[test]
fn buffer_cut_respects_wrap_discarded_space_and_avoids_phantom_blank_row() {
    // Wrap-discarded space at capacity boundary: "aaa<span m> </span>b" with 5-byte
    // buffer (chunk 1 = "aaa ") at 45 px content width wraps before the space and
    // discards it, so B starts at x=16 (matching the uncut layout).
    let run_wrap_cut = |cap: usize| -> Vec<(Vec<u8>, i16, i16)> {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
        with_text_capacity(&mut backend, 4096, cap, |e| {
            e.params.width = 77; // 45 px content
            e.params.height = 300;
            e.params.left = 16;
            e.params.right = 16;
            e.params.base_size = 30;
            e.start(b"body", None, None, None, None, None);
            e.start(
                b"p",
                None,
                Some(b"text-transform:uppercase;text-align:left;margin-bottom:0"),
                None,
                None,
                None,
            );
            e.text(b"aaa");
            e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
            e.text(b" ");
            e.end(b"span");
            e.text(b"b");
            e.end(b"p");
            e.end(b"body");
            assert!(e.finish());
        });
        sink.inner.pages[0].2.iter().map(|r| (r.0.clone(), r.1, r.2)).collect()
    };
    let uncut = run_wrap_cut(256);
    let cut = run_wrap_cut(5);
    assert_eq!(cut, uncut, "wrap-discarded space before buffer cut must not leak margin");

    // Wrapped margin-bearing soft hyphen at capacity boundary: "aaa<span m>&shy;</span>b"
    // with 6-byte buffer (chunk 1 = "aaa\u{00ad}") at 45 px content width carries the 30 px
    // margin onto B without inserting a phantom blank row.
    let run_shy_wrap_cut = |cap: usize| -> Vec<(Vec<u8>, i16, i16)> {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
        with_text_capacity(&mut backend, 4096, cap, |e| {
            e.params.width = 77; // 45 px content
            e.params.height = 300;
            e.params.left = 16;
            e.params.right = 16;
            e.params.base_size = 30;
            e.start(b"body", None, None, None, None, None);
            e.start(
                b"p",
                None,
                Some(b"text-transform:uppercase;text-align:left;margin-bottom:0"),
                None,
                None,
                None,
            );
            e.text(b"aaa");
            e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
            e.text("\u{00ad}".as_bytes());
            e.end(b"span");
            e.text(b"b");
            e.end(b"p");
            e.end(b"body");
            assert!(e.finish());
        });
        sink.inner.pages[0].2.iter().map(|r| (r.0.clone(), r.1, r.2)).collect()
    };
    let uncut_shy = run_shy_wrap_cut(256);
    let cut_shy = run_shy_wrap_cut(6);
    assert_eq!(cut_shy, uncut_shy, "wrapped marker cut must carry margin with no phantom blank row");
    assert_eq!(cut_shy[1].1, 46);
    assert_eq!(cut_shy[1].2 - cut_shy[0].2, 36);
}

#[test]
fn emergency_wrap_after_collapsible_space_retains_following_margin_marker() {
    // R17-1: 50 px content width (82 px viewport, 16 px insets), 30 px uppercase left:
    // "A<br/> &shy;<span style='margin-left:1em'>B</span>" vs
    // "A<br/> &shy;<span style='margin-left:1em'>&shy;B</span>".
    // Discarding the collapsible leading space must keep the 30 px margin on the
    // following marker so B lands at x=46 on row 2 in both inputs.
    let run_case = |with_inner_shy: bool| -> Vec<(Vec<u8>, i16, i16)> {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        run_case_layout(
            &mut font,
            &mut sink,
            82,
            300,
            30,
            b"p{text-transform:uppercase;text-align:left}",
            |e| {
                e.start(b"body", None, None, None, None, None);
                e.start(b"p", None, None, None, None, None);
                e.text(b"A");
                e.start(b"br", None, None, None, None, None);
                e.end(b"br");
                e.text(" \u{00ad}".as_bytes());
                e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
                e.text(if with_inner_shy { "\u{00ad}B".as_bytes() } else { b"B" });
                e.end(b"span");
                e.end(b"p");
                e.end(b"body");
            },
        );
        sink.inner.pages[0]
            .2
            .iter()
            .map(|r| (r.0.clone(), r.1, r.2))
            .collect()
    };
    let control = run_case(false);
    let variant = run_case(true);
    assert_eq!(control.len(), 2);
    assert_eq!(control[1].0, b"B");
    assert_eq!(control[1].1, 46);
    assert_eq!(
        variant, control,
        "marker after collapsible leading space lost 30 px margin on emergency wrap"
    );
}

#[test]
fn marker_only_middle_buffer_preserves_continuation_without_reindenting() {
    // R17-2: 5-byte text buffer (4 usable), text-indent:1em, margin-bottom:0:
    // "AAAA&shy;&shy;B" has a visible first chunk, a wholly invisible middle chunk,
    // and a continuation B; B must NOT reapply the first-line indent (x=16, matching "AAAAB").
    let run_case = |body: &[u8]| -> Vec<(Vec<u8>, i16, i16)> {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
        with_text_capacity(&mut backend, 4096, 5, |e| {
            e.params.width = 400;
            e.params.height = 300;
            e.params.left = 16;
            e.params.right = 16;
            e.params.base_size = 30;
            e.start(b"body", None, None, None, None, None);
            e.start(
                b"p",
                None,
                Some(b"text-transform:uppercase;text-align:left;text-indent:1em;margin-bottom:0"),
                None,
                None,
                None,
            );
            e.text(body);
            e.end(b"p");
            e.end(b"body");
            assert!(e.finish());
        });
        sink.inner.pages[0]
            .2
            .iter()
            .map(|r| (r.0.clone(), r.1, r.2))
            .collect()
    };
    let control = run_case(b"AAAAB");
    let variant = run_case("AAAA\u{00ad}\u{00ad}B".as_bytes());
    assert_eq!(control.len(), 2);
    assert_eq!(control[0].1, 46);
    assert_eq!(control[1].1, 16);
    assert_eq!(
        variant, control,
        "marker-only middle chunk reapplied first-line text-indent"
    );
}

#[test]
fn long_leading_soft_hyphen_run_with_oversized_margin_keeps_margin_in_linear_time() {
    // R17-3: 40 px content width, 30 px base size, 2em (60 px) margin followed by 1,535
    // U+00AD markers (3,071 usable bytes, matching the firmware text bank) and final B
    // must stay O(n) and keep B at x=76.
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
    let mut body = Vec::with_capacity(1535 * 2 + 1);
    for _ in 0..1535 {
        body.extend_from_slice(&[0xc2, 0xad]);
    }
    body.push(b'B');
    let mut text = vec![0; 3072];
    let mut breaks = vec![0; 3072];
    let mut levels = vec![0; 3072];
    let mut spans = vec![Span::default(); 32];
    let mut lines = vec![Line::default(); 32];
    let mut elements = vec![Element::default(); 16];
    let mut runs = vec![Run::default(); 64];
    let mut images = vec![Image::default(); 2];
    let mut links = vec![Link::default(); 8];
    let mut page = vec![0; 4096];
    let mut style = vec![0; 256];
    let mut pl = vec![ParLink::default(); 4];
    let mut ch_rules = vec![CssRule::default(); 512];
    let mut ch_css = CssStylesheetBuilder::default();
    assert!(ch_css.begin(&mut ch_rules));
    let storage = Storage {
        text: &mut text,
        breaks: &mut breaks,
        levels: &mut levels,
        spans: &mut spans,
        lines: &mut lines,
        elements: &mut elements,
        runs: &mut runs,
        images: &mut images,
        links: &mut links,
        page: &mut page,
        style_text: &mut style,
        paragraph_links: &mut pl,
    };
    let mut breaker = SpaceBreak;
    let mut img = FixtureImages;
    let params = Params {
        width: 72, // 40 px content
        height: 300,
        left: 16,
        right: 16,
        top: 16,
        bottom: 16,
        base_size: 30,
        ..Params::default()
    };
    let mut e = Engine::new(params, storage, &mut backend, &mut breaker, &mut img, None, ch_css)
        .unwrap();
    e.start(b"body", None, None, None, None, None);
    e.start(b"p", None, Some(b"text-align:left"), None, None, None);
    e.start(
        b"span",
        None,
        Some(b"text-transform:uppercase;margin-left:2em"),
        None,
        None,
        None,
    );
    e.text(&body);
    e.end(b"span");
    e.end(b"p");
    e.end(b"body");
    assert!(e.finish());
    let runs = &sink.inner.pages[0].2;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].0, b"B");
    assert_eq!(
        runs[0].1, 76,
        "2em margin (60 px) + left inset (16 px) must place B at x=76: {runs:?}"
    );
}

#[test]
fn terminal_soft_hyphen_in_final_buffer_adds_no_blank_row_or_extra_margin() {
    // R18-1: 5-byte buffer (4 usable), two successive uppercase paragraphs "AAAA&shy;"
    // and "B" must match both a larger buffer and "AAAA" without "&shy;" (B baseline
    // 36 px below AAAA with margin-bottom:0, and 48 px with default margin-bottom:0.4em).
    let run_pair = |cap: usize, style: &[u8], with_margin_span: bool, first_body: &[u8]| -> Vec<(Vec<u8>, i16, i16)> {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
        with_text_capacity(&mut backend, 4096, cap, |e| {
            e.params.width = 400;
            e.params.height = 300;
            e.params.left = 16;
            e.params.right = 16;
            e.params.base_size = 30;
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, Some(style), None, None, None);
            e.text(first_body);
            if with_margin_span {
                e.start(b"span", None, Some(b"margin-left:1em"), None, None, None);
                e.text("\u{00ad}".as_bytes());
                e.end(b"span");
            }
            e.end(b"p");
            e.start(b"p", None, Some(style), None, None, None);
            e.text(b"B");
            e.end(b"p");
            e.end(b"body");
            assert!(e.finish());
        });
        sink.inner.pages[0]
            .2
            .iter()
            .map(|r| (r.0.clone(), r.1, r.2))
            .collect()
    };
    let no_margin = b"text-transform:uppercase;text-align:left;margin-top:0;margin-bottom:0";
    let wide = run_pair(256, no_margin, false, "AAAA\u{00ad}".as_bytes());
    let plain_cut = run_pair(5, no_margin, false, b"AAAA");
    let shy_cut = run_pair(5, no_margin, false, "AAAA\u{00ad}".as_bytes());
    let margin_shy_cut = run_pair(5, no_margin, true, b"AAAA");
    assert_eq!(wide.len(), 2);
    assert_eq!(wide[1].2 - wide[0].2, 36);
    assert_eq!(shy_cut, wide, "terminal soft-hyphen chunk added a blank row");
    assert_eq!(shy_cut, plain_cut);
    assert_eq!(margin_shy_cut, wide, "terminal margin-bearing marker chunk added a blank row");

    // Non-zero default paragraph margin-bottom must not double-apply on the empty terminal chunk.
    let with_after = b"text-transform:uppercase;text-align:left;margin-top:0";
    let after_plain = run_pair(5, with_after, false, b"AAAA");
    let after_shy = run_pair(5, with_after, false, "AAAA\u{00ad}".as_bytes());
    assert_eq!(after_shy, after_plain, "empty terminal chunk doubled paragraph margin-bottom");

    // Explicit <br/> in the final chunk still preserves its intentional blank row.
    let mut font_br = StrictCaseFont::default();
    let mut sink_br = AnchorCapture::default();
    let mut backend_br = SplitBackend { font: &mut font_br, sink: &mut sink_br };
    with_text_capacity(&mut backend_br, 4096, 5, |e| {
        e.params.width = 400;
        e.params.height = 300;
        e.params.left = 16;
        e.params.right = 16;
        e.params.base_size = 30;
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, Some(no_margin), None, None, None);
        e.text(b"AAAA");
        e.start(b"br", None, None, None, None, None);
        e.end(b"br");
        e.end(b"p");
        e.start(b"p", None, Some(no_margin), None, None, None);
        e.text(b"B");
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
    });
    let br_cut = &sink_br.inner.pages[0].2;
    assert_eq!(br_cut[1].2 - br_cut[0].2, 72, "explicit trailing <br/> must keep its blank row");
}

#[test]
fn asymmetric_case_letters_keep_strong_ltr_bidi_before_rtl() {
    // R18-2: Kelvin (U+212A -> k) and Angstrom (U+212B -> å) before Hebrew alef (U+05D0)
    // under text-transform:lowercase must match K + Å + alef in visual order and alignment.
    let layout = |css: &[u8], text: &str| -> Vec<(Vec<u8>, i16)> {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        run_case_layout(&mut font, &mut sink, 240, 300, 30, css, |e| {
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, None, None, None, None);
            e.text(text.as_bytes());
            e.end(b"p");
            e.end(b"body");
        });
        sink.inner.pages[0]
            .2
            .iter()
            .map(|r| (r.0.clone(), r.1))
            .collect()
    };
    let lower_css = b"p{text-transform:lowercase;text-align:left}";
    let control = layout(lower_css, "\u{004b}\u{00c5}\u{05d0}");
    let kelvin_angstrom = layout(lower_css, "\u{212a}\u{212b}\u{05d0}");
    assert_eq!(control[0].1, 16, "LTR base must align left: {control:?}");
    assert_eq!(
        kelvin_angstrom, control,
        "Kelvin/Angstrom before alef reversed or shifted under lowercase"
    );

    // Also cover Ohm (U+2126 -> ω), Capital Sharp S (U+1E9E -> ß), and Micro (U+00B5 -> Μ).
    assert_eq!(
        layout(lower_css, "\u{2126}\u{1e9e}\u{05d0}"),
        layout(lower_css, "\u{03a9}\u{00df}\u{05d0}"),
        "Ohm/Capital Sharp S before alef must stay strong L"
    );
    let upper_css = b"p{text-transform:uppercase;text-align:left}";
    assert_eq!(
        layout(upper_css, "\u{00b5}\u{05d0}"),
        layout(upper_css, "\u{039c}\u{05d0}"),
        "Micro sign before alef must stay strong L under uppercase"
    );
}

#[test]
fn multi_continuation_segment_cap_keeps_trailing_discretionary_hyphen_attached() {
    // R18-3: 96x "aA" (192 alternating small-cap runs = 2,592 px) + 1,000 U+00AD + B
    // at 2,602 px content width (width=2634) wraps at the final marker and splits across
    // 3 segment-cap continuations; row 4 carries the 192nd run 'A' with its 30 px '-'.
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
    let mut body = Vec::with_capacity(96 * 2 + 1000 * 2 + 1);
    for _ in 0..96 {
        body.extend_from_slice(b"aA");
    }
    for _ in 0..1000 {
        body.extend_from_slice(&[0xc2, 0xad]);
    }
    body.push(b'B');
    let mut text = vec![0; 3072];
    let mut breaks = vec![0; 3072];
    let mut levels = vec![0; 3072];
    let mut spans = vec![Span::default(); 32];
    let mut lines = vec![Line::default(); 32];
    let mut elements = vec![Element::default(); 16];
    let mut runs = vec![Run::default(); 256];
    let mut images = vec![Image::default(); 2];
    let mut links = vec![Link::default(); 8];
    let mut page = vec![0; 4096];
    let mut style = vec![0; 256];
    let mut pl = vec![ParLink::default(); 4];
    let mut ch_rules = vec![CssRule::default(); 512];
    let mut ch_css = CssStylesheetBuilder::default();
    assert!(ch_css.begin(&mut ch_rules));
    let storage = Storage {
        text: &mut text,
        breaks: &mut breaks,
        levels: &mut levels,
        spans: &mut spans,
        lines: &mut lines,
        elements: &mut elements,
        runs: &mut runs,
        images: &mut images,
        links: &mut links,
        page: &mut page,
        style_text: &mut style,
        paragraph_links: &mut pl,
    };
    let mut breaker = SpaceBreak;
    let mut img = FixtureImages;
    let params = Params {
        width: 2634, // 2602 px content
        height: 400,
        left: 16,
        right: 16,
        top: 16,
        bottom: 16,
        base_size: 30,
        ..Params::default()
    };
    let mut e = Engine::new(params, storage, &mut backend, &mut breaker, &mut img, None, ch_css)
        .unwrap();
    e.start(b"body", None, None, None, None, None);
    e.start(b"p", None, Some(b"font-variant:small-caps;text-align:left"), None, None, None);
    e.text(&body);
    e.end(b"p");
    e.end(b"body");
    assert!(e.finish());
    let out = &sink.inner.pages[0].2;
    let mut rows: Vec<(i16, usize)> = Vec::new();
    for r in out {
        if rows.last().is_some_and(|(y, _)| *y == r.2) {
            rows.last_mut().unwrap().1 += 1;
        } else {
            rows.push((r.2, 1));
        }
    }
    assert_eq!(rows.len(), 4, "expected 64+64+64(ending A-)+1(B) rows, got {rows:?}");
    assert_eq!(rows[0].1, 64);
    assert_eq!(rows[1].1, 64);
    assert_eq!(rows[2].1, 64);
    assert_eq!(rows[3].1, 1);
    let hyph_run = out.iter().filter(|r| r.2 == rows[2].0).last().unwrap();
    assert_eq!(hyph_run.0, b"A-");
    assert_eq!(hyph_run.3, 30);
    let b_run = out.iter().find(|r| r.2 == rows[3].0).unwrap();
    assert_eq!(b_run.0, b"B");
}

#[test]
fn case_only_span_bank_exhaustion_preserves_subsequent_plain_text_and_links() {
    // R19-1: With production 96-span capacity, 47 pairs of "a<span uppercase>b</span>"
    // plus "c<a href='#first' uppercase>d</a>tail<a href='#second'>more</a>" fills all
    // 96 slots on 'd'. Closing #first must not bleed uppercase or #first onto "tail"
    // or redirect "more" away from #second.
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
    let mut text = vec![0; 512];
    let mut breaks = vec![0; 512];
    let mut levels = vec![0; 512];
    let mut spans = vec![Span::default(); 96];
    let mut lines = vec![Line::default(); 32];
    let mut elements = vec![Element::default(); 16];
    let mut runs = vec![Run::default(); 256];
    let mut images = vec![Image::default(); 2];
    let mut links = vec![Link::default(); 16];
    let mut page = vec![0; 4096];
    let mut style = vec![0; 256];
    let mut pl = vec![ParLink::default(); 8];
    let mut ch_rules = vec![CssRule::default(); 512];
    let mut ch_css = CssStylesheetBuilder::default();
    assert!(ch_css.begin(&mut ch_rules));
    let storage = Storage {
        text: &mut text,
        breaks: &mut breaks,
        levels: &mut levels,
        spans: &mut spans,
        lines: &mut lines,
        elements: &mut elements,
        runs: &mut runs,
        images: &mut images,
        links: &mut links,
        page: &mut page,
        style_text: &mut style,
        paragraph_links: &mut pl,
    };
    let mut breaker = SpaceBreak;
    let mut img = FixtureImages;
    let params = Params {
        width: 2400,
        height: 400,
        left: 16,
        right: 16,
        top: 16,
        bottom: 16,
        base_size: 30,
        ..Params::default()
    };
    let mut e = Engine::new(params, storage, &mut backend, &mut breaker, &mut img, None, ch_css)
        .unwrap();
    e.start(b"body", None, None, None, None, None);
    e.start(b"p", None, Some(b"text-align:left;margin-bottom:0"), None, None, None);
    for _ in 0..47 {
        e.text(b"a");
        e.start(b"span", None, Some(b"text-transform:uppercase"), None, None, None);
        e.text(b"b");
        e.end(b"span");
    }
    e.text(b"c");
    e.start(
        b"a",
        None,
        Some(b"text-transform:uppercase"),
        None,
        Some(b"#first"),
        None,
    );
    e.text(b"d");
    e.end(b"a");
    e.text(b"tail");
    e.start(b"a", None, None, None, Some(b"#second"), None);
    e.text(b"more");
    e.end(b"a");
    e.end(b"p");
    e.end(b"body");
    assert!(e.finish());

    let runs = &sink.inner.pages[0].2;
    let d_run = runs.iter().find(|r| r.0 == b"D").expect("missing uppercase D run");
    let tail_run = runs.iter().find(|r| r.0 == b"tail").expect("missing plain 'tail' run");
    let more_run = runs.iter().find(|r| r.0 == b"more").expect("missing plain 'more' run");
    assert!(
        !runs.iter().any(|r| r.0.windows(4).any(|w| w == b"TAIL" || w == b"MORE")),
        "uppercase leaked past closed link onto suffix: {runs:?}"
    );
    let links = &sink.inner.links;
    let rects = &sink.inner.link_rects;
    assert_eq!(links.len(), 2, "expected exactly 2 links (#first and #second), got {links:?}");
    assert_eq!(links[0].2, b"first");
    assert_eq!(rects[0].1, d_run.1, "#first link x must match D");
    assert_eq!(rects[0].3, 15, "#first hit width must cover only D (15 px), not 'tail'");
    assert_eq!(links[1].2, b"second");
    assert_eq!(rects[1].1, more_run.1, "#second link x must match 'more'");
    assert_eq!(rects[1].3, 60, "#second hit width must cover 'more' (60 px)");
    assert!(
        tail_run.1 + 60 <= more_run.1 && tail_run.1 < rects[1].1,
        "'tail' must sit outside #second's hit box: tail={tail_run:?} rects={rects:?}"
    );
}

#[test]
fn last_measured_line_slot_suppresses_invisible_remainder_while_keeping_explicit_breaks() {
    // R19-2: With 2 line slots vs 3 line slots, "<p uppercase>A<br/>&shy;</p><p>B</p>"
    // must place B 1 line height (36 px) below A in both capacities, while
    // "<p uppercase>A<br/><br/>&shy;</p><p>B</p>" keeps the explicit hard-break blank row (72 px),
    // and the production 176-line-slot equivalent (175x "A<br/>" + "&shy;") adds no blank row.
    let run_slots = |line_cap: usize, br_reps: usize, extra_br: bool, with_shy: bool| -> i16 {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
        let mut text = vec![0; 1024];
        let mut breaks = vec![0; 1024];
        let mut levels = vec![0; 1024];
        let mut spans = vec![Span::default(); 32];
        let mut lines = vec![Line::default(); line_cap];
        let mut elements = vec![Element::default(); 16];
        let mut runs = vec![Run::default(); 256];
        let mut images = vec![Image::default(); 2];
        let mut links = vec![Link::default(); 8];
        let mut page = vec![0; 4096];
        let mut style = vec![0; 256];
        let mut pl = vec![ParLink::default(); 4];
        let mut ch_rules = vec![CssRule::default(); 512];
        let mut ch_css = CssStylesheetBuilder::default();
        assert!(ch_css.begin(&mut ch_rules));
        let storage = Storage {
            text: &mut text,
            breaks: &mut breaks,
            levels: &mut levels,
            spans: &mut spans,
            lines: &mut lines,
            elements: &mut elements,
            runs: &mut runs,
            images: &mut images,
            links: &mut links,
            page: &mut page,
            style_text: &mut style,
            paragraph_links: &mut pl,
        };
        let mut breaker = SpaceBreak;
        let mut img = FixtureImages;
        let params = Params {
            width: 300,
            height: 8000,
            left: 16,
            right: 16,
            top: 16,
            bottom: 16,
            base_size: 30,
            ..Params::default()
        };
        let mut e = Engine::new(params, storage, &mut backend, &mut breaker, &mut img, None, ch_css)
            .unwrap();
        let p_style = b"text-transform:uppercase;text-align:left;margin-top:0;margin-bottom:0";
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, Some(p_style), None, None, None);
        for _ in 0..br_reps {
            e.text(b"A");
            e.start(b"br", None, None, None, None, None);
            e.end(b"br");
        }
        if extra_br {
            e.start(b"br", None, None, None, None, None);
            e.end(b"br");
        }
        if with_shy {
            e.text("\u{00ad}".as_bytes());
        }
        e.end(b"p");
        e.start(b"p", None, Some(p_style), None, None, None);
        e.text(b"B");
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
        let runs = &sink.inner.pages[0].2;
        let last_a = runs.iter().rev().find(|r| r.0 == b"A").unwrap();
        let b_run = runs.iter().find(|r| r.0 == b"B").unwrap();
        b_run.2 - last_a.2
    };
    assert_eq!(run_slots(2, 1, false, true), 36, "2-slot terminal marker added blank row");
    assert_eq!(run_slots(3, 1, false, true), 36);
    assert_eq!(run_slots(2, 1, false, false), 36);
    assert_eq!(run_slots(2, 1, true, true), 72, "explicit second <br/> before terminal marker must stay");
    assert_eq!(run_slots(176, 175, false, true), 36, "176-slot production equivalent added blank row");
}

#[test]
fn span_capacity_flush_applies_paragraph_bottom_margin_once_at_paragraph_end() {
    // R20-1: 48 pairs of "a<span uppercase>b</span>" + "<br/>tail" with default paragraph
    // margin-bottom (0.4em = 12 px) followed by "<p>next</p>". 96 spans (forcing a continuing
    // flush before "tail") must match 97 spans on both "tail"'s baseline (36 px below last B)
    // and "next"'s baseline (48 px below "tail").
    let run_spans = |span_cap: usize, trailing_shy: bool| -> Vec<(Vec<u8>, i16, i16)> {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
        let mut text = vec![0; 512];
        let mut breaks = vec![0; 512];
        let mut levels = vec![0; 512];
        let mut spans = vec![Span::default(); span_cap];
        let mut lines = vec![Line::default(); 32];
        let mut elements = vec![Element::default(); 16];
        let mut runs = vec![Run::default(); 256];
        let mut images = vec![Image::default(); 2];
        let mut links = vec![Link::default(); 8];
        let mut page = vec![0; 4096];
        let mut style = vec![0; 256];
        let mut pl = vec![ParLink::default(); 4];
        let mut ch_rules = vec![CssRule::default(); 512];
        let mut ch_css = CssStylesheetBuilder::default();
        assert!(ch_css.begin(&mut ch_rules));
        let storage = Storage {
            text: &mut text,
            breaks: &mut breaks,
            levels: &mut levels,
            spans: &mut spans,
            lines: &mut lines,
            elements: &mut elements,
            runs: &mut runs,
            images: &mut images,
            links: &mut links,
            page: &mut page,
            style_text: &mut style,
            paragraph_links: &mut pl,
        };
        let mut breaker = SpaceBreak;
        let mut img = FixtureImages;
        let params = Params {
            width: 2400,
            height: 400,
            left: 16,
            right: 16,
            top: 16,
            bottom: 16,
            base_size: 30,
            ..Params::default()
        };
        let mut e = Engine::new(params, storage, &mut backend, &mut breaker, &mut img, None, ch_css)
            .unwrap();
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, Some(b"text-align:left"), None, None, None);
        for _ in 0..48 {
            e.text(b"a");
            e.start(b"span", None, Some(b"text-transform:uppercase"), None, None, None);
            e.text(b"b");
            e.end(b"span");
        }
        e.start(b"br", None, None, None, None, None);
        e.end(b"br");
        e.text(if trailing_shy { "\u{00ad}".as_bytes() } else { b"tail" });
        e.end(b"p");
        e.start(b"p", None, Some(b"text-align:left"), None, None, None);
        e.text(b"next");
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
        sink.inner.pages[0]
            .2
            .iter()
            .map(|r| (r.0.clone(), r.1, r.2))
            .collect()
    };
    let s97 = run_spans(97, false);
    let s96 = run_spans(96, false);
    let last_b97 = s97.iter().rev().find(|r| r.0 == b"B").unwrap();
    let last_b96 = s96.iter().rev().find(|r| r.0 == b"B").unwrap();
    let tail97 = s97.iter().find(|r| r.0 == b"tail").unwrap();
    let tail96 = s96.iter().find(|r| r.0 == b"tail").unwrap();
    let next97 = s97.iter().find(|r| r.0 == b"next").unwrap();
    let next96 = s96.iter().find(|r| r.0 == b"next").unwrap();
    assert_eq!(tail97.2 - last_b97.2, 36);
    assert_eq!(tail96.2 - last_b96.2, 36, "96-span flush inserted paragraph bottom margin mid-paragraph");
    assert_eq!(next97.2 - tail97.2, 48);
    assert_eq!(next96.2 - tail96.2, 48, "96-span paragraph end lost or doubled bottom margin");
    assert_eq!(s96, s97);

    // Final chunk containing only an invisible marker still applies paragraph bottom margin once.
    let shy97 = run_spans(97, true);
    let shy96 = run_spans(96, true);
    let shy_next97 = shy97.iter().find(|r| r.0 == b"next").unwrap();
    let shy_next96 = shy96.iter().find(|r| r.0 == b"next").unwrap();
    assert_eq!(shy_next96.2, shy_next97.2, "marker-only final chunk after span flush shifted next paragraph");
}

#[test]
fn span_capacity_flush_preserves_paragraph_bidi_direction_in_both_directions() {
    // R20-2: 48 pairs + <br/> + opposite-direction single letter:
    // - Latin prefix + Hebrew alef: both 96 and 97 spans keep LTR base (alef at x=16).
    // - Hebrew prefix + Latin 'a': both 96 and 97 spans keep RTL base ('a' at x=2369).
    let run_dir = |span_cap: usize, rtl_first: bool| -> (i16, i16) {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
        let mut text = vec![0; 512];
        let mut breaks = vec![0; 512];
        let mut levels = vec![0; 512];
        let mut spans = vec![Span::default(); span_cap];
        let mut lines = vec![Line::default(); 32];
        let mut elements = vec![Element::default(); 16];
        let mut runs = vec![Run::default(); 256];
        let mut images = vec![Image::default(); 2];
        let mut links = vec![Link::default(); 8];
        let mut page = vec![0; 4096];
        let mut style = vec![0; 256];
        let mut pl = vec![ParLink::default(); 4];
        let mut ch_rules = vec![CssRule::default(); 512];
        let mut ch_css = CssStylesheetBuilder::default();
        assert!(ch_css.begin(&mut ch_rules));
        let storage = Storage {
            text: &mut text,
            breaks: &mut breaks,
            levels: &mut levels,
            spans: &mut spans,
            lines: &mut lines,
            elements: &mut elements,
            runs: &mut runs,
            images: &mut images,
            links: &mut links,
            page: &mut page,
            style_text: &mut style,
            paragraph_links: &mut pl,
        };
        let mut breaker = SpaceBreak;
        let mut img = FixtureImages;
        let params = Params {
            width: 2400,
            height: 400,
            left: 16,
            right: 16,
            top: 16,
            bottom: 16,
            base_size: 30,
            ..Params::default()
        };
        let mut e = Engine::new(params, storage, &mut backend, &mut breaker, &mut img, None, ch_css)
            .unwrap();
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, Some(b"text-align:left;margin-bottom:0"), None, None, None);
        let (first_ch, second_ch, tail_ch): (&[u8], &[u8], &[u8]) = if rtl_first {
            ("א".as_bytes(), "ב".as_bytes(), b"a")
        } else {
            (b"a", b"b", "א".as_bytes())
        };
        for _ in 0..48 {
            e.text(first_ch);
            e.start(b"span", None, Some(b"text-transform:uppercase"), None, None, None);
            e.text(second_ch);
            e.end(b"span");
        }
        e.start(b"br", None, None, None, None, None);
        e.end(b"br");
        e.text(tail_ch);
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
        let last = sink.inner.pages[0].2.last().unwrap();
        assert_eq!(last.0, tail_ch);
        (last.1, last.2)
    };
    let ltr97 = run_dir(97, false);
    let ltr96 = run_dir(96, false);
    assert_eq!(ltr97.0, 16);
    assert_eq!(ltr96, ltr97, "96-span flush flipped LTR paragraph continuation to RTL");

    let rtl97 = run_dir(97, true);
    let rtl96 = run_dir(96, true);
    assert_eq!(rtl97.0, 2369);
    assert_eq!(rtl96, rtl97, "96-span flush flipped RTL paragraph continuation to LTR");
}

#[test]
fn span_capacity_flush_preserves_bidi_previous_level_for_punctuation_mirroring() {
    // R21-1: 392 px page (360 px content = 24 glyphs at 15 px), 48 pairs of plain 'a' +
    // uppercase 'b' with the 48th 'b' replaced by Hebrew alef, followed by plain "(ב".
    // Both 96 and 97 span slots must place row 5 as bet then mirrored ')' at identical x.
    let run_punct = |span_cap: usize| -> Vec<(Vec<u8>, i16)> {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
        let mut text = vec![0; 512];
        let mut breaks = vec![0; 512];
        let mut levels = vec![0; 512];
        let mut spans = vec![Span::default(); span_cap];
        let mut lines = vec![Line::default(); 32];
        let mut elements = vec![Element::default(); 16];
        let mut runs = vec![Run::default(); 256];
        let mut images = vec![Image::default(); 2];
        let mut links = vec![Link::default(); 8];
        let mut page = vec![0; 4096];
        let mut style = vec![0; 256];
        let mut pl = vec![ParLink::default(); 4];
        let mut ch_rules = vec![CssRule::default(); 512];
        let mut ch_css = CssStylesheetBuilder::default();
        assert!(ch_css.begin(&mut ch_rules));
        let storage = Storage {
            text: &mut text,
            breaks: &mut breaks,
            levels: &mut levels,
            spans: &mut spans,
            lines: &mut lines,
            elements: &mut elements,
            runs: &mut runs,
            images: &mut images,
            links: &mut links,
            page: &mut page,
            style_text: &mut style,
            paragraph_links: &mut pl,
        };
        let mut breaker = SpaceBreak;
        let mut img = FixtureImages;
        let params = Params {
            width: 392,
            height: 400,
            left: 16,
            right: 16,
            top: 16,
            bottom: 16,
            base_size: 30,
            ..Params::default()
        };
        let mut e = Engine::new(params, storage, &mut backend, &mut breaker, &mut img, None, ch_css)
            .unwrap();
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, Some(b"text-align:left;margin-bottom:0"), None, None, None);
        for k in 0..48 {
            e.text(b"a");
            e.start(b"span", None, Some(b"text-transform:uppercase"), None, None, None);
            e.text(if k == 47 { "א".as_bytes() } else { b"b" });
            e.end(b"span");
        }
        e.text("(\u{05d1}".as_bytes());
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
        let runs = &sink.inner.pages[0].2;
        let last_y = runs.last().unwrap().2;
        runs.iter()
            .filter(|r| r.2 == last_y)
            .map(|r| (r.0.clone(), r.1))
            .collect()
    };
    let s97 = run_punct(97);
    let s96 = run_punct(96);
    assert_eq!(s97, vec![("\u{05d1})".as_bytes().to_vec(), 16)]);
    assert_eq!(
        s96, s97,
        "96-span flush lost previous BiDi level and failed to mirror/reorder '(' before bet"
    );
}

#[test]
fn span_capacity_flush_preserves_whole_word_focus_emphasis() {
    // R21-2: focus enabled, 47 pairs of "a<span uppercase>b</span>" + " x" +
    // "<span uppercase>a</span>bcdefgh" (9-letter word xabcdefgh). Both 96 and 97 span
    // slots must bold the exact first 4 letters (x, A, b, c) and leave d..h regular.
    let run_focus = |span_cap: usize, single_giant_word: bool| -> Vec<(u8, bool)> {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
        let mut text = vec![0; 512];
        let mut breaks = vec![0; 512];
        let mut levels = vec![0; 512];
        let mut spans = vec![Span::default(); span_cap];
        let mut lines = vec![Line::default(); 32];
        let mut elements = vec![Element::default(); 16];
        let mut runs = vec![Run::default(); 256];
        let mut images = vec![Image::default(); 2];
        let mut links = vec![Link::default(); 8];
        let mut page = vec![0; 4096];
        let mut style = vec![0; 256];
        let mut pl = vec![ParLink::default(); 4];
        let mut ch_rules = vec![CssRule::default(); 512];
        let mut ch_css = CssStylesheetBuilder::default();
        assert!(ch_css.begin(&mut ch_rules));
        let storage = Storage {
            text: &mut text,
            breaks: &mut breaks,
            levels: &mut levels,
            spans: &mut spans,
            lines: &mut lines,
            elements: &mut elements,
            runs: &mut runs,
            images: &mut images,
            links: &mut links,
            page: &mut page,
            style_text: &mut style,
            paragraph_links: &mut pl,
        };
        let mut breaker = SpaceBreak;
        let mut img = FixtureImages;
        let params = Params {
            width: 2400,
            height: 400,
            left: 16,
            right: 16,
            top: 16,
            bottom: 16,
            base_size: 30,
            focus: true,
            ..Params::default()
        };
        let mut e = Engine::new(params, storage, &mut backend, &mut breaker, &mut img, None, ch_css)
            .unwrap();
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, Some(b"text-align:left;margin-bottom:0"), None, None, None);
        if single_giant_word {
            for _ in 0..49 {
                e.text(b"a");
                e.start(b"span", None, Some(b"text-transform:uppercase"), None, None, None);
                e.text(b"b");
                e.end(b"span");
            }
        } else {
            for _ in 0..47 {
                e.text(b"a");
                e.start(b"span", None, Some(b"text-transform:uppercase"), None, None, None);
                e.text(b"b");
                e.end(b"span");
            }
            e.text(b" x");
            e.start(b"span", None, Some(b"text-transform:uppercase"), None, None, None);
            e.text(b"a");
            e.end(b"span");
            e.text(b"bcdefgh");
        }
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
        let mut glyphs = Vec::new();
        for r in &sink.inner.pages[0].2 {
            let bold = r.4 & BOLD != 0;
            for &b in &r.0 {
                if b != b' ' {
                    glyphs.push((b, bold));
                }
            }
        }
        glyphs
    };
    let g97 = run_focus(97, false);
    let g96 = run_focus(96, false);
    let tail97 = &g97[g97.len() - 9..];
    let tail96 = &g96[g96.len() - 9..];
    assert_eq!(
        tail97,
        &[
            (b'x', true),
            (b'A', true),
            (b'b', true),
            (b'c', true),
            (b'd', false),
            (b'e', false),
            (b'f', false),
            (b'g', false),
            (b'h', false),
        ]
    );
    assert_eq!(tail96, tail97, "96-span flush restarted focus emphasis inside 'xabcdefgh'");

    // Also verify a single 98-letter zero-space word does not restart bolding at letter 97.
    let w99 = run_focus(99, true);
    let w96 = run_focus(96, true);
    assert_eq!(w96, w99, "96-span flush restarted focus emphasis inside a 98-letter word");
}

#[test]
fn trailing_neutral_at_span_capacity_boundary_defers_for_right_hand_rtl_context() {
    // R22-1: 47 pairs of "a<span uppercase>b</span>" + plain alef (span 95) +
    // "<span uppercase>(</span>" (span 96) + plain bet (span 97). At 96 span slots,
    // the trailing '(' must defer until bet arrives so it mirrors to ')' rather than
    // painting an unmirrored '(' at the end of row 4.
    let run_case = |span_cap: usize| -> Vec<Vec<u8>> {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
        let mut text = vec![0; 512];
        let mut breaks = vec![0; 512];
        let mut levels = vec![0; 512];
        let mut spans = vec![Span::default(); span_cap];
        let mut lines = vec![Line::default(); 32];
        let mut elements = vec![Element::default(); 16];
        let mut runs = vec![Run::default(); 256];
        let mut images = vec![Image::default(); 2];
        let mut links = vec![Link::default(); 8];
        let mut page = vec![0; 4096];
        let mut style = vec![0; 256];
        let mut pl = vec![ParLink::default(); 4];
        let mut ch_rules = vec![CssRule::default(); 512];
        let mut ch_css = CssStylesheetBuilder::default();
        assert!(ch_css.begin(&mut ch_rules));
        let storage = Storage {
            text: &mut text,
            breaks: &mut breaks,
            levels: &mut levels,
            spans: &mut spans,
            lines: &mut lines,
            elements: &mut elements,
            runs: &mut runs,
            images: &mut images,
            links: &mut links,
            page: &mut page,
            style_text: &mut style,
            paragraph_links: &mut pl,
        };
        let mut breaker = SpaceBreak;
        let mut img = FixtureImages;
        let params = Params {
            width: 392,
            height: 400,
            left: 16,
            right: 16,
            top: 16,
            bottom: 16,
            base_size: 30,
            ..Params::default()
        };
        let mut e = Engine::new(params, storage, &mut backend, &mut breaker, &mut img, None, ch_css)
            .unwrap();
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, Some(b"text-align:left;margin-bottom:0"), None, None, None);
        for _ in 0..47 {
            e.text(b"a");
            e.start(b"span", None, Some(b"text-transform:uppercase"), None, None, None);
            e.text(b"b");
            e.end(b"span");
        }
        e.text("\u{05d0}".as_bytes());
        e.start(b"span", None, Some(b"text-transform:uppercase"), None, None, None);
        e.text(b"(");
        e.end(b"span");
        e.text("\u{05d1}".as_bytes());
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
        sink.inner.pages[0].2.iter().map(|r| r.0.clone()).collect()
    };
    let s97 = run_case(97);
    let s96 = run_case(96);
    assert!(s97.iter().any(|t| t == b")"), "control must mirror '(' to ')': {s97:?}");
    assert!(
        !s96.iter().any(|t| t.contains(&b'(')),
        "96-span flush emitted unmirrored '(' before bet arrived: {s96:?}"
    );
    assert!(
        s96.iter().any(|t| t == b")"),
        "96-span flush must emit mirrored ')' once bet arrives: {s96:?}"
    );
}

#[test]
fn full_text_buffer_preserves_short_word_focus_bolding_including_simultaneous_span_cut() {
    // R22-2a: 1,534 copies of "z " (3,068 B) + "xabcdefgh" in the firmware 3,072-byte
    // text buffer with focus + uppercase must bold all 4 letters XABC (not just X).
    let run_long = |text_cap: usize| -> Vec<(u8, bool)> {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
        let mut text = vec![0; text_cap];
        let mut breaks = vec![0; text_cap];
        let mut levels = vec![0; text_cap];
        let mut spans = vec![Span::default(); 32];
        let mut lines = vec![Line::default(); 256];
        let mut elements = vec![Element::default(); 16];
        let mut runs = vec![Run::default(); 512];
        let mut images = vec![Image::default(); 2];
        let mut links = vec![Link::default(); 8];
        let mut page = vec![0; 8192];
        let mut style = vec![0; 256];
        let mut pl = vec![ParLink::default(); 4];
        let mut ch_rules = vec![CssRule::default(); 512];
        let mut ch_css = CssStylesheetBuilder::default();
        assert!(ch_css.begin(&mut ch_rules));
        let storage = Storage {
            text: &mut text,
            breaks: &mut breaks,
            levels: &mut levels,
            spans: &mut spans,
            lines: &mut lines,
            elements: &mut elements,
            runs: &mut runs,
            images: &mut images,
            links: &mut links,
            page: &mut page,
            style_text: &mut style,
            paragraph_links: &mut pl,
        };
        let mut breaker = SpaceBreak;
        let mut img = FixtureImages;
        let params = Params {
            width: 800,
            height: 4000,
            left: 16,
            right: 16,
            top: 16,
            bottom: 16,
            base_size: 30,
            focus: true,
            ..Params::default()
        };
        let mut e = Engine::new(params, storage, &mut backend, &mut breaker, &mut img, None, ch_css)
            .unwrap();
        let mut body = Vec::with_capacity(1534 * 2 + 9);
        for _ in 0..1534 {
            body.extend_from_slice(b"z ");
        }
        body.extend_from_slice(b"xabcdefgh");
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, Some(b"text-transform:uppercase;text-align:left"), None, None, None);
        e.text(&body);
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
        let mut glyphs = Vec::new();
        for p in &sink.inner.pages {
            for r in &p.2 {
                let bold = r.4 & BOLD != 0;
                for &b in &r.0 {
                    if b != b' ' {
                        glyphs.push((b, bold));
                    }
                }
            }
        }
        glyphs[glyphs.len() - 9..].to_vec()
    };
    let uncut = run_long(4096);
    let fw_cap = run_long(3072);
    assert_eq!(
        uncut,
        vec![
            (b'X', true),
            (b'A', true),
            (b'B', true),
            (b'C', true),
            (b'D', false),
            (b'E', false),
            (b'F', false),
            (b'G', false),
            (b'H', false),
        ]
    );
    assert_eq!(fw_cap, uncut, "3,072-byte text-buffer cut dropped focus bolding on A/B/C");

    // R22-2b: simultaneous text_full + span_flush (98-byte text cap, 96 spans) on
    // 47 pairs + " x" + "<span uppercase>a</span>bcdefgh" keeps xAbc bold.
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
    let mut text = vec![0; 98];
    let mut breaks = vec![0; 98];
    let mut levels = vec![0; 98];
    let mut spans = vec![Span::default(); 96];
    let mut lines = vec![Line::default(); 32];
    let mut elements = vec![Element::default(); 16];
    let mut runs = vec![Run::default(); 256];
    let mut images = vec![Image::default(); 2];
    let mut links = vec![Link::default(); 8];
    let mut page = vec![0; 4096];
    let mut style = vec![0; 256];
    let mut pl = vec![ParLink::default(); 4];
    let mut ch_rules = vec![CssRule::default(); 512];
    let mut ch_css = CssStylesheetBuilder::default();
    assert!(ch_css.begin(&mut ch_rules));
    let storage = Storage {
        text: &mut text,
        breaks: &mut breaks,
        levels: &mut levels,
        spans: &mut spans,
        lines: &mut lines,
        elements: &mut elements,
        runs: &mut runs,
        images: &mut images,
        links: &mut links,
        page: &mut page,
        style_text: &mut style,
        paragraph_links: &mut pl,
    };
    let mut breaker = SpaceBreak;
    let mut img = FixtureImages;
    let params = Params {
        width: 2400,
        height: 400,
        left: 16,
        right: 16,
        top: 16,
        bottom: 16,
        base_size: 30,
        focus: true,
        ..Params::default()
    };
    let mut e = Engine::new(params, storage, &mut backend, &mut breaker, &mut img, None, ch_css)
        .unwrap();
    e.start(b"body", None, None, None, None, None);
    e.start(b"p", None, Some(b"text-align:left;margin-bottom:0"), None, None, None);
    for _ in 0..47 {
        e.text(b"a");
        e.start(b"span", None, Some(b"text-transform:uppercase"), None, None, None);
        e.text(b"b");
        e.end(b"span");
    }
    e.text(b" x");
    e.start(b"span", None, Some(b"text-transform:uppercase"), None, None, None);
    e.text(b"a");
    e.end(b"span");
    e.text(b"bcdefgh");
    e.end(b"p");
    e.end(b"body");
    assert!(e.finish());
    let mut glyphs = Vec::new();
    for r in &sink.inner.pages[0].2 {
        let bold = r.4 & BOLD != 0;
        for &b in &r.0 {
            if b != b' ' {
                glyphs.push((b, bold));
            }
        }
    }
    assert_eq!(
        &glyphs[glyphs.len() - 9..],
        &[
            (b'x', true),
            (b'A', true),
            (b'b', true),
            (b'c', true),
            (b'd', false),
            (b'e', false),
            (b'f', false),
            (b'g', false),
            (b'h', false),
        ],
        "simultaneous text-full + span-full cut lost focus bolding on 'xabcdefgh'"
    );
}

#[test]
fn span_capacity_flush_preserves_arabic_lam_alef_and_joining_forms() {
    // R22-3: 47 pairs of "a<span uppercase>b</span>" + plain 'a' (span 95) +
    // "<span uppercase>lam</span>" (span 96) + plain alef (span 97) must form U+FEFB
    // in both 96 and 97 span capacities; likewise two beh letters must emit initial
    // U+FE91 + final U+FE90 instead of isolated forms.
    let run_arabic = |span_cap: usize, first: &str, second: &str| -> Vec<Vec<u8>> {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
        let mut text = vec![0; 512];
        let mut breaks = vec![0; 512];
        let mut levels = vec![0; 512];
        let mut spans = vec![Span::default(); span_cap];
        let mut lines = vec![Line::default(); 32];
        let mut elements = vec![Element::default(); 16];
        let mut runs = vec![Run::default(); 256];
        let mut images = vec![Image::default(); 2];
        let mut links = vec![Link::default(); 8];
        let mut page = vec![0; 4096];
        let mut style = vec![0; 256];
        let mut pl = vec![ParLink::default(); 4];
        let mut ch_rules = vec![CssRule::default(); 512];
        let mut ch_css = CssStylesheetBuilder::default();
        assert!(ch_css.begin(&mut ch_rules));
        let storage = Storage {
            text: &mut text,
            breaks: &mut breaks,
            levels: &mut levels,
            spans: &mut spans,
            lines: &mut lines,
            elements: &mut elements,
            runs: &mut runs,
            images: &mut images,
            links: &mut links,
            page: &mut page,
            style_text: &mut style,
            paragraph_links: &mut pl,
        };
        let mut breaker = SpaceBreak;
        let mut img = FixtureImages;
        let params = Params {
            width: 2400,
            height: 400,
            left: 16,
            right: 16,
            top: 16,
            bottom: 16,
            base_size: 30,
            ..Params::default()
        };
        let mut e = Engine::new(params, storage, &mut backend, &mut breaker, &mut img, None, ch_css)
            .unwrap();
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, Some(b"text-align:left;margin-bottom:0"), None, None, None);
        for _ in 0..47 {
            e.text(b"a");
            e.start(b"span", None, Some(b"text-transform:uppercase"), None, None, None);
            e.text(b"b");
            e.end(b"span");
        }
        e.text(b"a");
        e.start(b"span", None, Some(b"text-transform:uppercase"), None, None, None);
        e.text(first.as_bytes());
        e.end(b"span");
        e.text(second.as_bytes());
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
        sink.inner.pages[0].2.iter().map(|r| r.0.clone()).collect()
    };
    let lam97 = run_arabic(97, "\u{0644}", "\u{0627}");
    let lam96 = run_arabic(96, "\u{0644}", "\u{0627}");
    let fefb = "\u{fefb}".as_bytes().to_vec();
    assert!(lam97.contains(&fefb), "97-span control must form U+FEFB: {lam97:?}");
    assert!(lam96.contains(&fefb), "96-span flush broke lam-alef ligature into isolated glyphs: {lam96:?}");

    let beh97 = run_arabic(97, "\u{0628}", "\u{0628}");
    let beh96 = run_arabic(96, "\u{0628}", "\u{0628}");
    let init_beh = "\u{fe91}".as_bytes().to_vec();
    let final_beh = "\u{fe90}".as_bytes().to_vec();
    assert!(beh97.contains(&init_beh) && beh97.contains(&final_beh));
    assert!(
        beh96.contains(&init_beh) && beh96.contains(&final_beh),
        "96-span flush broke Arabic beh-beh joining into isolated forms: {beh96:?}"
    );
}

#[test]
fn utf8_split_at_text_capacity_preserves_arabic_focus_and_rtl_punctuation() {
    // R23-1: 3,072-byte firmware text buffer (3,071 payload bytes) cutting inside a
    // multi-byte UTF-8 scalar must still apply continuation_rewind at the recovered
    // scalar boundary for: (a) Arabic lam+alef -> U+FEFB, (b) focus on "xaébcdefg" ->
    // X,A,É,B bold, and (c) LTR prefix + alef + '(' + split bet -> mirrored ')'.
    let run_fw = |text_cap: usize, focus: bool, body: &[u8]| -> Vec<(Vec<u8>, u8)> {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
        let mut text = vec![0; text_cap];
        let mut breaks = vec![0; text_cap];
        let mut levels = vec![0; text_cap];
        let mut spans = vec![Span::default(); 32];
        let mut lines = vec![Line::default(); 256];
        let mut elements = vec![Element::default(); 16];
        let mut runs = vec![Run::default(); 512];
        let mut images = vec![Image::default(); 2];
        let mut links = vec![Link::default(); 8];
        let mut page = vec![0; 8192];
        let mut style = vec![0; 256];
        let mut pl = vec![ParLink::default(); 4];
        let mut ch_rules = vec![CssRule::default(); 512];
        let mut ch_css = CssStylesheetBuilder::default();
        assert!(ch_css.begin(&mut ch_rules));
        let storage = Storage {
            text: &mut text,
            breaks: &mut breaks,
            levels: &mut levels,
            spans: &mut spans,
            lines: &mut lines,
            elements: &mut elements,
            runs: &mut runs,
            images: &mut images,
            links: &mut links,
            page: &mut page,
            style_text: &mut style,
            paragraph_links: &mut pl,
        };
        let mut breaker = SpaceBreak;
        let mut img = FixtureImages;
        let params = Params {
            width: 800,
            height: 4000,
            left: 16,
            right: 16,
            top: 16,
            bottom: 16,
            base_size: 30,
            focus,
            ..Params::default()
        };
        let mut e = Engine::new(params, storage, &mut backend, &mut breaker, &mut img, None, ch_css)
            .unwrap();
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, Some(b"text-transform:uppercase;text-align:left"), None, None, None);
        e.text(body);
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
        sink.inner
            .pages
            .iter()
            .flat_map(|p| p.2.iter().map(|r| (r.0.clone(), r.4)))
            .collect()
    };

    // (a) 1,534x "z " (3,068 B) + lam (2 B) + alef (2 B, split at byte 3,071).
    let mut arabic_body = Vec::with_capacity(3072);
    for _ in 0..1534 {
        arabic_body.extend_from_slice(b"z ");
    }
    arabic_body.extend_from_slice("\u{0644}\u{0627}".as_bytes());
    let fefb = "\u{fefb}".as_bytes();
    assert!(
        run_fw(3072, false, &arabic_body)
            .iter()
            .any(|(t, _)| t.windows(fefb.len()).any(|w| w == fefb)),
        "UTF-8 split inside alef broke lam-alef U+FEFB"
    );

    // (b) 1,534x "z " (3,068 B) + "xaébcdefg" (é split at byte 3,071) with focus.
    let mut focus_body = Vec::with_capacity(3080);
    for _ in 0..1534 {
        focus_body.extend_from_slice(b"z ");
    }
    focus_body.extend_from_slice("xa\u{00e9}bcdefg".as_bytes());
    let extract_chars = |runs: Vec<(Vec<u8>, u8)>| -> Vec<(char, bool)> {
        let mut out = Vec::new();
        for (bytes, flags) in runs {
            let bold = flags & BOLD != 0;
            for ch in core::str::from_utf8(&bytes).unwrap().chars() {
                if ch != ' ' {
                    out.push((ch, bold));
                }
            }
        }
        out[out.len() - 9..].to_vec()
    };
    let focus_uncut = extract_chars(run_fw(4096, true, &focus_body));
    let focus_cut = extract_chars(run_fw(3072, true, &focus_body));
    assert_eq!(
        focus_uncut,
        vec![
            ('X', true),
            ('A', true),
            ('\u{00c9}', true),
            ('B', true),
            ('C', false),
            ('D', false),
            ('E', false),
            ('F', false),
            ('G', false),
        ]
    );
    assert_eq!(focus_cut, focus_uncut, "UTF-8 split inside 'é' lost focus bolding");

    // (c) 3,067 B LTR prefix ending in ' ' + alef (2 B) + '(' (1 B) + bet (2 B, split at 3,071).
    let mut rtl_body = Vec::with_capacity(3072);
    rtl_body.extend_from_slice(b"aa");
    for _ in 0..1532 {
        rtl_body.extend_from_slice(b" z");
    }
    rtl_body.push(b' ');
    assert_eq!(rtl_body.len(), 3067);
    rtl_body.extend_from_slice("\u{05d0}(\u{05d1}".as_bytes());
    let rtl_cut = run_fw(3072, false, &rtl_body);
    assert!(
        !rtl_cut.iter().any(|(t, _)| t.contains(&b'(')),
        "UTF-8 split inside bet emitted unmirrored '('"
    );
    assert!(
        rtl_cut.iter().any(|(t, _)| t.contains(&b')')),
        "UTF-8 split inside bet must mirror '(' to ')'"
    );
}

#[test]
fn neutral_deferral_spans_spaces_and_cascades_after_arabic_rewind() {
    // R23-2a: 3,067 B LTR prefix ending in ' ' + alef + "( " + bet in one callback
    // (payload ends on the space after '('). Must defer "( " and mirror '(' to ')'.
    let mut body = Vec::with_capacity(3074);
    body.extend_from_slice(b"aa");
    for _ in 0..1532 {
        body.extend_from_slice(b" z");
    }
    body.push(b' ');
    assert_eq!(body.len(), 3067);
    body.extend_from_slice("\u{05d0}( \u{05d1}".as_bytes());
    let mut font = StrictCaseFont::default();
    let mut sink = AnchorCapture::default();
    let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
    let mut text = vec![0; 3072];
    let mut breaks = vec![0; 3072];
    let mut levels = vec![0; 3072];
    let mut spans = vec![Span::default(); 32];
    let mut lines = vec![Line::default(); 256];
    let mut elements = vec![Element::default(); 16];
    let mut runs = vec![Run::default(); 512];
    let mut images = vec![Image::default(); 2];
    let mut links = vec![Link::default(); 8];
    let mut page = vec![0; 8192];
    let mut style = vec![0; 256];
    let mut pl = vec![ParLink::default(); 4];
    let mut ch_rules = vec![CssRule::default(); 512];
    let mut ch_css = CssStylesheetBuilder::default();
    assert!(ch_css.begin(&mut ch_rules));
    let storage = Storage {
        text: &mut text,
        breaks: &mut breaks,
        levels: &mut levels,
        spans: &mut spans,
        lines: &mut lines,
        elements: &mut elements,
        runs: &mut runs,
        images: &mut images,
        links: &mut links,
        page: &mut page,
        style_text: &mut style,
        paragraph_links: &mut pl,
    };
    let mut breaker = SpaceBreak;
    let mut img = FixtureImages;
    let params = Params {
        width: 800,
        height: 4000,
        left: 16,
        right: 16,
        top: 16,
        bottom: 16,
        base_size: 30,
        ..Params::default()
    };
    let mut e = Engine::new(params, storage, &mut backend, &mut breaker, &mut img, None, ch_css)
        .unwrap();
    e.start(b"body", None, None, None, None, None);
    e.start(b"p", None, Some(b"text-transform:uppercase;text-align:left"), None, None, None);
    e.text(&body);
    e.end(b"p");
    e.end(b"body");
    assert!(e.finish());
    let all_runs: Vec<Vec<u8>> = sink
        .inner
        .pages
        .iter()
        .flat_map(|p| p.2.iter().map(|r| r.0.clone()))
        .collect();
    assert!(!all_runs.iter().any(|t| t.contains(&b'(')), "trailing '( ' emitted unmirrored '('");
    assert!(all_runs.iter().any(|t| t.contains(&b')')), "trailing '( ' must mirror to ')'");

    // R23-2b: 47 pairs + plain "א(" (span 95) + styled Arabic beh (span 96) + plain beh (span 97).
    // At 96 span slots, Arabic rewind past beh must cascade to defer '(' too so '(' mirrors
    // to ')' AND both beh letters keep their joining forms U+FE91 + U+FE90.
    let mut font2 = StrictCaseFont::default();
    let mut sink2 = AnchorCapture::default();
    let mut backend2 = SplitBackend { font: &mut font2, sink: &mut sink2 };
    let mut text2 = vec![0; 512];
    let mut breaks2 = vec![0; 512];
    let mut levels2 = vec![0; 512];
    let mut spans2 = vec![Span::default(); 96];
    let mut lines2 = vec![Line::default(); 32];
    let mut elements2 = vec![Element::default(); 16];
    let mut runs2 = vec![Run::default(); 256];
    let mut images2 = vec![Image::default(); 2];
    let mut links2 = vec![Link::default(); 8];
    let mut page2 = vec![0; 4096];
    let mut style2 = vec![0; 256];
    let mut pl2 = vec![ParLink::default(); 4];
    let mut ch_rules2 = vec![CssRule::default(); 512];
    let mut ch_css2 = CssStylesheetBuilder::default();
    assert!(ch_css2.begin(&mut ch_rules2));
    let storage2 = Storage {
        text: &mut text2,
        breaks: &mut breaks2,
        levels: &mut levels2,
        spans: &mut spans2,
        lines: &mut lines2,
        elements: &mut elements2,
        runs: &mut runs2,
        images: &mut images2,
        links: &mut links2,
        page: &mut page2,
        style_text: &mut style2,
        paragraph_links: &mut pl2,
    };
    let mut breaker2 = SpaceBreak;
    let mut img2 = FixtureImages;
    let params2 = Params {
        width: 2400,
        height: 400,
        left: 16,
        right: 16,
        top: 16,
        bottom: 16,
        base_size: 30,
        ..Params::default()
    };
    let mut e2 = Engine::new(params2, storage2, &mut backend2, &mut breaker2, &mut img2, None, ch_css2)
        .unwrap();
    e2.start(b"body", None, None, None, None, None);
    e2.start(b"p", None, Some(b"text-align:left;margin-bottom:0"), None, None, None);
    for _ in 0..47 {
        e2.text(b"a");
        e2.start(b"span", None, Some(b"text-transform:uppercase"), None, None, None);
        e2.text(b"b");
        e2.end(b"span");
    }
    e2.text("\u{05d0}(".as_bytes());
    e2.start(b"span", None, Some(b"text-transform:uppercase"), None, None, None);
    e2.text("\u{0628}".as_bytes());
    e2.end(b"span");
    e2.text("\u{0628}".as_bytes());
    e2.end(b"p");
    e2.end(b"body");
    assert!(e2.finish());
    let runs_b: Vec<Vec<u8>> = sink2.inner.pages[0].2.iter().map(|r| r.0.clone()).collect();
    assert!(!runs_b.iter().any(|t| t.contains(&b'(')), "Arabic rewind left '(' unmirrored: {runs_b:?}");
    assert!(runs_b.iter().any(|t| t.contains(&b')')), "Arabic rewind must cascade to mirror '(': {runs_b:?}");
    assert!(runs_b.iter().any(|t| t.windows(3).any(|w| w == "\u{fe91}".as_bytes())));
    assert!(runs_b.iter().any(|t| t.windows(3).any(|w| w == "\u{fe90}".as_bytes())));
}

#[test]
fn lowercased_latin_ligature_survives_text_capacity_boundary_without_focus() {
    // R23-3: 1,535 copies of "z " (3,070 B) + "FI" under text-transform:lowercase with
    // ligatures enabled and focus OFF places 'F' in byte 3,070 and 'I' in chunk 2.
    // Both 3,072-byte and 4,096-byte text buffers must emit the U+FB01 fi ligature.
    let run_lig = |text_cap: usize| -> Vec<Vec<u8>> {
        let mut font = StrictCaseFont { enable_ligature: true, ..StrictCaseFont::default() };
        let mut sink = AnchorCapture::default();
        let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
        let mut text = vec![0; text_cap];
        let mut breaks = vec![0; text_cap];
        let mut levels = vec![0; text_cap];
        let mut spans = vec![Span::default(); 32];
        let mut lines = vec![Line::default(); 256];
        let mut elements = vec![Element::default(); 16];
        let mut runs = vec![Run::default(); 512];
        let mut images = vec![Image::default(); 2];
        let mut links = vec![Link::default(); 8];
        let mut page = vec![0; 8192];
        let mut style = vec![0; 256];
        let mut pl = vec![ParLink::default(); 4];
        let mut ch_rules = vec![CssRule::default(); 512];
        let mut ch_css = CssStylesheetBuilder::default();
        assert!(ch_css.begin(&mut ch_rules));
        let storage = Storage {
            text: &mut text,
            breaks: &mut breaks,
            levels: &mut levels,
            spans: &mut spans,
            lines: &mut lines,
            elements: &mut elements,
            runs: &mut runs,
            images: &mut images,
            links: &mut links,
            page: &mut page,
            style_text: &mut style,
            paragraph_links: &mut pl,
        };
        let mut breaker = SpaceBreak;
        let mut img = FixtureImages;
        let params = Params {
            width: 800,
            height: 4000,
            left: 16,
            right: 16,
            top: 16,
            bottom: 16,
            base_size: 30,
            focus: false,
            ..Params::default()
        };
        let mut e = Engine::new(params, storage, &mut backend, &mut breaker, &mut img, None, ch_css)
            .unwrap();
        let mut body = Vec::with_capacity(3072);
        for _ in 0..1535 {
            body.extend_from_slice(b"z ");
        }
        body.extend_from_slice(b"FI");
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, Some(b"text-transform:lowercase;text-align:left"), None, None, None);
        e.text(&body);
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
        sink.inner
            .pages
            .iter()
            .flat_map(|p| p.2.iter().map(|r| r.0.clone()))
            .collect()
    };
    let fb01 = "\u{fb01}".as_bytes();
    assert!(run_lig(4096).iter().any(|t| t.windows(fb01.len()).any(|w| w == fb01)));
    assert!(
        run_lig(3072).iter().any(|t| t.windows(fb01.len()).any(|w| w == fb01)),
        "3,072-byte text-buffer cut broke lowercased 'FI' -> U+FB01 ligature"
    );
}

#[test]
fn inline_ascent_uses_transformed_glyph_and_small_cap_size_with_masked_style() {
    // Interop with #163 inline_ascent: Font::ink_extents must receive the transformed
    // codepoint (e.g. 'ǚ' U+01DA -> 'Ǚ' U+01D9), the reduced small-cap pixel size for
    // lowercase small-caps runs (including trailing break hyphens), and STYLE_MASK-only
    // style bits (no case bits leaked).
    struct InkAuditFont {
        calls: Vec<(u32, u16, u8)>,
    }
    impl Font for InkAuditFont {
        fn covers(&mut self, _: u32) -> bool { true }
        fn advance(&mut self, _: u32, size: u16, flags: u8) -> i16 {
            assert_eq!(flags & !STYLE_MASK, 0);
            (size / 2).max(1) as i16
        }
        fn line_height(&mut self, size: u16) -> i16 { (i32::from(size) * 12 / 10) as i16 }
        fn ascent(&mut self, size: u16) -> i16 { size as i16 }
        fn ink_extents(&mut self, cp: u32, size: u16, flags: u8) -> Option<(i16, i16)> {
            assert_eq!(flags & !STYLE_MASK, 0, "case bits leaked into Font::ink_extents");
            self.calls.push((cp, size, flags));
            if cp == 0x01d9 {
                Some(((i32::from(size) * 80 / 75) as i16, 0))
            } else {
                Some((size as i16, 0))
            }
        }
    }
    let mut font = InkAuditFont { calls: Vec::new() };
    let mut sink = AnchorCapture::default();
    let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
    let mut breaker = OneLine;
    let mut images = FixtureImages;
    let mut text = [0; 256];
    let mut breaks = [0; 256];
    let mut levels = [0; 256];
    let mut spans = [Span::default(); 64];
    let mut lines = [Line::default(); 16];
    let mut elements = [Element::default(); 16];
    let mut runs = [Run::default(); 64];
    let mut imgs = [Image::default(); 2];
    let mut links = [Link::default(); 4];
    let mut page = [0; 4096];
    let mut style = [0; 128];
    let mut pl = [ParLink::default(); 4];
    let params = Params {
        width: 240,
        height: 400,
        left: 16,
        right: 16,
        top: 16,
        bottom: 16,
        base_size: 30,
        inline_ascent: true,
        ..Params::default()
    };
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
    let mut e = Engine::new(
        params,
        storage,
        &mut backend,
        &mut breaker,
        &mut images,
        None,
        CssStylesheetBuilder::default(),
    )
    .unwrap();
    e.start(b"body", None, None, None, None, None);
    e.start(b"p", None, Some(b"text-align:left"), None, None, None);
    e.start(b"span", None, Some(b"font-size:250%;text-transform:uppercase"), None, None, None);
    e.text("\u{01da}".as_bytes());
    e.end(b"span");
    e.start(b"span", None, Some(b"font-size:250%;font-variant:small-caps"), None, None, None);
    e.text(b"a");
    e.end(b"span");
    e.end(b"p");
    // Second paragraph: tall small-caps word with a discretionary break in a trailing non-tall
    // soft-hyphen span. The emitted trailing '-' uses small_cap_px(75) = 60 px in both
    // emit_segment and line_extra.
    e.start(b"p", None, Some(b"text-align:left"), None, None, None);
    e.start(b"span", None, Some(b"font-size:250%;font-variant:small-caps"), None, None, None);
    e.text(b"abc");
    e.end(b"span");
    e.start(b"span", None, None, None, None, None);
    e.text("\u{00ad}".as_bytes());
    e.end(b"span");
    e.start(b"span", None, Some(b"font-size:250%;font-variant:small-caps"), None, None, None);
    e.text(b"defgh");
    e.end(b"span");
    e.end(b"p");
    e.end(b"body");
    assert!(e.finish());
    // First span transformed U+01DA -> U+01D9 at 75 px (80 px above => baseline = 16 + 80 = 96).
    assert!(
        font.calls.contains(&(0x01d9, 75, 0)),
        "ink_extents did not receive transformed U+01D9 at 75 px: {:?}",
        font.calls
    );
    // Second span is lowercase small-caps 'a' -> 'A' at small_cap_px(75) = 60 px.
    let sc75 = case::small_cap_px(75);
    assert!(
        font.calls.contains(&(b'A' as u32, sc75, 0)),
        "ink_extents did not receive small-cap 'A' at {sc75} px: {:?}",
        font.calls
    );
    // Discretionary hyphen inherits the preceding tall small-caps run size (60 px).
    assert!(
        font.calls.contains(&(b'-' as u32, sc75, 0)),
        "ink_extents did not receive small-cap '-' at {sc75} px: {:?}",
        font.calls
    );
    let runs = &sink.inner.pages[0].2;
    assert_eq!(runs[0].2, 96, "page-top transformed Ǚ must reserve 80 px above baseline: {runs:?}");
}

#[test]
fn neutral_rewind_preserves_ltr_possessives_punctuation_and_rtl_embedded_possessives() {
    // R24-2: When a trailing BiDi neutral's level is already determined (`previous == base`),
    // continuation_rewind must not split closing punctuation/apostrophes onto a new row or
    // break focus bolding (`dogs' toys`, `dogs. toys`). When `previous != base` (e.g., an
    // embedded English possessive in an RTL paragraph), moving the apostrophe must also move
    // its word so focus length stays intact.
    let run_case = |cap: usize, prefix: &[u8], z_copies: usize, tail: &[u8], focus: bool| {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
        let mut breaker = SpaceBreak;
        let mut images = FixtureImages;
        let mut text = vec![0u8; cap];
        let mut breaks = vec![0u8; cap];
        let mut levels = vec![0u8; cap];
        let mut spans = [Span::default(); 96];
        let mut lines = [Line::default(); 256];
        let mut elements = [Element::default(); 16];
        let mut runs = [Run::default(); 512];
        let mut imgs = [Image::default(); 2];
        let mut links = [Link::default(); 4];
        let mut page = vec![0u8; 32768];
        let mut style = [0u8; 128];
        let mut pl = [ParLink::default(); 4];
        let params = Params {
            width: 300,
            height: 2000,
            left: 10,
            right: 10,
            top: 10,
            bottom: 10,
            base_size: 20,
            focus,
            ..Params::default()
        };
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
        let mut e = Engine::new(
            params,
            storage,
            &mut backend,
            &mut breaker,
            &mut images,
            None,
            CssStylesheetBuilder::default(),
        )
        .unwrap();
        let mut body = Vec::with_capacity(prefix.len() + z_copies * 2 + tail.len());
        body.extend_from_slice(prefix);
        for _ in 0..z_copies {
            body.extend_from_slice(b"z ");
        }
        body.extend_from_slice(tail);
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, Some(b"text-transform:uppercase;text-align:left"), None, None, None);
        e.text(&body);
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
        sink.inner
            .pages
            .into_iter()
            .flat_map(|p| p.2.into_iter())
            .collect::<Vec<_>>()
    };

    // 1. LTR focus possessive: 1,533 * "z " (3,066 B) + "dogs'" (5 B) = 3,071 B.
    //    Both 3,072 B and 4,096 B bold "DO" and keep "GS'" unbolded on the same baseline.
    for cap in [4096usize, 3072] {
        let runs = run_case(cap, b"", 1533, b"dogs' toys", true);
        let pos = runs.iter().position(|r| r.0 == b"DO" && r.4 == BOLD)
            .unwrap_or_else(|| panic!("cap={cap}: missing bold 'DO' in {:?}", &runs[runs.len().saturating_sub(8)..]));
        let next = &runs[pos + 1];
        assert!(
            next.0.starts_with(b"GS'") && next.4 == 0 && next.2 == runs[pos].2,
            "cap={cap}: possessive split across row or focus: {:?} -> {:?}",
            runs[pos], next
        );
    }

    // 2. LTR non-focus period: "dogs. toys" must not split "." onto the next row.
    let dot_runs = run_case(3072, b"", 1533, b"dogs. toys", false);
    assert!(
        dot_runs.iter().any(|r| r.0.windows(5).any(|w| w == b"DOGS.")),
        "LTR closing period split onto a new row: {:?}",
        &dot_runs[dot_runs.len().saturating_sub(6)..]
    );

    // 3. RTL paragraph with embedded LTR possessive ending at byte 3,071:
    //    "\u{05d0}a " (4 B) + 1,531 * "z " (3,062 B) + "dogs'" (5 B) = 3,071 B.
    for cap in [4096usize, 3072] {
        let runs = run_case(cap, "\u{05d0}a ".as_bytes(), 1531, b"dogs' toys", true);
        let pos = runs.iter().position(|r| r.0 == b"DO" && r.4 == BOLD)
            .unwrap_or_else(|| panic!("rtl cap={cap}: missing bold 'DO' in {:?}", &runs[runs.len().saturating_sub(8)..]));
        let next = &runs[pos + 1];
        assert!(
            next.0.starts_with(b"GS'") && next.4 == 0 && next.2 == runs[pos].2,
            "rtl cap={cap}: embedded possessive split focus/row: {:?} -> {:?}",
            runs[pos], next
        );
    }

    // 4. R25-1: RTL paragraph with embedded LTR closing period ("dogs. toys", focus off)
    //    and combining acute accent ("the\u{0301} toys", focus off), both ending at byte 3,071.
    //    Neither the period nor U+0301 may split onto a later baseline from its preceding letter,
    //    and both must keep level 2 with the right-hand LTR context ("DOGS. TOYS" / "THÉ TOYS").
    for cap in [4096usize, 3072] {
        let runs = run_case(cap, "\u{05d0}a ".as_bytes(), 1531, b"dogs. toys", false);
        assert!(
            runs.iter().any(|r| r.0.windows(10).any(|w| w == b"DOGS. TOYS")),
            "rtl period cap={cap}: 'DOGS. TOYS' split baseline or lost BiDi level 2: {:?}",
            &runs[runs.len().saturating_sub(6)..]
        );
    }
    let acute_seq = "THE\u{0301} TOYS".as_bytes();
    for cap in [4096usize, 3072] {
        let runs = run_case(cap, "\u{05d0}a ".as_bytes(), 1531, "the\u{0301} toys".as_bytes(), false);
        assert!(
            runs.iter().any(|r| r.0.windows(acute_seq.len()).any(|w| w == acute_seq)),
            "rtl combining acute cap={cap}: U+0301 split baseline or lost BiDi level 2: {:?}",
            &runs[runs.len().saturating_sub(6)..]
        );
    }

    // 5. Direct LTR boundary right before closing period ("dogs" | ". toys") and right before
    //    combining acute ("the" | "\u{0301} toys") keeps both attached to their base word.
    let ltr_direct_dot = run_case(3072, b"a", 1533, b"dogs. toys", false);
    assert!(
        ltr_direct_dot.iter().any(|r| r.0.windows(5).any(|w| w == b"DOGS.")),
        "LTR direct period split across rows: {:?}",
        &ltr_direct_dot[ltr_direct_dot.len().saturating_sub(6)..]
    );
    let ltr_direct_acute = run_case(3072, b"aa", 1533, "the\u{0301} toys".as_bytes(), false);
    assert!(
        ltr_direct_acute
            .iter()
            .any(|r| r.0.windows(5).any(|w| w == "THE\u{0301}".as_bytes())),
        "LTR direct combining acute split across rows: {:?}",
        &ltr_direct_acute[ltr_direct_acute.len().saturating_sub(6)..]
    );

    // 6. R26-1: Direct cut between a Hebrew consonant (U+05D1 BET) and its vowel point
    //    (U+05B7 PATAH) must keep both on the same baseline (matching 4,096 B control).
    //    Note RTL run reversal places PATAH before BET in visual run bytes.
    let bet_patah_visual = "\u{05b7}\u{05d1}".as_bytes();
    for cap in [4096usize, 3072] {
        let runs = run_case(cap, b"a", 1534, "\u{05d1}\u{05b7}t".as_bytes(), false);
        assert!(
            runs.iter().any(|r| r.0.windows(bet_patah_visual.len()).any(|w| w == bet_patah_visual)),
            "Hebrew bet+patah cap={cap}: vowel point split onto another baseline: {:?}",
            &runs[runs.len().saturating_sub(6)..]
        );
    }

    // 7. R27-1 & R31-1: At the 96-span limit, leading punctuation ('!'), an all-letter
    //    spaceless word, or an intervening U+00AD soft hyphen before a combining acute accent
    //    (U+0301) must never discard a valid grapheme/word cut and move U+0301 onto a later
    //    baseline than its preceding base letter.
    let run_span_accent = |span_cap: usize, first: &[u8], tail_shy: bool| {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
        let mut text = vec![0u8; 512];
        let mut breaks = vec![0u8; 512];
        let mut levels = vec![0u8; 512];
        let mut spans = vec![Span::default(); span_cap];
        let mut lines = vec![Line::default(); 32];
        let mut elements = vec![Element::default(); 16];
        let mut runs = vec![Run::default(); 256];
        let mut imgs = [Image::default(); 2];
        let mut links = [Link::default(); 4];
        let mut page = vec![0u8; 8192];
        let mut style = [0u8; 128];
        let mut pl = [ParLink::default(); 4];
        let params = Params {
            width: 3000,
            height: 400,
            left: 16,
            right: 16,
            top: 16,
            bottom: 16,
            base_size: 20,
            ..Params::default()
        };
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
        let mut breaker = SpaceBreak;
        let mut img = FixtureImages;
        let mut e = Engine::new(
            params,
            storage,
            &mut backend,
            &mut breaker,
            &mut img,
            None,
            CssStylesheetBuilder::default(),
        )
        .unwrap();
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, Some(b"text-align:left;margin-bottom:0"), None, None, None);
        e.text(first);
        for k in 0..95 {
            if k % 2 == 0 {
                e.start(b"span", None, Some(b"text-transform:uppercase"), None, None, None);
                if tail_shy && k == 94 {
                    e.text("a\u{00ad}".as_bytes());
                } else {
                    e.text(b"a");
                }
                e.end(b"span");
            } else {
                e.text(b"a");
            }
        }
        e.text("\u{0301}".as_bytes());
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
        sink.inner.pages[0].2.clone()
    };
    let acute_bytes = "\u{0301}".as_bytes();
    for (first, tail_shy) in [(b"!".as_slice(), false), (b"a".as_slice(), false), (b"a".as_slice(), true)] {
        for span_cap in [97usize, 96] {
            let runs = run_span_accent(span_cap, first, tail_shy);
            let acute_idx = runs
                .iter()
                .rposition(|r| r.0.windows(acute_bytes.len()).any(|w| w == acute_bytes))
                .unwrap_or_else(|| panic!("first={first:?} tail_shy={tail_shy} span_cap={span_cap}: missing U+0301"));
            let base_idx = runs[..=acute_idx]
                .iter()
                .rposition(|r| r.0.iter().any(|&b| b == b'A' || b == b'a'))
                .unwrap_or_else(|| panic!("first={first:?} tail_shy={tail_shy} span_cap={span_cap}: missing base letter"));
            assert_eq!(
                runs[acute_idx].2, runs[base_idx].2,
                "first={first:?} tail_shy={tail_shy} span_cap={span_cap}: U+0301 moved to a different baseline from its base letter: {:?}",
                &runs[runs.len().saturating_sub(4)..]
            );
        }
    }
}

#[test]
fn split_utf8_callback_across_text_capacity_preserves_focus_word() {
    // R24-3: When a caller splits a multi-byte UTF-8 scalar (here U+2019 = E2 80 99) across
    // two text() callbacks right at the 3,072-byte buffer boundary, conservative lookahead
    // preserves the preceding word so focus emphasis matches a 4,096-byte buffer.
    let run_split = |cap: usize| {
        let mut font = StrictCaseFont::default();
        let mut sink = AnchorCapture::default();
        let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
        let mut breaker = SpaceBreak;
        let mut images = FixtureImages;
        let mut text = vec![0u8; cap];
        let mut breaks = vec![0u8; cap];
        let mut levels = vec![0u8; cap];
        let mut spans = [Span::default(); 96];
        let mut lines = [Line::default(); 256];
        let mut elements = [Element::default(); 16];
        let mut runs = [Run::default(); 512];
        let mut imgs = [Image::default(); 2];
        let mut links = [Link::default(); 4];
        let mut page = vec![0u8; 32768];
        let mut style = [0u8; 128];
        let mut pl = [ParLink::default(); 4];
        let params = Params {
            width: 300,
            height: 2000,
            left: 10,
            right: 10,
            top: 10,
            bottom: 10,
            base_size: 20,
            focus: true,
            ..Params::default()
        };
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
        let mut e = Engine::new(
            params,
            storage,
            &mut backend,
            &mut breaker,
            &mut images,
            None,
            CssStylesheetBuilder::default(),
        )
        .unwrap();
        let mut cb1 = Vec::with_capacity(1534 * 2 + 4);
        for _ in 0..1534 {
            cb1.extend_from_slice(b"z ");
        }
        cb1.extend_from_slice(b"ab");
        cb1.extend_from_slice(&[0xe2, 0x80]);
        let cb2 = [0x99, b'c', b'd', b'e', b'f', b'g', b'h'];
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, Some(b"text-transform:uppercase;text-align:left"), None, None, None);
        e.text(&cb1);
        e.text(&cb2);
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
        sink.inner
            .pages
            .into_iter()
            .flat_map(|p| p.2.into_iter())
            .collect::<Vec<_>>()
    };
    let expected_bold = "AB\u{2019}C".as_bytes();
    for cap in [4096usize, 3072] {
        let runs = run_split(cap);
        let pos = runs.iter().position(|r| r.0 == expected_bold && r.4 == BOLD)
            .unwrap_or_else(|| panic!("cap={cap}: missing 4-char bold prefix 'AB’C' in {:?}", &runs[runs.len().saturating_sub(8)..]));
        let next = &runs[pos + 1];
        assert_eq!(
            (next.0.as_slice(), next.4, next.2),
            (b"DEFGH".as_slice(), 0, runs[pos].2),
            "cap={cap}: remainder mismatch after bold prefix"
        );
    }
}

#[test]
fn inline_ascent_measures_cross_span_arabic_lam_alef_ligature() {
    // R24-4: When Arabic lam sits in a 250% span and alef sits in a nested case-styled span,
    // segment_line forms the U+FEFB ligature in the outer tall span. line_extra must query
    // ink_extents for the emitted U+FEFB (not U+FEDF / U+FE8E) and reserve its 85 px height.
    struct ArabicInkFont {
        calls: Vec<(u32, u16, u8)>,
    }
    impl Font for ArabicInkFont {
        fn covers(&mut self, _: u32) -> bool { true }
        fn advance(&mut self, _: u32, size: u16, flags: u8) -> i16 {
            assert_eq!(flags & !STYLE_MASK, 0);
            (size / 2).max(1) as i16
        }
        fn line_height(&mut self, size: u16) -> i16 { (i32::from(size) * 12 / 10) as i16 }
        fn ascent(&mut self, size: u16) -> i16 { size as i16 }
        fn ink_extents(&mut self, cp: u32, size: u16, flags: u8) -> Option<(i16, i16)> {
            assert_eq!(flags & !STYLE_MASK, 0);
            self.calls.push((cp, size, flags));
            if cp == 0xfefb {
                Some((85, 0))
            } else {
                Some((size as i16, 0))
            }
        }
    }
    let run_lam_alef = |outer_style: &[u8], inner_style: &[u8]| {
        let mut font = ArabicInkFont { calls: Vec::new() };
        let mut sink = AnchorCapture::default();
        let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
        let mut breaker = OneLine;
        let mut images = FixtureImages;
        let mut text = [0; 256];
        let mut breaks = [0; 256];
        let mut levels = [0; 256];
        let mut spans = [Span::default(); 32];
        let mut lines = [Line::default(); 16];
        let mut elements = [Element::default(); 16];
        let mut runs = [Run::default(); 32];
        let mut imgs = [Image::default(); 2];
        let mut links = [Link::default(); 4];
        let mut page = [0; 4096];
        let mut style = [0; 128];
        let mut pl = [ParLink::default(); 4];
        let params = Params {
            width: 300,
            height: 400,
            left: 16,
            right: 16,
            top: 16,
            bottom: 16,
            base_size: 30,
            inline_ascent: true,
            ..Params::default()
        };
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
        let mut e = Engine::new(
            params,
            storage,
            &mut backend,
            &mut breaker,
            &mut images,
            None,
            CssStylesheetBuilder::default(),
        )
        .unwrap();
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, None, None, None, None);
        e.start(b"span", None, Some(outer_style), None, None, None);
        e.text("\u{0644}".as_bytes());
        e.start(b"span", None, Some(inner_style), None, None, None);
        e.text("\u{0627}".as_bytes());
        e.end(b"span");
        e.end(b"span");
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
        (font.calls, sink.inner.pages[0].2.clone())
    };

    // Case 1: 250% outer lam + nested uppercase alef -> emits U+FEFB at 75 px, reserves 85 px.
    let (calls, runs) = run_lam_alef(b"font-size:250%", b"text-transform:uppercase");
    assert_eq!(runs.len(), 1, "expected single lam-alef run: {runs:?}");
    assert_eq!(runs[0].0, "\u{fefb}".as_bytes());
    assert_eq!(runs[0].3, 75);
    assert_eq!(runs[0].2, 16 + 85, "baseline must reserve 85 px above top margin: {runs:?}");
    assert!(calls.contains(&(0xfefb, 75, 0)), "missing U+FEFB ink query: {calls:?}");
    assert!(
        !calls.iter().any(|&(cp, _, _)| cp == 0xfedf || cp == 0xfe8e),
        "must not query unligated lam/alef forms: {calls:?}"
    );

    // Case 2: 100% outer lam + nested 250% uppercase alef -> alef is absorbed at 30 px,
    // so line_extra must not reserve phantom 75 px ascent for the consumed alef.
    let (calls2, runs2) = run_lam_alef(b"font-size:100%", b"font-size:250%;text-transform:uppercase");
    assert_eq!(runs2.len(), 1);
    assert_eq!(runs2[0].0, "\u{fefb}".as_bytes());
    assert_eq!(runs2[0].3, 30);
    assert_eq!(runs2[0].2, 16 + 30, "consumed tall alef must not add phantom ascent: {runs2:?}");
    assert!(calls2.is_empty(), "non-tall lam-alef must not query tall ink_extents: {calls2:?}");
}
