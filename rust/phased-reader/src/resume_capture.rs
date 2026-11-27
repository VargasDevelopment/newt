//! Observational resume-point capture at producer feed boundaries. MIT.
#![forbid(unsafe_code)]
use crate::{
    phase_storage::{LAYOUT_IMAGES, LAYOUT_LINKS, LAYOUT_PAGE_POOL, LAYOUT_RUNS},
    styles::Slot,
};
use newt_core::{
    cache_names::segment::PAGES,
    chapter_layout::engine::EngineState,
    member_check::Checked,
    page_cache::PageCacheWriter,
    page_envelope,
    resume_point::{
        Binding, CrcOut, DEPTH_MAX, Decline, HEADER_BYTES, Layout, Measure, Mode, NAMES_MAX,
        PROLOG_MAX, PointOut, SECTIONS, Section, WINDOW_PAGES, encode_source,
        validate_utf8_prolog,
    },
    xml_session::{Attributes, Handler, ParseObservation, StreamView},
};

/// 896 B covers the 7,168 B page pool in one linear bitset pass.
pub const LIVE_BITS_BYTES: usize = LAYOUT_PAGE_POOL / 8;

/// Proven worst-case serialized byte size of one `PageCacheWriter::on_page`
/// record under this producer layout profile: 10 B page header + 10 B/run +
/// 10 B/image + 12 B/link + disjoint page pool bytes (all run text, image href,
/// link target and link fragment slices are allocated sequentially without
/// aliasing from `storage.page[..LAYOUT_PAGE_POOL]`).
pub const MAX_PAGE_WIRE_BYTES: u32 =
    (10 + LAYOUT_RUNS * 10 + LAYOUT_IMAGES * 10 + LAYOUT_LINKS * 12 + LAYOUT_PAGE_POOL) as u32;

/// Layout profile tag bound in `Binding::profile` (Small/Standard shared-bank profile).
pub const LAYOUT_PROFILE_STANDARD: u8 = 0;

/// Fixed-capacity prolog and open-name tracker borrowed from the shared bank remainder.
pub struct Tap<'a> {
    prolog: &'a mut [u8; PROLOG_MAX],
    names: &'a mut [u8; NAMES_MAX],
    lens: &'a mut [u8; DEPTH_MAX],
    leading: [u8; 4],
    leading_len: u8,
    prolog_len: u16,
    prolog_over: bool,
    root_seen: bool,
    root: u64,
    depth: u8,
    used: u16,
    omitted: u32,
    pub(crate) edge_done: u32,
}

impl<'a> Tap<'a> {
    pub(crate) fn new(
        prolog: &'a mut [u8; PROLOG_MAX],
        names: &'a mut [u8; NAMES_MAX],
        lens: &'a mut [u8; DEPTH_MAX],
    ) -> Self {
        Self {
            prolog,
            names,
            lens,
            leading: [0; 4],
            leading_len: 0,
            prolog_len: 0,
            prolog_over: false,
            root_seen: false,
            root: 0,
            depth: 0,
            used: 0,
            omitted: 0,
            edge_done: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.leading = [0; 4];
        self.leading_len = 0;
        self.prolog_len = 0;
        self.prolog_over = false;
        self.root_seen = false;
        self.root = 0;
        self.depth = 0;
        self.used = 0;
        self.omitted = 0;
        self.edge_done = 0;
    }

    pub(crate) fn depth(&self) -> u8 {
        self.depth
    }

    pub(crate) fn prolog_slice_mut(&mut self, len: usize) -> &mut [u8] {
        &mut self.prolog[..len]
    }

    /// Finish seeding the tap after the caller already copied `prolog_len` bytes
    /// into `self.prolog[..prolog_len]`, avoiding any stack copy of the prolog.
    pub(crate) fn finish_restore_prolog(
        &mut self,
        prolog_len: usize,
        root_name: &[u8],
        edge_done: u32,
    ) -> bool {
        if prolog_len > PROLOG_MAX {
            return false;
        }
        self.reset();
        self.prolog_len = prolog_len as u16;
        self.root_seen = true;
        self.root = prolog_len as u64;
        self.edge_done = edge_done;
        let mut n = 0usize;
        for &b in self.prolog[..prolog_len].iter().take(4) {
            self.leading[n] = b;
            n += 1;
        }
        if n < 4 {
            self.leading[n] = b'<';
            n += 1;
        }
        for &b in root_name.iter().take(4 - n) {
            self.leading[n] = b;
            n += 1;
        }
        while n < 4 {
            self.leading[n] = b'>';
            n += 1;
        }
        self.leading_len = n as u8;
        true
    }

    pub(crate) fn push_restored(&mut self, name: &[u8]) -> bool {
        let before_depth = self.depth;
        let before_omitted = self.omitted;
        self.push(name);
        self.omitted == before_omitted && self.depth == before_depth + 1
    }

    /// Observe filtered bytes before feeding them to the parser.
    pub fn observe(&mut self, bytes: &[u8]) {
        if (self.leading_len as usize) < self.leading.len() {
            let have = self.leading_len as usize;
            let take = bytes.len().min(self.leading.len() - have);
            self.leading[have..have + take].copy_from_slice(&bytes[..take]);
            self.leading_len = (have + take) as u8;
        }
        if self.root_seen || self.prolog_over {
            return;
        }
        let have = usize::from(self.prolog_len);
        let take = bytes.len().min(PROLOG_MAX - have);
        self.prolog[have..have + take].copy_from_slice(&bytes[..take]);
        self.prolog_len = (have + take) as u16;
    }

    /// Lock the pre-root prolog extent once the parser reports the root element offset.
    pub fn finish_root(&mut self, root_index: i64) {
        if self.root_seen || root_index < 0 {
            return;
        }
        self.root_seen = true;
        let at = root_index as u64;
        self.root = at;
        if at > u64::from(self.prolog_len) {
            self.prolog_over = true;
        } else {
            self.prolog_len = at as u16;
        }
    }

    pub fn wrap<'t>(&'t mut self, inner: &'t mut dyn Handler) -> TapHandler<'t, 'a> {
        TapHandler { tap: self, inner }
    }

    /// Records one Expat-delivered open element name.
    ///
    /// Precondition: capture-time filter over Expat-accepted names, not a
    /// standalone XML Name/UTF-8 validator for untrusted on-card sections.
    fn push(&mut self, name: &[u8]) {
        let bad = name.is_empty()
            || name.len() > u8::MAX as usize
            || name.iter().any(|&b| {
                matches!(
                    b,
                    0 | b'<' | b'>' | b'/' | b'=' | b'"' | b'\'' | b' ' | b'\t' | b'\r' | b'\n'
                )
            });
        if self.omitted > 0
            || usize::from(self.depth) >= DEPTH_MAX
            || bad
            || usize::from(self.used) + name.len() > NAMES_MAX
        {
            self.omitted = self.omitted.saturating_add(1);
            return;
        }
        let used = usize::from(self.used);
        self.names[used..used + name.len()].copy_from_slice(name);
        self.lens[usize::from(self.depth)] = name.len() as u8;
        self.used = (used + name.len()) as u16;
        self.depth += 1;
    }

    fn pop(&mut self) {
        if self.omitted > 0 {
            self.omitted -= 1;
            return;
        }
        if self.depth > 0 {
            self.depth -= 1;
            self.used -= u16::from(self.lens[usize::from(self.depth)]);
        }
    }

    fn encode_names(&self, out: &mut dyn PointOut) -> bool {
        if !out.put(&[self.depth]) {
            return false;
        }
        let mut at = 0usize;
        for &len in &self.lens[..usize::from(self.depth)] {
            let n = usize::from(len);
            if !out.put(&[len]) || !out.put(&self.names[at..at + n]) {
                return false;
            }
            at += n;
        }
        true
    }
}

pub struct TapHandler<'t, 'a> {
    tap: &'t mut Tap<'a>,
    inner: &'t mut dyn Handler,
}
impl Handler for TapHandler<'_, '_> {
    fn start_element(&mut self, name: &[u8], attributes: &mut dyn Attributes) {
        self.tap.push(name);
        self.inner.start_element(name, attributes);
    }
    fn end_element(&mut self, name: &[u8]) {
        self.tap.pop();
        self.inner.end_element(name);
    }
    fn text(&mut self, text: &[u8]) {
        self.inner.text(text);
    }
    fn stop_parse(&self) -> bool {
        self.inner.stop_parse()
    }
}

/// Producer-only capture loan carved from `Phase::shared` remainder during layout.
pub struct CaptureState<'a> {
    pub obs: ParseObservation,
    pub tap: Tap<'a>,
    pub live_bits: &'a mut [u8; LIVE_BITS_BYTES],
}

impl<'a> CaptureState<'a> {
    pub fn new(
        prolog: &'a mut [u8; PROLOG_MAX],
        names: &'a mut [u8; NAMES_MAX],
        lens: &'a mut [u8; DEPTH_MAX],
        live_bits: &'a mut [u8; LIVE_BITS_BYTES],
    ) -> Self {
        Self {
            obs: ParseObservation::new(),
            tap: Tap::new(prolog, names, lens),
            live_bits,
        }
    }
    pub(crate) fn reset(&mut self) {
        self.obs = ParseObservation::new();
        self.tap.reset();
    }
}

/// Snapshot of read-only stream/source facts at the current feed boundary.
pub struct BoundaryInput<'a> {
    pub entry: &'a Checked,
    pub source_bytes: u64,
    pub source_consumed: u32,
    pub source_method: u16,
    pub parser_index: i64,
    pub stream: StreamView<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureOutcome {
    Idle,
    Captured(u32),
    Declined(u32, Decline),
    WriteFailed,
}

struct EmitCtx<'a, 's, 'e, 'b> {
    binding: &'a Binding,
    consumed: u32,
    stream: &'a StreamView<'s>,
    engine: &'a EngineState<'e>,
    capture: &'a mut CaptureState<'b>,
    replay: &'a [u8],
}

#[inline(never)]
fn emit_section(ctx: &mut EmitCtx<'_, '_, '_, '_>, section: Section, out: &mut dyn PointOut) -> bool {
    match section {
        Section::Binding => ctx.binding.encode(out),
        Section::Source => encode_source(ctx.consumed, out),
        Section::Stream => ctx.stream.save_resume(out),
        Section::Engine => ctx.engine.save_resume(ctx.capture.live_bits, out),
        Section::Prolog => out.put(&ctx.capture.tap.prolog[..usize::from(ctx.capture.tap.prolog_len)]),
        Section::Names => ctx.capture.tap.encode_names(out),
        Section::Replay => out.put(ctx.replay),
    }
}

/// Preflight 4 MiB envelope payload ceiling and remaining producer write credit
/// using the proven worst-case serialized page bound (`MAX_PAGE_WIRE_BYTES`) for
/// every remaining page in the segment, plus index, anchors, v4 footer and
/// envelope directory/manifest overhead.
pub fn preflight_fit(writer_offset: u32, written_pages: u32, point_bytes: u32, credit: Option<u64>) -> bool {
    if written_pages == 0 || written_pages >= PAGES {
        return false;
    }
    let rem_pages = PAGES - written_pages;
    let max_rem = rem_pages.saturating_mul(MAX_PAGE_WIRE_BYTES);
    // Index (128 * 8) + anchors (192 * 8) + v4 whole footer (32) = 2,592 B.
    const TAIL_BYTES: u32 = PAGES * 8 + 192 * 8 + 32;
    let Some(projected) = writer_offset
        .checked_add(point_bytes)
        .and_then(|v| v.checked_add(max_rem))
        .and_then(|v| v.checked_add(TAIL_BYTES))
    else {
        return false;
    };
    if projected > page_envelope::MAX_PAYLOAD {
        return false;
    }
    if let Some(remaining) = credit {
        let envelope_overhead =
            4u32.saturating_mul(page_envelope::blocks(projected)) + page_envelope::MANIFEST_BYTES;
        let need = u64::from(point_bytes)
            + u64::from(max_rem)
            + u64::from(TAIL_BYTES)
            + u64::from(envelope_overhead);
        if remaining < need {
            return false;
        }
    }
    true
}

/// Certified route facts needed for `Binding` without borrowing the hasher.
pub struct CaptureRoute<'a> {
    pub digest: &'a [u8; 32],
    pub spine: u32,
    pub generation: u32,
    pub page_count: u32,
    pub total_chars: u32,
}

/// Non-generic writer view so `maybe_capture` compiles once across all store types.
pub struct CaptureWriter<'a> {
    pub cache: &'a mut PageCacheWriter,
    pub first: u32,
    pub seen: u32,
    pub open: bool,
    pub refused: &'a mut bool,
}

/// Attempt to capture one resume point at the current feed boundary into the
/// open interior v4 segment temp. Never blocks or fails an ordinary segment on
/// decline; returns `WriteFailed` only if a real store write fails mid-point.
#[inline(never)]
pub fn maybe_capture(
    capture: &mut CaptureState<'_>,
    engine: &EngineState<'_>,
    slot: &Slot<'_>,
    writer: CaptureWriter<'_>,
    credit: Option<u64>,
    write: &mut dyn FnMut(&[u8]) -> bool,
    route: &CaptureRoute<'_>,
    input: &BoundaryInput<'_>,
) -> CaptureOutcome {
    if !writer.open || !writer.cache.is_v4() || writer.cache.has_point() {
        return CaptureOutcome::Idle;
    }
    let edge = writer.first.wrapping_add(PAGES);
    if edge >= route.page_count || edge <= capture.tap.edge_done {
        return CaptureOutcome::Idle;
    }
    let pages = engine.pages;
    if pages >= edge {
        capture.tap.edge_done = edge;
        return CaptureOutcome::Declined(edge, Decline::Late);
    }
    if edge - pages > WINDOW_PAGES || writer.cache.page_count() == 0 || writer.seen != pages {
        return CaptureOutcome::Idle;
    }
    let sink = CaptureSink { writer, credit, write };
    let outcome = attempt_capture(capture, engine, slot, sink, route, input, edge);
    if let CaptureOutcome::Captured(done) = outcome {
        capture.tap.edge_done = done;
    }
    outcome
}

struct CaptureSink<'a> {
    writer: CaptureWriter<'a>,
    credit: Option<u64>,
    write: &'a mut dyn FnMut(&[u8]) -> bool,
}

#[inline(never)]
fn attempt_capture(
    capture: &mut CaptureState<'_>,
    engine: &EngineState<'_>,
    slot: &Slot<'_>,
    sink: CaptureSink<'_>,
    route: &CaptureRoute<'_>,
    input: &BoundaryInput<'_>,
    edge: u32,
) -> CaptureOutcome {
    let CaptureSink { writer, credit, write } = sink;
    if !capture.tap.root_seen
        || capture.tap.prolog_over
        || capture.tap.root != u64::from(capture.tap.prolog_len)
    {
        return CaptureOutcome::Declined(edge, Decline::Prolog);
    }
    let leading = &capture.tap.leading[..usize::from(capture.tap.leading_len)];
    let prolog = &capture.tap.prolog[..usize::from(capture.tap.prolog_len)];
    if let Err(why) = validate_utf8_prolog(leading, prolog) {
        return CaptureOutcome::Declined(edge, why);
    }
    if capture.obs.cdata_depth > 0 {
        return CaptureOutcome::Declined(edge, Decline::Cdata);
    }
    if capture.tap.omitted > 0 || capture.tap.depth == 0 {
        return CaptureOutcome::Declined(edge, Decline::Depth);
    }
    if input.parser_index < 0 {
        return CaptureOutcome::Declined(edge, Decline::Backlog);
    }
    let Some(cut) = input
        .parser_index
        .checked_add(input.stream.shift)
        .and_then(|v| u64::try_from(v).ok())
    else {
        return CaptureOutcome::Declined(edge, Decline::Backlog);
    };
    let fed = input.stream.bytes_consumed;
    let Some(backlog) = fed.checked_sub(cut) else {
        return CaptureOutcome::Declined(edge, Decline::Backlog);
    };
    if backlog > input.stream.last_feed.len() as u64 || cut < capture.tap.root {
        return CaptureOutcome::Declined(edge, Decline::Backlog);
    }
    let mode = match input.source_method {
        0 => Mode::Seek,
        8 => Mode::Reinflate,
        _ => return CaptureOutcome::Declined(edge, Decline::Source),
    };
    if !input.stream.open
        || input.stream.finished
        || input.stream.reader_produced != input.source_consumed
        || input.source_consumed > input.entry.uncompressed_size
    {
        return CaptureOutcome::Declined(edge, Decline::Source);
    }
    let replay = &input.stream.last_feed[input.stream.last_feed.len() - backlog as usize..];
    let binding = Binding {
        book_digest: *route.digest,
        spine: route.spine,
        generation: route.generation,
        segment_index: edge / PAGES,
        page_count: route.page_count,
        total_chars: route.total_chars,
        style_hash: slot.hash,
        style_count: slot.count as u16,
        style_issues: slot.issues,
        style_state: slot.state as u8,
        profile: LAYOUT_PROFILE_STANDARD,
        member_method: input.entry.method,
        member_offset: input.entry.local_header_offset,
        member_compressed: input.entry.compressed_size,
        member_uncompressed: input.entry.uncompressed_size,
        source_bytes: input.source_bytes,
        pages_at: engine.pages,
        cut,
        fed,
    };

    const SECTION_DECLINE: [Decline; SECTIONS.len()] = [
        Decline::Source,
        Decline::Source,
        Decline::Source,
        Decline::Engine,
        Decline::Prolog,
        Decline::Depth,
        Decline::Backlog,
    ];
    let mut ctx = EmitCtx {
        binding: &binding,
        consumed: input.source_consumed,
        stream: &input.stream,
        engine,
        capture,
        replay,
    };

    // Pass 1: measure every section in frozen Pass B consumption order without I/O.
    let mut lengths = [0u32; SECTIONS.len()];
    let mut measure = Measure::default();
    for i in 0..SECTIONS.len() {
        measure.bytes = 0;
        measure.overflow = false;
        if !emit_section(&mut ctx, SECTIONS[i], &mut measure) || measure.overflow {
            return CaptureOutcome::Declined(edge, SECTION_DECLINE[i]);
        }
        lengths[i] = measure.bytes;
    }
    let Some((layout, total_bytes)) = Layout::from_lengths(mode, lengths) else {
        return CaptureOutcome::Declined(edge, Decline::Engine);
    };
    if !preflight_fit(
        writer.cache.offset(),
        writer.cache.page_count(),
        total_bytes,
        credit,
    ) {
        return CaptureOutcome::Declined(edge, Decline::Budget);
    }

    // Pass 2: stream directly into the open temp through the 1 KiB store window.
    let mut emit = |crc_out: &mut CrcOut<'_>| {
        let mut header = [0u8; HEADER_BYTES];
        layout.write_header(&mut header);
        if !crc_out.put(&header) {
            return false;
        }
        for section in SECTIONS {
            if !emit_section(&mut ctx, section, crc_out) {
                return false;
            }
        }
        let trailer = crc_out.finish_crc().to_le_bytes();
        crc_out.put(&trailer)
    };
    let wrote = writer.cache.write_point(total_bytes, write, &mut emit);
    if !wrote {
        *writer.refused = true;
        return CaptureOutcome::WriteFailed;
    }
    CaptureOutcome::Captured(edge)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    struct VecOut(Vec<u8>);
    impl PointOut for VecOut {
        fn put(&mut self, bytes: &[u8]) -> bool {
            self.0.extend_from_slice(bytes);
            true
        }
    }

    #[test]
    fn tap_recovers_after_omitted_depth_and_caps_prolog() {
        let mut prolog = [0u8; PROLOG_MAX];
        let mut names = [0u8; NAMES_MAX];
        let mut lens = [0u8; DEPTH_MAX];
        let mut tap = Tap::new(&mut prolog, &mut names, &mut lens);

        tap.observe(b"<?xml ");
        tap.observe(b"version='1.0'?><html>");
        tap.finish_root(21);
        assert!(tap.root_seen);
        assert!(!tap.prolog_over);
        assert_eq!(tap.prolog_len, 21);

        tap.push(b"html");
        tap.push(b"body");
        for _ in 0..50 {
            tap.push(b"div");
        }
        assert!(tap.omitted > 0, "depth past DEPTH_MAX must record omitted elements");
        for _ in 0..50 {
            tap.pop();
        }
        assert_eq!(tap.omitted, 0, "unwinding deep subtree must restore eligibility");
        assert_eq!(tap.depth, 2);
        let mut encoded = VecOut(Vec::new());
        assert!(tap.encode_names(&mut encoded));
        assert_eq!(encoded.0, b"\x02\x04html\x04body");

        // Overlong prolog marks prolog_over.
        let mut p2 = [0u8; PROLOG_MAX];
        let mut n2 = [0u8; NAMES_MAX];
        let mut l2 = [0u8; DEPTH_MAX];
        let mut tap2 = Tap::new(&mut p2, &mut n2, &mut l2);
        tap2.observe(&[b' '; 600]);
        tap2.finish_root(600);
        assert!(tap2.prolog_over);
    }

    #[test]
    fn preflight_fit_enforces_4mib_ceiling_and_credit_reserve() {
        assert_eq!(MAX_PAGE_WIRE_BYTES, 10_490);
        // 4 remaining pages -> reserves 4 * 10,490 + 2,592 = 44,552 B plus point + envelope.
        assert!(preflight_fit(124 * 1024, 124, 2048, None));
        assert!(!preflight_fit(page_envelope::MAX_PAYLOAD - 40_000, 124, 2048, None));
        // Sparse 124-page prefix (124 * 100 B) still reserves full proven tail (4 * 10,490 B).
        assert!(!preflight_fit(12_400, 124, 2048, Some(16_384)));
        assert!(preflight_fit(12_400, 124, 2048, Some(64 * 1024)));
        assert!(!preflight_fit(0, 0, 1024, None));
        assert!(!preflight_fit(128 * 1024, PAGES, 1024, None));
    }
}
