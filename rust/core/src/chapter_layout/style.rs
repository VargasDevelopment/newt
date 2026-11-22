#![forbid(unsafe_code)]
use crate::css::CssDecl;
pub fn defaults(n: &[u8]) -> CssDecl {
    let mut d = CssDecl::default();
    match n {
        b"h1" => {
            d.size_pct = 200;
            d.weight_bold = 1;
            d.margin_top_pct = 100;
            d.margin_bottom_pct = 50
        }
        b"h2" => {
            d.size_pct = 160;
            d.weight_bold = 1;
            d.margin_top_pct = 90;
            d.margin_bottom_pct = 45
        }
        b"h3" => {
            d.size_pct = 130;
            d.weight_bold = 1;
            d.margin_top_pct = 80;
            d.margin_bottom_pct = 40
        }
        b"h4" | b"h5" | b"h6" => {
            d.size_pct = 115;
            d.weight_bold = 1;
            d.margin_top_pct = 70;
            d.margin_bottom_pct = 35
        }
        b"blockquote" => {
            d.style_italic = 1;
            d.margin_top_pct = 40;
            d.margin_bottom_pct = 40
        }
        b"li" => {
            d.margin_top_pct = 10;
            d.margin_bottom_pct = 10
        }
        b"p" | b"div" => d.margin_bottom_pct = 40,
        b"b" | b"strong" => d.weight_bold = 1,
        b"i" | b"em" | b"cite" => d.style_italic = 1,
        b"small" => d.size_pct = 80,
        b"big" => d.size_pct = 120,
        b"sub" => {
            d.size_pct = 50;
            d.vert_align = 2
        }
        b"sup" => {
            d.size_pct = 50;
            d.vert_align = 1
        }
        b"u" | b"ins" => d.underline = 1,
        _ => {}
    }
    d
}
pub fn block(n: &[u8]) -> bool {
    matches!(
        n,
        b"p" | b"h1"
            | b"h2"
            | b"h3"
            | b"h4"
            | b"h5"
            | b"h6"
            | b"blockquote"
            | b"li"
            | b"div"
            | b"section"
            | b"article"
            | b"figure"
            | b"aside"
            | b"figcaption"
            | b"ul"
            | b"ol"
            | b"table"
            | b"tr"
            | b"td"
            | b"th"
            | b"dt"
            | b"dd"
    )
}
pub fn suppressed(n: &[u8]) -> bool {
    matches!(n, b"head" | b"style" | b"script" | b"title")
}
