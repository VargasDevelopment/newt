//! Canonical layout-engine state capture for resume points. MIT.
#![forbid(unsafe_code)]
use super::*;
use crate::resume_point::{DEPTH_MAX, PointOut, SectionIn, put_u16s, put_u32s};

fn align_from(v: u8) -> Option<TextAlign> {
    match v {
        0 => Some(TextAlign::Inherit),
        1 => Some(TextAlign::Left),
        2 => Some(TextAlign::Right),
        3 => Some(TextAlign::Center),
        4 => Some(TextAlign::Justify),
        _ => None,
    }
}

fn bool_from(v: u8) -> Option<bool> {
    match v {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn bytes_inside(b: Bytes, pool_len: usize) -> bool {
    (b.start as usize)
        .checked_add(usize::from(b.len))
        .is_some_and(|end| end <= pool_len)
}

/// Mark `[b.start .. b.start + b.len)` inside the window `[base .. base + bits)`
/// in `live_bits`. Returns false if the range escapes `pool_len`.
#[inline(never)]
fn mark_range(live_bits: &mut [u8], base: usize, bits: usize, pool_len: usize, b: Bytes) -> bool {
    let start = b.start as usize;
    let Some(end) = start.checked_add(usize::from(b.len)) else {
        return false;
    };
    if end > pool_len {
        return false;
    }
    let lo = start.max(base);
    let hi = end.min(base.saturating_add(bits));
    for idx in lo..hi {
        let rel = idx - base;
        if let Some(slot) = live_bits.get_mut(rel >> 3) {
            *slot |= 1u8 << (rel & 7);
        }
    }
    true
}

impl EngineState<'_> {
    /// Emit the engine's layout carry in canonical wire form.
    ///
    /// `live_bits` is a caller-lent scratch buffer (896 B covers the 7,168 B
    /// page pool in one strictly linear pass) used to zero dead rewound bytes
    /// below `pool_len` so identical layouts emit identical point bytes.
    #[inline(never)]
    pub fn save_resume(&self, live_bits: &mut [u8], out: &mut dyn PointOut) -> bool {
        let s = &self.storage;
        if self.stopped
            || self.failed
            || self.picture_note_at.is_some()
            || live_bits.is_empty()
            || self.line_count > s.lines.len()
            || self.chapter_css.finish().rule_count() != 0
            || (self.len
                | self.span_count
                | self.line_count
                | self.top
                | self.style_len
                | self.run_count
                | self.image_count
                | self.link_count
                | self.pool_len)
                > u16::MAX as usize
            || self.par_link_count > u8::MAX as usize
        {
            return false;
        }
        let (
            Some(text),
            Some(spans),
            Some(elements),
            Some(runs),
            Some(images),
            Some(links),
            Some(page),
            Some(style_text),
            Some(par_links),
        ) = (
            s.text.get(..self.len),
            s.spans.get(..self.span_count),
            self.top.checked_add(1).and_then(|n| s.elements.get(..n)),
            s.runs.get(..self.run_count),
            s.images.get(..self.image_count),
            s.links.get(..self.link_count),
            s.page.get(..self.pool_len),
            s.style_text.get(..self.style_len),
            s.paragraph_links.get(..self.par_link_count),
        )
        else {
            return false;
        };
        if core::str::from_utf8(text).is_err() {
            return false;
        }
        let p = self.paragraph;
        let u32s = [
            self.pages,
            self.chars,
            self.overflow,
            self.depth as u32,
            self.suppress as u32,
            self.in_style as u32,
            self.pending_margin,
            self.page_chars,
        ];
        let u16s = [
            self.len as u16,
            self.span_count as u16,
            self.line_count as u16,
            self.top as u16,
            self.style_len as u16,
            p.size,
            p.indent as u16,
            p.text_indent as u16,
            p.before,
            p.after,
            self.run_count as u16,
            self.image_count as u16,
            self.link_count as u16,
            self.pool_len as u16,
            self.y as u16,
            self.carried_margin_px as u16,
        ];
        let u8s = [
            u8::from(self.in_body),
            u8::from(self.style_ok),
            u8::from(self.pending_space),
            u8::from(self.continued),
            p.flags,
            p.align as u8,
            u8::from(self.rtl),
            self.par_link_count as u8,
            self.current_link,
            u8::from(self.last_shy),
            self.word_state,
            u8::from(self.par_case),
            self.par_dir,
        ];
        if !out.put(b"ENG1")
            || !put_u32s(out, &u32s)
            || !put_u16s(out, &u16s)
            || !out.put(&u8s)
            || !out.put(text)
        {
            return false;
        }
        for sp in spans {
            if !put_u16s(out, &[sp.start, sp.size, sp.margin]) || !out.put(&[sp.flags, sp.link]) {
                return false;
            }
        }
        for e in elements {
            if !put_u16s(out, &[e.size, e.indent as u16])
                || !out.put(&[e.flags, e.align as u8, u8::from(e.hidden)])
            {
                return false;
            }
        }
        for r in runs {
            if !put_u32s(out, &[r.text.start])
                || !put_u16s(out, &[r.text.len, r.x as u16, r.baseline as u16, r.size])
                || !out.put(&[r.flags])
            {
                return false;
            }
        }
        for im in images {
            if !put_u32s(out, &[im.href.start])
                || !put_u16s(out, &[im.href.len, im.x as u16, im.y as u16, im.width, im.height])
            {
                return false;
            }
        }
        for l in links {
            if !put_u32s(out, &[l.target.start, l.fragment.start])
                || !put_u16s(
                    out,
                    &[l.target.len, l.fragment.len, l.x as u16, l.y as u16, l.width, l.height],
                )
            {
                return false;
            }
        }
        // Canonical page pool: linear live-byte bitset zeros dead rewound bytes.
        let bits = live_bits.len().saturating_mul(8);
        let mut base = 0;
        let mut chunk = [0u8; 64];
        for page_win in page.chunks(bits) {
            let window = page_win.len();
            live_bits.fill(0);
            for r in runs {
                if !mark_range(live_bits, base, window, self.pool_len, r.text) {
                    return false;
                }
            }
            for im in images {
                if !mark_range(live_bits, base, window, self.pool_len, im.href) {
                    return false;
                }
            }
            for l in links {
                if !mark_range(live_bits, base, window, self.pool_len, l.target)
                    || !mark_range(live_bits, base, window, self.pool_len, l.fragment)
                {
                    return false;
                }
            }
            let mut off = 0;
            for sub in page_win.chunks(chunk.len()) {
                for (j, (dst, &src)) in chunk.iter_mut().zip(sub).enumerate() {
                    let rel = off + j;
                    let live = live_bits.get(rel >> 3).is_some_and(|&m| (m >> (rel & 7)) & 1 != 0);
                    *dst = if live { src } else { 0 };
                }
                if !out.put(&chunk[..sub.len()]) {
                    return false;
                }
                off += sub.len();
            }
            base += window;
        }
        if !out.put(style_text) {
            return false;
        }
        for pl in par_links {
            let (Some(path), Some(frag)) =
                (pl.path.get(..pl.path_len), pl.fragment.get(..pl.fragment_len))
            else {
                return false;
            };
            let (Ok(plen), Ok(flen)) = (u8::try_from(path.len()), u8::try_from(frag.len())) else {
                return false;
            };
            if !out.put(&[plen]) || !out.put(path) || !out.put(&[flen]) || !out.put(frag) {
                return false;
            }
        }
        true
    }

    /// Lower bound on open XML elements required by this engine's `depth`,
    /// `suppress` and `in_body` state, checked against Section 5 (`Names`).
    pub fn open_stack_bound(&self) -> usize {
        (self.depth.max(0) as usize)
            .saturating_add(self.suppress.max(0) as usize)
            .saturating_add(usize::from(self.in_body))
    }

    /// Classify one open element name from Section 5 (`Names`) using the exact
    /// same rules as `start_tag`, tallying `(depth, suppress, in_style, body)`.
    pub fn count_open_tag(
        name: &[u8],
        depth: &mut usize,
        suppress: &mut usize,
        in_style: &mut usize,
        body: &mut usize,
    ) {
        let n = local(name);
        if style::suppressed(n) {
            *suppress += 1;
            if n == b"style" {
                *in_style += 1;
            }
        } else if n == b"body" {
            *body += 1;
        } else {
            *depth += 1;
        }
    }

    /// Verify that restored `depth`, `suppress`, `in_style` and `in_body` match
    /// the exact open XML element stack decoded from Section 5 (`Names`).
    pub fn matches_open_names(
        &self,
        names_depth: usize,
        names_suppress: usize,
        names_style: usize,
        names_body: usize,
    ) -> bool {
        self.depth as usize == names_depth
            && self.suppress as usize == names_suppress
            && self.in_style as usize == names_style
            && match names_body {
                0 => !self.in_body,
                1 => self.in_body,
                _ => true,
            }
    }

    /// Restore layout carry from a verified, section-bounded `ENG1` stream into
    /// a freshly constructed engine. Validates every count, enum, boolean,
    /// UTF-8 paragraph/run slice, strictly increasing char-boundary span table,
    /// link index, pool slice and depth cap before returning `true`. Resets
    /// cleanly on any failure so partial state never persists.
    #[inline(never)]
    pub fn load_resume(
        &mut self,
        sec: &mut SectionIn<'_, '_>,
        expected_pages: u32,
        max_chars: u32,
    ) -> bool {
        if self.load_resume_inner(sec, expected_pages, max_chars) {
            true
        } else {
            self.reset();
            false
        }
    }

    #[inline(never)]
    fn load_resume_inner(
        &mut self,
        sec: &mut SectionIn<'_, '_>,
        expected_pages: u32,
        max_chars: u32,
    ) -> bool {
        let mut magic = [0u8; 4];
        let mut u32s = [0u32; 8];
        let mut u16s = [0u16; 16];
        let mut u8s = [0u8; 13];
        if !sec.read_exact(&mut magic)
            || &magic != b"ENG1"
            || !sec.get_u32s(&mut u32s)
            || !sec.get_u16s(&mut u16s)
            || !sec.read_exact(&mut u8s)
        {
            return false;
        }
        let pages = u32s[0];
        let chars = u32s[1];
        let overflow = u32s[2];
        let depth = u32s[3] as i32;
        let suppress = u32s[4] as i32;
        let in_style = u32s[5] as i32;
        let pending_margin = u32s[6];
        let page_chars = u32s[7];

        let len = usize::from(u16s[0]);
        let span_count = usize::from(u16s[1]);
        let line_count = usize::from(u16s[2]);
        let top = usize::from(u16s[3]);
        let style_len = usize::from(u16s[4]);
        let p_size = u16s[5];
        let p_indent = u16s[6] as i16;
        let p_text_indent = u16s[7] as i16;
        let run_count = usize::from(u16s[10]);
        let image_count = usize::from(u16s[11]);
        let link_count = usize::from(u16s[12]);
        let pool_len = usize::from(u16s[13]);
        let y = u16s[14] as i16;
        let carried_margin_px = u16s[15] as i16;

        let (
            Some(in_body),
            Some(style_ok),
            Some(pending_space),
            Some(continued),
            Some(p_align),
            Some(rtl),
            Some(last_shy),
            Some(par_case),
        ) = (
            bool_from(u8s[0]),
            bool_from(u8s[1]),
            bool_from(u8s[2]),
            bool_from(u8s[3]),
            align_from(u8s[5]),
            bool_from(u8s[6]),
            bool_from(u8s[9]),
            bool_from(u8s[11]),
        )
        else {
            return false;
        };
        let p_flags = u8s[4];
        let par_link_count = usize::from(u8s[7]);
        let current_link = u8s[8];
        let word_state = u8s[10];
        let par_dir = u8s[12];

        let s = &self.storage;
        let max_depth = DEPTH_MAX as i32;
        // Strict bounds before reading any variable-length payloads:
        // `len < text.len()` ensures `append`'s `free.len() - 1` cannot underflow.
        if pages != expected_pages
            || chars > max_chars
            || page_chars > chars
            || depth < 0
            || depth > max_depth
            || suppress < 0
            || suppress > max_depth
            || in_style < 0
            || in_style > suppress
            || pending_margin > u32::from(u16::MAX)
            || (depth as usize)
                .saturating_add(suppress as usize)
                .saturating_add(usize::from(in_body))
                > DEPTH_MAX
            || overflow > (DEPTH_MAX as u32) + 1
            || (overflow > 0 && top + 1 != s.elements.len())
            || len >= s.text.len()
            || (len == 0) != (span_count == 0)
            || (len == 0 && pending_space)
            || span_count > s.spans.len()
            || line_count > s.lines.len()
            || top >= s.elements.len()
            || style_len >= s.style_text.len()
            || (in_style == 0 && style_len > 0)
            || !(30..=250).contains(&p_size)
            || (p_flags & (SUP | SUB)) == (SUP | SUB)
            || p_indent < 0
            || p_text_indent < 0
            || run_count > s.runs.len()
            || image_count > s.images.len()
            || link_count > s.links.len()
            || (link_count > 0 && run_count == 0)
            || (run_count == 0 && image_count == 0 && link_count == 0 && pool_len > 0)
            || pool_len > s.page.len()
            || par_link_count > s.paragraph_links.len()
            || usize::from(current_link) > par_link_count
            || word_state > 2
            || (par_dir & !0x3f) != 0
            || (par_dir & 0x03) > 2
            || ((par_dir >> 3) & 0x03) > 2
            || y < self.params.top
            || y > i16::MAX / 2
            || carried_margin_px < 0
        {
            return false;
        }

        self.stopped = false;
        self.failed = false;
        self.picture_note_at = None;
        self.pages = pages;
        self.chars = chars;
        self.len = len;
        self.span_count = span_count;
        self.line_count = line_count;
        self.top = top;
        self.overflow = overflow;
        self.depth = depth;
        self.suppress = suppress;
        self.in_body = in_body;
        self.in_style = in_style;
        self.style_len = style_len;
        self.style_ok = style_ok;
        self.pending_margin = pending_margin;
        self.pending_space = pending_space;
        self.continued = continued;
        self.paragraph = Paragraph {
            size: p_size,
            flags: p_flags,
            align: p_align,
            indent: p_indent,
            text_indent: p_text_indent,
            before: u16s[8],
            after: u16s[9],
        };
        self.rtl = rtl;
        self.run_count = run_count;
        self.image_count = image_count;
        self.link_count = link_count;
        self.pool_len = pool_len;
        self.y = y;
        self.page_chars = page_chars;
        self.par_link_count = par_link_count;
        self.current_link = current_link;
        self.last_shy = last_shy;
        self.word_state = word_state;
        self.par_case = par_case;
        self.par_dir = par_dir;
        self.carried_margin_px = carried_margin_px;

        let s = &mut self.storage;
        if !sec.read_exact(&mut s.text[..len]) {
            return false;
        }
        // Valid UTF-8 guarantees `emit_segment`'s `n + n/2 + 4` reservation
        // cannot be overrun by lone continuation bytes expanding 1 B -> 2 B.
        if core::str::from_utf8(&s.text[..len]).is_err() {
            return false;
        }
        let mut prev_start = 0u16;
        for i in 0..span_count {
            let mut v = [0u16; 3];
            let mut b = [0u8; 2];
            if !sec.get_u16s(&mut v) || !sec.read_exact(&mut b) {
                return false;
            }
            let st = usize::from(v[0]);
            if (i == 0 && v[0] != 0)
                || (i > 0 && v[0] <= prev_start)
                || st > len
                || (st < len && (s.text[st] & 0xc0) == 0x80)
                || !(30..=250).contains(&v[1])
                || (b[0] & (SUP | SUB)) == (SUP | SUB)
                || usize::from(b[1]) > par_link_count
            {
                return false;
            }
            prev_start = v[0];
            s.spans[i] = Span {
                start: v[0],
                flags: b[0],
                size: v[1],
                margin: v[2],
                link: b[1],
            };
        }
        for i in 0..=top {
            let mut v = [0u16; 2];
            let mut b = [0u8; 3];
            if !sec.get_u16s(&mut v) || !sec.read_exact(&mut b) {
                return false;
            }
            let indent = v[1] as i16;
            let (Some(align), Some(hidden)) = (align_from(b[1]), bool_from(b[2])) else {
                return false;
            };
            if !(30..=250).contains(&v[0])
                || (b[0] & (SUP | SUB)) == (SUP | SUB)
                || indent < 0
            {
                return false;
            }
            s.elements[i] = Element {
                size: v[0],
                flags: b[0],
                align,
                indent,
                hidden,
            };
        }
        for i in 0..run_count {
            let mut start = [0u32; 1];
            let mut v = [0u16; 4];
            let mut flags = [0u8; 1];
            if !sec.get_u32s(&mut start) || !sec.get_u16s(&mut v) || !sec.read_exact(&mut flags) {
                return false;
            }
            let text = Bytes { start: start[0], len: v[0] };
            if !bytes_inside(text, pool_len)
                || v[3] == 0
                || (flags[0] & !STYLE_MASK) != 0
                || (flags[0] & (SUP | SUB)) == (SUP | SUB)
            {
                return false;
            }
            s.runs[i] = Run {
                text,
                x: v[1] as i16,
                baseline: v[2] as i16,
                size: v[3],
                flags: flags[0],
            };
        }
        for i in 0..image_count {
            let mut start = [0u32; 1];
            let mut v = [0u16; 5];
            if !sec.get_u32s(&mut start) || !sec.get_u16s(&mut v) {
                return false;
            }
            let href = Bytes { start: start[0], len: v[0] };
            if !bytes_inside(href, pool_len) {
                return false;
            }
            s.images[i] = Image {
                href,
                x: v[1] as i16,
                y: v[2] as i16,
                width: v[3],
                height: v[4],
            };
        }
        for i in 0..link_count {
            let mut starts = [0u32; 2];
            let mut v = [0u16; 6];
            if !sec.get_u32s(&mut starts) || !sec.get_u16s(&mut v) {
                return false;
            }
            let target = Bytes { start: starts[0], len: v[0] };
            let fragment = Bytes { start: starts[1], len: v[1] };
            if !bytes_inside(target, pool_len) || !bytes_inside(fragment, pool_len) {
                return false;
            }
            s.links[i] = Link {
                target,
                fragment,
                x: v[2] as i16,
                y: v[3] as i16,
                width: v[4],
                height: v[5],
            };
        }
        s.page[pool_len..].fill(0);
        if !sec.read_exact(&mut s.page[..pool_len])
            || !sec.read_exact(&mut s.style_text[..style_len])
        {
            return false;
        }
        for r in &s.runs[..run_count] {
            if core::str::from_utf8(r.text.get(&s.page[..pool_len])).is_err() {
                return false;
            }
        }
        for i in 0..par_link_count {
            let pl = &mut s.paragraph_links[i];
            *pl = ParLink::default();
            let mut plen = [0u8; 1];
            if !sec.read_exact(&mut plen) {
                return false;
            }
            let pn = usize::from(plen[0]);
            if pn >= pl.path.len() || !sec.read_exact(&mut pl.path[..pn]) {
                return false;
            }
            pl.path_len = pn;
            let mut flen = [0u8; 1];
            if !sec.read_exact(&mut flen) {
                return false;
            }
            let fn_ = usize::from(flen[0]);
            if fn_ >= pl.fragment.len() || !sec.read_exact(&mut pl.fragment[..fn_]) {
                return false;
            }
            pl.fragment_len = fn_;
        }
        sec.finish()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::css::{CssRule, CssStylesheetBuilder};
    use crate::resume_point::PointOut;
    use std::vec::Vec;

    struct DummyBreak;
    impl LineBreak for DummyBreak {
        fn breaks(&mut self, _: &[u8], _: Option<&core::ffi::CStr>, out: &mut [u8]) {
            out.fill(0);
        }
    }
    struct DummyImages;
    impl Images for DummyImages {
        fn resolve(&mut self, _: &[u8], _: &mut [u8], _: &mut [u8]) -> Option<(usize, usize)> {
            None
        }
        fn probe(&mut self, _: &[u8]) -> Option<(u32, u32)> {
            None
        }
    }
    struct VecOut(Vec<u8>);
    impl PointOut for VecOut {
        fn put(&mut self, bytes: &[u8]) -> bool {
            self.0.extend_from_slice(bytes);
            true
        }
    }

    #[test]
    fn save_resume_zeros_dead_pool_bytes_and_declines_embedded_css_or_stopped() {
        let mut text = [0u8; 64];
        let mut breaks = [0u8; 64];
        let mut levels = [0u8; 64];
        let mut spans = [Span::default(); 4];
        let mut lines = [Line::default(); 4];
        let mut elements = [Element::default(); 4];
        let mut runs = [Run::default(); 4];
        let mut images = [Image::default(); 2];
        let mut links = [Link::default(); 2];
        let mut page = [0u8; 128];
        let mut style_text = [0u8; 32];
        let mut par_links = [ParLink::default(); 2];
        let mut breaker = DummyBreak;
        let mut img_src = DummyImages;
        let mut css_rules = [CssRule::default(); crate::css::MAX_RULES];

        let build = |poison: u8,
                     text: &mut [u8; 64],
                     breaks: &mut [u8; 64],
                     levels: &mut [u8; 64],
                     spans: &mut [Span; 4],
                     lines: &mut [Line; 4],
                     elements: &mut [Element; 4],
                     runs: &mut [Run; 4],
                     images: &mut [Image; 2],
                     links: &mut [Link; 2],
                     page: &mut [u8; 128],
                     style_text: &mut [u8; 32],
                     par_links: &mut [ParLink; 2],
                     breaker: &mut DummyBreak,
                     img_src: &mut DummyImages,
                     live_bits: &mut [u8]| {
            page.fill(poison);
            page[2..7].copy_from_slice(b"hello");
            page[10..17].copy_from_slice(b"img.png");
            page[20..26].copy_from_slice(b"a.html");
            page[30..34].copy_from_slice(b"sec1");
            runs[0] = Run { text: Bytes { start: 2, len: 5 }, x: 8, baseline: 16, size: 16, flags: 1 };
            images[0] = Image {
                href: Bytes { start: 10, len: 7 },
                x: 4,
                y: 20,
                width: 64,
                height: 48,
            };
            links[0] = Link {
                target: Bytes { start: 20, len: 6 },
                fragment: Bytes { start: 30, len: 4 },
                x: 8,
                y: 4,
                width: 40,
                height: 16,
            };
            let storage = Storage {
                text,
                breaks,
                levels,
                spans,
                lines,
                elements,
                runs,
                images,
                links,
                page,
                style_text,
                paragraph_links: par_links,
            };
            let mut st = EngineState::new(
                Params::default(),
                storage,
                breaker,
                img_src,
                None,
                CssStylesheetBuilder::default(),
            )
            .unwrap();
            st.run_count = 1;
            st.image_count = 1;
            st.link_count = 1;
            st.pool_len = 64;
            let mut out = VecOut(Vec::new());
            assert!(st.save_resume(live_bits, &mut out));
            out.0
        };

        let mut full_bits = [0u8; 896];
        let a = build(
            0xaa, &mut text, &mut breaks, &mut levels, &mut spans, &mut lines, &mut elements,
            &mut runs, &mut images, &mut links, &mut page, &mut style_text, &mut par_links,
            &mut breaker, &mut img_src, &mut full_bits,
        );
        let b = build(
            0x55, &mut text, &mut breaks, &mut levels, &mut spans, &mut lines, &mut elements,
            &mut runs, &mut images, &mut links, &mut page, &mut style_text, &mut par_links,
            &mut breaker, &mut img_src, &mut full_bits,
        );
        assert_eq!(a, b, "dead bytes below pool_len must zero to identical canonical bytes");
        // Tiny 2-byte live_bits exercises multi-window chunking and must emit identical bytes.
        let mut tiny_bits = [0u8; 2];
        let c = build(
            0xaa, &mut text, &mut breaks, &mut levels, &mut spans, &mut lines, &mut elements,
            &mut runs, &mut images, &mut links, &mut page, &mut style_text, &mut par_links,
            &mut breaker, &mut img_src, &mut tiny_bits,
        );
        assert_eq!(a, c);
        assert_eq!(&a[..4], b"ENG1");
        // Exact pool layout: 81 B header + 0 text + 0 spans + 1 element (7 B)
        // + 1 run (13 B) + 1 image (14 B) + 1 link (20 B) = 135 B; pool = 64 B.
        const POOL_OFF: usize = 81 + 7 + 13 + 14 + 20;
        let mut expected_pool = [0u8; 64];
        expected_pool[2..7].copy_from_slice(b"hello");
        expected_pool[10..17].copy_from_slice(b"img.png");
        expected_pool[20..26].copy_from_slice(b"a.html");
        expected_pool[30..34].copy_from_slice(b"sec1");
        assert_eq!(a.len(), POOL_OFF + 64);
        assert_eq!(
            &a[POOL_OFF..POOL_OFF + 64],
            &expected_pool,
            "canonical pool must preserve all four live slice kinds and zero every gap"
        );

        // Declines on stopped, failed, empty live_bits, and embedded chapter_css rules.
        let mut css_builder = CssStylesheetBuilder::default();
        assert!(css_builder.begin(&mut css_rules));
        css_builder.add_text(b"p { font-weight: bold; }");
        assert!(css_builder.finish().rule_count() > 0);
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
            style_text: &mut style_text,
            paragraph_links: &mut par_links,
        };
        let mut st = EngineState::new(
            Params::default(),
            storage,
            &mut breaker,
            &mut img_src,
            None,
            css_builder,
        )
        .unwrap();
        assert!(!st.save_resume(&mut full_bits, &mut VecOut(Vec::new())));
        st.chapter_css = CssStylesheetBuilder::default();
        assert!(!st.save_resume(&mut [], &mut VecOut(Vec::new())));
        st.stopped = true;
        assert!(!st.save_resume(&mut full_bits, &mut VecOut(Vec::new())));
        st.stopped = false;
        st.failed = true;
        assert!(!st.save_resume(&mut full_bits, &mut VecOut(Vec::new())));
        st.failed = false;
        st.picture_note_at = Some(7);
        assert!(!st.save_resume(&mut full_bits, &mut VecOut(Vec::new())));
    }

    #[test]
    fn load_resume_roundtrips_and_rejects_every_adversarial_field_without_panic() {
        use crate::page_envelope;
        use crate::resume_point::PointCursor;

        let params = Params::default();
        let make_valid_bytes = || -> Vec<u8> {
            let mut text = [0u8; 64];
            let mut breaks = [0u8; 64];
            let mut levels = [0u8; 64];
            let mut spans = [Span::default(); 4];
            let mut lines = [Line::default(); 4];
            let mut elements = [Element::default(); 4];
            let mut runs = [Run::default(); 4];
            let mut images = [Image::default(); 2];
            let mut links = [Link::default(); 2];
            let mut page = [0xdd; 128];
            let mut style_text = [0u8; 32];
            let mut par_links = [ParLink::default(); 2];
            let mut breaker = DummyBreak;
            let mut img_src = DummyImages;

            text[..11].copy_from_slice(b"hello world");
            spans[0] = Span { start: 0, flags: 1, size: 100, margin: 2, link: 0 };
            spans[1] = Span { start: 6, flags: 3, size: 120, margin: 0, link: 1 };
            elements[0] = Element { size: 100, flags: 0, align: TextAlign::Left, indent: 0, hidden: false };
            elements[1] = Element { size: 110, flags: 2, align: TextAlign::Justify, indent: 12, hidden: false };
            elements[2] = Element { size: 120, flags: 1, align: TextAlign::Left, indent: 0, hidden: false };
            elements[3] = Element { size: 130, flags: 0, align: TextAlign::Right, indent: 4, hidden: true };
            page[2..7].copy_from_slice(b"hello");
            page[10..17].copy_from_slice(b"img.png");
            page[20..26].copy_from_slice(b"a.html");
            page[30..34].copy_from_slice(b"sec1");
            runs[0] = Run { text: Bytes { start: 2, len: 5 }, x: 8, baseline: 16, size: 16, flags: 1 };
            images[0] = Image { href: Bytes { start: 10, len: 7 }, x: 4, y: 20, width: 64, height: 48 };
            links[0] = Link {
                target: Bytes { start: 20, len: 6 },
                fragment: Bytes { start: 30, len: 4 },
                x: 8,
                y: 4,
                width: 40,
                height: 16,
            };
            style_text[..5].copy_from_slice(b"p{a;}");
            par_links[0].path[..6].copy_from_slice(b"b.html");
            par_links[0].path_len = 6;
            par_links[0].fragment[..2].copy_from_slice(b"f1");
            par_links[0].fragment_len = 2;

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
                style_text: &mut style_text,
                paragraph_links: &mut par_links,
            };
            let mut st = EngineState::new(
                params,
                storage,
                &mut breaker,
                &mut img_src,
                None,
                CssStylesheetBuilder::default(),
            )
            .unwrap();
            st.pages = 126;
            st.chars = 42_000;
            st.page_chars = 41_900;
            st.overflow = 2;
            st.depth = 4;
            st.suppress = 1;
            st.in_style = 1;
            st.pending_margin = 14;
            st.len = 11;
            st.span_count = 2;
            st.line_count = 1;
            st.top = 3;
            st.style_len = 5;
            st.run_count = 1;
            st.image_count = 1;
            st.link_count = 1;
            st.pool_len = 64;
            st.y = params.top + 32;
            st.carried_margin_px = 6;
            st.in_body = true;
            st.style_ok = true;
            st.pending_space = true;
            st.continued = false;
            st.paragraph = Paragraph {
                size: 120,
                flags: 5,
                align: TextAlign::Center,
                indent: 8,
                text_indent: 4,
                before: 10,
                after: 12,
            };
            st.rtl = false;
            st.par_link_count = 1;
            st.current_link = 1;
            st.last_shy = true;
            st.word_state = 2;
            st.par_case = true;
            st.par_dir = 0x25; // DIR_LTR | DIR_PREV_SET | DIR_FOCUS_WORD
            let mut bits = [0u8; 896];
            let mut out = VecOut(Vec::new());
            assert!(st.save_resume(&mut bits, &mut out));
            out.0
        };

        let try_load = |payload: &[u8], expected_pages: u32, max_chars: u32| -> Option<Vec<u8>> {
            let mut framed = payload.to_vec();
            let crc = page_envelope::crc(u32::MAX, payload) ^ u32::MAX;
            framed.extend_from_slice(&crc.to_le_bytes());

            let mut text = [0xee; 64];
            let mut breaks = [0xee; 64];
            let mut levels = [0xee; 64];
            let mut spans = [Span::default(); 4];
            let mut lines = [Line::default(); 4];
            let mut elements = [Element::default(); 4];
            let mut runs = [Run::default(); 4];
            let mut images = [Image::default(); 2];
            let mut links = [Link::default(); 2];
            let mut page = [0xee; 128];
            let mut style_text = [0xee; 32];
            let mut par_links = [ParLink::default(); 2];
            let mut breaker = DummyBreak;
            let mut img_src = DummyImages;
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
                style_text: &mut style_text,
                paragraph_links: &mut par_links,
            };
            let mut st = EngineState::new(
                params,
                storage,
                &mut breaker,
                &mut img_src,
                None,
                CssStylesheetBuilder::default(),
            )
            .unwrap();
            let mut cursor = PointCursor::new(0, framed.len() as u32)?;
            let mut win = [0u8; 1024];
            let mut read = |off: u32, dst: &mut [u8]| {
                let at = off as usize;
                framed.get(at..at + dst.len()).map(|s| dst.copy_from_slice(s)).is_some()
            };
            let mut pin = cursor.bind(&mut win, &mut read);
            let ok = st.load_resume(&mut pin.section(payload.len() as u32), expected_pages, max_chars);
            if !ok {
                assert_eq!(st.pages, 0, "failed load_resume must reset engine state");
                assert_eq!(st.len, 0, "failed load_resume must reset text len");
                assert_eq!(st.run_count, 0, "failed load_resume must reset run_count");
                return None;
            }
            assert!(pin.finish_crc(crc));
            assert_eq!(st.open_stack_bound(), 6);
            assert!(st.matches_open_names(4, 1, 1, 1));
            assert!(!st.matches_open_names(4, 1, 1, 0));
            assert!(!st.matches_open_names(3, 1, 1, 1));
            assert!(st.storage.page[st.pool_len..].iter().all(|&b| b == 0));
            let mut bits = [0u8; 896];
            let mut out = VecOut(Vec::new());
            assert!(st.save_resume(&mut bits, &mut out));
            Some(out.0)
        };

        let valid = make_valid_bytes();
        let re_emitted = try_load(&valid, 126, 50_000).expect("valid engine section must restore");
        assert_eq!(re_emitted, valid, "restore -> save must be byte-identical");

        // Header offsets: magic 0..4, u32s[0..8] at 4..36, u16s[0..16] at 36..68, u8s[0..13] at 68..81.
        let put32 = |buf: &mut [u8], idx: usize, v: u32| {
            let at = 4 + idx * 4;
            buf[at..at + 4].copy_from_slice(&v.to_le_bytes());
        };
        let put16 = |buf: &mut [u8], idx: usize, v: u16| {
            let at = 36 + idx * 2;
            buf[at..at + 2].copy_from_slice(&v.to_le_bytes());
        };

        // Every malformed mutation with valid outer CRC must return None without panicking.
        let mut cases: Vec<(&str, Vec<u8>)> = Vec::new();
        let mut m = valid.clone(); m[0] ^= 1; cases.push(("bad magic", m));
        let mut m = valid.clone(); put32(&mut m, 0, 125); cases.push(("pages mismatch", m));
        let mut m = valid.clone(); put32(&mut m, 1, 50_001); cases.push(("chars > max_chars", m));
        let mut m = valid.clone(); put32(&mut m, 7, 42_001); cases.push(("page_chars > chars", m));
        let mut m = valid.clone(); put32(&mut m, 2, (DEPTH_MAX as u32) + 2); cases.push(("overflow > cap", m));
        let mut m = valid.clone(); put16(&mut m, 3, 1); cases.push(("overflow > 0 with top not full", m));
        let mut m = valid.clone(); put32(&mut m, 3, i32::MAX as u32); cases.push(("depth = i32::MAX", m));
        let mut m = valid.clone(); put32(&mut m, 3, (-1i32) as u32); cases.push(("depth < 0", m));
        let mut m = valid.clone(); put32(&mut m, 4, (-1i32) as u32); cases.push(("suppress < 0", m));
        let mut m = valid.clone(); put32(&mut m, 5, 2); cases.push(("in_style > suppress", m));
        let mut m = valid.clone(); put32(&mut m, 5, 0); cases.push(("in_style == 0 with style_len > 0", m));
        let mut m = valid.clone(); put32(&mut m, 6, u32::from(u16::MAX) + 1); cases.push(("pending_margin > u16::MAX", m));
        let mut m = valid.clone(); put32(&mut m, 3, 40); put32(&mut m, 4, 8); cases.push(("depth + suppress + in_body > 48", m));
        let mut m = valid.clone(); put16(&mut m, 0, 64); cases.push(("len == text.len()", m));
        let mut m = valid.clone(); put16(&mut m, 1, 0); cases.push(("len > 0 with span_count == 0", m));
        let mut m = valid.clone(); put16(&mut m, 3, 4); cases.push(("top >= elements.len()", m));
        let mut m = valid.clone(); put16(&mut m, 4, 32); cases.push(("style_len >= style_text.len()", m));
        let mut m = valid.clone(); put16(&mut m, 5, 29); cases.push(("paragraph.size < 30", m));
        let mut m = valid.clone(); put16(&mut m, 5, 251); cases.push(("paragraph.size > 250", m));
        let mut m = valid.clone(); put16(&mut m, 6, (-1i16) as u16); cases.push(("paragraph.indent < 0", m));
        let mut m = valid.clone(); put16(&mut m, 7, (-1i16) as u16); cases.push(("paragraph.text_indent < 0", m));
        let mut m = valid.clone(); put16(&mut m, 13, 129); cases.push(("pool_len > page.len()", m));
        let mut m = valid.clone(); put16(&mut m, 14, (params.top - 1) as u16); cases.push(("y < top", m));
        let mut m = valid.clone(); put16(&mut m, 14, (i16::MAX / 2 + 1) as u16); cases.push(("y > i16::MAX/2", m));
        let mut m = valid.clone(); put16(&mut m, 15, (-1i16) as u16); cases.push(("carried_margin_px < 0", m));
        let mut m = valid.clone(); m[68] = 2; cases.push(("bad bool in_body", m));
        let mut m = valid.clone(); m[68 + 4] = SUP | SUB; cases.push(("paragraph SUP|SUB", m));
        let mut m = valid.clone(); m[68 + 5] = 5; cases.push(("bad align enum", m));
        let mut m = valid.clone(); m[68 + 8] = 2; cases.push(("current_link > par_link_count", m));
        let mut m = valid.clone(); m[68 + 10] = 3; cases.push(("word_state > 2", m));
        let mut m = valid.clone(); m[68 + 12] = 0x40; cases.push(("par_dir reserved bit", m));
        let mut m = valid.clone(); m[68 + 12] = 0x03; cases.push(("par_dir invalid base", m));
        let mut m = valid.clone(); m[68 + 12] = 0x1d; cases.push(("par_dir invalid prev level", m));
        // Invalid UTF-8 continuation bytes inside text[..len].
        let mut m = valid.clone(); m[81] = 0x80; cases.push(("invalid utf8 text", m));
        // Span 0 starts at 1 != 0.
        let span0_off = 81 + 11;
        let mut m = valid.clone(); m[span0_off..span0_off + 2].copy_from_slice(&1u16.to_le_bytes()); cases.push(("span0.start != 0", m));
        // Duplicate span start (non-strictly-increasing).
        let span1_off = span0_off + 8;
        let mut m = valid.clone(); m[span1_off..span1_off + 2].copy_from_slice(&0u16.to_le_bytes()); cases.push(("span1.start == span0.start", m));
        // Span 1 start > len (OOB).
        let mut m = valid.clone(); m[span1_off..span1_off + 2].copy_from_slice(&12u16.to_le_bytes()); cases.push(("span1.start > len", m));
        // Span 1 size out of range.
        let mut m = valid.clone(); m[span1_off + 2..span1_off + 4].copy_from_slice(&10u16.to_le_bytes()); cases.push(("span1.size < 30", m));
        // Span 1 link > par_link_count.
        let mut m = valid.clone(); m[span1_off + 7] = 2; cases.push(("span1.link > par_link_count", m));
        // Run 0 text slice escapes pool_len, zero size, bad flags, or invalid UTF-8 in page pool.
        let run0_off = 81 + 11 + 2 * 8 + 4 * 7;
        let mut m = valid.clone(); m[run0_off..run0_off + 4].copy_from_slice(&60u32.to_le_bytes()); cases.push(("run0 pool OOB", m));
        let mut m = valid.clone(); m[run0_off + 10..run0_off + 12].copy_from_slice(&0u16.to_le_bytes()); cases.push(("run0 size == 0", m));
        let mut m = valid.clone(); m[run0_off + 12] = SMALL_CAPS; cases.push(("run0 non-style flag", m));
        let pool_off = run0_off + 13 + 14 + 20;
        let mut m = valid.clone(); m[pool_off + 2] = 0x80; cases.push(("run0 invalid utf8 in pool", m));
        // Extra trailing byte / truncated section.
        let mut m = valid.clone(); m.push(0); cases.push(("trailing byte", m));
        let mut m = valid.clone(); m.pop(); cases.push(("truncated section", m));

        for (label, bytes) in cases {
            assert!(
                try_load(&bytes, 126, 50_000).is_none(),
                "malformed engine record ({label}) must be rejected"
            );
        }

        // Loading boundary `pending_margin == u16::MAX` followed by a styled
        // `margin-left` tag must saturate without overflowing.
        struct DummyBackend;
        impl Font for DummyBackend {
            fn advance(&mut self, _: u32, _: u16, _: u8) -> i16 { 8 }
            fn line_height(&mut self, _: u16) -> i16 { 18 }
            fn ascent(&mut self, _: u16) -> i16 { 14 }
            fn ligature(&mut self, _: u32, _: u32, _: u8) -> u32 { 0 }
            fn kerning(&mut self, _: u32, _: u32, _: u16, _: u8) -> i16 { 0 }
            fn covers(&mut self, _: u32) -> bool { true }
        }
        impl Sink for DummyBackend {
            fn page(&mut self, _: Page<'_>) -> bool { true }
        }
        let mut boundary = valid.clone();
        put32(&mut boundary, 6, u32::from(u16::MAX));
        put32(&mut boundary, 4, 0);
        put32(&mut boundary, 5, 0);
        put16(&mut boundary, 4, 0);
        // Strip the 5-byte style_text slice so section length stays exact.
        let style_off = 81 + 11 + 2 * 8 + 4 * 7 + 13 + 14 + 20 + 64;
        boundary.drain(style_off..style_off + 5);
        let mut framed = boundary.clone();
        let crc = page_envelope::crc(u32::MAX, &boundary) ^ u32::MAX;
        framed.extend_from_slice(&crc.to_le_bytes());
        let (mut text, mut breaks, mut levels) = ([0u8; 64], [0u8; 64], [0u8; 64]);
        let (mut spans, mut lines, mut elements) =
            ([Span::default(); 4], [Line::default(); 4], [Element::default(); 4]);
        let (mut runs, mut images, mut links) =
            ([Run::default(); 4], [Image::default(); 2], [Link::default(); 2]);
        let (mut page, mut style_text, mut par_links) =
            ([0u8; 128], [0u8; 32], [ParLink::default(); 2]);
        let (mut breaker, mut img_src) = (DummyBreak, DummyImages);
        let mut st = EngineState::new(
            params,
            Storage {
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
                style_text: &mut style_text,
                paragraph_links: &mut par_links,
            },
            &mut breaker,
            &mut img_src,
            None,
            CssStylesheetBuilder::default(),
        )
        .unwrap();
        let mut cursor = PointCursor::new(0, framed.len() as u32).unwrap();
        let mut win = [0u8; 1024];
        let mut read = |off: u32, dst: &mut [u8]| {
            let at = off as usize;
            framed.get(at..at + dst.len()).map(|s| dst.copy_from_slice(s)).is_some()
        };
        let mut pin = cursor.bind(&mut win, &mut read);
        assert!(st.load_resume(&mut pin.section(boundary.len() as u32), 126, 50_000));
        assert!(pin.finish_crc(crc));
        let mut backend = DummyBackend;
        st.start_tag(
            &mut backend,
            b"span",
            StartAttrs { inline: Some(b"margin-left: 2em"), ..StartAttrs::default() },
        );
        assert!(st.pending_margin <= u32::from(u16::MAX));
    }
}
