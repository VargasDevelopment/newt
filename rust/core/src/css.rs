// Copyright (c) 2026 FreeInk. MIT; see rust/THIRD_PARTY.md.
//! Safe translation of pinned Css.h/Css.cpp. Byte syntax, not web CSS.
#![forbid(unsafe_code)]
use crate::member_check::Checked;
use crate::zip_container::{InflateScratch, Reader, Status};
use crate::zip_window::source::BookSource;

pub const MAX_RULES: usize = 512;
pub const MAX_SHEET_BYTES: u32 = 64 * 1024;
pub const HASH_SEED: u32 = 2166136261;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum TextAlign {
    #[default]
    Inherit,
    Left,
    Right,
    Center,
    Justify,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CssDecl {
    pub size_pct: u16,
    pub size_root_relative: bool,
    pub weight_bold: i8,
    pub style_italic: i8,
    pub align: TextAlign,
    pub text_indent_pct: i16,
    pub margin_left_pct: i16,
    pub margin_top_pct: i16,
    pub margin_bottom_pct: i16,
    pub display_none: i8,
    pub underline: i8,
    pub vert_align: i8,
}
impl Default for CssDecl {
    fn default() -> Self {
        Self {
            size_pct: 0,
            size_root_relative: false,
            weight_bold: -1,
            style_italic: -1,
            align: TextAlign::Inherit,
            text_indent_pct: -1,
            margin_left_pct: -1,
            margin_top_pct: -1,
            margin_bottom_pct: -1,
            display_none: -1,
            underline: -1,
            vert_align: -1,
        }
    }
}
impl CssDecl {
    pub fn apply_over(&mut self, over: &Self) {
        if over.size_pct != 0 {
            self.size_pct = over.size_pct;
            self.size_root_relative = over.size_root_relative;
        }
        if over.align != TextAlign::Inherit {
            self.align = over.align;
        }
        macro_rules! overlay { ($($f:ident),*) => { $(if over.$f >= 0 { self.$f = over.$f; })* }; }
        overlay!(
            weight_bold,
            style_italic,
            text_indent_pct,
            margin_left_pct,
            margin_top_pct,
            margin_bottom_pct,
            display_none,
            underline,
            vert_align
        );
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CssRule {
    pub elem_hash: u32,
    pub class_hash: u32,
    pub decl: CssDecl,
}
#[derive(Clone, Copy, Debug)]
pub struct CssStylesheet<'a> {
    pub rules: &'a [CssRule],
    pub content_hash: u32,
}
impl Default for CssStylesheet<'_> {
    fn default() -> Self {
        Self {
            rules: &[],
            content_hash: 0,
        }
    }
}
impl CssStylesheet<'_> {
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}
pub fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 12)
}
fn trim(mut b: &[u8]) -> &[u8] {
    while b.first().is_some_and(|b| is_space(*b)) {
        b = &b[1..];
    }
    while b.last().is_some_and(|b| is_space(*b)) {
        b = &b[..b.len() - 1];
    }
    b
}
fn c_bytes(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|b| *b == 0).unwrap_or(b.len())]
}
pub fn hash(bytes: &[u8]) -> u32 {
    fold_hash(HASH_SEED, bytes)
}
fn fold_hash(seed: u32, bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(seed, |h, b| (h ^ u32::from(*b)).wrapping_mul(16777619))
}

// Checked arithmetic rejects values outside the original signed defined domain.
// At most three fractional digits are consumed; extra digits become the unit.
fn length_to_pct(v: &[u8]) -> Option<(i32, bool)> {
    let mut i = 0;
    let mut neg = false;
    if matches!(v.first(), Some(b'-' | b'+')) {
        neg = v[0] == b'-';
        i += 1;
    }
    let (mut whole, mut frac, mut div) = (0i32, 0i32, 1i32);
    let mut any = false;
    while i < v.len() && v[i].is_ascii_digit() {
        whole = whole.checked_mul(10)?.checked_add(i32::from(v[i] - b'0'))?;
        i += 1;
        any = true;
    }
    if v.get(i) == Some(&b'.') {
        i += 1;
        while i < v.len() && v[i].is_ascii_digit() && div < 1000 {
            frac = frac * 10 + i32::from(v[i] - b'0');
            div *= 10;
            i += 1;
            any = true;
        }
    }
    if !any {
        return None;
    }
    let unit = &v[i..];
    let pct = match unit {
        b"" if whole == 0 && frac == 0 => 0,
        b"em" | b"rem" | b"px" | b"pt" => {
            let p = whole.checked_mul(100)?.checked_add(frac * 100 / div)?;
            p / match unit {
                b"px" => 16,
                b"pt" => 12,
                _ => 1,
            }
        }
        b"%" => whole.checked_add(if div > 1 { frac / div } else { 0 })?,
        _ => return None,
    };
    Some((if neg { 0 } else { pct }, unit == b"rem"))
}
fn margin(v: &[u8]) -> Option<i16> {
    length_to_pct(v).map(|(n, _)| n.min(1000) as i16)
}
fn apply_declaration(d: &mut CssDecl, prop: &[u8], value: &[u8]) {
    let v = trim(value);
    if v.is_empty() {
        return;
    }
    match prop {
        b"font-size" => {
            let keyword = match v {
                b"xx-small" => 58,
                b"x-small" => 69,
                b"small" | b"smaller" => 87,
                b"medium" => 100,
                b"large" | b"larger" => 120,
                b"x-large" => 150,
                b"xx-large" => 200,
                _ => 0,
            };
            if keyword != 0 {
                d.size_pct = keyword;
            } else if let Some((pct, root)) = length_to_pct(v) {
                if pct > 0 {
                    d.size_pct = pct.min(400) as u16;
                    d.size_root_relative = root;
                }
            }
        }
        b"font-weight" => match v {
            b"bold" | b"bolder" => d.weight_bold = 1,
            b"normal" | b"lighter" => d.weight_bold = 0,
            _ if (b'1'..=b'9').contains(&v[0]) => d.weight_bold = i8::from(v[0] >= b'6'),
            _ => {}
        },
        b"font-style" => match v {
            b"italic" | b"oblique" => d.style_italic = 1,
            b"normal" => d.style_italic = 0,
            _ => {}
        },
        b"text-align" => match v {
            b"left" | b"start" => d.align = TextAlign::Left,
            b"right" | b"end" => d.align = TextAlign::Right,
            b"center" => d.align = TextAlign::Center,
            b"justify" => d.align = TextAlign::Justify,
            _ => {}
        },
        b"text-indent" => {
            if let Some(n) = margin(v) {
                d.text_indent_pct = n;
            }
        }
        b"margin-left" => {
            if let Some(n) = margin(v) {
                d.margin_left_pct = n;
            }
        }
        b"margin-top" => {
            if let Some(n) = margin(v) {
                d.margin_top_pct = n;
            }
        }
        b"margin-bottom" => {
            if let Some(n) = margin(v) {
                d.margin_bottom_pct = n;
            }
        }
        b"margin" => {
            let mut parts = [&[][..]; 4];
            let mut count = 0;
            for p in v.split(|b| is_space(*b)).filter(|p| !p.is_empty()).take(4) {
                parts[count] = p;
                count += 1;
            }
            if count > 0 {
                if let Some(n) = margin(parts[0]) {
                    d.margin_top_pct = n;
                }
                if let Some(n) = margin(parts[if count >= 3 { 2 } else { 0 }]) {
                    d.margin_bottom_pct = n;
                }
                if let Some(n) = margin(
                    parts[if count >= 4 {
                        3
                    } else if count >= 2 {
                        1
                    } else {
                        0
                    }],
                ) {
                    d.margin_left_pct = n;
                }
            }
        }
        b"display" => {
            if v == b"none" {
                d.display_none = 1;
            }
        }
        b"text-decoration" | b"text-decoration-line" => match v {
            b"underline" => d.underline = 1,
            b"none" => d.underline = 0,
            _ => {}
        },
        b"vertical-align" => match v {
            b"super" => d.vert_align = 1,
            b"sub" => d.vert_align = 2,
            b"baseline" => d.vert_align = 0,
            _ => {}
        },
        _ => {}
    }
}
fn parse_declarations(text: &[u8]) -> CssDecl {
    let mut d = CssDecl::default();
    let mut i = 0;
    while i < text.len() {
        while i < text.len() && (is_space(text[i]) || text[i] == b';') {
            i += 1;
        }
        let start = i;
        while i < text.len() && !matches!(text[i], b':' | b';' | b'}') {
            i += 1;
        }
        if text.get(i) != Some(&b':') {
            while i < text.len() && text[i] != b';' {
                i += 1;
            }
            continue;
        }
        let mut end = i;
        while end > start && is_space(text[end - 1]) {
            end -= 1;
        }
        i += 1;
        let value = i;
        while i < text.len() && text[i] != b';' {
            i += 1;
        }
        apply_declaration(&mut d, &text[start..end], &text[value..i]);
    }
    d
}
pub fn parse_inline_style(text: Option<&[u8]>) -> CssDecl {
    text.map_or_else(CssDecl::default, |t| parse_declarations(c_bytes(t)))
}
fn simple_selector(s: &[u8]) -> Option<(u32, u32)> {
    if s == b"*" {
        return Some((0, 0));
    }
    let mut i = 0;
    while i < s.len() && !matches!(s[i], b'.' | b'#' | b':' | b'[') {
        i += 1;
    }
    let elem = if i > 0 { hash(&s[..i]) } else { 0 };
    let mut class = 0;
    if s.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while i < s.len() && !matches!(s[i], b'.' | b'#' | b':' | b'[') {
            i += 1;
        }
        if i == start {
            return None;
        }
        class = hash(&s[start..i]);
    }
    if i == s.len() {
        Some((elem, class))
    } else {
        None
    }
}

pub struct CssStylesheetBuilder<'a> {
    rules: Option<&'a mut [CssRule]>,
    count: usize,
    skipped: u16,
    content_hash: u32,
}
impl Default for CssStylesheetBuilder<'_> {
    fn default() -> Self {
        Self {
            rules: None,
            count: 0,
            skipped: 0,
            content_hash: HASH_SEED,
        }
    }
}
impl<'a> CssStylesheetBuilder<'a> {
    /// Full 512 typed slots or failed/inert begin, independently of C ABI bytes.
    pub fn begin(&mut self, storage: &'a mut [CssRule]) -> bool {
        self.count = 0;
        self.skipped = 0;
        self.content_hash = HASH_SEED;
        self.rules = if storage.len() >= MAX_RULES {
            Some(&mut storage[..MAX_RULES])
        } else {
            None
        };
        self.rules.is_some()
    }
    pub fn skipped_sheets(&self) -> u16 {
        self.skipped
    }
    /// A borrowed sheet prevents later mutation of this builder until released.
    pub fn finish(&self) -> CssStylesheet<'_> {
        CssStylesheet {
            rules: self.rules.as_deref().map_or(&[], |r| &r[..self.count]),
            content_hash: self.content_hash,
        }
    }
    pub fn add_text(&mut self, css: &[u8]) {
        let Some(rules) = self.rules.as_deref_mut() else {
            return;
        };
        self.content_hash = fold_hash(self.content_hash, css);
        let mut i = 0;
        loop {
            loop {
                while i < css.len() && is_space(css[i]) {
                    i += 1;
                }
                if css.get(i..i.saturating_add(2)) == Some(b"/*") {
                    i += 2;
                    while i + 1 < css.len() && &css[i..i + 2] != b"*/" {
                        i += 1;
                    }
                    i = (i + 2).min(css.len());
                } else {
                    break;
                }
            }
            if i >= css.len() {
                return;
            }
            if css[i] == b'@' {
                let mut depth = 0usize;
                let mut saw = false;
                while i < css.len() {
                    let c = css[i];
                    i += 1;
                    match c {
                        b'{' => {
                            depth += 1;
                            saw = true;
                        }
                        b'}' => {
                            if depth <= 1 {
                                break;
                            }
                            depth -= 1;
                        }
                        b';' if !saw => break,
                        _ => {}
                    }
                }
                continue;
            }
            let start = i;
            while i < css.len() && css[i] != b'{' {
                i += 1;
            }
            if i == css.len() {
                return;
            }
            let end = i;
            i += 1;
            let ds = i;
            while i < css.len() && css[i] != b'}' {
                i += 1;
            }
            let decl = parse_declarations(&css[ds..i]);
            if i < css.len() {
                i += 1;
            }
            for selector in css[start..end].split(|b| *b == b',') {
                let mut te = selector.len();
                while te > 0 && is_space(selector[te - 1]) {
                    te -= 1;
                }
                let mut ts = te;
                while ts > 0
                    && !is_space(selector[ts - 1])
                    && !matches!(selector[ts - 1], b'>' | b'+' | b'~')
                {
                    ts -= 1;
                }
                if te > ts {
                    if let Some((elem_hash, class_hash)) = simple_selector(&selector[ts..te]) {
                        if self.count < MAX_RULES {
                            rules[self.count] = CssRule {
                                elem_hash,
                                class_hash,
                                decl,
                            };
                            self.count += 1;
                        }
                    }
                }
            }
        }
    }
    /// Buffers are caller-owned scratch. All borrows end on return; no hidden
    /// arena reservation or large state construction. Reader open wins over
    /// insufficient text storage, even after failed begin.
    pub fn add_sheet<S: BookSource + ?Sized>(
        &mut self,
        source: &mut S,
        entry: &Checked,
        decode: Option<InflateScratch<'_>>,
        text: &mut [u8],
    ) -> Status {
        if entry.uncompressed_size > MAX_SHEET_BYTES {
            self.skipped = self.skipped.wrapping_add(1);
            return Status::Ok;
        }
        let mut reader = Reader::new(source, decode);
        let status = reader.open(entry);
        if status != Status::Ok {
            return status;
        }
        let len = entry.uncompressed_size as usize;
        if text.len() < len + 1 {
            return Status::OutOfMemory;
        }
        let mut total = 0;
        while total < len {
            let n = reader.read(&mut text[total..len]);
            if n < 0 {
                return Status::IoError;
            }
            if n == 0 {
                break;
            }
            total += n as usize;
        }
        self.add_text(&text[..total]);
        Status::Ok
    }
}
pub fn cascade_for(
    sheet: &CssStylesheet<'_>,
    element: Option<&[u8]>,
    classes: Option<&[u8]>,
    inline: Option<&CssDecl>,
) -> CssDecl {
    let elem = element.map_or(0, |e| hash(c_bytes(e)));
    let mut hashes = [0u32; 8];
    let mut count = 0;
    if let Some(c) = classes {
        for token in c_bytes(c)
            .split(|b| is_space(*b))
            .filter(|p| !p.is_empty())
            .take(8)
        {
            hashes[count] = hash(token);
            count += 1;
        }
    }
    let mut out = CssDecl::default();
    for pass in 0..3 {
        for r in sheet.rules {
            let spec = if r.class_hash != 0 {
                if r.elem_hash != 0 { 2 } else { 1 }
            } else {
                0
            };
            if spec != pass || (r.elem_hash != 0 && r.elem_hash != elem) {
                continue;
            }
            let matches = if r.class_hash != 0 {
                hashes[..count].contains(&r.class_hash)
            } else {
                r.elem_hash != 0
            };
            if matches {
                out.apply_over(&r.decl);
            }
        }
    }
    if let Some(d) = inline {
        out.apply_over(d);
    }
    out
}
