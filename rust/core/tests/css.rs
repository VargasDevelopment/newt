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
            vert_align: 2
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
            vert_align: 0
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
