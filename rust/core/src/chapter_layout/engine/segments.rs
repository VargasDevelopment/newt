// SPDX-License-Identifier: MIT
//! Split a measured line into runs of one style and direction.
use super::{Backend, EngineState, Line, cjk, compression, crosses, hangul};

pub(super) const MAX_SEGMENTS: usize = 64;

pub(super) const SEG_GAP: u8 = 1;
pub(super) const SEG_SPACE: u8 = 2;
pub(super) const SEG_SCRIPT: u8 = 4;
pub(super) const SEG_COMPRESS: u8 = 8;
pub(super) const SEG_SMALL_CAP: u8 = 16;

#[derive(Clone, Copy, Default)]
pub(super) struct Segment {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) span: usize,
    /// Accumulated advance of pending justified space(s) before this segment
    /// in case-styled paragraphs, priced in each space's own span style.
    pub(super) space_adv: i16,
    /// Physical left inline margin carried across skipped soft hyphen(s)
    /// directly onto this segment (never reordered as a BiDi word gap; fits
    /// in the original 20 B RV32 / 32 B x86_64 size of `Segment`).
    pub(super) shy_margin: i16,
    /// Number of pending justification gaps before this segment.
    pub(super) space_gaps: u8,
    pub(super) level: u8,
    pub(super) extra: u8,
    /// Bitmask of `SEG_*` flags.
    pub(super) flags: u8,
}
const _: () = assert!(
    core::mem::size_of::<Segment>()
        == if cfg!(target_pointer_width = "64") {
            32
        } else {
            20
        }
);

#[derive(Clone, Copy, Default)]
pub(super) struct TrailingAdvance {
    pub(super) margin: i16,
    pub(super) space_adv: i16,
    pub(super) space_gaps: u8,
}
impl TrailingAdvance {
    fn from_segment(seg: Segment) -> Self {
        let has_gap = seg.flags & (SEG_GAP | SEG_SPACE) != 0;
        Self {
            margin: seg.shy_margin,
            space_adv: if has_gap { seg.space_adv } else { 0 },
            space_gaps: if has_gap { seg.space_gaps } else { 0 },
        }
    }
    pub(super) fn is_empty(self) -> bool {
        self.margin == 0 && self.space_adv == 0 && self.space_gaps == 0
    }
}

pub(super) struct LineSegments {
    pub(super) count: usize,
    pub(super) next_start: Option<usize>,
    /// Nonzero margin/space advance after the final visible run when no
    /// trailing hyphen run is emitted (e.g. paragraph-end or RTL suppressed
    /// break ending on a margin-bearing soft hyphen).
    pub(super) trailing: TrailingAdvance,
}

impl EngineState<'_> {
    #[inline(never)]
    pub(super) fn segment_line(
        &mut self,
        backend: &mut dyn Backend,
        line: Line,
        justify: bool,
        justify_hangul: bool,
        segments: &mut [Segment; MAX_SEGMENTS],
    ) -> LineSegments {
        let mut used = 0;
        let first_span = self.span_index(line.start);
        let mut segment = Segment {
            start: line.start,
            span: first_span,
            level: self.storage.levels[line.start] & 7,
            extra: self.extra(line.start),
            ..Segment::default()
        };
        let mut has_visible = false;
        let (mut previous, mut offset) = (0, line.start);
        while offset < line.end && used < segments.len() {
            let at = offset;
            let span = self.span_index(at);
            let level = self.storage.levels[at] & 7;
            let extra = self.extra(at);
            let same_span_sc =
                has_visible && span == segment.span && segment.flags & SEG_SMALL_CAP != 0;
            let (codepoint, small_cap) =
                self.shaped_case(backend, line.end, &mut offset, span, same_span_sc);
            if justify && codepoint == 32 {
                if has_visible || !self.par_case {
                    close_segment(segments, &mut used, segment, at);
                    let next = if offset < line.end { offset } else { at };
                    let (seg_span, seg_level, seg_extra, sp_adv) = if self.par_case {
                        let sp = self.storage.spans[span];
                        let margin = self.margin_at_byte(span, at);
                        let adv = backend
                            .advance(32, self.size(sp.size), (sp.flags | extra) & super::STYLE_MASK)
                            .saturating_add(margin as i16);
                        (span, level, extra, adv)
                    } else {
                        (
                            self.span_index(next),
                            self.storage.levels[next] & 7,
                            self.extra(next),
                            0,
                        )
                    };
                    segment = Segment {
                        start: offset,
                        span: seg_span,
                        level: seg_level,
                        extra: seg_extra,
                        space_adv: sp_adv,
                        space_gaps: 1,
                        flags: SEG_GAP | SEG_SPACE,
                        ..Segment::default()
                    };
                } else {
                    // Consecutive spaces or a space after a margin-bearing soft hyphen:
                    // any pending shy_margin preceded this space, so it joins the
                    // intervening justified gap rather than shifting the next glyph.
                    let sp = self.storage.spans[span];
                    let margin = self.margin_at_byte(span, at);
                    let sp_adv = backend
                        .advance(32, self.size(sp.size), (sp.flags | extra) & super::STYLE_MASK)
                        .saturating_add(margin as i16);
                    segment.start = offset;
                    segment.span = span;
                    segment.level = level;
                    segment.extra = extra;
                    segment.space_adv = segment
                        .space_adv
                        .saturating_add(segment.shy_margin)
                        .saturating_add(sp_adv);
                    segment.shy_margin = 0;
                    segment.space_gaps = segment.space_gaps.saturating_add(1);
                    segment.flags |= SEG_GAP | SEG_SPACE;
                }
                has_visible = false;
                previous = codepoint;
                continue;
            }
            // In case-styled paragraphs, invisible soft hyphens inherit the
            // active run's size and tail so they never split small-cap words,
            // spend extra run slots, or force a full-size hyphen across a span end.
            // When a soft hyphen sits at a margin-bearing span start, carry that
            // margin forward as a physical left advance without emitting an empty run.
            if codepoint == 0x00ad && self.par_case {
                let shy_margin = self.margin_at_byte(span, at);
                if shy_margin > 0 {
                    if has_visible {
                        close_segment(segments, &mut used, segment, at);
                        segment = Segment {
                            start: at,
                            span: segment.span,
                            level: segment.level,
                            extra: segment.extra,
                            shy_margin: shy_margin as i16,
                            flags: segment.flags & SEG_SMALL_CAP,
                            ..Segment::default()
                        };
                        has_visible = false;
                    } else {
                        segment.shy_margin = segment.shy_margin.saturating_add(shy_margin as i16);
                    }
                }
                continue;
            }
            let compress = previous != 0 && compression(previous, codepoint, 2) != 0;
            let cjk_gap = previous != 0
                && !compress
                && justify
                && cjk(previous)
                && cjk(codepoint)
                && (justify_hangul || !(hangul(previous) && hangul(codepoint)));
            let mixed = previous != 0 && crosses(previous, codepoint);
            if !has_visible {
                if self.par_case {
                    segment.start = at;
                    segment.span = span;
                    segment.level = level;
                    segment.extra = extra;
                    let base_sz = self.size(self.storage.spans[span].size);
                    if mixed {
                        segment.space_adv =
                            segment.space_adv.saturating_add((base_sz / 4) as i16);
                        segment.flags |= SEG_SPACE;
                    }
                    if compress {
                        segment.space_adv =
                            segment.space_adv.saturating_sub((base_sz / 2) as i16);
                        segment.flags |= SEG_SPACE;
                    }
                    if cjk_gap {
                        segment.space_gaps = segment.space_gaps.saturating_add(1);
                        segment.flags |= SEG_GAP;
                    }
                }
                if small_cap {
                    segment.flags |= SEG_SMALL_CAP;
                } else {
                    segment.flags &= !SEG_SMALL_CAP;
                }
                has_visible = true;
                if self.par_case {
                    previous = codepoint;
                    continue;
                }
            }
            let seg_sc = segment.flags & SEG_SMALL_CAP != 0;
            if span != segment.span
                || level != segment.level
                || extra != segment.extra
                || small_cap != seg_sc
                || cjk_gap
                || mixed
                || compress
            {
                close_segment(segments, &mut used, segment, at);
                let mut flags = 0;
                if small_cap {
                    flags |= SEG_SMALL_CAP;
                }
                if cjk_gap {
                    flags |= SEG_GAP;
                }
                if mixed {
                    flags |= SEG_SCRIPT;
                }
                if compress {
                    flags |= SEG_COMPRESS;
                }
                segment = Segment {
                    start: at,
                    span,
                    level,
                    extra,
                    space_adv: 0,
                    space_gaps: u8::from(cjk_gap),
                    flags,
                    ..Segment::default()
                };
                has_visible = true;
            }
            previous = codepoint
        }
        let (next_start, trailing) = if used < segments.len() {
            if has_visible
                || !self.par_case
                || (line.flags & 2 != 0 && !self.rtl && segment.start < line.end)
            {
                close_segment(segments, &mut used, segment, line.end);
                (None, TrailingAdvance::default())
            } else {
                (None, TrailingAdvance::from_segment(segment))
            }
        } else {
            // A new segment after the last slot starts a forced line break.
            let mut tr_out = TrailingAdvance::default();
            let next = if self.par_case {
                let keep_trailing_shy = line.flags & 2 != 0 && !self.rtl;
                let mut only_invisible_tail = false;
                if !has_visible && !keep_trailing_shy {
                    let mut probe = offset;
                    let mut tr = segment;
                    let mut all_invisible = true;
                    while probe < line.end {
                        let at = probe;
                        let span = self.span_index(at);
                        let cp = super::decode(&self.storage.text[..line.end], &mut probe);
                        if cp == 0x00ad {
                            let m = self.margin_at_byte(span, at);
                            tr.shy_margin = tr.shy_margin.saturating_add(m as i16);
                        } else if justify && cp == 32 {
                            let sp = self.storage.spans[span];
                            let extra = self.extra(at);
                            let sp_adv = backend
                                .advance(
                                    32,
                                    self.size(sp.size),
                                    (sp.flags | extra) & super::STYLE_MASK,
                                )
                                .saturating_add(self.margin_at_byte(span, at) as i16);
                            tr.space_adv = tr
                                .space_adv
                                .saturating_add(tr.shy_margin)
                                .saturating_add(sp_adv);
                            tr.shy_margin = 0;
                            tr.space_gaps = tr.space_gaps.saturating_add(1);
                            tr.flags |= SEG_GAP | SEG_SPACE;
                        } else {
                            all_invisible = false;
                            break;
                        }
                    }
                    if all_invisible {
                        tr_out = TrailingAdvance::from_segment(tr);
                        only_invisible_tail = true;
                    }
                }
                if only_invisible_tail {
                    None
                } else {
                    let skip_leading = |mut s: usize| -> (usize, bool) {
                        loop {
                            if s < line.end && self.storage.text[s] == b' ' {
                                s += 1;
                            } else if s + 2 <= line.end
                                && self.storage.text[s..s + 2] == [0xc2, 0xad]
                            {
                                let mut run_end = s;
                                let mut first_margin = None;
                                while run_end + 2 <= line.end
                                    && self.storage.text[run_end..run_end + 2] == [0xc2, 0xad]
                                {
                                    if first_margin.is_none()
                                        && self.margin_at_offset(run_end) != 0
                                    {
                                        first_margin = Some(run_end);
                                        if !keep_trailing_shy {
                                            break;
                                        }
                                    }
                                    run_end += 2;
                                }
                                if keep_trailing_shy && run_end == line.end {
                                    return (first_margin.unwrap_or(s), true);
                                }
                                if let Some(m) = first_margin {
                                    return (m, false);
                                }
                                s = run_end;
                            } else {
                                return (s, false);
                            }
                        }
                    };
                    let (mut start, only_shy_tail) = skip_leading(segment.start);
                    if keep_trailing_shy && only_shy_tail && used > 1 {
                        // Keep a chosen LTR discretionary hyphen attached to the final
                        // visible run so it retains that run's size and style on row 2.
                        used -= 1;
                        start = skip_leading(segments[used - 1].end).0;
                    }
                    if start < line.end && used > 1 {
                        let mut probe = start;
                        let mut first_cp = 0u32;
                        while probe < line.end {
                            first_cp = super::decode(&self.storage.text[..line.end], &mut probe);
                            if first_cp != 0x00ad {
                                break;
                            }
                        }
                        if super::is_combining(first_cp) {
                            // Keep combining mark(s) attached to their preceding base segment
                            // across the 64-segment cap (including after a hyphen carry or a
                            // retained margin-bearing soft hyphen).
                            while used > 1 {
                                let prev_seg = segments[used - 1];
                                if prev_seg.end == prev_seg.start
                                    || self.storage.text[prev_seg.end - 1] == b' '
                                    || self.storage.text[prev_seg.end..start].contains(&b' ')
                                {
                                    break;
                                }
                                used -= 1;
                                start = skip_leading(segments[used - 1].end).0;
                                let mut p = prev_seg.start;
                                let mut cp = 0u32;
                                while p < prev_seg.end {
                                    cp = super::decode(&self.storage.text[..prev_seg.end], &mut p);
                                    if cp != 0x00ad {
                                        break;
                                    }
                                }
                                if !super::is_combining(cp) {
                                    break;
                                }
                            }
                        }
                    }
                    if start >= line.end { None } else { Some(start) }
                }
            } else {
                let mut start = segment.start;
                while start < line.end && self.storage.text[start] == b' ' {
                    start += 1;
                }
                Some(start)
            };
            (next, tr_out)
        };
        LineSegments { count: used, next_start, trailing }
    }
}

fn close_segment(segments: &mut [Segment; MAX_SEGMENTS], used: &mut usize, mut segment: Segment, end: usize) {
    if end > segment.start {
        debug_assert!(*used < segments.len());
        segment.end = end;
        segments[*used] = segment;
        *used += 1;
    }
}
