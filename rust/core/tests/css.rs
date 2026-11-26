use newt_core::member_check::Checked;
use newt_core::{
    zip_container::{Entry, Status},
    zip_window::source::BookSource,
};
use newt_core::css::*;
fn inline(b: &[u8]) -> CssDecl {
    parse_inline_style(Some(b))
}
#[test]
fn independent_every_field_and_stale_root_flag() {
    let got=inline(b"font-size:1.25rem;font-size:small;font-weight:600junk;font-style:oblique;text-align:end;text-indent:-4em;margin:2px 3pt 11em 1.019em;display:none;text-decoration-line:underline;vertical-align:sub");
    assert_eq!(
        got,
        CssDecl {
            size_pct: 87,
            size_root_relative: true,
            weight_bold: 1,
            style_italic: 1,
            align: TextAlign::Right,
            text_indent_pct: 0,
            margin_left_pct: 101,
            margin_top_pct: 12,
            margin_bottom_pct: 1000,
            display_none: 1,
            underline: 1,
            vert_align: 2,
            text_case: 0,
        }
    );
    let mut over = got;
    over.apply_over(&inline(b"font-size:0;font-weight:normal;font-style:normal;text-align:start;text-indent:0;margin:0;display:block;text-decoration:none;vertical-align:baseline"));
    assert_eq!(
        over,
        CssDecl {
            size_pct: 87,
            size_root_relative: true,
            weight_bold: 0,
            style_italic: 0,
            align: TextAlign::Left,
            text_indent_pct: 0,
            margin_left_pct: 0,
            margin_top_pct: 0,
            margin_bottom_pct: 0,
            display_none: 1,
            underline: 0,
            vert_align: 0,
            text_case: 0,
        }
    );
}
#[test]
fn numeric_lexical_domain() {
    for (text, n) in [
        (b"font-size:1.999em".as_slice(), 199),
        (b"font-size:199.999%", 199),
        (b"font-size:.5pt", 4),
        (b"font-size:+16px", 100),
        (b"font-size:99em", 400),
    ] {
        assert_eq!(inline(text).size_pct, n);
    }
    for v in [
        b"1.0000em".as_slice(),
        b"1EM",
        b"1 em",
        b"12",
        b"-1em",
        b"0",
        b"2147483648em",
        b"21474837em",
    ] {
        let mut b = Vec::from(b"font-size:".as_slice());
        b.extend(v);
        assert_eq!(inline(&b).size_pct, 0);
    }
    assert_eq!(inline(b"font-weight:9nonsense").weight_bold, 1);
    assert_eq!(inline(b"\x0bfont-size:2em").size_pct, 0);
    assert_eq!(inline(b"\x0cfont-size:2em").size_pct, 200);
    assert_eq!(inline(b"font-size:2em\0;font-size:3em").size_pct, 200);
    assert_eq!(inline(b"font-size:/*x*/2em").size_pct, 0);
}
#[test]
fn selectors_cascade_and_eighth_class_boundary() {
    let mut slots = vec![CssRule::default(); MAX_RULES];
    let mut b = CssStylesheetBuilder::default();
    assert!(b.begin(&mut slots));
    b.add_text(b"*{display:none} @media x{p{font-size:4em}} p.a{font-size:2em}.a{font-size:3em} body>p{font-size:1em} p:hover{display:none} .ninth{font-style:italic} .eighth{font-weight:bold} p{font-size:1.5em}");
    let s = b.finish();
    let d = cascade_for(&s, Some(b"p"), Some(b"a b c d e f g eighth ninth"), None);
    assert_eq!(
        d,
        CssDecl {
            size_pct: 200,
            weight_bold: 1,
            ..CssDecl::default()
        }
    );
    let d = cascade_for(
        &s,
        Some(b"p"),
        Some(b"a"),
        Some(&inline(b"font-size:50%;text-align:center")),
    );
    assert_eq!(
        d,
        CssDecl {
            size_pct: 50,
            align: TextAlign::Center,
            ..CssDecl::default()
        }
    );
    assert_eq!(cascade_for(&s, None, None, None), CssDecl::default());
}
#[test]
fn cap_inert_hash_and_rebegin() {
    let mut tiny = [];
    let mut b = CssStylesheetBuilder::default();
    assert!(!b.begin(&mut tiny));
    b.add_text(b"p{font-size:2em}");
    assert_eq!(b.finish().content_hash, HASH_SEED);
    let mut slots = vec![CssRule::default(); MAX_RULES];
    assert!(b.begin(&mut slots));
    for _ in 0..513 {
        b.add_text(b"p{font-size:2em}");
    }
    assert_eq!(b.finish().rules.len(), 512);
    let before = b.finish().content_hash;
    b.add_text(b"@ignored;");
    assert_ne!(before, b.finish().content_hash);
    let mut fresh = vec![CssRule::default(); MAX_RULES];
    assert!(b.begin(&mut fresh));
    assert_eq!(b.finish().rules.len(), 0);
    assert_eq!(b.finish().content_hash, HASH_SEED);
}
struct Memory<'a> {
    data: &'a [u8],
    calls: usize,
}
impl BookSource for Memory<'_> {
    fn size(&self) -> u64 {
        self.data.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        self.calls += 1;
        let data = self.data.get(at as usize..).unwrap_or(&[]);
        let n = data.len().min(out.len()).min(2);
        out[..n].copy_from_slice(&data[..n]);
        n as i32
    }
}
#[test]
fn sheet_status_precedence_and_no_partial_commit() {
    let mut slots = vec![CssRule::default(); MAX_RULES];
    let mut b = CssStylesheetBuilder::default();
    b.begin(&mut slots);
    let mut src = Memory {
        data: b"p{font-size:2em}",
        calls: 0,
    };
    let n = src.data.len() as u32;
    assert_eq!(
        b.add_sheet(&mut src, &Checked::raw(n), None, &mut []),
        Status::OutOfMemory
    );
    assert_eq!(src.calls, 0);
    let mut e = Entry::raw(n);
    e.method = 99;
    assert_eq!(
        b.add_sheet(&mut src, &Checked::unverified(e), None, &mut []),
        Status::Unsupported
    );
    e.uncompressed_size = 65537;
    assert_eq!(b.add_sheet(&mut src, &Checked::unverified(e), None, &mut []), Status::Ok);
    assert_eq!(src.calls, 0);
    assert_eq!(b.skipped_sheets(), 1);
    let mut text = [0; 64];
    assert_eq!(
        b.add_sheet(&mut src, &Checked::raw(n + 1), None, &mut text),
        Status::IoError
    );
    assert_eq!(b.finish().content_hash, HASH_SEED);
    assert!(b.finish().rules.is_empty());
    assert_eq!(
        b.add_sheet(&mut src, &Checked::raw(n), None, &mut text),
        Status::Ok
    );
    assert_eq!(b.finish().rules[0].decl.size_pct, 200);
}

#[test]
fn small_caps_and_text_transform_parse_and_cascade_independently() {
    assert_eq!(core::mem::size_of::<CssDecl>(), 18);
    assert_eq!(core::mem::size_of::<CssRule>(), 28);

    // font-variant tokens and font-variant-caps single values.
    for decl in [
        b"font-variant: small-caps".as_slice(),
        b"font-variant: all-small-caps",
        b"font-variant: petite-caps",
        b"font-variant: all-petite-caps",
        b"font-variant: common-ligatures small-caps oldstyle-nums",
        b"font-variant-caps: small-caps",
        b"font-variant-caps: all-small-caps",
        b"font-variant-caps: petite-caps",
    ] {
        let d = inline(decl);
        assert_eq!(d.text_case & SMALL_CAPS_MASK, SMALL_CAPS_ON, "{decl:?}");
        assert_eq!(d.text_case & TEXT_TRANSFORM_MASK, 0, "{decl:?}");
    }
    for decl in [
        b"font-variant: normal".as_slice(),
        b"font-variant-caps: normal",
    ] {
        assert_eq!(inline(decl).text_case & SMALL_CAPS_MASK, SMALL_CAPS_NORMAL);
    }
    // Invalid or unrelated values are ignored without clobbering prior state,
    // including explicit `normal` or mutually exclusive caps tokens.
    let invalid = inline(
        b"font-variant:small-caps;font-variant:normal small-caps;font-variant:small-caps petite-caps;font-variant-caps:common-ligatures small-caps;font-variant:bogus",
    );
    assert_eq!(invalid.text_case & SMALL_CAPS_MASK, SMALL_CAPS_ON);
    let invalid_from_normal = inline(
        b"font-variant:normal;font-variant:small-caps petite-caps;font-variant:small-caps small-caps",
    );
    assert_eq!(invalid_from_normal.text_case & SMALL_CAPS_MASK, SMALL_CAPS_NORMAL);

    // text-transform keywords and invalid-value preservation.
    for (decl, expected) in [
        (b"text-transform:none".as_slice(), TEXT_TRANSFORM_NONE),
        (b"text-transform:uppercase", TEXT_TRANSFORM_UPPERCASE),
        (b"text-transform:lowercase", TEXT_TRANSFORM_LOWERCASE),
        (b"text-transform:capitalize", TEXT_TRANSFORM_CAPITALIZE),
    ] {
        let d = inline(decl);
        assert_eq!(
            (d.text_case & TEXT_TRANSFORM_MASK) >> TEXT_TRANSFORM_SHIFT,
            expected
        );
        assert_eq!(d.text_case & SMALL_CAPS_MASK, SMALL_CAPS_INHERIT);
    }
    let preserved = inline(b"text-transform:capitalize;text-transform:full-width");
    assert_eq!(
        (preserved.text_case & TEXT_TRANSFORM_MASK) >> TEXT_TRANSFORM_SHIFT,
        TEXT_TRANSFORM_CAPITALIZE
    );

    // Independent cascade: resetting one axis never clobbers the other.
    let mut slots = vec![CssRule::default(); MAX_RULES];
    let mut b = CssStylesheetBuilder::default();
    assert!(b.begin(&mut slots));
    b.add_text(
        b"span{font-variant:small-caps;text-transform:lowercase} \
          .up{text-transform:uppercase} \
          span.norm{font-variant-caps:normal}",
    );
    let sheet = b.finish();
    let d1 = cascade_for(&sheet, Some(b"span"), Some(b"up"), None);
    assert_eq!(d1.text_case & SMALL_CAPS_MASK, SMALL_CAPS_ON);
    assert_eq!(
        (d1.text_case & TEXT_TRANSFORM_MASK) >> TEXT_TRANSFORM_SHIFT,
        TEXT_TRANSFORM_UPPERCASE
    );
    let d2 = cascade_for(&sheet, Some(b"span"), Some(b"up norm"), None);
    assert_eq!(d2.text_case & SMALL_CAPS_MASK, SMALL_CAPS_NORMAL);
    assert_eq!(
        (d2.text_case & TEXT_TRANSFORM_MASK) >> TEXT_TRANSFORM_SHIFT,
        TEXT_TRANSFORM_UPPERCASE
    );
    let d3 = cascade_for(
        &sheet,
        Some(b"span"),
        Some(b"up"),
        Some(&inline(b"text-transform:none")),
    );
    assert_eq!(d3.text_case & SMALL_CAPS_MASK, SMALL_CAPS_ON);
    assert_eq!(
        (d3.text_case & TEXT_TRANSFORM_MASK) >> TEXT_TRANSFORM_SHIFT,
        TEXT_TRANSFORM_NONE
    );

    // Mixed-case property names and keywords work identically in both inline
    // styles and stylesheet rules (CSS ASCII case-insensitivity).
    let mixed_inline = inline(b"Font-Variant: Small-Caps; Text-Transform: Uppercase");
    assert_eq!(mixed_inline.text_case & SMALL_CAPS_MASK, SMALL_CAPS_ON);
    assert_eq!(
        (mixed_inline.text_case & TEXT_TRANSFORM_MASK) >> TEXT_TRANSFORM_SHIFT,
        TEXT_TRANSFORM_UPPERCASE
    );
    let mut slots2 = vec![CssRule::default(); MAX_RULES];
    let mut b2 = CssStylesheetBuilder::default();
    assert!(b2.begin(&mut slots2));
    b2.add_text(b".mc{FONT-VARIANT-CAPS: All-Petite-Caps; TEXT-TRANSFORM: Capitalize}");
    let d_mc = cascade_for(&b2.finish(), Some(b"p"), Some(b"mc"), None);
    assert_eq!(d_mc.text_case & SMALL_CAPS_MASK, SMALL_CAPS_ON);
    assert_eq!(
        (d_mc.text_case & TEXT_TRANSFORM_MASK) >> TEXT_TRANSFORM_SHIFT,
        TEXT_TRANSFORM_CAPITALIZE
    );
}
