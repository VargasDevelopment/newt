//! Streaming verified resume-point restore and prefix-start verification for
//! the idle background producer. MIT.
#![forbid(unsafe_code)]
use crate::{
    producer::{Route, Segments},
    producer_store::{BUFFER_BYTES, ProducerStore},
    producer_writer::CappedWriter,
    resume_capture::{CaptureState, LAYOUT_PROFILE_STANDARD},
    styles::Slot,
};
use newt_core::{
    cache_names::segment::{ChapterSegment, PAGES},
    chapter_layout::{
        engine::EngineState,
        session::{ChapterSession, OperationHandler},
        types::{Page, Sink, SplitBackend},
    },
    member_check::Checked,
    page_cache::{
        CacheStorage, Entry, MAX_PAGE_BYTES, PointRange, VERSION_PAGES, VERSION_POINT,
        verified_point_owner, verified_v4_page_extent,
    },
    page_envelope::{self, EnvelopeIo, Query},
    producer_policy::{ProducerBookSource, ProducerCheck, ProducerReceipt},
    resume_point::{
        BINDING_BYTES, Binding, DEPTH_MAX, HEADER_BYTES, Layout, MAX_POINT_BYTES, Mode, NAMES_MAX,
        PROLOG_MAX, PointIn, REPLAY_MAX, SOURCE_BYTES, Section, TRAILER_BYTES, WINDOW_PAGES,
        decode_source, validate_utf8_prolog, validate_xml_name,
    },
    xml_session::{Attributes, Handler, PARSE_CHUNK, ParseObservation, ProducerBackend, SessionPolicy},
    zip_container::Status,
    zip_window::source::BookSource,
};
use newt_reader_atlas::ReaderAtlas;

/// Outcome of verifying on-card segment headers to certify missing `NTSM` slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CollectStarts {
    /// Every segment `0..count` is certified and monotone.
    Certified,
    /// Duty repeatedly reclaimed the read descriptor before a slot could finish;
    /// retryable without latching terminal failure.
    Suspended,
    /// Storage supports envelope reads, and an on-card segment failed verification
    /// or monotonicity: withhold map publication and never fall back to byte 0.
    Defect,
    /// Storage does not implement envelope reads (in-memory test stub only).
    Unsupported,
}

/// Local candidate rejection vs real attempt-ending condition (cancellation,
/// duty failure, stale source/receipt, or grant/target write error).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RestoreError {
    LocalReject,
    Stop(Status),
}

/// Verified point metadata produced by Pass A under one open generation-bound lease.
pub(crate) struct VerifiedPoint {
    pub(crate) range: PointRange,
    pub(crate) point_owner: u32,
    pub(crate) header: [u8; HEADER_BYTES],
    pub(crate) layout: Layout,
    pub(crate) crc: u32,
}

/// Expected route, layout, member and candidate bounds checked against `Binding`.
pub(crate) struct ExpectedPoint<'a> {
    pub(crate) digest: &'a [u8; 32],
    pub(crate) spine: u32,
    pub(crate) generation: u32,
    pub(crate) candidate_seg: u32,
    pub(crate) target_first: u32,
    pub(crate) page_count: u32,
    pub(crate) total_chars: u32,
    pub(crate) slot: &'a Slot<'a>,
    pub(crate) entry: &'a Checked,
    pub(crate) source_bytes: u64,
}

/// Prefix facts handed to the post-restore replay step once Pass B finishes.
pub(crate) struct LoadedPrefix {
    pub(crate) parser_count: u64,
    pub(crate) replay_len: usize,
    pub(crate) prolog_len: usize,
}

/// Short adapter pairing the outer `ProducerStore` (so `pause_point` reaches
/// the real duty hook) with source freshness validation when guarded.
pub(crate) struct GuardedLease<'op, 'pass, 'loan, 'window, 'ctx, 'pause, C: ?Sized, S: CacheStorage + ?Sized, F: BookSource + ProducerReceipt + ?Sized> {
    store: &'op mut ProducerStore<'pass, 'window, 'ctx, 'pause, C, S>,
    source: &'op mut ProducerBookSource<'loan, 'ctx, C, F>,
    source_bytes: u64,
    guarded: bool,
    source_failed: bool,
}

impl<'op, 'pass, 'loan, 'window, 'ctx, 'pause, C: ?Sized, S: CacheStorage + ?Sized, F: BookSource + ProducerReceipt + ?Sized>
    GuardedLease<'op, 'pass, 'loan, 'window, 'ctx, 'pause, C, S, F>
{
    pub(crate) fn new(
        store: &'op mut ProducerStore<'pass, 'window, 'ctx, 'pause, C, S>,
        source: &'op mut ProducerBookSource<'loan, 'ctx, C, F>,
        source_bytes: u64,
        guarded: bool,
    ) -> Self {
        Self {
            store,
            source,
            source_bytes,
            guarded,
            source_failed: false,
        }
    }

    pub(crate) fn source_failed(&self) -> bool {
        self.source_failed
    }

    fn source_live(&mut self) -> bool {
        if !self.guarded {
            return true;
        }
        let ok = self
            .source
            .with_checks(self.source_bytes, false, |check| check(ProducerCheck::Source));
        if !ok {
            self.source_failed = true;
        }
        ok
    }

    pub(crate) fn poll(&mut self) -> bool {
        if !self.store.pause_point() {
            return false;
        }
        self.source_live()
    }

    pub(crate) fn begin(&mut self, name: &[u8], query: &mut Query<'_>) -> bool {
        if !self.poll() {
            return false;
        }
        let ok = self.store.begin_envelope_read(name, query);
        if !ok {
            return false;
        }
        if !self.source_live() {
            let _ = self.store.finish_envelope_read(query);
            return false;
        }
        true
    }

    pub(crate) fn read_raw(&mut self, query: &Query<'_>, offset: u32, out: &mut [u8]) -> bool {
        self.source_live() && self.store.read_envelope_raw(query, offset, out) && self.source_live()
    }

    pub(crate) fn finish(&mut self, query: &mut Query<'_>) -> bool {
        let ok = self.store.finish_envelope_read(query);
        ok && self.source_live()
    }
}

impl<C: ?Sized, S: CacheStorage + ?Sized, F: BookSource + ProducerReceipt + ?Sized> EnvelopeIo
    for GuardedLease<'_, '_, '_, '_, '_, '_, C, S, F>
{
    fn read(&mut self, query: &Query<'_>, offset: u32, out: &mut [u8]) -> bool {
        self.read_raw(query, offset, out)
    }
    fn poll(&mut self) -> bool {
        GuardedLease::poll(self)
    }
}

fn u16_le(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Verify an attempt is still healthy (no cancellation, duty OK, source and
/// receipt live) before treating a defect as a local candidate rejection.
#[inline(never)]
pub(crate) fn verify_attempt_alive<
    C: ?Sized,
    S: CacheStorage + ?Sized,
    F: BookSource + ProducerReceipt + ?Sized,
>(
    writer: &CappedWriter<'_, '_, C>,
    store: &mut ProducerStore<'_, '_, '_, '_, C, S>,
    source: &mut ProducerBookSource<'_, '_, C, F>,
    source_bytes: u64,
    guarded: bool,
) -> Result<(), Status> {
    let cancel = writer.cancellation();
    if cancel.stopped() || !store.pause_point() {
        return Err(Status::IoError);
    }
    if !source.validate_source(source_bytes) {
        return Err(Status::IoError);
    }
    if guarded {
        let live = source.with_checks(source_bytes, false, |check| check(ProducerCheck::Receipt));
        if !live {
            cancel.fail_terminal();
            return Err(Status::Stale);
        }
    }
    Ok(())
}

/// Stream all `count <= 128` index entries through verified 4 KiB envelope blocks.
#[inline(never)]
fn stream_index_entries(
    query: &Query<'_>,
    window: &mut [u8; BUFFER_BYTES],
    index: u32,
    count: u32,
    entries: &mut [Entry; 128],
    io: &mut dyn EnvelopeIo,
) -> bool {
    if count == 0 || count > PAGES {
        return false;
    }
    let index_bytes = count * 8;
    let mut pair = [0u8; 8];
    page_envelope::stream_verified_range(
        query,
        window,
        index,
        index_bytes,
        io,
        &mut |rel, slice| {
            let mut pos = rel as usize;
            for &b in slice {
                pair[pos & 7] = b;
                if (pos & 7) == 7 {
                    let slot = pos >> 3;
                    if slot >= count as usize {
                        return false;
                    }
                    entries[slot] = Entry {
                        offset: u32_le(&pair, 0),
                        chars: u32_le(&pair, 4),
                    };
                }
                pos += 1;
            }
            true
        },
    )
}

/// Pass A over one already-open generation-bound envelope lease using only the
/// idle 1 KiB store window and the idle 128-entry index chunk:
/// 1. Verify manifest + streamed directory CRC.
/// 2. Verify v4 whole segment header, footer, and unique `point_owner`.
/// 3. Stream every 4 KiB block touched by `FIRP`, verifying block CRCs,
///    `Layout::parse`, and full-section CRC-32.
#[inline(never)]
pub(crate) fn pass_a_verify_point(
    query: &mut Query<'_>,
    window: &mut [u8; BUFFER_BYTES],
    entries: &mut [Entry; 128],
    expected_gen: u32,
    expected_total_chars: u32,
    expected_file_pages: u32,
    io: &mut dyn EnvelopeIo,
) -> Option<VerifiedPoint> {
    if !page_envelope::stream_attach(query, window, io) {
        return None;
    }
    let payload = query.payload;
    if payload < 12 + 32 {
        return None;
    }
    let mut hdr = [0u8; 12];
    if !page_envelope::stream_verified_exact(query, window, 0, &mut hdr, io) {
        return None;
    }
    if &hdr[..4] != b"FIBP"
        || u16_le(&hdr, 4) != VERSION_POINT
        || u32_le(&hdr, 8) != expected_gen
    {
        return None;
    }
    let mut ftr = [0u8; 32];
    if !page_envelope::stream_verified_exact(query, window, payload - 32, &mut ftr, io) {
        return None;
    }
    if &ftr[28..32] != b"FIBX" {
        return None;
    }
    let index = u32_le(&ftr, 0);
    let count = u32_le(&ftr, 4);
    let anchor_off = u32_le(&ftr, 8);
    let anchors = u32_le(&ftr, 12);
    let total = u32_le(&ftr, 16);
    let point = PointRange {
        offset: u32_le(&ftr, 20),
        length: u32_le(&ftr, 24),
    };
    if count == 0
        || count > PAGES
        || count != expected_file_pages
        || total != expected_total_chars
        || index < 12
        || point.length < HEADER_BYTES as u32 + TRAILER_BYTES
        || point.length > MAX_POINT_BYTES
        || point.offset.checked_add(point.length).is_none_or(|end| end > index)
        || u64::from(index) + u64::from(count) * 8 != u64::from(anchor_off)
        || u64::from(anchor_off) + u64::from(anchors) * 8 + 32 != u64::from(payload)
    {
        return None;
    }
    if !stream_index_entries(query, window, index, count, entries, io) {
        return None;
    }
    let point_owner = verified_point_owner(index, point, &entries[..count as usize])?;
    let body_len = point.length.checked_sub(4)?;
    let mut header = [0u8; HEADER_BYTES];
    let mut trailer = [0u8; 4];
    let mut crc = u32::MAX;
    let mut seen = 0u32;
    let streamed = page_envelope::stream_verified_range(
        query,
        window,
        point.offset,
        point.length,
        io,
        &mut |rel, slice| {
            if rel != seen {
                return false;
            }
            let end = rel.saturating_add(slice.len() as u32);
            if rel < HEADER_BYTES as u32 {
                let hi = end.min(HEADER_BYTES as u32);
                let n = (hi - rel) as usize;
                header[rel as usize..hi as usize].copy_from_slice(&slice[..n]);
            }
            if rel < body_len {
                let hi = end.min(body_len);
                crc = page_envelope::crc(crc, &slice[..(hi - rel) as usize]);
            }
            if end > body_len {
                let lo = rel.max(body_len);
                let s_off = (lo - rel) as usize;
                let t_off = (lo - body_len) as usize;
                let n = (end - lo) as usize;
                trailer[t_off..t_off + n].copy_from_slice(&slice[s_off..s_off + n]);
            }
            seen = end;
            true
        },
    );
    if !streamed || seen != point.length {
        return None;
    }
    let layout = Layout::parse(&header, point.length)?;
    let expected_crc = u32::from_le_bytes(trailer);
    if crc ^ u32::MAX != expected_crc {
        return None;
    }
    Some(VerifiedPoint {
        range: point,
        point_owner,
        header,
        layout,
        crc: expected_crc,
    })
}

/// Pass B stage 1: decode and validate the header, Section 0 (`Binding`) and
/// Section 1 (`Source`) BEFORE constructing any parser or layout engine state.
#[inline(never)]
pub(crate) fn pass_b_binding_and_source(
    point_in: &mut PointIn<'_>,
    verified: &VerifiedPoint,
    expected: &ExpectedPoint<'_>,
) -> Option<(Binding, u32)> {
    let mut hdr = [0u8; HEADER_BYTES];
    if !point_in.read_body(&mut hdr) || hdr != verified.header {
        return None;
    }
    let (_, binding_len) = verified.layout.range(Section::Binding);
    if binding_len != BINDING_BYTES as u32 {
        return None;
    }
    let binding = Binding::decode(&mut point_in.section(binding_len))?;
    let expected_seg_idx = expected.candidate_seg.checked_add(1)?;
    let edge = binding.segment_index.checked_mul(PAGES)?;
    let expected_pages_at = expected
        .candidate_seg
        .checked_mul(PAGES)?
        .checked_add(verified.point_owner)?
        .checked_add(1)?;
    let replay_len = binding.fed.checked_sub(binding.cut)?;
    let (_, expected_replay_len) = verified.layout.range(Section::Replay);
    let (_, prolog_len) = verified.layout.range(Section::Prolog);
    let mode_ok = matches!(
        (verified.layout.mode, binding.member_method),
        (Mode::Seek, 0) | (Mode::Reinflate, 8)
    );
    if !mode_ok
        || binding.book_digest != *expected.digest
        || binding.spine != expected.spine
        || binding.generation != expected.generation
        || binding.segment_index != expected_seg_idx
        || binding.page_count != expected.page_count
        || binding.total_chars != expected.total_chars
        || binding.style_hash != expected.slot.hash
        || usize::from(binding.style_count) != expected.slot.count
        || binding.style_issues != expected.slot.issues
        || binding.style_state != expected.slot.state as u8
        || binding.profile != LAYOUT_PROFILE_STANDARD
        || binding.member_method != expected.entry.method
        || binding.member_offset != expected.entry.local_header_offset
        || binding.member_compressed != expected.entry.compressed_size
        || binding.member_uncompressed != expected.entry.uncompressed_size
        || binding.source_bytes != expected.source_bytes
        || edge == 0
        || edge >= expected.page_count
        || edge > expected.target_first
        || binding.pages_at != expected_pages_at
        || binding.pages_at >= edge
        || edge - binding.pages_at > WINDOW_PAGES
        || binding.pages_at >= expected.target_first
        || replay_len > REPLAY_MAX as u64
        || replay_len != u64::from(expected_replay_len)
        || binding.cut < u64::from(prolog_len)
    {
        return None;
    }
    let (_, source_len) = verified.layout.range(Section::Source);
    if source_len != SOURCE_BYTES as u32 {
        return None;
    }
    let source_consumed = decode_source(&mut point_in.section(source_len))?;
    if source_consumed > expected.entry.uncompressed_size
        || binding.fed > u64::from(source_consumed).saturating_mul(5)
    {
        return None;
    }
    Some((binding, source_consumed))
}

struct MutedProlog {
    saw_element: bool,
}
impl Handler for MutedProlog {
    fn start_element(&mut self, _: &[u8], _: &mut dyn Attributes) {
        self.saw_element = true;
    }
    fn end_element(&mut self, _: &[u8]) {
        self.saw_element = true;
    }
    fn stop_parse(&self) -> bool {
        self.saw_element
    }
}

struct MutedTag {
    starts: u8,
    bad: bool,
}
impl Handler for MutedTag {
    fn start_element(&mut self, _: &[u8], _: &mut dyn Attributes) {
        self.starts = self.starts.saturating_add(1);
    }
    fn end_element(&mut self, _: &[u8]) {
        self.bad = true;
    }
    fn text(&mut self, _: &[u8]) {
        self.bad = true;
    }
    fn stop_parse(&self) -> bool {
        self.bad || self.starts > 1
    }
}

/// Pass B stage 2: stream Sections 2..6 (`Stream`, `Engine`, `Prolog`, `Names`,
/// `Replay`) directly into the live session, engine, tap and XML feed buffer,
/// then verify the Pass B CRC-32 against Pass A before returning.
#[inline(never)]
pub(crate) fn pass_b_load_rest<
    S: BookSource + ?Sized,
    B: ProducerBackend + ?Sized,
    P: SessionPolicy<B>,
>(
    mut point_in: PointIn<'_>,
    verified: &VerifiedPoint,
    binding: &Binding,
    source_consumed: u32,
    member_uncompressed: u32,
    session: &mut ChapterSession<'_, S, B, P>,
    engine: &mut EngineState<'_>,
    capture: &mut CaptureState<'_>,
) -> Option<LoadedPrefix> {
    let (_, stream_len) = verified.layout.range(Section::Stream);
    if !session.stream.load_resume(
        &mut point_in.section(stream_len),
        binding.fed,
        source_consumed,
        member_uncompressed,
    ) {
        return None;
    }
    let (_, engine_len) = verified.layout.range(Section::Engine);
    if !engine.load_resume(
        &mut point_in.section(engine_len),
        binding.pages_at,
        binding.total_chars,
    ) {
        return None;
    }
    // Section 4: Prolog (loaded into idle xml_buffer, copied to tap, fed muted).
    let prolog_len = verified.layout.range(Section::Prolog).1 as usize;
    if prolog_len > PROLOG_MAX || prolog_len > PARSE_CHUNK {
        return None;
    }
    capture.reset();
    {
        let mut sec = point_in.section(prolog_len as u32);
        let ok = session.stream.with_buffer_mut(&mut |buf| {
            let p = &mut buf[..prolog_len];
            if !sec.read_exact(p) || !sec.finish() {
                return false;
            }
            if core::str::from_utf8(p).is_err() {
                return false;
            }
            if prolog_len > 0 && validate_utf8_prolog(&p[..prolog_len.min(4)], p).is_err() {
                return false;
            }
            capture.tap.prolog_slice_mut(prolog_len).copy_from_slice(p);
            true
        });
        if !ok {
            return None;
        }
    }
    let mut parser_count = 0u64;
    if prolog_len > 0 {
        let mut muted = MutedProlog { saw_element: false };
        let st = session
            .stream
            .feed_buffered_direct(prolog_len, parser_count, &mut |_, parser, bytes| {
                parser.feed(bytes, false, &mut muted)
            });
        if st != Status::Ok || muted.saw_element {
            return None;
        }
        parser_count += prolog_len as u64;
        let idx = session.stream.with_source(|_, p| p.current_byte_index());
        if idx != parser_count as i64 {
            return None;
        }
    }
    // Section 5: Names (depth 1..=48, validated UTF-8 XML names, fed muted as `<name>`).
    let names_len = verified.layout.range(Section::Names).1;
    let edge = binding.segment_index.checked_mul(PAGES)?;
    {
        let mut sec = point_in.section(names_len);
        let mut depth_byte = [0u8; 1];
        if !sec.read_exact(&mut depth_byte) {
            return None;
        }
        let depth = usize::from(depth_byte[0]);
        if depth == 0
            || depth > DEPTH_MAX
            || engine.open_stack_bound() > depth
        {
            return None;
        }
        let mut used = 0usize;
        let (mut names_depth, mut names_suppress, mut names_style, mut names_body) =
            (0usize, 0usize, 0usize, 0usize);
        for i in 0..depth {
            let mut nlen = [0u8; 1];
            if !sec.read_exact(&mut nlen) {
                return None;
            }
            let n = usize::from(nlen[0]);
            used = used.checked_add(n)?;
            if n == 0 || used > NAMES_MAX || n + 2 > PARSE_CHUNK {
                return None;
            }
            let valid = session.stream.with_buffer_mut(&mut |buf| {
                if !sec.read_exact(&mut buf[1..1 + n]) {
                    return false;
                }
                let name = &buf[1..1 + n];
                if !validate_xml_name(name) {
                    return false;
                }
                EngineState::count_open_tag(
                    name,
                    &mut names_depth,
                    &mut names_suppress,
                    &mut names_style,
                    &mut names_body,
                );
                if i == 0 && !capture.tap.finish_restore_prolog(prolog_len, name, edge) {
                    return false;
                }
                if !capture.tap.push_restored(name) {
                    return false;
                }
                buf[0] = b'<';
                buf[1 + n] = b'>';
                true
            });
            if !valid {
                return None;
            }
            let mut tag = MutedTag { starts: 0, bad: false };
            let st = session
                .stream
                .feed_buffered_direct(n + 2, parser_count, &mut |_, parser, bytes| {
                    parser.feed(bytes, false, &mut tag)
                });
            if st != Status::Ok || tag.starts != 1 || tag.bad {
                return None;
            }
            parser_count += (n + 2) as u64;
            let idx = session.stream.with_source(|_, p| p.current_byte_index());
            if idx != parser_count as i64 {
                return None;
            }
        }
        if !sec.finish()
            || !engine.matches_open_names(names_depth, names_suppress, names_style, names_body)
        {
            return None;
        }
    }
    // Section 6: Replay (loaded directly into xml_buffer, NOT fed until Pass B CRC passes).
    let replay_len = verified.layout.range(Section::Replay).1 as usize;
    if replay_len > REPLAY_MAX || replay_len > PARSE_CHUNK {
        return None;
    }
    {
        let mut sec = point_in.section(replay_len as u32);
        let ok = session
            .stream
            .with_buffer_mut(&mut |buf| sec.read_exact(&mut buf[..replay_len]) && sec.finish());
        if !ok {
            return None;
        }
    }
    // Re-verify CRC-32 over EVERY byte Pass B actually consumed.
    if !point_in.finish_crc(verified.crc) {
        return None;
    }
    let idx = session.stream.with_source(|_, p| p.current_byte_index());
    if idx != parser_count as i64 {
        return None;
    }
    Some(LoadedPrefix {
        parser_count,
        replay_len,
        prolog_len,
    })
}

/// Dry-run sink used while feeding Section 6 (`Replay`) BEFORE `begin_write`
/// opens the target temp. Rejects locally if a forged replay attempts to cross
/// the point's own edge or emit an anchor for the target segment, without ever
/// spending a census entry grant or latching terminal failure.
struct ReplaySink<'a, 'ctx, C: ?Sized> {
    seen: &'a mut u32,
    edge: u32,
    target_first: u32,
    cancellation: &'a newt_core::producer_policy::Cancellation<'ctx, C>,
    rejected: bool,
}

impl<C: ?Sized> Sink for ReplaySink<'_, '_, C> {
    fn page(&mut self, _: Page<'_>) -> bool {
        if !self.cancellation.checkpoint() {
            return false;
        }
        let next = self.seen.wrapping_add(1);
        if *self.seen >= self.edge || next >= self.edge {
            self.rejected = true;
            return false;
        }
        *self.seen = next;
        true
    }
    fn anchor(&mut self, _: u32, _: u32) {
        if *self.seen >= self.target_first {
            self.rejected = true;
        }
    }
}

/// Feed the already-loaded `Replay` bytes from `xml_buffer[..prefix.replay_len]`
/// with the real layout handler and tap (before `begin_write`), then install
/// the parser-to-stream shift translation and verify the post-replay index.
#[inline(never)]
pub(crate) fn feed_replay_and_arm_shift<
    S: BookSource + ?Sized,
    B: ProducerBackend + ?Sized,
    P: SessionPolicy<B>,
    C: ?Sized,
>(
    session: &mut ChapterSession<'_, S, B, P>,
    engine: &mut EngineState<'_>,
    capture: &mut CaptureState<'_>,
    writer: &mut CappedWriter<'_, '_, C>,
    binding: &Binding,
    prefix: &LoadedPrefix,
    target_first: u32,
) -> bool {
    let Some(edge) = binding.segment_index.checked_mul(PAGES) else {
        return false;
    };
    writer.seen = binding.pages_at;
    capture.obs = ParseObservation {
        root_index: prefix.prolog_len as i64,
        cdata_depth: 0,
    };
    if prefix.replay_len > 0 {
        let cancel = writer.cancellation();
        let mut sink = ReplaySink {
            seen: &mut writer.seen,
            edge,
            target_first,
            cancellation: cancel,
            rejected: false,
        };
        let st = session.stream.feed_buffered_direct(
            prefix.replay_len,
            prefix.parser_count,
            &mut |_, parser, bytes| {
                let mut font = ReaderAtlas;
                let mut backend = SplitBackend {
                    font: &mut font,
                    sink: &mut sink,
                };
                let mut op = OperationHandler {
                    engine,
                    backend: &mut backend,
                };
                let mut tap = capture.tap.wrap(&mut op);
                match parser.feed_observed(bytes, false, &mut tap, &mut capture.obs) {
                    Some(ok) => ok,
                    None => parser.feed(bytes, false, &mut tap),
                }
            },
        );
        if st != Status::Ok || sink.rejected || engine.failed || engine.stopped {
            return false;
        }
    }
    if engine.pages != writer.seen
        || writer.seen >= edge
        || capture.tap.depth() == 0
        || capture.obs.cdata_depth != 0
        || binding.cut < prefix.parser_count
    {
        return false;
    }
    let Some(shift) = binding
        .cut
        .checked_sub(prefix.parser_count)
        .and_then(|d| i64::try_from(d).ok())
    else {
        return false;
    };
    session.stream.set_restored(shift, prefix.replay_len);
    let post = session.stream.with_source(|_, p| p.current_byte_index());
    post >= 0
        && post
            .checked_add(shift)
            .and_then(|v| u64::try_from(v).ok())
            .is_some_and(|c| c <= binding.fed)
}

/// Read and verify page 0's `char_start` from an existing v3 or v4 segment file
/// through the 1 KiB verified envelope reader so missing `NTSM` map starts are
/// certified from verified on-card headers instead of unverified zeros.
#[inline(never)]
fn read_verified_segment_start(
    query: &mut Query<'_>,
    window: &mut [u8; BUFFER_BYTES],
    entries: &mut [Entry; 128],
    expected_gen: u32,
    expected_total_chars: u32,
    expected_file_pages: u32,
    io: &mut dyn EnvelopeIo,
) -> Option<u32> {
    if !page_envelope::stream_attach(query, window, io) {
        return None;
    }
    let payload = query.payload;
    let mut hdr = [0u8; 12];
    if !page_envelope::stream_verified_exact(query, window, 0, &mut hdr, io) {
        return None;
    }
    let version = u16_le(&hdr, 4);
    if &hdr[..4] != b"FIBP"
        || !matches!(version, VERSION_PAGES | VERSION_POINT)
        || u32_le(&hdr, 8) != expected_gen
    {
        return None;
    }
    let fs: u32 = if version == VERSION_POINT { 32 } else { 24 };
    if payload < 12 + fs {
        return None;
    }
    let mut ftr = [0u8; 32];
    if !page_envelope::stream_verified_exact(
        query,
        window,
        payload - fs,
        &mut ftr[..fs as usize],
        io,
    ) {
        return None;
    }
    if &ftr[fs as usize - 4..fs as usize] != b"FIBX" {
        return None;
    }
    let index = u32_le(&ftr, 0);
    let count = u32_le(&ftr, 4);
    let anchor_off = u32_le(&ftr, 8);
    let anchors = u32_le(&ftr, 12);
    let total = u32_le(&ftr, 16);
    if count == 0
        || count > PAGES
        || count != expected_file_pages
        || total != expected_total_chars
        || index < 12
        || u64::from(index) + u64::from(count) * 8 != u64::from(anchor_off)
        || u64::from(anchor_off) + u64::from(anchors) * 8 + u64::from(fs) != u64::from(payload)
    {
        return None;
    }
    let (page0_off, page0_end, page0_chars) = if version == VERSION_POINT {
        if !stream_index_entries(query, window, index, count, entries, io) {
            return None;
        }
        let point = PointRange {
            offset: u32_le(&ftr, 20),
            length: u32_le(&ftr, 24),
        };
        let (start, end) = verified_v4_page_extent(index, point, &entries[..count as usize], 0)?;
        (start, end, entries[0].chars)
    } else {
        let mut entries_buf = [0u8; 16];
        let entry_bytes = if count > 1 { 16 } else { 8 };
        if !page_envelope::stream_verified_exact(
            query,
            window,
            index,
            &mut entries_buf[..entry_bytes],
            io,
        ) {
            return None;
        }
        let start = u32_le(&entries_buf, 0);
        let chars = u32_le(&entries_buf, 4);
        let end = if count > 1 { u32_le(&entries_buf, 8) } else { index };
        (start, end, chars)
    };
    if page0_off < 12
        || page0_end > index
        || page0_off.checked_add(10).is_none_or(|end| end > page0_end)
        || page0_end - page0_off > MAX_PAGE_BYTES
        || page0_chars > expected_total_chars
    {
        return None;
    }
    let exact = version == VERSION_POINT;
    let rec_len = page0_end - page0_off;
    verify_page_record_stream(query, window, page0_off, rec_len, page0_chars, exact, io)
        .then_some(page0_chars)
}

/// Stream-verify a page record `[page_off, page_off + rec_len)` against the exact
/// structural layout enforced by `PageCacheReader::read_page` (`decode_page`):
/// 10-byte header with matching `expected_chars`, every run/image/link header and
/// payload fitting inside `rec_len`, and exact length match for v4 (`exact == true`).
#[inline(never)]
fn verify_page_record_stream(
    query: &Query<'_>,
    window: &mut [u8; BUFFER_BYTES],
    page_off: u32,
    rec_len: u32,
    expected_chars: u32,
    exact: bool,
    io: &mut dyn EnvelopeIo,
) -> bool {
    let mut hdr = [0u8; 12];
    let mut hdr_fill = 0usize;
    let mut hdr_need = 10usize;
    let mut skip = 0u32;
    let mut phase = 0u8; // 0=page hdr, 1=runs, 2=images, 3=links, 4=done
    let mut rem_runs = 0u16;
    let mut rem_imgs = 0u16;
    let mut rem_links = 0u16;
    let mut consumed = 0u32;

    let advance = |phase: &mut u8, hdr_need: &mut usize, runs: u16, imgs: u16, links: u16| {
        if runs > 0 {
            *phase = 1;
            *hdr_need = 10;
        } else if imgs > 0 {
            *phase = 2;
            *hdr_need = 10;
        } else if links > 0 {
            *phase = 3;
            *hdr_need = 12;
        } else {
            *phase = 4;
            *hdr_need = 0;
        }
    };

    let streamed = page_envelope::stream_verified_range(
        query,
        window,
        page_off,
        rec_len,
        io,
        &mut |rel, slice| {
            if rel != consumed {
                return false;
            }
            let mut at = 0usize;
            while at < slice.len() {
                let avail = slice.len() - at;
                if skip > 0 {
                    let take = avail.min(skip as usize);
                    skip -= take as u32;
                    at += take;
                    consumed += take as u32;
                    continue;
                }
                if phase == 4 {
                    if exact {
                        return false;
                    }
                    consumed += avail as u32;
                    return true;
                }
                let take = avail.min(hdr_need - hdr_fill);
                hdr[hdr_fill..hdr_fill + take].copy_from_slice(&slice[at..at + take]);
                hdr_fill += take;
                at += take;
                consumed += take as u32;
                if hdr_fill < hdr_need {
                    continue;
                }
                hdr_fill = 0;
                match phase {
                    0 => {
                        if u32_le(&hdr, 0) != expected_chars {
                            return false;
                        }
                        rem_runs = u16_le(&hdr, 4);
                        rem_imgs = u16_le(&hdr, 6);
                        rem_links = u16_le(&hdr, 8);
                        let min_bytes = 10u64
                            + u64::from(rem_runs) * 10
                            + u64::from(rem_imgs) * 10
                            + u64::from(rem_links) * 12;
                        if min_bytes > u64::from(rec_len) {
                            return false;
                        }
                        advance(&mut phase, &mut hdr_need, rem_runs, rem_imgs, rem_links);
                    }
                    1 => {
                        skip = u32::from(u16_le(&hdr, 8));
                        rem_runs -= 1;
                        if rem_runs == 0 {
                            advance(&mut phase, &mut hdr_need, 0, rem_imgs, rem_links);
                        }
                    }
                    2 => {
                        skip = u32::from(u16_le(&hdr, 8));
                        rem_imgs -= 1;
                        if rem_imgs == 0 {
                            advance(&mut phase, &mut hdr_need, 0, 0, rem_links);
                        }
                    }
                    3 => {
                        skip = u32::from(u16_le(&hdr, 8)) + u32::from(u16_le(&hdr, 10));
                        rem_links -= 1;
                        if rem_links == 0 {
                            advance(&mut phase, &mut hdr_need, 0, 0, 0);
                        }
                    }
                    _ => return false,
                }
            }
            true
        },
    );
    streamed && phase == 4 && skip == 0 && hdr_fill == 0 && consumed == rec_len
}

/// Populate every unrecorded slot in `writer.starts[0..state.count]` from
/// verified on-card segment files using the idle 1 KiB store window and idle
/// 128-entry index buffer.
/// Returns `Ok(CollectStarts::Certified)` iff all `0..state.count` starts are
/// certified and monotone, `Ok(CollectStarts::Suspended)` if duty reclaimed the
/// read descriptor repeatedly, `Ok(CollectStarts::Defect)` on any on-card defect,
/// `Ok(CollectStarts::Unsupported)` when the storage does not implement envelope
/// reads, or `Err(status)` on cancellation, duty failure, or stale source/receipt.
#[inline(never)]
pub(crate) fn collect_missing_starts<
    'ctx,
    C: ?Sized,
    S: CacheStorage + ?Sized,
    F: BookSource + ProducerReceipt + ?Sized,
>(
    state: &mut Segments,
    writer: &mut CappedWriter<'_, 'ctx, C>,
    store: &mut ProducerStore<'_, '_, 'ctx, '_, C, S>,
    source: &mut ProducerBookSource<'_, 'ctx, C, F>,
    window: &mut [u8; BUFFER_BYTES],
    entries: &mut [Entry; 128],
    route: &mut Route<'_>,
    source_bytes: u64,
    guarded: bool,
) -> Result<CollectStarts, Status> {
    if store.buffered_bytes() != 0 {
        return Err(Status::Unsupported);
    }
    if !store.supports_envelope_read() {
        return Ok(CollectStarts::Unsupported);
    }
    for idx in 0..state.count {
        if (writer.starts_mask & (1u32 << idx)) != 0 {
            continue;
        }
        verify_attempt_alive(writer, store, source, source_bytes, guarded)?;
        if !state.name_segment(route, idx) {
            return Err(Status::IoError);
        }
        let seg = ChapterSegment {
            index: idx,
            pages: route.info.page_count,
            chars: route.info.total_chars,
        };
        let seg_name = state.name;
        let expected_gen = state.generation;
        let mut certified_chars = None;
        let mut suspended_exhausted = false;
        for attempt in 0..8u8 {
            let mut query = Query::new(Some(&seg_name[..69]));
            let mut lease = GuardedLease::new(store, source, source_bytes, guarded);
            let start_val = if lease.begin(&seg_name[..69], &mut query) {
                let got = read_verified_segment_start(
                    &mut query,
                    window,
                    entries,
                    expected_gen,
                    route.info.total_chars,
                    seg.file_pages(),
                    &mut lease,
                );
                let fresh = lease.finish(&mut query);
                got.filter(|_| fresh)
            } else {
                None
            };
            let source_failed = lease.source_failed();
            drop(lease);
            if source_failed {
                return Err(Status::IoError);
            }
            verify_attempt_alive(writer, store, source, source_bytes, guarded)?;
            if let Some(chars) = start_val {
                certified_chars = Some(chars);
                break;
            }
            if store.envelope_read_suspended() {
                if attempt + 1 == 8 {
                    suspended_exhausted = true;
                }
                continue;
            }
            return Ok(CollectStarts::Defect);
        }
        if suspended_exhausted {
            return Ok(CollectStarts::Suspended);
        }
        let Some(chars) = certified_chars else {
            return Ok(CollectStarts::Defect);
        };
        writer.record_start(idx as usize, chars);
    }
    if !writer.starts_complete(state.count) {
        return Ok(CollectStarts::Defect);
    }
    // Match `segment::find` monotonicity & upper bound before certifying the map.
    for i in 0..state.count as usize {
        if writer.starts[i] > route.info.total_chars
            || (i > 0 && writer.starts[i] < writer.starts[i - 1])
        {
            writer.starts_mask = 0;
            return Ok(CollectStarts::Defect);
        }
    }
    Ok(CollectStarts::Certified)
}
