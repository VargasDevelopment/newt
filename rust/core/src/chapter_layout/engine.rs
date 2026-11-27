// FreeInk cd6f5b8 ChapterLayout.cpp translation. MIT; reference closure retained.
#![forbid(unsafe_code)]
use super::{arabic, case, style, text::*, types::*};
mod links;
mod initialization;
mod segments;
use segments::{
    LineSegments, MAX_SEGMENTS, SEG_COMPRESS, SEG_GAP, SEG_SCRIPT, SEG_SMALL_CAP, SEG_SPACE,
    Segment,
};
use crate::{
    css::{self, CssDecl, CssStylesheetBuilder, TextAlign},
    hyphenator::Hyphenator,
};
/// One start tag's attribute views, borrowed only for the callback.
#[derive(Clone, Copy, Default)]
pub struct StartAttrs<'x> {
    pub class: Option<&'x [u8]>,
    pub inline: Option<&'x [u8]>,
    pub id: Option<&'x [u8]>,
    pub href: Option<&'x [u8]>,
    pub src: Option<&'x [u8]>,
    pub alt: Option<&'x [u8]>,
}
/// Existing callers retain their backend; all layout policy lives in EngineState.
pub struct Engine<'a> {
    pub state: EngineState<'a>,
    pub backend: &'a mut dyn Backend,
}
impl<'a> core::ops::Deref for Engine<'a> {
    type Target = EngineState<'a>;
    fn deref(&self) -> &Self::Target { &self.state }
}
impl core::ops::DerefMut for Engine<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.state }
}
impl<'a> Engine<'a> {
    pub fn new(
        params: Params<'a>, storage: Storage<'a>, backend: &'a mut dyn Backend,
        breaker: &'a mut dyn LineBreak, image_source: &'a mut dyn Images,
        hyphenator: Option<&'a Hyphenator<'a>>, chapter_css: CssStylesheetBuilder<'a>,
    ) -> Result<Self, Capacity> {
        Ok(Self { state: EngineState::new(params, storage, breaker, image_source, hyphenator, chapter_css)?, backend })
    }
    pub fn start(&mut self, name: &[u8], class: Option<&[u8]>, inline: Option<&[u8]>, id: Option<&[u8]>, href: Option<&[u8]>, src: Option<&[u8]>) {
        self.state.start(self.backend, name, class, inline, id, href, src)
    }
    pub fn start_tag(&mut self, name: &[u8], attrs: StartAttrs<'_>) {
        self.state.start_tag(self.backend, name, attrs)
    }
    pub fn end(&mut self, name: &[u8]) { self.state.end(self.backend, name) }
    pub fn text(&mut self, text: &[u8]) { self.state.text(self.backend, text) }
    pub fn flush(&mut self) { self.state.flush(self.backend) }
    pub fn finish(&mut self) -> bool { self.state.finish(self.backend) }
}

/// Layout state; backend/source loans last only one operation.
pub struct EngineState<'a> {
    pub params: Params<'a>,
    pub storage: Storage<'a>,
    pub breaker: &'a mut dyn LineBreak,
    pub image_source: &'a mut dyn Images,
    pub hyphenator: Option<&'a Hyphenator<'a>>,
    pub chapter_css: CssStylesheetBuilder<'a>,
    pub stopped: bool,
    pub failed: bool,
    pub pages: u32,
    pub chars: u32,
    len: usize,
    span_count: usize,
    line_count: usize,
    top: usize,
    overflow: u32,
    depth: i32,
    suppress: i32,
    in_body: bool,
    in_style: i32,
    style_len: usize,
    style_ok: bool,
    pending_margin: u32,
    pending_space: bool,
    continued: bool,
    paragraph: Paragraph,
    rtl: bool,
    run_count: usize,
    image_count: usize,
    link_count: usize,
    pool_len: usize,
    y: i16,
    page_chars: u32,
    /// Chapter position held while a skipped-picture note lays out.
    picture_note_at: Option<u32>,
    par_link_count: usize,
    current_link: u8,
    last_shy: bool,
    word_state: u8,
    /// True when any span in the current paragraph carries `SMALL_CAPS` or
    /// `TRANSFORM_MASK`. False preserves frozen FreeInk SDK behavior verbatim.
    par_case: bool,
    /// Paragraph BiDi base direction established by the first strong character
    /// (`DIR_NONE` / `DIR_LTR` / `DIR_RTL`), retained across continuing flushes.
    par_dir: u8,
    /// Exact resolved pixel advance of unplaced inline margins carried across
    /// a continuing text-buffer cut (avoids re-aggregating fractional percents).
    carried_margin_px: i16,
}
impl<'a> EngineState<'a> {
    fn size(&self, pct: u16) -> u16 {
        ((u32::from(self.params.base_size) * u32::from(pct) / 100).max(1)) as u16
    }
    fn state(&self) -> Element {
        self.storage.elements[self.top]
    }
    fn push(&mut self, d: CssDecl) {
        let mut n = self.state();
        if d.size_pct != 0 {
            n.size = if d.size_root_relative {
                u32::from(d.size_pct)
            } else {
                u32::from(n.size) * u32::from(d.size_pct) / 100
            }
            .clamp(30, 250) as u16
        }
        for (v, bit) in [
            (d.weight_bold, BOLD),
            (d.style_italic, ITALIC),
            (d.underline, UNDERLINE),
        ] {
            if v == 1 {
                n.flags |= bit
            }
            if v == 0 {
                n.flags &= !bit
            }
        }
        if d.vert_align >= 0 {
            n.flags &= !(SUP | SUB);
            if d.vert_align == 1 {
                n.flags |= SUP
            }
            if d.vert_align == 2 {
                n.flags |= SUB
            }
        }
        match d.text_case & css::SMALL_CAPS_MASK {
            css::SMALL_CAPS_ON => n.flags |= SMALL_CAPS,
            css::SMALL_CAPS_NORMAL => n.flags &= !SMALL_CAPS,
            _ => {}
        }
        let transform = (d.text_case & css::TEXT_TRANSFORM_MASK) >> css::TEXT_TRANSFORM_SHIFT;
        if transform != css::TEXT_TRANSFORM_INHERIT {
            n.flags = (n.flags & !TRANSFORM_MASK) | ((transform - 1) << TRANSFORM_SHIFT);
        }
        if d.align != TextAlign::Inherit {
            n.align = d.align
        }
        if d.text_indent_pct >= 0 {
            n.indent = d.text_indent_pct
        }
        if d.display_none == 1 {
            n.hidden = true
        }
        if self.top + 1 < self.storage.elements.len() {
            self.top += 1;
            self.storage.elements[self.top] = n
        } else {
            self.overflow += 1
        }
    }
    fn pop(&mut self) {
        if self.overflow > 0 {
            self.overflow -= 1
        } else if self.top > 0 {
            self.top -= 1
        }
    }
    fn reset_par(&mut self) {
        self.len = 0;
        self.span_count = 0;
        self.pending_margin = 0;
        self.pending_space = false
    }
    fn latch(&mut self, n: &[u8], d: CssDecl, parent: bool) {
        let s = self.state();
        self.paragraph = Paragraph {
            size: s.size,
            flags: s.flags,
            align: s.align,
            indent: if parent {
                0
            } else if n == b"blockquote" {
                (u32::from(self.params.base_size) * 2) as i16
            } else if n == b"li" {
                (u32::from(self.params.base_size) * 3 / 2) as i16
            } else {
                0
            },
            text_indent: if parent { 0 } else { s.indent },
            before: if parent {
                0
            } else {
                d.margin_top_pct.max(0) as u16
            },
            after: if parent {
                0
            } else {
                d.margin_bottom_pct.max(0) as u16
            },
        };
        self.par_link_count = 0;
        self.current_link = 0;
        self.word_state = case::WS_SEP;
        self.par_case = false;
        self.par_dir = DIR_NONE;
        self.carried_margin_px = 0;
        self.reset_par()
    }
    fn note(&mut self) {
        if self.span_count == 0 {
            return;
        }
        let s = self.state();
        let margin = self.pending_margin.min(65535) as u16;
        let last = &mut self.storage.spans[self.span_count - 1];
        if last.flags == s.flags
            && last.size == s.size
            && last.link == self.current_link
            && margin == 0
        {
            return;
        }
        if usize::from(last.start) == self.len {
            last.flags = s.flags;
            last.size = s.size;
            last.link = self.current_link;
            last.margin = last.margin.saturating_add(margin);
            self.pending_margin = 0
        } else if self.span_count < self.storage.spans.len() {
            self.storage.spans[self.span_count] = Span {
                start: self.len as u16,
                flags: s.flags,
                size: s.size,
                margin,
                link: self.current_link,
            };
            self.span_count += 1;
            self.pending_margin = 0
        }
    }
    fn span_bank_needs_flush(&self) -> bool {
        if self.span_count == 0 || self.span_count < self.storage.spans.len() {
            return false;
        }
        let s = self.state();
        let last = self.storage.spans[self.span_count - 1];
        if usize::from(last.start) == self.len {
            return false;
        }
        if last.flags == s.flags
            && last.size == s.size
            && last.link == self.current_link
            && self.pending_margin == 0
        {
            return false;
        }
        self.par_case || (s.flags & !STYLE_MASK != 0)
    }
    #[inline(never)]
    fn rewind_case_cut(&self, cut: usize, end: usize, bytes: &[u8], span_flush: bool) -> usize {
        let mut tmp = [0u8; 4];
        let head = (end - cut).min(4);
        if head > 0 {
            tmp[..head].copy_from_slice(&self.storage.text[cut..cut + head]);
        }
        let tail = (4 - head).min(bytes.len());
        tmp[head..head + tail].copy_from_slice(&bytes[..tail]);
        let look = &tmp[..head + tail];
        let next_cp = if utf8_incomplete(look) {
            None
        } else {
            let mut p = 0;
            Some(decode(look, &mut p))
        };
        let min_cut = if span_flush {
            if self.span_count > 1 {
                usize::from(self.storage.spans[1].start).max(1)
            } else {
                cut
            }
        } else {
            1
        };
        let rewind = continuation_rewind(
            &self.storage.text[..cut],
            next_cp,
            true,
            self.par_dir,
            min_cut,
        );
        if rewind >= min_cut && rewind < cut {
            rewind
        } else {
            cut
        }
    }
    fn append(&mut self, backend: &mut dyn Backend, bytes: &[u8]) -> usize {
        let text_full = self.len + 1 >= self.storage.text.len();
        let span_flush = bytes != b"\n" && self.span_bank_needs_flush();
        if !text_full && bytes == b"\n" && self.span_bank_needs_flush() {
            self.pending_margin = 0;
        }
        if text_full || span_flush {
            if span_flush || (text_full && self.state().flags & !STYLE_MASK != 0) {
                self.par_case = true;
            }
            let end = self.len;
            let mut cut = end;
            if bytes[0] & 0xc0 == 0x80 {
                // A full buffer can end inside a character, even across callbacks.
                // A lead byte at distance `back` must start a sequence longer than `back + 1` bytes.
                for (back, (&byte, min_lead)) in self.storage.text[..end].iter().rev()
                    .zip([0xc2, 0xe0, 0xf0]).enumerate()
                {
                    if byte & 0xc0 != 0x80 {
                        if (min_lead..=0xf4).contains(&byte) {
                            cut = end - 1 - back;
                        }
                        break;
                    }
                }
            }
            if cut == 0 {
                self.failed = true;
                return 0;
            }
            if self.par_case {
                cut = self.rewind_case_cut(cut, end, bytes, span_flush);
            }
            let span_at_cut = if cut < end { self.span_index(cut) } else { 0 };
            let old_span_count = self.span_count;
            let saved_pending_margin = if span_flush { self.pending_margin } else { 0 };
            if cut < end {
                let first_moved_start = usize::from(self.storage.spans[span_at_cut].start);
                self.span_count = span_at_cut + usize::from(first_moved_start < cut);
            }
            self.len = cut;
            let carry_from = self.flush_inner(backend, true);
            if self.failed || self.stopped {
                return 0;
            }
            let carry_end = if cut < end { cut } else { usize::MAX };
            let mut carry_px = 0i32;
            if self.par_case && old_span_count > 0 {
                if carry_from == 0 {
                    carry_px = carry_px.saturating_add(i32::from(self.carried_margin_px));
                }
                for sp in &self.storage.spans[..old_span_count] {
                    let st = usize::from(sp.start);
                    if st >= carry_from && st < carry_end && sp.margin > 0 {
                        carry_px = carry_px.saturating_add(self.margin(*sp));
                    }
                }
            }
            self.carried_margin_px = carry_px.clamp(0, i32::from(i16::MAX)) as i16;
            self.pending_margin = saved_pending_margin;
            self.paragraph.before = 0;
            if !self.par_case || self.line_count > 0 {
                self.continued = true;
            }
            if cut < end {
                // Flushing resets the counts while leaving the buffered data intact.
                let moved = old_span_count - span_at_cut;
                self.storage.spans.copy_within(span_at_cut..old_span_count, 0);
                let first = &mut self.storage.spans[0];
                if usize::from(first.start) < cut {
                    first.margin = 0;
                }
                first.start = 0;
                for sp in &mut self.storage.spans[1..moved] {
                    sp.start -= cut as u16;
                }
                self.storage.text.copy_within(cut..end, 0);
                self.len = end - cut;
                self.span_count = moved;
                if self.par_case {
                    self.note();
                }
            }
        }
        if self.span_count == 0 {
            let s = self.state();
            self.storage.spans[0] = Span {
                start: 0,
                flags: s.flags,
                size: s.size,
                margin: self.pending_margin.min(65535) as u16,
                link: self.current_link,
            };
            self.span_count = 1;
            self.pending_margin = 0
        }
        let free = &mut self.storage.text[self.len..];
        let count = (free.len() - 1).min(bytes.len());
        free[..count].copy_from_slice(&bytes[..count]);
        self.len += count;
        if count > 0 && self.storage.spans[self.span_count - 1].flags & !STYLE_MASK != 0 {
            self.par_case = true;
        }
        count
    }
    /// Attribute values are consumed during the callback; caller does not retain XML views.
    pub fn start(
        &mut self, backend: &mut dyn Backend,
        name: &[u8],
        class: Option<&[u8]>,
        inline: Option<&[u8]>,
        id: Option<&[u8]>,
        href: Option<&[u8]>,
        src: Option<&[u8]>,
    ) {
        self.start_tag(backend, name, StartAttrs { class, inline, id, href, src, alt: None })
    }
    pub fn start_tag(&mut self, backend: &mut dyn Backend, name: &[u8], attrs: StartAttrs<'_>) {
        let StartAttrs { class, inline, id, href, src, alt } = attrs;
        if self.failed || self.stopped {
            return;
        }
        let n = local(name);
        if style::suppressed(n) {
            self.suppress += 1;
            if n == b"style" {
                self.in_style += 1
            }
            return;
        }
        if n == b"body" {
            self.in_body = true;
            let chapter_case =
                css::cascade_for(&self.chapter_css.finish(), Some(n), class, None).text_case;
            let inline_case = inline
                .map(|v| css::parse_inline_style(Some(v)).text_case)
                .unwrap_or(0);
            if self.params.stylesheet.is_some() || chapter_case != 0 || inline_case != 0 {
                let mut d = style::defaults(n);
                if let Some(sheet) = self.params.stylesheet {
                    d.apply_over(&css::cascade_for(&sheet, Some(n), class, None));
                }
                for tc in [chapter_case, inline_case] {
                    if tc != 0 {
                        d.apply_over(&CssDecl {
                            text_case: tc,
                            ..CssDecl::default()
                        });
                    }
                }
                self.push(d);
                self.latch(b"", CssDecl::default(), true)
            }
            return;
        }
        self.depth += 1;
        if !self.in_body || self.suppress > 0 {
            return;
        }
        let mut d = style::defaults(n);
        if let Some(sheet) = self.params.stylesheet {
            d.apply_over(&css::cascade_for(&sheet, Some(n), class, None));
        }
        d.apply_over(&css::cascade_for(
            &self.chapter_css.finish(),
            Some(n),
            class,
            None,
        ));
        if let Some(v) = inline {
            d.apply_over(&css::parse_inline_style(Some(v)))
        }
        self.push(d);
        if d.margin_left_pct >= 0 {
            self.pending_margin += u32::from(self.state().size) * d.margin_left_pct as u32 / 100;
            self.note()
        }
        if let Some(id) = id {
            backend.anchor(
                css::hash(id),
                self.chars
                    .wrapping_add(count(&self.storage.text[..self.len])),
            )
        }
        if n == b"a" {
            if let Some(h) = href {
                self.start_link(h)
            }
        }
        if style::block(n) {
            self.flush(backend);
            self.latch(n, d, false)
        } else if n == b"br" {
            self.append(backend, b"\n");
        } else if n == b"hr" {
            self.flush(backend);
            let lh = self.line_height(backend, self.params.base_size);
            self.advance_y(lh)
        } else if matches!(n, b"img" | b"image") {
            if let Some(s) = src.or(href) {
                if !self.state().hidden {
                    self.flush(backend);
                    let placed = self.place_image(backend, s);
                    if self.params.picture_notes {
                        self.take_picture_position(backend, placed, PictureNote::of(alt))
                    }
                }
            }
        } else {
            self.note()
        }
    }
    pub fn end(&mut self, backend: &mut dyn Backend, name: &[u8]) {
        if self.failed || self.stopped {
            return;
        }
        let n = local(name);
        if style::suppressed(n) {
            if self.suppress > 0 {
                self.suppress -= 1
            }
            if n == b"style" && self.in_style > 0 {
                self.in_style -= 1;
                if self.style_ok && self.style_len > 0 {
                    self.chapter_css
                        .add_text(&self.storage.style_text[..self.style_len]);
                    self.style_len = 0
                }
            }
            return;
        }
        if n == b"body" {
            self.flush(backend);
            self.in_body = false;
            return;
        }
        if self.depth > 0 {
            self.depth -= 1
        }
        if !self.in_body || self.suppress > 0 {
            return;
        }
        self.pop();
        if n == b"a" && self.current_link != 0 {
            self.current_link = 0;
            self.note()
        }
        if style::block(n) {
            self.flush(backend);
            self.latch(b"", CssDecl::default(), true)
        } else {
            self.note()
        }
    }
    pub fn text(&mut self, backend: &mut dyn Backend, t: &[u8]) {
        if self.in_style > 0 && self.style_ok && self.params.embedded_styles {
            for b in t {
                if self.style_len + 1 < self.storage.style_text.len() {
                    self.storage.style_text[self.style_len] = *b;
                    self.style_len += 1
                }
            }
            return;
        }
        if self.failed || self.stopped || !self.in_body || self.suppress > 0 || self.state().hidden
        {
            return;
        }
        // Copy collapsed runs; append keeps the original span/overflow boundary.
        let mut at = 0;
        while at < t.len() {
            if matches!(t[at], b' ' | b'\t' | b'\r' | b'\n') {
                self.pending_space = self.len > 0;
                at += 1;
            } else {
                if self.pending_space {
                    if self.append(backend, b" ") == 0 {
                        return;
                    }
                    self.pending_space = false;
                }
                let start = at;
                at += 1;
                while at < t.len() {
                    if matches!(t[at], b'\t' | b'\r' | b'\n') {
                        break;
                    }
                    if t[at] == b' ' && (at + 1 == t.len()
                        || matches!(t[at + 1], b' ' | b'\t' | b'\r' | b'\n')) {
                        break;
                    }
                    at += 1;
                }
                let mut run = &t[start..at];
                while !run.is_empty() {
                    let count = self.append(backend, run);
                    if count == 0 {
                        return;
                    }
                    run = &run[count..];
                }
            }
        }
    }
    pub fn finish(&mut self, backend: &mut dyn Backend) -> bool {
        if self.failed {
            return false;
        }
        self.flush(backend);
        if !self.failed && (self.run_count > 0 || self.image_count > 0) {
            self.emit_page(backend)
        }
        !self.failed
    }
    fn line_height(&mut self, backend: &mut dyn Backend, size: u16) -> i16 {
        (i32::from(backend.line_height(size)) * i32::from(self.params.line_spacing) / 100)
            as i16
    }
    fn advance_y(&mut self, dy: i16) {
        if self.y > self.params.top {
            self.y = self.y.wrapping_add(dy)
        }
    }
    fn span_index(&self, at: usize) -> usize {
        let mut found = 0;
        for i in 0..self.span_count {
            if usize::from(self.storage.spans[i].start) > at {
                break;
            }
            found = i
        }
        found
    }
    fn extra(&self, at: usize) -> u8 {
        if self.params.focus && self.storage.levels[at] & 64 != 0 {
            BOLD
        } else {
            0
        }
    }
    #[inline(never)]
    fn transform_codepoint(&self, backend: &mut dyn Backend, cp: u32, pos: usize, flags: u8) -> u32 {
        let mapped = match (flags & TRANSFORM_MASK) >> TRANSFORM_SHIFT {
            TRANSFORM_UPPER => case::to_upper(cp),
            TRANSFORM_LOWER => case::to_lower(cp),
            TRANSFORM_CAPITALIZE => {
                if self.storage.levels[pos] & case::WORD_START_BIT != 0 {
                    case::to_upper(cp)
                } else {
                    cp
                }
            }
            _ => cp,
        };
        if mapped != cp && backend.covers(mapped) {
            mapped
        } else {
            cp
        }
    }
    #[inline(never)]
    fn shaped_case_styled(
        &self,
        backend: &mut dyn Backend,
        pos: usize,
        mut cp: u32,
        span_flags: u8,
        prev_small_cap: bool,
    ) -> (u32, bool, bool) {
        if cp == 0x00ad {
            return (cp, prev_small_cap, true);
        }
        if span_flags & TRANSFORM_MASK != 0 {
            cp = self.transform_codepoint(backend, cp, pos, span_flags);
        }
        if span_flags & SMALL_CAPS != 0 {
            // Inhibit lowercase ligatures (such as fi/fl) in small-caps runs.
            let up = case::to_upper(cp);
            if up != cp && backend.covers(up) {
                return (up, true, true);
            }
            if is_combining(cp) {
                return (cp, prev_small_cap, true);
            }
            return (cp, false, true);
        }
        (cp, false, false)
    }
    #[inline(never)]
    pub(super) fn shaped_case(
        &mut self,
        backend: &mut dyn Backend,
        end: usize,
        i: &mut usize,
        si: usize,
        prev_small_cap: bool,
    ) -> (u32, bool) {
        let pos = *i;
        let mut cp = decode(&self.storage.text[..end], i);
        let mut form = (self.storage.levels[pos] >> 3) & 7;
        if form == 7 {
            form = 2
        }
        if form == 5 || form == 6 {
            let mut lig = 0;
            if *i < end {
                let mut j = *i;
                lig = arabic::lam(decode(&self.storage.text[..end], &mut j), form == 6);
                if lig != 0 {
                    *i = j
                }
            }
            if lig != 0 {
                cp = lig
            } else {
                let solo = arabic::presentation(cp, if form == 6 { 4 } else { 3 });
                if solo != 0 && backend.covers(solo) {
                    cp = solo
                }
            }
        } else if form != 0 {
            let p = arabic::presentation(cp, form);
            if p != 0 && backend.covers(p) {
                cp = p
            }
        }
        let span_flags = self.storage.spans[si].flags;
        let mut has_transform = false;
        if self.par_case && (cp == 0x00ad || span_flags & !STYLE_MASK != 0) {
            let (mapped, small_cap, done) =
                self.shaped_case_styled(backend, pos, cp, span_flags, prev_small_cap);
            if done {
                return (mapped, small_cap);
            }
            cp = mapped;
            has_transform = span_flags & TRANSFORM_MASK != 0;
        }
        let style = span_flags & STYLE_MASK;
        // Ordered spans make the next start the exact boundary of this span.
        let span_end = if si + 1 < self.span_count {
            usize::from(self.storage.spans[si + 1].start)
        } else {
            end
        };
        while *i < end && *i < span_end {
            if self.params.focus && (self.storage.levels[pos] ^ self.storage.levels[*i]) & 64 != 0 {
                break;
            }
            let next_pos = *i;
            let mut j = next_pos;
            let mut next = decode(&self.storage.text[..end], &mut j);
            if has_transform {
                next = self.transform_codepoint(backend, next, next_pos, span_flags);
            }
            let lig = backend.ligature(cp, next, style);
            if lig == 0 {
                break;
            }
            cp = lig;
            *i = j
        }
        (cp, false)
    }
    #[inline(always)]
    fn advance_plain(backend: &mut dyn Backend, cp: u32, prev: u32, sz: u16, style: u8) -> i32 {
        if cp == 10 || cp == 0xad {
            return 0;
        }
        let mut a = i32::from(backend.advance(cp, sz, style));
        if prev != 0 {
            a += i32::from(backend.kerning(prev, cp, sz, style));
            // Both spacing rules require at least one CJK-range codepoint.
            if (prev | cp) >= 0x2e80 {
                if crosses(prev, cp) {
                    a += i32::from(sz / 4)
                }
                a += compression(prev, cp, sz)
            }
        }
        a
    }
    fn advance_sized(
        backend: &mut dyn Backend,
        cp: u32,
        prev_kern: u32,
        prev_script: u32,
        sz: u16,
        base_sz: u16,
        flags: u8,
        reset_kern_on_script: bool,
    ) -> i32 {
        if cp == 10 || cp == 0xad {
            return 0;
        }
        let style = flags & STYLE_MASK;
        let mut a = i32::from(backend.advance(cp, sz, style));
        let mut script_split = false;
        // Both spacing rules require at least one CJK-range codepoint and match
        // place_line's unreduced span size so mixed-script order is symmetric.
        if prev_script != 0 && (prev_script | cp) >= 0x2e80 {
            if crosses(prev_script, cp) {
                a += i32::from(base_sz / 4);
                script_split = true;
            }
            let comp = compression(prev_script, cp, base_sz);
            if comp != 0 {
                a += comp;
                script_split = true;
            }
        }
        if prev_kern != 0 && !(reset_kern_on_script && script_split) {
            a += i32::from(backend.kerning(prev_kern, cp, sz, style));
        }
        a
    }
    #[inline(never)]
    fn step_advance_case(
        &self,
        backend: &mut dyn Backend,
        st: &mut StepState,
        cp: u32,
        small_cap: bool,
        s: Span,
        base_sz: u16,
        si: usize,
        at: usize,
    ) -> i32 {
        if cp == 0x00ad {
            return 0;
        }
        let lvl = self.storage.levels[at];
        let extra = if self.params.focus && lvl & 64 != 0 { BOLD } else { 0 };
        let level = lvl & 7;
        let sz = if small_cap { case::small_cap_px(base_sz) } else { base_sz };
        let run_split = small_cap != st.prev_small_cap
            || si as u16 != st.prev_span
            || extra != st.prev_extra
            || level != st.prev_level;
        let kern_prev = if run_split { 0 } else { st.prev_kern };
        let adv = Self::advance_sized(
            backend,
            cp,
            kern_prev,
            st.prev_script,
            sz,
            base_sz,
            s.flags | extra,
            true,
        );
        st.prev_kern = cp;
        st.prev_script = cp;
        st.prev_span = si as u16;
        st.prev_extra = extra;
        st.prev_level = level;
        st.prev_small_cap = small_cap;
        adv
    }
    #[inline(always)]
    fn step_advance(
        &self,
        backend: &mut dyn Backend,
        st: &mut StepState,
        cp: u32,
        small_cap: bool,
        s: Span,
        base_sz: u16,
        si: usize,
        at: usize,
    ) -> i32 {
        if !self.par_case {
            let adv = Self::advance_plain(
                backend,
                cp,
                st.prev_kern,
                base_sz,
                (s.flags | self.extra(at)) & STYLE_MASK,
            );
            st.prev_kern = cp;
            st.prev_script = cp;
            return adv;
        }
        self.step_advance_case(backend, st, cp, small_cap, s, base_sz, si, at)
    }
    #[inline(never)]
    fn hyphen_width(&self, backend: &mut dyn Backend, tail: Tail) -> i32 {
        // Placement suppresses trailing hyphens on RTL paragraphs (`!self.rtl`),
        // so case-styled RTL measurement and candidate pricing charge 0 px.
        if self.par_case && self.rtl {
            return 0;
        }
        let s = self.storage.spans[usize::from(tail.span)];
        let base_sz = self.size(s.size);
        let sz = if tail.small_cap { case::small_cap_px(base_sz) } else { base_sz };
        let style = (if self.par_case { s.flags | tail.extra } else { s.flags }) & STYLE_MASK;
        let mut w = i32::from(backend.advance(45, sz, style));
        if self.par_case && tail.cp != 0 {
            w += i32::from(backend.kerning(tail.cp, 45, sz, style));
        }
        w
    }
    fn margin(&self, s: Span) -> i32 {
        i32::from(self.params.base_size) * i32::from(s.margin) / 100
    }
    fn margin_at_byte(&self, si: usize, at: usize) -> i32 {
        let s = self.storage.spans[si];
        let mut m = if at == usize::from(s.start) { self.margin(s) } else { 0 };
        if self.par_case && at == 0 {
            m += i32::from(self.carried_margin_px);
        }
        m
    }
    fn margin_at_offset(&self, at: usize) -> i32 {
        if self.span_count == 0 {
            return 0;
        }
        self.margin_at_byte(self.span_index(at), at)
    }
    #[inline(never)]
    fn measure_plain(&mut self, backend: &mut dyn Backend, from: usize, to: usize) -> i32 {
        let (mut width, mut prev, mut i) = (0, 0, from);
        let mut si = self.span_index(from);
        while i < to {
            while si + 1 < self.span_count && usize::from(self.storage.spans[si + 1].start) <= i {
                si += 1;
            }
            let s = self.storage.spans[si];
            let span_size = self.size(s.size);
            let span_end = if si + 1 < self.span_count {
                usize::from(self.storage.spans[si + 1].start).min(to)
            } else {
                to
            };
            if i == usize::from(s.start) && s.margin > 0 {
                width += self.margin(s);
                prev = 0;
            }
            while i < span_end {
                let start = i;
                let (cp, _) = self.shaped_case(backend, to, &mut i, si, false);
                width += Self::advance_plain(
                    backend,
                    cp,
                    prev,
                    span_size,
                    (s.flags | self.extra(start)) & STYLE_MASK,
                );
                prev = cp;
            }
        }
        width
    }
    #[inline(never)]
    fn measure_case(
        &mut self,
        backend: &mut dyn Backend,
        from: usize,
        to: usize,
        st: &mut StepState,
    ) -> i32 {
        let (mut width, mut i) = (0, from);
        let mut si = self.span_index(from);
        while i < to {
            while si + 1 < self.span_count && usize::from(self.storage.spans[si + 1].start) <= i {
                si += 1;
            }
            let s = self.storage.spans[si];
            let span_size = self.size(s.size);
            let span_end = if si + 1 < self.span_count {
                usize::from(self.storage.spans[si + 1].start).min(to)
            } else {
                to
            };
            if i == usize::from(s.start) {
                let m = self.margin_at_byte(si, i);
                width += m;
                if m > 0 {
                    st.prev_kern = 0;
                }
            }
            while i < span_end {
                let start = i;
                let same_span_sc = st.prev_small_cap && si as u16 == st.prev_span;
                let (cp, small_cap) = self.shaped_case(backend, to, &mut i, si, same_span_sc);
                width += self.step_advance_case(backend, st, cp, small_cap, s, span_size, si, start);
            }
        }
        width
    }
    #[inline(never)]
    fn measure_line_case(
        &mut self,
        backend: &mut dyn Backend,
        start: usize,
        end: usize,
        flags: u8,
    ) -> i32 {
        let mut st = StepState::reset(self.span_index(start));
        let mut width = self.measure_case(backend, start, end, &mut st);
        if flags & 2 != 0 {
            width += self.hyphen_width(backend, st.tail());
        }
        width
    }
    #[inline(never)]
    fn try_hyphen_case(
        &mut self,
        backend: &mut dyn Backend,
        line: usize,
        word: usize,
        budget: i32,
        pos: &[u8],
    ) -> usize {
        let mut pre_st = StepState::reset(self.span_index(line));
        let prefix = self.measure_case(backend, line, word, &mut pre_st);
        // Preserve #172's word-start hyphen probe callback order in host traces.
        let word_span = self.storage.spans[self.span_index(word)];
        let _ = backend.advance(45, self.size(word_span.size), word_span.flags & STYLE_MASK);
        for p in pos.iter().rev() {
            let split = word + usize::from(*p);
            if split > line {
                let mut end_st = pre_st;
                let part_w = self.measure_case(backend, word, split, &mut end_st);
                let hw = self.hyphen_width(backend, end_st.tail());
                if prefix + part_w + hw <= budget {
                    return split;
                }
            }
        }
        0
    }
    fn try_hyphen(&mut self, backend: &mut dyn Backend, line: usize, word: usize, budget: i32) -> usize {
        let Some(h) = self.hyphenator else { return 0 };
        if !h.ready() {
            return 0;
        }
        let (mut end, mut chars) = (word, 0);
        while end < self.len {
            let mut next = end;
            if !hyph(decode(&self.storage.text[..self.len], &mut next)) {
                break;
            }
            end = next;
            chars += 1
        }
        if chars < 5 {
            return 0;
        }
        let mut pos = [0; 16];
        let n = h.break_positions(&self.storage.text[word..end], &mut pos);
        if n == 0 {
            return 0;
        }
        if self.par_case {
            return self.try_hyphen_case(backend, line, word, budget, &pos[..n]);
        }
        let prefix = self.measure_plain(backend, line, word);
        let word_span = self.storage.spans[self.span_index(word)];
        let hw = i32::from(backend.advance(45, self.size(word_span.size), word_span.flags & STYLE_MASK));
        for p in pos[..n].iter().rev() {
            let split = word + usize::from(*p);
            if split > line && prefix + self.measure_plain(backend, word, split) + hw <= budget {
                return split;
            }
        }
        0
    }
    #[inline(never)]
    fn record_case(
        &mut self,
        backend: &mut dyn Backend,
        start: usize,
        mut end: usize,
        flags: u8,
        carry_cut: bool,
        has_visible: bool,
    ) -> bool {
        let had_newline = end > start && self.storage.text[end - 1] == b'\n';
        if !has_visible && !had_newline && (carry_cut || self.continued || self.line_count > 0) {
            return false;
        }
        let trim_shy = flags & 2 == 0 || self.rtl;
        loop {
            if end > start && matches!(self.storage.text[end - 1], b' ' | b'\n') {
                end -= 1;
            } else if trim_shy
                && end >= start + 2
                && self.storage.text[end - 2..end] == [0xc2, 0xad]
                && (carry_cut || self.margin_at_offset(end - 2) == 0)
            {
                end -= 2;
            } else {
                break;
            }
        }
        self.storage.lines[self.line_count] = self.line_metrics(backend, start, end, flags);
        self.line_count += 1;
        true
    }
    #[inline(never)]
    fn record(
        &mut self,
        backend: &mut dyn Backend,
        start: usize,
        mut end: usize,
        flags: u8,
        carry_cut: bool,
        has_visible: bool,
    ) -> bool {
        if self.par_case {
            return self.record_case(backend, start, end, flags, carry_cut, has_visible);
        }
        while end > start && matches!(self.storage.text[end - 1], b' ' | b'\n') {
            end -= 1;
        }
        self.storage.lines[self.line_count] = self.line_metrics(backend, start, end, flags);
        self.line_count += 1;
        true
    }
    fn line_metrics(&mut self, backend: &mut dyn Backend, start: usize, end: usize, flags: u8) -> Line {
        let mut r = Line {
            start,
            end,
            flags,
            ..Line::default()
        };
        let (mut prev, mut i) = (0, start);
        while i < end {
            let cp = decode(&self.storage.text[..end], &mut i);
            if cp == 0x00ad && self.par_case {
                continue;
            }
            if cp == 32 {
                r.spaces += 1
            } else if prev != 0 && cjk(prev) && cjk(cp) && compression(prev, cp, 2) == 0 {
                if hangul(prev) && hangul(cp) {
                    r.hangul += 1
                } else {
                    r.cjk += 1
                }
            }
            prev = cp
        }
        if !self.par_case {
            r.width = self.measure_plain(backend, start, end);
            if flags & 2 != 0 {
                let s = self.storage.spans[self.span_index(if end > start { end - 1 } else { start })];
                r.width += i32::from(backend.advance(45, self.size(s.size), s.flags & STYLE_MASK));
            }
        } else {
            r.width = self.measure_line_case(backend, start, end, flags);
        }
        r
    }
    #[inline(never)]
    fn rewind_combining_cut(&self, start: usize, at: usize) -> usize {
        let mut cand = at;
        while cand > start {
            let mut prev = cand - 1;
            while prev > start && self.storage.text[prev] & 0xc0 == 0x80 {
                prev -= 1;
            }
            let mut probe = prev;
            let pcp = decode(&self.storage.text[..cand], &mut probe);
            if probe != cand {
                break;
            }
            if is_combining(pcp) || pcp == 0x00ad {
                cand = prev;
            } else {
                if pcp != 32 && pcp != 10 {
                    cand = prev;
                }
                break;
            }
        }
        let mut scan = start;
        let mut prefix_visible = false;
        while scan < cand {
            let vcp = decode(&self.storage.text[..cand], &mut scan);
            if vcp != 32 && vcp != 10 && vcp != 0x00ad {
                prefix_visible = true;
                break;
            }
        }
        if prefix_visible { cand } else { at }
    }
    #[inline(never)]
    fn measure_lines(&mut self, backend: &mut dyn Backend, continuing: bool) -> usize {
        self.line_count = 0;
        let mut carry_from = self.len;
        let (mut has_case, mut has_capitalize) = (false, false);
        for s in &self.storage.spans[..self.span_count] {
            if s.flags & !STYLE_MASK != 0 {
                has_case = true;
                if (s.flags & TRANSFORM_MASK) >> TRANSFORM_SHIFT == TRANSFORM_CAPITALIZE {
                    has_capitalize = true;
                }
            }
        }
        self.par_case |= has_case;
        self.breaker.breaks(
            &self.storage.text[..self.len],
            self.params.language,
            &mut self.storage.breaks[..self.len],
        );
        let (rtl, has_arabic) = bidi_with_arabic(
            &self.storage.text[..self.len],
            &mut self.storage.levels[..self.len],
            self.par_case,
            &mut self.par_dir,
            self.par_case,
        );
        self.rtl = rtl;
        if has_arabic {
            arabic::shape(
                &self.storage.text[..self.len],
                &mut self.storage.levels[..self.len],
                backend,
            );
        }
        if self.params.focus {
            let skip = self.par_case && self.par_dir & DIR_FOCUS_WORD != 0;
            let ends = focus_with_carry(
                &self.storage.text[..self.len],
                &mut self.storage.levels[..self.len],
                skip,
            );
            if ends {
                self.par_dir |= DIR_FOCUS_WORD;
            } else {
                self.par_dir &= !DIR_FOCUS_WORD;
            }
        }
        if continuing || has_capitalize {
            self.word_state = case::mark_word_starts(
                &self.storage.text[..self.len],
                &mut self.storage.levels[..self.len],
                self.word_state,
            );
        }
        if self
            .params
            .language
            .is_some_and(|l| l.to_bytes().ends_with(b"-keep-all"))
        {
            let (mut i, mut prev, mut end) = (0, 0, 0);
            while i < self.len {
                let cp = decode(&self.storage.text[..self.len], &mut i);
                if end > 0 && hangul(prev) && hangul(cp) && self.storage.breaks[end - 1] == 1 {
                    self.storage.breaks[end - 1] = 2
                }
                prev = cp;
                end = i
            }
        }
        let base = i32::from(self.params.width)
            - i32::from(self.params.left)
            - i32::from(self.params.right)
            - i32::from(self.paragraph.indent);
        let indent = i32::from(self.size(self.paragraph.size))
            * i32::from(self.paragraph.text_indent.max(0))
            / 100;
        let (mut start, mut i, mut width, mut last, mut word) = (0, 0, 0, 0, 0);
        let mut line_visible = false;
        let mut has_space = false;
        let mut resume_after_space = 0;
        let mut st = StepState::reset(0);
        let (mut si, mut span_start, mut span_end, mut span_size) = (0, 0, 0, 0);
        let mut s = Span::default();
        while i < self.len && self.line_count + 1 < self.storage.lines.len() {
            let max = base
                - if self.line_count == 0 && !self.continued {
                    indent
                } else {
                    0
                };
            let at = i;
            // A line rewind can cross a span boundary; refresh only on exit.
            if at < span_start || at >= span_end {
                si = self.span_index(at);
                s = self.storage.spans[si];
                span_start = usize::from(s.start);
                span_end = if si + 1 < self.span_count {
                    usize::from(self.storage.spans[si + 1].start)
                } else {
                    self.len
                };
                span_size = self.size(s.size);
            }
            let same_span_sc = st.prev_small_cap && si as u16 == st.prev_span;
            let (cp, small_cap) = self.shaped_case(backend, self.len, &mut i, si, same_span_sc);
            let margin = if !self.par_case {
                if at == span_start { self.margin(s) } else { 0 }
            } else {
                self.margin_at_byte(si, at)
            };
            if margin > 0 {
                if self.par_case {
                    st.prev_kern = 0;
                } else {
                    st = StepState::reset(si);
                }
            }
            let adv = margin + self.step_advance(backend, &mut st, cp, small_cap, s, span_size, si, at);
            let first_glyph = at == start || (self.par_case && !line_visible && !has_space);
            if cp != 10 && width + adv > max && !first_glyph {
                if self.par_case && !line_visible {
                    start = resume_after_space;
                    i = start;
                    width = 0;
                    has_space = false;
                    resume_after_space = start;
                    st = StepState::reset(0);
                    last = 0;
                    word = start;
                    continue;
                }
                let hy = self.try_hyphen(backend, start, word.max(start), max);
                let (end, flags) = if hy > start && (last <= start || hy > last) {
                    (hy, 2)
                } else if last > start {
                    (last, if self.last_shy { 2 } else { 0 })
                } else {
                    let cut = if self.par_case && is_combining(cp) {
                        self.rewind_combining_cut(start, at)
                    } else {
                        at
                    };
                    (cut, 0)
                };
                self.record(backend, start, end, flags, false, true);
                start = end;
                loop {
                    if start < self.len && self.storage.text[start] == 32 {
                        start += 1;
                    } else if self.par_case
                        && start + 2 <= self.len
                        && self.storage.text[start..start + 2] == [0xc2, 0xad]
                        && self.margin_at_offset(start) == 0
                    {
                        start += 2;
                    } else {
                        break;
                    }
                }
                i = start;
                width = 0;
                line_visible = false;
                has_space = false;
                resume_after_space = start;
                st = StepState::reset(0);
                last = 0;
                word = start;
                continue;
            }
            width += adv;
            if self.par_case {
                if cp == 32 {
                    if !line_visible {
                        has_space = true;
                        resume_after_space = i;
                    }
                } else if cp == 0xad {
                    if !line_visible && has_space && margin == 0 && resume_after_space == at {
                        resume_after_space = i;
                    }
                } else if cp != 10 {
                    line_visible = true;
                }
            }
            match self.storage.breaks[i - 1] {
                0 => {
                    let cut_end = continuing && i == self.len && self.storage.text[i - 1] != b'\n';
                    let kept = self.record(backend, start, i, 1, cut_end, line_visible);
                    if cut_end {
                        carry_from = if kept {
                            self.storage.lines[self.line_count - 1].end
                        } else {
                            start
                        };
                    }
                    start = i;
                    width = 0;
                    line_visible = false;
                    has_space = false;
                    resume_after_space = i;
                    st = StepState::reset(0);
                    last = 0;
                    word = i
                }
                1 => {
                    if (!self.par_case || line_visible)
                        && (cp != 0xad
                            || !self.par_case
                            || width + self.hyphen_width(backend, st.tail()) <= max)
                    {
                        last = i;
                        self.last_shy = cp == 0xad;
                    }
                    word = i;
                    while word < self.len && self.storage.text[word] == 32 {
                        word += 1
                    }
                }
                _ => {}
            }
        }
        if start < self.len && self.line_count < self.storage.lines.len() {
            let mut tail_keep = line_visible;
            if self.par_case {
                let mut scan = i;
                while !tail_keep && scan < self.len {
                    let cp = decode(&self.storage.text[..self.len], &mut scan);
                    if cp != 32 && cp != 0xad {
                        tail_keep = true;
                    }
                }
            }
            let cut_end = continuing && self.storage.text[self.len - 1] != b'\n';
            let kept = self.record(backend, start, self.len, 1, cut_end, tail_keep);
            if cut_end {
                carry_from = if kept {
                    self.storage.lines[self.line_count - 1].end
                } else {
                    start
                };
            }
        }
        if self.line_count > 0 {
            self.storage.lines[self.line_count - 1].flags |= 1
        }
        carry_from
    }
    fn accumulate_glyph_extent(
        &mut self,
        backend: &mut dyn Backend,
        cp: u32,
        glyph_sz: u16,
        base_sz: u16,
        span_flags: u8,
        run_style: u8,
        run_ascent: &mut Option<i32>,
        fallback_descent: &mut Option<i32>,
        max_above: &mut i32,
        max_below: &mut i32,
    ) {
        let (above_raw, below_raw) = if let Some((ink_above, ink_below)) =
            backend.ink_extents(cp, glyph_sz, run_style)
        {
            if ink_above <= 0 && ink_below <= 0 {
                return;
            }
            let ascent = *run_ascent.get_or_insert_with(|| i32::from(backend.ascent(glyph_sz)));
            (ascent.max(i32::from(ink_above)), i32::from(ink_below).max(0))
        } else {
            if !is_visible_cp(cp) {
                return;
            }
            let ascent = *run_ascent.get_or_insert_with(|| i32::from(backend.ascent(glyph_sz)));
            let descent = *fallback_descent.get_or_insert_with(|| {
                let run_lh = i32::from(self.line_height(backend, glyph_sz));
                (run_lh - ascent).max(i32::from(glyph_sz.div_ceil(4)))
            });
            (ascent, descent)
        };
        let (above, below) = if span_flags & SUP != 0 {
            let shift = (u32::from(base_sz) * 33 / 100) as i32;
            (above_raw + shift, below_raw - shift)
        } else if span_flags & SUB != 0 {
            let shift = (u32::from(base_sz) * 12 / 100) as i32;
            (above_raw - shift, below_raw + shift)
        } else {
            (above_raw, below_raw)
        };
        *max_above = (*max_above).max(above);
        *max_below = (*max_below).max(below);
    }
    fn line_extra(&mut self, backend: &mut dyn Backend, line: Line, size: u16, lh: i16) -> LineExtra {
        if !self.params.inline_ascent || line.end <= line.start {
            return LineExtra::default();
        }
        let base_ascent = i32::from(backend.ascent(size));
        // Keep base_below signed (lh - base_ascent) so condensed line_spacing (< 100%)
        // accounts for the full distance from the placed baseline to the row bottom.
        let base_below = i32::from(lh) - base_ascent;
        let mut max_above = base_ascent;
        let mut max_below = base_below;
        let mut i = line.start;
        let mut si = self.span_index(line.start);
        let mut cached_si = usize::MAX;
        let (mut full_ascent, mut full_descent) = (None, None);
        let (mut sc_ascent, mut sc_descent) = (None, None);
        let mut prev_small_cap = false;
        let mut last_vis: Option<(u16, u16, u8, u8)> = None;
        while i < line.end {
            while si + 1 < self.span_count && usize::from(self.storage.spans[si + 1].start) <= i {
                si += 1;
            }
            if si != cached_si {
                cached_si = si;
                full_ascent = None;
                full_descent = None;
                sc_ascent = None;
                sc_descent = None;
                prev_small_cap = false;
            }
            let pos = i;
            let s = self.storage.spans[si];
            let base_sz = self.size(s.size);
            let tall = base_sz > size || s.flags & (SUP | SUB) != 0;
            let (mut cp, small_cap) =
                self.shaped_case(backend, line.end, &mut i, si, prev_small_cap);
            let run_style = (s.flags | self.extra(pos)) & STYLE_MASK;
            if cp == 0xad {
                if !self.par_case || last_vis.is_none() {
                    last_vis = Some((base_sz, base_sz, s.flags, run_style));
                }
                continue;
            }
            prev_small_cap = small_cap;
            let glyph_sz = if small_cap { case::small_cap_px(base_sz) } else { base_sz };
            last_vis = Some((glyph_sz, base_sz, s.flags, run_style));
            if tall {
                if self.storage.levels[pos] & 1 != 0 {
                    cp = mirror(cp);
                }
                let (ra, fd) = if small_cap {
                    (&mut sc_ascent, &mut sc_descent)
                } else {
                    (&mut full_ascent, &mut full_descent)
                };
                self.accumulate_glyph_extent(
                    backend,
                    cp,
                    glyph_sz,
                    base_sz,
                    s.flags,
                    run_style,
                    ra,
                    fd,
                    &mut max_above,
                    &mut max_below,
                );
            }
        }
        if line.flags & 2 != 0 && !self.rtl {
            if let Some((glyph_sz, base_sz, span_flags, run_style)) = last_vis {
                if base_sz > size || span_flags & (SUP | SUB) != 0 {
                    let (mut ra, mut fd) = (None, None);
                    self.accumulate_glyph_extent(
                        backend,
                        45,
                        glyph_sz,
                        base_sz,
                        span_flags,
                        run_style,
                        &mut ra,
                        &mut fd,
                        &mut max_above,
                        &mut max_below,
                    );
                }
            }
        }
        LineExtra {
            above: (max_above - base_ascent).clamp(0, i32::from(i16::MAX)) as i16,
            below: (max_below - base_below).clamp(0, i32::from(i16::MAX)) as i16,
        }
    }
    pub fn flush(&mut self, backend: &mut dyn Backend) {
        self.flush_inner(backend, false);
    }
    fn flush_inner(&mut self, backend: &mut dyn Backend, continuing: bool) -> usize {
        if self.len == 0 {
            if !continuing {
                if self.par_case && self.continued {
                    let size = self.size(self.paragraph.size);
                    self.advance_y(
                        (i32::from(size)
                            * i32::from(self.paragraph.after)
                            * i32::from(self.params.paragraph_spacing)
                            / 10000) as i16,
                    );
                }
                self.continued = false;
                self.word_state = case::WS_SEP;
                self.par_case = false;
                self.par_dir = DIR_NONE;
                self.carried_margin_px = 0;
            }
            return 0;
        }
        let size = self.size(self.paragraph.size);
        let lh = self.line_height(backend, size);
        if lh <= 0 {
            self.failed = true;
            return self.len;
        }
        self.advance_y(
            (i32::from(size)
                * i32::from(self.paragraph.before)
                * i32::from(self.params.paragraph_spacing)
                / 10000) as i16,
        );
        let carry_from = self.measure_lines(backend, continuing);
        let has_tall_span = self.params.inline_ascent
            && self.storage.spans[..self.span_count]
                .iter()
                .any(|s| self.size(s.size) > size || s.flags & (SUP | SUB) != 0);
        let mut idx = 0;
        let mut continuation = None;
        while idx < self.line_count && !self.failed && !self.stopped {
            let px =
                i32::from(self.params.height) - i32::from(self.params.bottom) - i32::from(self.y);
            let content = self.run_count > 0 || self.y > self.params.top;
            let avail = if px <= 0 {
                0
            } else if !has_tall_span {
                (px / i32::from(lh)) as usize
            } else {
                let (mut used, mut fit) = (0i32, 0usize);
                for i in idx..self.line_count {
                    let line = if i == idx {
                        continuation.unwrap_or(self.storage.lines[i])
                    } else {
                        self.storage.lines[i]
                    };
                    let extra = self.line_extra(backend, line, size, lh);
                    let need = i32::from(lh) + extra.total();
                    if used + need > px {
                        break;
                    }
                    used += need;
                    fit += 1;
                }
                if fit == 0 && !content && px >= i32::from(lh) {
                    // An empty page still places one oversized line rather than dropping text.
                    1
                } else {
                    fit
                }
            };
            let remain = self.line_count - idx;
            if avail == 0 {
                if !content || self.note_fills_its_page() {
                    break;
                }
                self.emit_page(backend);
                continue;
            }
            let mut take = avail.min(remain);
            while take < remain && remain - take < usize::from(self.params.widow) && take > 1 {
                take -= 1
            }
            if idx == 0 && continuation.is_none() && take < remain
                && take < usize::from(self.params.orphan) && content
            {
                if self.note_fills_its_page() {
                    break;
                }
                self.emit_page(backend);
                continue;
            }
            let end = idx + take;
            // A segment break may use spare rows without consuming another measured line.
            let slots = if take == remain {
                if has_tall_span {
                    (px / i32::from(lh)).max(take as i32) as usize
                } else {
                    avail
                }
            } else {
                take
            };
            for _ in 0..slots {
                if idx == end { break; }
                let first = idx == 0 && continuation.is_none();
                let line = continuation.unwrap_or(self.storage.lines[idx]);
                match self.place_line(backend, line, size, lh, first, has_tall_span) {
                    LinePlacement::Placed(rest) => {
                        continuation = rest;
                        if rest.is_none() { idx += 1; }
                    }
                    LinePlacement::NextPage => break,
                    LinePlacement::TooLarge => {
                        self.failed = true;
                        break;
                    }
                }
            }
            if idx < self.line_count && !self.failed {
                if self.note_fills_its_page() {
                    break;
                }
                self.emit_page(backend)
            }
        }
        if !(self.par_case && continuing) {
            self.advance_y(
                (i32::from(size)
                    * i32::from(self.paragraph.after)
                    * i32::from(self.params.paragraph_spacing)
                    / 10000) as i16,
            );
        }
        self.chars = self
            .chars
            .wrapping_add(count(&self.storage.text[..self.len]));
        self.reset_par();
        if !continuing {
            self.continued = false;
            self.word_state = case::WS_SEP;
            self.par_case = false;
            self.par_dir = DIR_NONE;
            self.carried_margin_px = 0;
        }
        carry_from
    }
    /// Newt (#179): the open page starts at the note being laid out, so
    /// another page would share its start and one of the two could never be
    /// reopened. The note stops here; its remaining lines are dropped.
    fn note_fills_its_page(&self) -> bool {
        matches!(self.picture_note_at, Some(at) if self.run_count > 0 && self.page_chars == at)
    }
    fn refused_line(&self) -> LinePlacement {
        if self.run_count > 0 || self.image_count > 0 {
            LinePlacement::NextPage
        } else {
            LinePlacement::TooLarge
        }
    }
    fn overflows_page_bottom(&self, row_lh: i16) -> bool {
        i32::from(self.y) + i32::from(row_lh)
            > i32::from(self.params.height) - i32::from(self.params.bottom)
            && (self.run_count > 0 || self.image_count > 0 || self.y > self.params.top)
    }
    #[inline(never)]
    fn place_line(
        &mut self,
        backend: &mut dyn Backend,
        mut r: Line,
        size: u16,
        lh: i16,
        first: bool,
        has_tall_span: bool,
    ) -> LinePlacement {
        if r.end == r.start {
            if has_tall_span && self.overflows_page_bottom(lh) {
                return LinePlacement::NextPage;
            }
            self.y = if has_tall_span {
                self.y.saturating_add(lh)
            } else {
                self.y.wrapping_add(lh)
            };
            return LinePlacement::Placed(None);
        }
        let indent = if first && !self.continued && self.paragraph.text_indent > 0 {
            i32::from(size) * i32::from(self.paragraph.text_indent) / 100
        } else {
            0
        };
        let max = i32::from(self.params.width)
            - i32::from(self.params.left)
            - i32::from(self.params.right)
            - i32::from(self.paragraph.indent)
            - indent;
        let mut left = max - r.width;
        let justify_hangul = r.spaces == 0;
        let mut gaps = justification_gaps(r, justify_hangul);
        let justify =
            self.paragraph.align == TextAlign::Justify && r.flags & 1 == 0 && gaps > 0 && left > 0;
        let baseline = self.y.wrapping_add(backend.ascent(size));
        let mut segs = [Segment::default(); MAX_SEGMENTS];
        let LineSegments { count: n, next_start, trailing } =
            self.segment_line(backend, r, justify, justify_hangul, &mut segs);
        if n == 0 {
            if has_tall_span && self.overflows_page_bottom(lh) {
                return LinePlacement::NextPage;
            }
            self.y = if has_tall_span {
                self.y.saturating_add(lh)
            } else {
                self.y.wrapping_add(lh)
            };
            return LinePlacement::Placed(None);
        }
        if n > self.storage.runs.len() - self.run_count {
            return self.refused_line();
        }
        let split_rest = if let Some(start) = next_start {
            let (end, flags) = (r.end, r.flags);
            let prev_end = segs[n - 1].end;
            r = self.line_metrics(backend, r.start, prev_end, flags & !2);
            left = max - r.width;
            gaps = justification_gaps(r, justify_hangul);
            Some((start, end, flags))
        } else {
            None
        };
        let extra = if has_tall_span {
            self.line_extra(backend, r, size, lh)
        } else {
            LineExtra::default()
        };
        let line_lh = (i32::from(lh) + extra.total()).min(i32::from(i16::MAX)) as i16;
        if has_tall_span && self.overflows_page_bottom(line_lh) {
            return LinePlacement::NextPage;
        }
        let baseline = if has_tall_span {
            baseline.saturating_add(extra.above)
        } else {
            baseline
        };
        let justify = justify && gaps > 0 && left > 0;
        let per = if justify { left / gaps } else { 0 };
        let mut remainder = if justify { left % gaps } else { 0 };
        let mut align = self.paragraph.align;
        if self.rtl {
            align = match align {
                TextAlign::Left => TextAlign::Right,
                TextAlign::Right => TextAlign::Left,
                _ => align,
            }
        }
        let mut x = i32::from(self.params.left)
            + i32::from(self.paragraph.indent)
            + if self.rtl { 0 } else { indent };
        if align == TextAlign::Right {
            x += left
        } else if align == TextAlign::Center {
            x += left / 2
        }
        // Match frozen placeLine's byte indices for at most 64 segments.
        let mut order = [0u8; MAX_SEGMENTS];
        let mut gap_at = [0u8; MAX_SEGMENTS];
        let (mut high, mut odd) = (0, 255);
        for s in 0..n {
            order[s] = s as u8;
            if s + 1 < n {
                gap_at[s] = (s + 1) as u8;
            }
            high = high.max(segs[s].level);
            if segs[s].level & 1 != 0 {
                odd = odd.min(segs[s].level)
            }
        }
        let base = if self.rtl { 1 } else { 0 };
        let low = (if odd == 255 { 1 } else { odd }).max(base + 1);
        for level in (low..=high).rev() {
            let mut s = 0;
            while s < n {
                if segs[usize::from(order[s])].level < level {
                    s += 1;
                    continue;
                }
                let mut end = s;
                while end < n && segs[usize::from(order[end])].level >= level {
                    end += 1
                }
                order[s..end].reverse();
                if end > s + 1 {
                    gap_at[s..end - 1].reverse();
                }
                s = end
            }
        }
        if self.rtl {
            order[..n].reverse();
            if n > 1 {
                gap_at[..n - 1].reverse();
            }
        }
        let before = (self.run_count, self.link_count, self.pool_len);
        let mut active_link = 0;
        if self.par_case && !self.rtl && segs[0].flags & (SEG_GAP | SEG_SPACE) != 0 {
            let sp0 = self.storage.spans[segs[0].span];
            self.apply_seg_gap_case(&segs[0], sp0, justify, per, &mut remainder, &mut x);
        }
        if self.par_case && self.rtl && !trailing.is_empty() {
            x += i32::from(trailing.margin) + i32::from(trailing.space_adv);
            if justify {
                for _ in 0..trailing.space_gaps {
                    x += per;
                    if remainder > 0 {
                        x += 1;
                        remainder -= 1;
                    }
                }
            }
        }
        for v in 0..n {
            let sg = &segs[usize::from(order[v])];
            let span = self.storage.spans[sg.span];
            let base_sz = self.size(span.size);
            if !self.par_case && v > 0 {
                if sg.flags & (SEG_GAP | SEG_SPACE) != 0 {
                    if sg.flags & SEG_SPACE != 0 {
                        x += i32::from(backend.advance(32, base_sz, span.flags & STYLE_MASK))
                    }
                    if justify {
                        x += per;
                        if remainder > 0 {
                            x += 1;
                            remainder -= 1
                        }
                    }
                } else if sg.flags & SEG_SCRIPT != 0 {
                    x += i32::from(base_sz / 4)
                } else if sg.flags & SEG_COMPRESS != 0 {
                    x -= i32::from(base_sz / 2)
                }
            }
            if self.par_case {
                x += i32::from(sg.shy_margin);
            }
            if (!self.par_case || !is_only_shy(&self.storage.text[sg.start..sg.end]))
                && sg.start == usize::from(span.start)
            {
                x += if !self.par_case {
                    self.margin(span)
                } else {
                    self.margin_at_byte(sg.span, sg.start)
                };
            }
            let hy = sg.end == r.end && r.flags & 2 != 0 && !self.rtl;
            if !self.emit_segment(backend, sg, span, base_sz, hy, baseline, &mut x, &mut active_link) {
                // Unpublished runs, links and bytes are discarded together.
                // The caller retries this whole line at the top of the next page.
                (self.run_count, self.link_count, self.pool_len) = before;
                return self.refused_line();
            }
            if self.par_case && v + 1 < n {
                let g = &segs[usize::from(gap_at[v])];
                let g_span = self.storage.spans[g.span];
                self.apply_seg_gap_case(g, g_span, justify, per, &mut remainder, &mut x);
            }
        }
        if self.par_case && self.rtl && segs[0].flags & (SEG_GAP | SEG_SPACE) != 0 {
            let sp0 = self.storage.spans[segs[0].span];
            self.apply_seg_gap_case(&segs[0], sp0, justify, per, &mut remainder, &mut x);
        }
        if before.0 == 0 {
            self.page_chars = match self.picture_note_at {
                Some(at) => at,
                None => self.chars.wrapping_add(count(&self.storage.text[..r.start])),
            };
        }
        let rest = if let Some((start, end, flags)) = split_rest {
            Some(self.line_metrics(backend, start, end, flags | 1))
        } else {
            None
        };
        self.y = if has_tall_span {
            self.y.saturating_add(line_lh)
        } else {
            self.y.wrapping_add(lh)
        };
        LinePlacement::Placed(rest)
    }
    #[inline(never)]
    fn apply_seg_gap_case(
        &self,
        sg: &Segment,
        span: Span,
        justify: bool,
        per: i32,
        remainder: &mut i32,
        x: &mut i32,
    ) {
        if sg.flags & (SEG_GAP | SEG_SPACE) != 0 {
            *x += i32::from(sg.space_adv);
            if justify {
                for _ in 0..sg.space_gaps {
                    *x += per;
                    if *remainder > 0 {
                        *x += 1;
                        *remainder -= 1
                    }
                }
            }
        } else if sg.flags & SEG_SCRIPT != 0 {
            *x += i32::from(self.size(span.size) / 4)
        } else if sg.flags & SEG_COMPRESS != 0 {
            *x -= i32::from(self.size(span.size) / 2)
        }
    }
    fn emit_segment(
        &mut self,
        backend: &mut dyn Backend,
        sg: &Segment,
        span: Span,
        base_sz: u16,
        hy: bool,
        baseline: i16,
        x: &mut i32,
        active_link: &mut u8,
    ) -> bool {
        let small_cap = sg.flags & SEG_SMALL_CAP != 0;
        let sz = if small_cap { case::small_cap_px(base_sz) } else { base_sz };
        let style = (span.flags | sg.extra) & STYLE_MASK;
        let (mut width, mut prev, mut i) = (0, 0, sg.start);
        while i < sg.end {
            let (cp, _) = self.shaped_case(backend, sg.end, &mut i, sg.span, small_cap);
            if cp == 0x00ad && self.par_case {
                continue;
            }
            width += if !small_cap {
                Self::advance_plain(backend, cp, prev, base_sz, style)
            } else {
                Self::advance_sized(backend, cp, prev, prev, sz, base_sz, style, false)
            };
            prev = cp
        }
        let n = sg.end - sg.start;
        let need = n + n / 2 + usize::from(hy) + 4;
        let Some(at) = reserve_page_bytes(self.storage.page, &mut self.pool_len, need) else {
            return false;
        };
        let (mut len, mut i) = (0, sg.start);
        while i < sg.end {
            let (mut cp, _) = self.shaped_case(backend, sg.end, &mut i, sg.span, small_cap);
            if cp == 0xad {
                continue;
            }
            if sg.level & 1 != 0 {
                cp = mirror(cp)
            }
            len += encode(cp, &mut self.storage.page[at + len..at + need]);
        }
        if sg.level & 1 != 0 {
            reverse(&mut self.storage.page[at..at + len])
        }
        if hy {
            self.storage.page[at + len] = 45;
            len += 1;
            width += if self.par_case {
                self.hyphen_width(
                    backend,
                    Tail {
                        cp: prev,
                        span: sg.span as u16,
                        extra: sg.extra,
                        small_cap,
                    },
                )
            } else {
                i32::from(backend.advance(45, sz, style))
            };
        }
        let mut rb = baseline;
        if span.flags & SUP != 0 {
            rb = rb.wrapping_sub((u32::from(base_sz) * 33 / 100) as i16)
        } else if span.flags & SUB != 0 {
            rb = rb.wrapping_add((u32::from(base_sz) * 12 / 100) as i16)
        }
        self.storage.runs[self.run_count] = Run {
            text: Bytes {
                start: at as u32,
                len: len as u16,
            },
            x: *x as i16,
            baseline: rb,
            size: sz,
            flags: style,
        };
        self.run_count += 1;
        if !self.record_link(
            span.link,
            active_link,
            *x as i16,
            rb.wrapping_sub(base_sz as i16),
            width.clamp(1, i32::from(u16::MAX)) as u16,
            base_sz.wrapping_add(base_sz / 4),
        ) {
            return false;
        }
        *x += width;
        true
    }
    /// False only when the picture can't be placed at all (no member, no
    /// size); a full page or failed layout still counts as handled.
    fn place_image(&mut self, backend: &mut dyn Backend, src: &[u8]) -> bool {
        let mut path = [0; 512];
        let mut frag = [];
        let Some((n, _)) = self.image_source.resolve(src, &mut path, &mut frag) else {
            return false;
        };
        let Some((iw, ih)) = self.image_source.probe(&path[..n]) else {
            return false;
        };
        if iw == 0 || ih == 0 {
            return false;
        }
        let cw = i32::from(self.params.width)
            - i32::from(self.params.left)
            - i32::from(self.params.right);
        let ch = i32::from(self.params.height)
            - i32::from(self.params.top)
            - i32::from(self.params.bottom);
        let (mut w, mut h) = (iw as i32, ih as i32);
        if w > cw {
            h = h * cw / w;
            w = cw
        }
        if h > ch {
            w = w * ch / h;
            h = ch
        }
        w = w.max(1);
        h = h.max(1);
        let content = self.run_count > 0 || self.image_count > 0 || self.y > self.params.top;
        if (i32::from(self.y) + h > i32::from(self.params.height) - i32::from(self.params.bottom)
            || self.image_count >= self.storage.images.len())
            && content
        {
            self.emit_page(backend);
            if self.stopped {
                return true;
            }
        }
        let href = copy_page_bytes(self.storage.page, &mut self.pool_len, &path[..n]);
        let Some(href) = href else {
            self.failed = true;
            return true;
        };
        if self.image_count >= self.storage.images.len() {
            return true;
        }
        self.storage.images[self.image_count] = Image {
            href,
            width: w as u16,
            height: h as u16,
            x: (i32::from(self.params.left) + (cw - w) / 2) as i16,
            y: self.y,
        };
        self.image_count += 1;
        if self.run_count == 0 && self.image_count == 1 {
            self.page_chars = self.chars
        }
        self.y = (i32::from(self.y) + h + i32::from(self.params.base_size / 2)) as i16;
        true
    }
    /// Newt (#179): a non-decorative picture takes one chapter position, drawn
    /// or not, matching the index counter. One the reader can't draw is shown
    /// as an italic note at text size. Every note line sits at the picture's
    /// position, and a note runs onto at most one more page
    /// (`note_fills_its_page`), so every page has its own locator.
    fn take_picture_position(&mut self, backend: &mut dyn Backend, placed: bool, note: PictureNote<'_>) {
        if note == PictureNote::Decorative {
            return;
        }
        let at = self.chars;
        if !placed {
            self.picture_note_at = Some(at);
            // Set the style in place: a push at the element-stack limit only
            // counts overflow and would lose it. Line height follows the
            // paragraph, so it goes to text size too.
            let (element, paragraph) = (self.storage.elements[self.top], self.paragraph);
            self.storage.elements[self.top] = Element { size: 100, flags: element.flags | ITALIC, ..element };
            self.paragraph.size = 100;
            match note {
                PictureNote::Alt { text, cut } => {
                    self.text(backend, b"[Picture: ");
                    self.text(backend, text);
                    self.text(backend, if cut { b"...]" } else { b"]" });
                }
                _ => self.text(backend, b"[Picture]"),
            }
            self.flush(backend);
            (self.storage.elements[self.top], self.paragraph) = (element, paragraph);
            self.picture_note_at = None;
        }
        self.chars = at.wrapping_add(1);
    }
    fn emit_page(&mut self, backend: &mut dyn Backend) {
        let page = Page {
            bytes: &self.storage.page[..self.pool_len],
            runs: &self.storage.runs[..self.run_count],
            images: &self.storage.images[..self.image_count],
            links: &self.storage.links[..self.link_count],
            index: self.pages,
            char_start: self.page_chars,
        };
        self.pages = self.pages.wrapping_add(1);
        if !backend.page(page) {
            self.stopped = true
        }
        self.run_count = 0;
        self.image_count = 0;
        self.link_count = 0;
        self.pool_len = 0;
        self.y = self.params.top
    }
}
#[derive(Clone, Copy, Default)]
struct LineExtra {
    above: i16,
    below: i16,
}
impl LineExtra {
    fn total(self) -> i32 {
        i32::from(self.above) + i32::from(self.below)
    }
}
fn is_visible_cp(cp: u32) -> bool {
    !matches!(
        cp,
        0x09 | 0x0a | 0x0d | 0x20 | 0x85 | 0xa0 | 0xad | 0x34f | 0x61c
            | 0x115f..=0x1160
            | 0x17b4..=0x17b5
            | 0x180b..=0x180f
            | 0x2000..=0x200f
            | 0x2028..=0x202f
            | 0x205f..=0x206f
            | 0x3000
            | 0x3164
            | 0xfe00..=0xfe0f
            | 0xfeff
            | 0xffa0
            | 0xfff0..=0xfff8
            | 0x1bca0..=0x1bca3
            | 0x1d173..=0x1d17a
            | 0xe0000..=0xe0fff
    )
}
#[derive(Clone, Copy)]
struct Tail {
    cp: u32,
    span: u16,
    extra: u8,
    small_cap: bool,
}
#[derive(Clone, Copy)]
struct StepState {
    prev_kern: u32,
    prev_script: u32,
    prev_span: u16,
    prev_extra: u8,
    prev_level: u8,
    prev_small_cap: bool,
}
impl StepState {
    fn reset(span: usize) -> Self {
        Self {
            prev_kern: 0,
            prev_script: 0,
            prev_span: span as u16,
            prev_extra: 0,
            prev_level: 0,
            prev_small_cap: false,
        }
    }
    fn tail(&self) -> Tail {
        Tail {
            cp: self.prev_kern,
            span: self.prev_span,
            extra: self.prev_extra,
            small_cap: self.prev_small_cap,
        }
    }
}
enum LinePlacement {
    /// The measured continuation if the line crossed the segment cap.
    Placed(Option<Line>),
    NextPage,
    TooLarge,
}
fn is_only_shy(bytes: &[u8]) -> bool {
    bytes.len() % 2 == 0 && bytes.chunks_exact(2).all(|b| b == [0xc2, 0xad])
}
fn justification_gaps(line: Line, justify_hangul: bool) -> i32 {
    i32::from(line.spaces) + i32::from(line.cjk)
        + if justify_hangul { i32::from(line.hangul) } else { 0 }
}
fn reserve_page_bytes(page: &[u8], pool_len: &mut usize, n: usize) -> Option<usize> {
    if n > page.len().saturating_sub(*pool_len) {
        None
    } else {
        let at = *pool_len;
        *pool_len += n;
        Some(at)
    }
}
#[inline(never)]
fn copy_page_bytes(page: &mut [u8], pool_len: &mut usize, bytes: &[u8]) -> Option<Bytes> {
    let at = reserve_page_bytes(page, pool_len, bytes.len() + 1)?;
    let dst = page.get_mut(at..at + bytes.len() + 1)?;
    dst[..bytes.len()].copy_from_slice(bytes);
    dst[bytes.len()] = 0;
    Some(Bytes {
        start: at as u32,
        len: bytes.len() as u16,
    })
}
