// SPDX-License-Identifier: MIT
//! Split a measured line into runs of one style and direction.
use super::{Backend, EngineState, Line, cjk, compression, crosses, hangul};

pub(super) const MAX_SEGMENTS: usize = 64;

#[derive(Clone, Copy, Default)]
pub(super) struct Segment {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) span: usize,
    pub(super) level: u8,
    pub(super) gap: bool,
    pub(super) space: bool,
    pub(super) script: bool,
    pub(super) compress: bool,
    pub(super) extra: u8,
}

pub(super) struct LineSegments<'a> {
    pub(super) segments: &'a [Segment; MAX_SEGMENTS],
    pub(super) count: usize,
    pub(super) next_start: Option<usize>,
}

impl EngineState<'_> {
    pub(super) fn segment_line<'a>(
        &mut self,
        backend: &mut dyn Backend,
        line: Line,
        justify: bool,
        segments: &'a mut [Segment; MAX_SEGMENTS],
    ) -> LineSegments<'a> {
        let hs = line.spaces == 0;
        let mut used = 0;
        let mut segment = Segment {
            start: line.start,
            span: self.span_index(line.start),
            level: self.storage.levels[line.start] & 7,
            extra: self.extra(line.start),
            ..Segment::default()
        };
        let (mut previous, mut offset) = (0, line.start);
        while offset < line.end && used < segments.len() {
            let at = offset;
            let span = self.span_index(at);
            let level = self.storage.levels[at] & 7;
            let extra = self.extra(at);
            let codepoint = self.shaped(backend, line.end, &mut offset, span);
            if justify && codepoint == 32 {
                close_segment(segments, &mut used, segment, at);
                let next = if offset < line.end { offset } else { at };
                segment = Segment {
                    start: offset,
                    span: self.span_index(next),
                    level: self.storage.levels[next] & 7,
                    extra: self.extra(next),
                    gap: true,
                    space: true,
                    ..Segment::default()
                };
                previous = codepoint;
                continue;
            }
            let compress = previous != 0 && compression(previous, codepoint, 2) != 0;
            let cjk_gap = previous != 0
                && !compress
                && justify
                && cjk(previous)
                && cjk(codepoint)
                && (hs || !(hangul(previous) && hangul(codepoint)));
            let mixed = previous != 0 && crosses(previous, codepoint);
            if span != segment.span || level != segment.level || extra != segment.extra || cjk_gap || mixed || compress
            {
                close_segment(segments, &mut used, segment, at);
                segment = Segment {
                    start: at,
                    span,
                    level,
                    extra,
                    gap: cjk_gap,
                    script: mixed,
                    compress,
                    ..Segment::default()
                }
            }
            previous = codepoint
        }
        let next_start = if used < segments.len() {
            close_segment(segments, &mut used, segment, line.end);
            None
        } else {
            // A new segment after the last slot starts a forced line break.
            let mut start = segment.start;
            while start < line.end && self.storage.text[start] == b' ' {
                start += 1;
            }
            Some(start)
        };
        LineSegments { segments, count: used, next_start }
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
