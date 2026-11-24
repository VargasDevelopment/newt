// FreeInk cd6f5b8 ChapterLayout.cpp translation. MIT; reference closure retained.
#![forbid(unsafe_code)]
use super::{arabic, style, text::*, types::*};
mod links;
mod initialization;
mod segments;
use segments::{LineSegments, MAX_SEGMENTS, Segment};
use crate::{
    css::{self, CssDecl, CssStylesheetBuilder, TextAlign},
    hyphenator::Hyphenator,
};
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
    par_link_count: usize,
    current_link: u8,
    last_shy: bool,
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
    fn append(&mut self, backend: &mut dyn Backend, b: u8) {
        if self.len + 1 >= self.storage.text.len() {
            let p = self.paragraph;
            self.flush(backend);
            self.paragraph = p;
            self.paragraph.before = 0;
            self.continued = true
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
        self.storage.text[self.len] = b;
        self.len += 1
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
            if let Some(sheet) = self.params.stylesheet {
                let mut d = style::defaults(n);
                d.apply_over(&css::cascade_for(&sheet, Some(n), class, None));
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
            self.append(backend, b'\n')
        } else if n == b"hr" {
            self.flush(backend);
            let lh = self.line_height(backend, self.params.base_size);
            self.advance_y(lh)
        } else if matches!(n, b"img" | b"image") {
            if let Some(s) = src.or(href) {
                if !self.state().hidden {
                    self.flush(backend);
                    self.place_image(backend, s)
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
                    self.append(backend, b' ');
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
                    self.append(backend, run[0]);
                    let count = self.storage.text.len().saturating_sub(self.len + 1).min(run.len() - 1);
                    if count > 0 {
                        self.storage.text[self.len..self.len + count].copy_from_slice(&run[1..1 + count]);
                        self.len += count;
                    }
                    run = &run[1 + count..];
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
    fn shaped(&mut self, backend: &mut dyn Backend, end: usize, i: &mut usize, si: usize) -> u32 {
        let pos = *i;
        let mut cp = decode(&self.storage.text[..end], i);
        let mut form = self.storage.levels[pos] >> 3;
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
            let mut j = *i;
            let next = decode(&self.storage.text[..end], &mut j);
            let lig = backend
                .ligature(cp, next, self.storage.spans[si].flags);
            if lig == 0 {
                break;
            }
            cp = lig;
            *i = j
        }
        cp
    }
    fn advance(&mut self, backend: &mut dyn Backend, cp: u32, prev: u32, s: Span, extra: u8) -> i32 {
        Self::advance_sized(backend, cp, prev, self.size(s.size), s.flags | extra)
    }
    fn advance_sized(backend: &mut dyn Backend, cp: u32, prev: u32, sz: u16, flags: u8) -> i32 {
        if cp == 10 || cp == 0xad {
            return 0;
        }
        let mut a = i32::from(backend.advance(cp, sz, flags));
        if prev != 0 {
            a += i32::from(backend.kerning(prev, cp, sz, flags));
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
    fn margin(&self, s: Span) -> i32 {
        i32::from(self.params.base_size) * i32::from(s.margin) / 100
    }
    fn measure(&mut self, backend: &mut dyn Backend, from: usize, to: usize) -> i32 {
        let (mut width, mut prev, mut i) = (0, 0, from);
        let mut si = self.span_index(from);
        while i < to {
            while si + 1 < self.span_count && usize::from(self.storage.spans[si + 1].start) <= i {
                si += 1;
            }
            let s = self.storage.spans[si];
            let span_end = if si + 1 < self.span_count {
                usize::from(self.storage.spans[si + 1].start).min(to)
            } else {
                to
            };
            let size = self.size(s.size);
            if i == usize::from(s.start) && s.margin > 0 {
                width += self.margin(s);
                prev = 0;
            }
            while i < span_end {
                let start = i;
                let cp = self.shaped(backend, to, &mut i, si);
                width += Self::advance_sized(backend, cp, prev, size, s.flags | self.extra(start));
                prev = cp;
            }
        }
        width
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
        let prefix = self.measure(backend, line, word);
        let s = self.storage.spans[self.span_index(word)];
        let hw = i32::from(backend.advance(45, self.size(s.size), s.flags));
        for p in pos[..n].iter().rev() {
            let split = word + usize::from(*p);
            if split > line && prefix + self.measure(backend, word, split) + hw <= budget {
                return split;
            }
        }
        0
    }
    fn record(&mut self, backend: &mut dyn Backend, start: usize, mut end: usize, flags: u8) {
        while end > start && matches!(self.storage.text[end - 1], b' ' | b'\n') {
            end -= 1
        }
        self.storage.lines[self.line_count] = self.line_metrics(backend, start, end, flags);
        self.line_count += 1
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
        r.width = self.measure(backend, start, end);
        if flags & 2 != 0 {
            let s = self.storage.spans[self.span_index(if end > start { end - 1 } else { start })];
            r.width += i32::from(backend.advance(45, self.size(s.size), s.flags))
        }
        r
    }
    fn measure_lines(&mut self, backend: &mut dyn Backend) {
        self.line_count = 0;
        self.breaker.breaks(
            &self.storage.text[..self.len],
            self.params.language,
            &mut self.storage.breaks[..self.len],
        );
        let (rtl, has_arabic) = bidi_with_arabic(
            &self.storage.text[..self.len],
            &mut self.storage.levels[..self.len],
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
            focus(
                &self.storage.text[..self.len],
                &mut self.storage.levels[..self.len],
            )
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
        let (mut start, mut i, mut width, mut prev, mut last, mut word) = (0, 0, 0, 0, 0, 0);
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
            let cp = self.shaped(backend, self.len, &mut i, si);
            let margin = if at == usize::from(s.start) { self.margin(s) } else { 0 };
            let adv = margin + Self::advance_sized(backend, cp,
                if margin > 0 { 0 } else { prev }, span_size, s.flags | self.extra(at));
            if cp != 10 && width + adv > max && at > start {
                let hy = self.try_hyphen(backend, start, word.max(start), max);
                let (end, flags) = if hy > start && (last <= start || hy > last) {
                    (hy, 2)
                } else if last > start {
                    (last, if self.last_shy { 2 } else { 0 })
                } else {
                    (at, 0)
                };
                self.record(backend, start, end, flags);
                start = end;
                while start < self.len && self.storage.text[start] == 32 {
                    start += 1
                }
                i = start;
                width = 0;
                prev = 0;
                last = 0;
                word = start;
                continue;
            }
            width += adv;
            prev = cp;
            match self.storage.breaks[i - 1] {
                0 => {
                    self.record(backend, start, i, 1);
                    start = i;
                    width = 0;
                    prev = 0;
                    last = 0;
                    word = i
                }
                1 => {
                    last = i;
                    self.last_shy = cp == 0xad;
                    word = i;
                    while word < self.len && self.storage.text[word] == 32 {
                        word += 1
                    }
                }
                _ => {}
            }
        }
        if start < self.len && self.line_count < self.storage.lines.len() {
            self.record(backend, start, self.len, 1)
        }
        if self.line_count > 0 {
            self.storage.lines[self.line_count - 1].flags |= 1
        }
    }
    pub fn flush(&mut self, backend: &mut dyn Backend) {
        if self.len == 0 {
            self.continued = false;
            return;
        }
        let size = self.size(self.paragraph.size);
        let lh = self.line_height(backend, size);
        if lh <= 0 {
            self.failed = true;
            return;
        }
        self.advance_y(
            (i32::from(size)
                * i32::from(self.paragraph.before)
                * i32::from(self.params.paragraph_spacing)
                / 10000) as i16,
        );
        self.measure_lines(backend);
        let mut idx = 0;
        let mut continuation = None;
        while idx < self.line_count && !self.failed && !self.stopped {
            let px =
                i32::from(self.params.height) - i32::from(self.params.bottom) - i32::from(self.y);
            let avail = if px > 0 {
                (px / i32::from(lh)) as usize
            } else {
                0
            };
            let remain = self.line_count - idx;
            let content = self.run_count > 0 || self.y > self.params.top;
            if avail == 0 {
                if !content {
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
                self.emit_page(backend);
                continue;
            }
            let end = idx + take;
            // A segment break may use spare rows without consuming another measured line.
            let slots = if take == remain { avail } else { take };
            for _ in 0..slots {
                if idx == end { break; }
                let first = idx == 0 && continuation.is_none();
                let line = continuation.unwrap_or(self.storage.lines[idx]);
                match self.place_line(backend, line, size, lh, first) {
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
                self.emit_page(backend)
            }
        }
        self.advance_y(
            (i32::from(size)
                * i32::from(self.paragraph.after)
                * i32::from(self.params.paragraph_spacing)
                / 10000) as i16,
        );
        self.chars = self
            .chars
            .wrapping_add(count(&self.storage.text[..self.len]));
        self.reset_par();
        self.continued = false
    }
    fn refused_line(&self) -> LinePlacement {
        if self.run_count > 0 || self.image_count > 0 {
            LinePlacement::NextPage
        } else {
            LinePlacement::TooLarge
        }
    }
    fn place_line(&mut self, backend: &mut dyn Backend, mut r: Line, size: u16, lh: i16, first: bool) -> LinePlacement {
        if r.end == r.start {
            self.y = self.y.wrapping_add(lh);
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
        let mut gaps = justification_gaps(r);
        let justify =
            self.paragraph.align == TextAlign::Justify && r.flags & 1 == 0 && gaps > 0 && left > 0;
        let baseline = self.y.wrapping_add(backend.ascent(size));
        let mut segments = [Segment::default(); MAX_SEGMENTS];
        let LineSegments { segments: segs, count: n, next_start } =
            self.segment_line(backend, r, justify, &mut segments);
        if n == 0 {
            self.y = self.y.wrapping_add(lh);
            return LinePlacement::Placed(None);
        }
        if n > self.storage.runs.len() - self.run_count {
            return self.refused_line();
        }
        let rest = next_start.map(|start| {
            let (end, flags) = (r.end, r.flags);
            r = self.line_metrics(backend, r.start, segs[n - 1].end, flags & !2);
            left = max - r.width;
            gaps = justification_gaps(r);
            self.line_metrics(backend, start, end, flags | 1)
        });
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
        let (mut high, mut odd) = (0, 255);
        for s in 0..n {
            order[s] = s as u8;
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
                s = end
            }
        }
        if self.rtl {
            order[..n].reverse()
        }
        let before = (self.run_count, self.link_count, self.pool_len);
        for v in 0..n {
            let sg = segs[usize::from(order[v])];
            let span = self.storage.spans[sg.span];
            if v > 0 {
                if sg.gap || sg.space {
                    if sg.space {
                        x += i32::from(backend.advance(32, self.size(span.size), span.flags))
                    }
                    if justify {
                        x += per;
                        if remainder > 0 {
                            x += 1;
                            remainder -= 1
                        }
                    }
                } else if sg.script {
                    x += i32::from(self.size(span.size) / 4)
                } else if sg.compress {
                    x -= i32::from(self.size(span.size) / 2)
                }
            }
            if sg.start == usize::from(span.start) && span.margin > 0 {
                x += self.margin(span)
            }
            let hy = sg.end == r.end && r.flags & 2 != 0 && !self.rtl;
            if !self.emit_segment(backend, sg, hy, baseline, &mut x) {
                // Unpublished runs, links and bytes are discarded together.
                // The caller retries this whole line at the top of the next page.
                (self.run_count, self.link_count, self.pool_len) = before;
                return self.refused_line();
            }
        }
        if before.0 == 0 {
            self.page_chars = self.chars.wrapping_add(count(&self.storage.text[..r.start]));
        }
        self.y = self.y.wrapping_add(lh);
        LinePlacement::Placed(rest)
    }
    fn reserve(&mut self, n: usize) -> Option<usize> {
        if n > self.storage.page.len().saturating_sub(self.pool_len) {
            None
        } else {
            let at = self.pool_len;
            self.pool_len += n;
            Some(at)
        }
    }
    fn copy_bytes(&mut self, bytes: &[u8]) -> Option<Bytes> {
        let at = self.reserve(bytes.len() + 1)?;
        self.storage.page[at..at + bytes.len()].copy_from_slice(bytes);
        self.storage.page[at + bytes.len()] = 0;
        Some(Bytes {
            start: at as u32,
            len: bytes.len() as u16,
        })
    }
    fn emit_segment(
        &mut self, backend: &mut dyn Backend,
        sg: Segment,
        hy: bool,
        baseline: i16,
        x: &mut i32,
    ) -> bool {
        let span = self.storage.spans[sg.span];
        let (mut width, mut prev, mut i) = (0, 0, sg.start);
        while i < sg.end {
            let cp = self.shaped(backend, sg.end, &mut i, sg.span);
            width += self.advance(backend, cp, prev, span, sg.extra);
            prev = cp
        }
        let flags = span.flags | sg.extra;
        let n = sg.end - sg.start;
        let need = n + n / 2 + usize::from(hy) + 4;
        let Some(at) = self.reserve(need) else {
            return false;
        };
        let (mut len, mut i) = (0, sg.start);
        while i < sg.end {
            let mut cp = self.shaped(backend, sg.end, &mut i, sg.span);
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
        let sz = self.size(span.size);
        if hy {
            self.storage.page[at + len] = 45;
            len += 1;
            width += i32::from(backend.advance(45, sz, flags))
        }
        let mut rb = baseline;
        if span.flags & SUP != 0 {
            rb = rb.wrapping_sub((u32::from(sz) * 33 / 100) as i16)
        } else if span.flags & SUB != 0 {
            rb = rb.wrapping_add((u32::from(sz) * 12 / 100) as i16)
        }
        self.storage.runs[self.run_count] = Run {
            text: Bytes {
                start: at as u32,
                len: len as u16,
            },
            x: *x as i16,
            baseline: rb,
            size: sz,
            flags,
        };
        self.run_count += 1;
        if span.link != 0 && self.link_count < self.storage.links.len() {
            let p = self.storage.paragraph_links[usize::from(span.link - 1)];
            let target = self.copy_bytes(&p.path[..p.path_len]);
            let fragment = self.copy_bytes(&p.fragment[..p.fragment_len]);
            if let (Some(target), Some(fragment)) = (target, fragment) {
                self.storage.links[self.link_count] = Link {
                    target,
                    fragment,
                    x: *x as i16,
                    y: rb.wrapping_sub(sz as i16),
                    width: width.max(1) as u16,
                    height: sz.wrapping_add(sz / 4),
                };
                self.link_count += 1
            }
        }
        *x += width;
        true
    }
    fn place_image(&mut self, backend: &mut dyn Backend, src: &[u8]) {
        let mut path = [0; 512];
        let mut frag = [];
        let Some((n, _)) = self.image_source.resolve(src, &mut path, &mut frag) else {
            return;
        };
        let Some((iw, ih)) = self.image_source.probe(&path[..n]) else {
            return;
        };
        if iw == 0 || ih == 0 {
            return;
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
                return;
            }
        }
        let href = self.copy_bytes(&path[..n]);
        let Some(href) = href else {
            self.failed = true;
            return;
        };
        if self.image_count >= self.storage.images.len() {
            return;
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
        self.y = (i32::from(self.y) + h + i32::from(self.params.base_size / 2)) as i16
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
enum LinePlacement {
    /// The measured continuation if the line crossed the segment cap.
    Placed(Option<Line>),
    NextPage,
    TooLarge,
}
fn justification_gaps(line: Line) -> i32 {
    i32::from(line.spaces) + i32::from(line.cjk)
        + if line.spaces == 0 { i32::from(line.hangul) } else { 0 }
}
