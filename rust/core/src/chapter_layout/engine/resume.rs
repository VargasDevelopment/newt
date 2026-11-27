//! Canonical layout-engine state capture for resume points. MIT.
#![forbid(unsafe_code)]
use super::*;
use crate::resume_point::{PointOut, put_u16s, put_u32s};

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
}
