// Derived from pinned FreeInk XmlSax.cpp/h (MIT); see rust/THIRD_PARTY.md.
#![forbid(unsafe_code)]

use crate::{
    xml_input::EntityFilter,
    zip_container::{Entry, InflateScratch, Reader, Status},
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

/// Producer backends must expose Expat-compatible byte-index semantics.
/// A negative index means no consumed input; the index is sampled BEFORE feed.
pub trait ProducerBackend: ParserBackend {
    fn create_producer(&mut self) -> bool;
    fn current_byte_index(&self) -> i64;
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
        }
    }
    pub fn open(
        &mut self,
        entry: &'a Entry,
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
        if !self
            .policy
            .permits_feed(self.backend, self.bytes_consumed, n as u64)
        {
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
    pub fn close(&mut self) {
        if self.open {
            self.backend.close();
            self.open = false;
        }
        self.handler_attached = false;
        self.buffer = None;
        self.filter = None;
        self.finished = false;
        // Count deliberately survives close, matching the source.
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
        entry: &'a Entry,
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
    entry: &'a Entry,
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
    entry: &'a Entry,
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
