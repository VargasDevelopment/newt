// Derived from pinned FreeInk XmlSax.cpp/h (MIT); see rust/THIRD_PARTY.md.
#![forbid(unsafe_code)]

use crate::member_check::Checked;
use crate::{
    xml_input::EntityFilter,
    zip_container::{InflateScratch, Reader, Status},
    zip_window::source::BookSource,
};

pub const PARSE_CHUNK: usize = 2048;

/// A callback-local view. Names and values must not outlive the callback.
pub trait Attributes {
    fn next(&mut self) -> Option<(&[u8], &[u8])>;
    fn local(&self, name: &[u8]) -> Option<&[u8]>;
    fn exact(&self, name: &[u8]) -> Option<&[u8]>;
}
pub trait Handler {
    fn start_element(&mut self, _name: &[u8], _attributes: &mut dyn Attributes) {}
    fn end_element(&mut self, _name: &[u8]) {}
    fn text(&mut self, _text: &[u8]) {}
    fn stop_parse(&self) -> bool {
        false
    }
}

/// A borrowed synchronous backend. No callback may escape `feed` or retain the
/// handler. `create` starts a fresh parser or returns false without a live parser.
/// A true create must be matched by exactly one close, even after parse errors.
/// This trait is an interface, not a claim that an arbitrary backend parses XML.
pub trait ParserBackend {
    fn create(&mut self) -> bool;
    fn feed(&mut self, bytes: &[u8], final_chunk: bool, handler: &mut dyn Handler) -> bool;
    fn close(&mut self);
    /// Optional bounded-parser capability; ordinary backends need none.
    fn producer_backend(&mut self) -> Option<&mut dyn ProducerBackend> { None }
}

/// Typed caller resources replace byte-arena availability, not arena accounting.
/// Decoder scratch belongs to `Reader`; parser storage belongs to the backend.
pub struct Resources<'a> {
    pub buffer: Option<&'a mut [u8; PARSE_CHUNK]>,
    pub filter: Option<&'a mut EntityFilter>,
}

/// Per-parser capture observations updated synchronously inside `feed_observed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseObservation {
    /// Byte index of the root `start_element`, or -1 before the root starts.
    pub root_index: i64,
    /// Active CDATA section depth (> 0 means inside CDATA at the end of a feed).
    pub cdata_depth: u32,
}
impl Default for ParseObservation {
    fn default() -> Self {
        Self::new()
    }
}
impl ParseObservation {
    pub const fn new() -> Self {
        Self { root_index: -1, cdata_depth: 0 }
    }
}

/// Producer backends must expose Expat-compatible byte-index semantics.
/// A negative index means no consumed input; the index is sampled BEFORE feed.
pub trait ProducerBackend: ParserBackend {
    fn create_producer(&mut self) -> bool;
    fn current_byte_index(&self) -> i64;
    /// Like `feed`, also recording root byte index and CDATA depth into
    /// `observation`. `None` means this backend does not support capture.
    fn feed_observed(
        &mut self,
        _bytes: &[u8],
        _final_chunk: bool,
        _handler: &mut dyn Handler,
        _observation: &mut ParseObservation,
    ) -> Option<bool> {
        None
    }
}

/// Borrowed for an entire synchronous attempt. No ownership or heap indirection.
/// XmlSax's authenticated patch never calls result(); settlement belongs upstream.
pub trait ProcessingPolicy {
    fn cancelled(&mut self) -> bool;
}

/// Shared read/EOF policy. Ordinary source adapters hold None; producer
/// adapters borrow the same attempt latch as the XML session.
pub trait SourcePolicy {
    fn cancelled(&self) -> bool;
    fn result(&self, ok: bool) -> bool;
}

pub struct NoPolicy;
pub struct BorrowedPolicy<'p>(pub &'p mut dyn ProcessingPolicy);

pub trait SessionPolicy<B: ParserBackend + ?Sized> {
    fn cancelled(&mut self) -> bool {
        false
    }
    fn create(&mut self, backend: &mut B) -> bool {
        backend.create()
    }
    fn permits_feed(&self, _backend: &mut B, _count: u64, _next: u64) -> bool {
        true
    }
}
impl<B: ParserBackend + ?Sized> SessionPolicy<B> for NoPolicy {}
impl<B: ProducerBackend + ?Sized> SessionPolicy<B> for BorrowedPolicy<'_> {
    fn cancelled(&mut self) -> bool {
        self.0.cancelled()
    }
    fn create(&mut self, backend: &mut B) -> bool {
        backend.create_producer()
    }
    fn permits_feed(&self, backend: &mut B, count: u64, next: u64) -> bool {
        permits_buffered_feed(backend.current_byte_index(), count, next)
    }
}

fn permits_buffered_feed(index: i64, count: u64, next: u64) -> bool {
    let consumed = index.max(0) as u64;
    consumed <= count && count - consumed <= 4096 && next <= 4096 - (count - consumed)
}

/// One metadata/style implementation handles ordinary and producer calls.
/// The shared source policy is the same attempt latch as the XML policy.
pub struct OptionalPolicy<'p>(pub Option<&'p dyn SourcePolicy>);
impl<B: ParserBackend + ?Sized> SessionPolicy<B> for OptionalPolicy<'_> {
    fn cancelled(&mut self) -> bool {
        self.0.is_some_and(SourcePolicy::cancelled)
    }
    fn create(&mut self, backend: &mut B) -> bool {
        if self.0.is_none() { return backend.create(); }
        backend.producer_backend().is_some_and(ProducerBackend::create_producer)
    }
    fn permits_feed(&self, backend: &mut B, count: u64, next: u64) -> bool {
        if self.0.is_none() { return true; }
        backend.producer_backend().is_some_and(|producer|
            permits_buffered_feed(producer.current_byte_index(), count, next))
    }
}

/// Read-only view of an `XmlStream`'s counters, last feed and entity filter,
/// available alongside a mutable source/backend loan without copying buffers.
pub struct StreamView<'a> {
    pub open: bool,
    pub finished: bool,
    pub bytes_consumed: u64,
    pub reader_produced: u32,
    pub shift: i64,
    pub last_feed: &'a [u8],
    pub filter: Option<&'a EntityFilter>,
}
impl StreamView<'_> {
    /// Emits Section 2 (`Stream`): `"XST1"`, filtered counter, raw counter and entity filter.
    pub fn save_resume(&self, out: &mut dyn crate::resume_point::PointOut) -> bool {
        if !self.open || self.finished {
            return false;
        }
        let mut h = [0u8; 17];
        h[..4].copy_from_slice(b"XST1");
        h[4..12].copy_from_slice(&self.bytes_consumed.to_le_bytes());
        h[12..16].copy_from_slice(&self.reader_produced.to_le_bytes());
        h[16] = u8::from(self.filter.is_some());
        if !out.put(&h) {
            return false;
        }
        match self.filter {
            Some(filter) => filter.save_resume(out),
            None => true,
        }
    }
}

pub struct XmlStream<
    'a,
    S: BookSource + ?Sized,
    B: ParserBackend + ?Sized,
    P: SessionPolicy<B> = NoPolicy,
> {
    policy: P,
    reader: Reader<'a, S>,
    backend: &'a mut B,
    buffer: Option<&'a mut [u8; PARSE_CHUNK]>,
    filter: Option<&'a mut EntityFilter>,
    open: bool,
    handler_attached: bool,
    bytes_consumed: u64,
    finished: bool,
    last_n: usize,
    /// Original filtered stream offset minus parser byte offset after restore;
    /// zero for an ordinary byte-0 stream.
    shift: i64,
}
impl<'a, S: BookSource + ?Sized, B: ParserBackend + ?Sized> XmlStream<'a, S, B> {
    pub fn new(source: &'a mut S, decoder: Option<InflateScratch<'a>>, backend: &'a mut B) -> Self {
        Self::with_policy(source, decoder, backend, NoPolicy)
    }
}
impl<'a, S: BookSource + ?Sized, B: ParserBackend + ?Sized, P: SessionPolicy<B>>
    XmlStream<'a, S, B, P>
{
    pub fn with_policy(
        source: &'a mut S,
        decoder: Option<InflateScratch<'a>>,
        backend: &'a mut B,
        policy: P,
    ) -> Self {
        Self {
            policy,
            reader: Reader::new(source, decoder),
            backend,
            buffer: None,
            filter: None,
            open: false,
            handler_attached: false,
            bytes_consumed: 0,
            finished: false,
            last_n: 0,
            shift: 0,
        }
    }
    pub fn open(
        &mut self,
        entry: &'a Checked,
        resources: Resources<'a>,
        filter_html_entities: bool,
    ) -> Status {
        self.close();
        // Producer open poll precedes progress reset.
        if self.policy.cancelled() {
            return Status::IoError;
        }
        self.handler_attached = true;
        self.bytes_consumed = 0;
        self.finished = false;
        self.last_n = 0;
        self.shift = 0;
        let status = self.reader.open(entry);
        if status != Status::Ok {
            return status;
        }
        self.buffer = resources.buffer;
        self.filter = if filter_html_entities {
            resources.filter
        } else {
            None
        };
        // SDK attempts/reset filter even when parse-buffer allocation failed.
        if let Some(filter) = self.filter.as_mut() {
            filter.reset();
        }
        if self.buffer.is_none() || (filter_html_entities && self.filter.is_none()) {
            return Status::OutOfMemory;
        }
        if !self.policy.create(self.backend) {
            return Status::OutOfMemory;
        }
        self.open = true;
        Status::Ok
    }
    /// Short source/backend operation between XML reads, also used for layout finish.
    pub fn with_source<R>(&mut self, operation: impl FnOnce(&mut S, &mut B) -> R) -> R {
        self.reader.with_source(|source| operation(source, self.backend))
    }
    /// Short source/backend operation alongside a disjoint read-only stream view.
    pub fn with_source_and_view<R>(
        &mut self,
        operation: impl FnOnce(&mut S, &mut B, StreamView<'_>) -> R,
    ) -> R {
        let view = StreamView {
            open: self.open,
            finished: self.finished,
            bytes_consumed: self.bytes_consumed,
            reader_produced: self.reader.total_produced(),
            shift: self.shift,
            last_feed: self
                .buffer
                .as_deref()
                .map_or(&[], |b| &b[..self.last_n.min(PARSE_CHUNK)]),
            filter: self.filter.as_deref(),
        };
        let backend = &mut *self.backend;
        self.reader
            .with_source(|source| operation(source, backend, view))
    }
    pub fn feed_chunk(
        &mut self,
        handler: &mut dyn Handler,
        at_end: Option<&mut bool>,
    ) -> Status {
        self.feed_chunk_with(at_end, |_, backend, bytes, last| {
            backend.feed(bytes, last, handler)
        })
    }
    /// After input/filter reads finish, lend the source for synchronous SAX work.
    /// The callback must feed this backend exactly once and return its result.
    /// No source reference can escape in the bool result. Policy/error ordering
    /// and byte accounting are shared with the ordinary feed path.
    pub fn feed_chunk_with(
        &mut self,
        mut at_end: Option<&mut bool>,
        feed: impl FnOnce(&mut S, &mut B, &[u8], bool) -> bool,
    ) -> Status {
        if let Some(end) = at_end.as_deref_mut() {
            *end = self.finished;
        }
        if !self.open || self.finished {
            return Status::Unsupported;
        }
        // Producer pre-read poll.
        if self.policy.cancelled() {
            return Status::IoError;
        }
        // Invariants established by successful open; no stop check in stepped mode.
        let buffer = self.buffer.as_mut().expect("open parse buffer");
        let n = if let Some(filter) = self.filter.as_mut() {
            filter.read(&mut self.reader, buffer.as_mut_slice())
        } else {
            self.reader.read(buffer.as_mut_slice())
        };
        if n < 0 {
            return Status::IoError;
        }
        // Producer post-read poll follows I/O failure.
        if self.policy.cancelled() {
            return Status::IoError;
        }
        // Translate stream offset into parser coordinates (identity when shift == 0).
        let Some(count) = i64::try_from(self.bytes_consumed)
            .ok()
            .and_then(|c| c.checked_sub(self.shift))
            .and_then(|c| u64::try_from(c).ok())
        else {
            return Status::Unsupported;
        };
        if !self.policy.permits_feed(self.backend, count, n as u64) {
            return Status::Unsupported;
        }
        let parsed = self.reader.with_source(|source| {
            feed(source, self.backend, &buffer[..n as usize], n == 0)
        });
        if !parsed {
            return Status::ParseError;
        }
        // Producer post-parse poll follows parse failure.
        if self.policy.cancelled() {
            return Status::IoError;
        }
        self.bytes_consumed = self.bytes_consumed.wrapping_add(n as u64);
        self.last_n = n as usize;
        if n == 0 {
            self.finished = true;
            if let Some(end) = at_end {
                *end = true;
            }
        }
        Status::Ok
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn bytes_consumed(&self) -> u64 {
        self.bytes_consumed
    }
    /// Filtered bytes of the most recent reader feed, valid between feeds.
    pub fn last_feed(&self) -> &[u8] {
        self.buffer.as_ref().map_or(&[], |b| &b[..self.last_n.min(PARSE_CHUNK)])
    }
    /// Raw bytes pulled from the underlying chapter source by this stream's reader.
    pub fn reader_produced(&self) -> u32 {
        self.reader.total_produced()
    }
    /// Emits Section 2 (`Stream`): `"XST1"`, filtered counter, raw counter and entity filter.
    pub fn save_resume(&self, out: &mut dyn crate::resume_point::PointOut) -> bool {
        StreamView {
            open: self.open,
            finished: self.finished,
            bytes_consumed: self.bytes_consumed,
            reader_produced: self.reader.total_produced(),
            shift: self.shift,
            last_feed: self.last_feed(),
            filter: self.filter.as_deref(),
        }
        .save_resume(out)
    }
    /// Restore stream counters, inner raw reader offset and entity filter from
    /// Section 2 (`Stream`), checking exact agreement with `Binding::fed` and
    /// `Source::consumed`. Call only after `open`.
    #[inline(never)]
    pub fn load_resume(
        &mut self,
        sec: &mut crate::resume_point::SectionIn<'_, '_>,
        expected_fed: u64,
        expected_produced: u32,
        member_uncompressed: u32,
    ) -> bool {
        if !self.open || self.finished {
            return false;
        }
        let mut h = [0u8; 17];
        if !sec.read_exact(&mut h) || &h[..4] != b"XST1" {
            return false;
        }
        let consumed = u64::from_le_bytes([
            h[4], h[5], h[6], h[7], h[8], h[9], h[10], h[11],
        ]);
        let produced = u32::from_le_bytes([h[12], h[13], h[14], h[15]]);
        if consumed != expected_fed || produced != expected_produced || h[16] > 1 {
            return false;
        }
        let has_filter = h[16] == 1;
        if !self.reader.seek_stored(produced) {
            return false;
        }
        let ok = match (has_filter, self.filter.as_mut()) {
            (true, Some(f)) => {
                f.load_resume(sec)
                    && f.raw_unread() <= produced as usize
                    && (!f.source_done() || produced == member_uncompressed)
            }
            (false, None) => true,
            _ => false,
        };
        if !ok || !sec.finish() {
            return false;
        }
        self.bytes_consumed = consumed;
        true
    }
    /// Borrow the idle parse buffer directly (for loading prolog, names or replay
    /// during restore without an extra stack buffer).
    #[inline(never)]
    pub fn with_buffer_mut(
        &mut self,
        f: &mut dyn FnMut(&mut [u8; PARSE_CHUNK]) -> bool,
    ) -> bool {
        self.buffer.as_deref_mut().is_some_and(f)
    }
    /// Feed `self.buffer[..len]` directly to the backend under the same
    /// pre-poll -> `permits_feed` -> parse -> post-poll order as `feed_chunk_with`.
    #[inline(never)]
    pub fn feed_buffered_direct(
        &mut self,
        len: usize,
        parser_count: u64,
        feed: &mut dyn FnMut(&mut S, &mut B, &[u8]) -> bool,
    ) -> Status {
        if !self.open || self.finished || len > PARSE_CHUNK {
            return Status::Unsupported;
        }
        if self.policy.cancelled() {
            return Status::IoError;
        }
        if !self.policy.permits_feed(self.backend, parser_count, len as u64) {
            return Status::Unsupported;
        }
        let Some(buffer) = self.buffer.as_deref() else {
            return Status::OutOfMemory;
        };
        let bytes = &buffer[..len];
        let backend = &mut *self.backend;
        if !self.reader.with_source(|source| feed(source, backend, bytes)) {
            return Status::ParseError;
        }
        if self.policy.cancelled() {
            return Status::IoError;
        }
        Status::Ok
    }
    /// Activate the restored parser-to-stream offset translation and replay slice length.
    pub fn set_restored(&mut self, shift: i64, last_n: usize) {
        self.shift = shift;
        self.last_n = last_n.min(PARSE_CHUNK);
    }
    pub fn shift(&self) -> i64 {
        self.shift
    }
    pub fn close(&mut self) {
        let _ = self.close_returning();
    }
    /// Close the active parser and return caller-lent parse resources so the
    /// same `XmlStream` can reopen cleanly without an extra stack frame.
    pub fn close_returning(&mut self) -> Resources<'a> {
        if self.open {
            self.backend.close();
            self.open = false;
        }
        self.handler_attached = false;
        let resources = Resources {
            buffer: self.buffer.take(),
            filter: self.filter.take(),
        };
        self.finished = false;
        self.last_n = 0;
        self.shift = 0;
        // Count deliberately survives close, matching the source.
        resources
    }
}
impl<S: BookSource + ?Sized, B: ParserBackend + ?Sized, P: SessionPolicy<B>> Drop
    for XmlStream<'_, S, B, P>
{
    fn drop(&mut self) {
        self.close();
    }
}

/// Compatibility wrapper; canonical open/feed/close live only in XmlStream.
pub struct XmlSaxSession<
    'a,
    S: BookSource + ?Sized,
    B: ParserBackend + ?Sized,
    P: SessionPolicy<B> = NoPolicy,
> {
    stream: XmlStream<'a, S, B, P>,
    handler: Option<&'a mut dyn Handler>,
}
impl<'a, S: BookSource + ?Sized, B: ParserBackend + ?Sized> XmlSaxSession<'a, S, B> {
    pub fn new(source: &'a mut S, decoder: Option<InflateScratch<'a>>, backend: &'a mut B) -> Self {
        Self::with_policy(source, decoder, backend, NoPolicy)
    }
}
impl<'a, S: BookSource + ?Sized, B: ParserBackend + ?Sized, P: SessionPolicy<B>>
    XmlSaxSession<'a, S, B, P>
{
    pub fn with_policy(
        source: &'a mut S,
        decoder: Option<InflateScratch<'a>>,
        backend: &'a mut B,
        policy: P,
    ) -> Self {
        Self {
            stream: XmlStream::with_policy(source, decoder, backend, policy),
            handler: None,
        }
    }
    pub fn open(
        &mut self,
        entry: &'a Checked,
        handler: &'a mut dyn Handler,
        resources: Resources<'a>,
        filter: bool,
    ) -> Status {
        self.handler = None;
        let status = self.stream.open(entry, resources, filter);
        if self.stream.handler_attached {
            self.handler = Some(handler);
        }
        status
    }
    pub fn feed_chunk(&mut self, end: Option<&mut bool>) -> Status {
        match self.handler.as_deref_mut() {
            Some(h) => self.stream.feed_chunk(h, end),
            None => {
                if let Some(end) = end {
                    *end = self.stream.finished;
                }
                Status::Unsupported
            }
        }
    }
    pub fn close(&mut self) {
        self.stream.close();
        self.handler = None;
    }
    pub fn is_open(&self) -> bool {
        self.stream.is_open()
    }
    pub fn bytes_consumed(&self) -> u64 {
        self.stream.bytes_consumed()
    }
    pub fn stop_requested(&self) -> bool {
        self.handler.as_ref().is_some_and(|h| h.stop_parse())
    }
}
/// Resources remain exclusively borrowed until this call returns. There is no
/// arena mark/release: caller storage is never consumed or allocated here.
pub fn parse_entry<'a, S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    source: &'a mut S,
    entry: &'a Checked,
    decoder: Option<InflateScratch<'a>>,
    backend: &'a mut B,
    handler: &'a mut dyn Handler,
    resources: Resources<'a>,
    filter_html_entities: bool,
) -> Status {
    parse_entry_with_policy(
        source,
        entry,
        decoder,
        backend,
        handler,
        resources,
        filter_html_entities,
        NoPolicy,
    )
}

/// Same one-shot loop as SDK, with explicit stable policy selection.
pub fn parse_entry_with_policy<
    'a,
    S: BookSource + ?Sized,
    B: ParserBackend + ?Sized,
    P: SessionPolicy<B>,
>(
    source: &'a mut S,
    entry: &'a Checked,
    decoder: Option<InflateScratch<'a>>,
    backend: &'a mut B,
    handler: &'a mut dyn Handler,
    resources: Resources<'a>,
    filter_html_entities: bool,
    policy: P,
) -> Status {
    let mut session = XmlSaxSession::with_policy(source, decoder, backend, policy);
    let mut status = session.open(entry, handler, resources, filter_html_entities);
    if status == Status::Ok {
        let mut at_end = false;
        while !at_end && !session.stop_requested() {
            status = session.feed_chunk(Some(&mut at_end));
            if status != Status::Ok {
                break;
            }
        }
    }
    session.close();
    status
}
