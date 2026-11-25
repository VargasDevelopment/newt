//! Scheduling/failure witnesses only. RecordingBackend is deliberately NOT XML.
use newt_core::member_check::Checked;
use newt_core::xml_session::*;
use newt_core::{
    xml_input::EntityFilter,
    zip_container::{Entry, Status},
    zip_window::source::BookSource,
};

struct Source<'a> {
    bytes: &'a [u8],
    max: usize,
    fail: Option<usize>,
    calls: usize,
}
impl<'a> Source<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            max: 2048,
            fail: None,
            calls: 0,
        }
    }
}
impl BookSource for Source<'_> {
    fn size(&self) -> u64 {
        self.bytes.len() as u64
    }
    fn read_at(&mut self, at: u64, dst: &mut [u8]) -> i32 {
        let call = self.calls;
        self.calls += 1;
        if self.fail == Some(call) {
            return -1;
        }
        let Some(tail) = usize::try_from(at).ok().and_then(|at| self.bytes.get(at..)) else {
            return -1;
        };
        let n = tail.len().min(dst.len()).min(self.max);
        dst[..n].copy_from_slice(&tail[..n]);
        n as i32
    }
}
#[derive(Debug, PartialEq)]
enum Op {
    Create,
    Feed(Vec<u8>, bool),
    Close,
}
#[derive(Default)]
struct RecordingBackend {
    ops: Vec<Op>,
    fail_create: bool,
    fail_feed: Option<usize>,
    feeds: usize,
}
impl ParserBackend for RecordingBackend {
    fn producer_backend(&mut self) -> Option<&mut dyn ProducerBackend> { Some(self) }
    fn create(&mut self) -> bool {
        self.ops.push(Op::Create);
        !self.fail_create
    }
    fn feed(&mut self, bytes: &[u8], final_chunk: bool, handler: &mut dyn Handler) -> bool {
        self.ops.push(Op::Feed(bytes.to_vec(), final_chunk));
        let index = self.feeds;
        self.feeds += 1;
        handler.text(bytes); // callbacks can precede a failed parse
        self.fail_feed != Some(index)
    }
    fn close(&mut self) {
        self.ops.push(Op::Close);
    }
}
#[derive(Default)]
struct Sink {
    bytes: Vec<u8>,
    stop: bool,
    stop_on_text: bool,
}
impl Handler for Sink {
    fn text(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
        if self.stop_on_text {
            self.stop = true;
        }
    }
    fn stop_parse(&self) -> bool {
        self.stop
    }
}
fn resources(buffer: &mut [u8; 2048]) -> Resources<'_> {
    Resources {
        buffer: Some(buffer),
        filter: None,
    }
}

#[test]
fn final_empty_feed_finished_at_end_and_close_retains_count() {
    let mut source = Source::new(b"abc");
    let entry = Checked::raw(3);
    let mut backend = RecordingBackend::default();
    let mut sink = Sink::default();
    let mut buffer = [0; 2048];
    {
        let mut session = XmlSaxSession::new(&mut source, None, &mut backend);
        let mut end = true;
        assert_eq!(session.feed_chunk(Some(&mut end)), Status::Unsupported);
        assert!(!end);
        assert_eq!(
            session.open(&entry, &mut sink, resources(&mut buffer), false),
            Status::Ok
        );
        assert_eq!(session.feed_chunk(Some(&mut end)), Status::Ok);
        assert!(!end);
        assert_eq!(session.bytes_consumed(), 3);
        assert_eq!(session.feed_chunk(Some(&mut end)), Status::Ok);
        assert!(end);
        assert!(session.is_open());
        end = false;
        assert_eq!(session.feed_chunk(Some(&mut end)), Status::Unsupported);
        assert!(end);
        session.close();
        assert!(!session.is_open());
        assert_eq!(session.bytes_consumed(), 3);
        assert_eq!(session.feed_chunk(Some(&mut end)), Status::Unsupported);
        assert!(!end);
        session.close();
    }
    assert_eq!(
        backend.ops,
        [
            Op::Create,
            Op::Feed(b"abc".to_vec(), false),
            Op::Feed(vec![], true),
            Op::Close
        ]
    );
}
#[test]
fn parser_failure_does_not_advance_count_or_finish_even_after_callbacks() {
    let mut source = Source::new(b"abcdef");
    source.max = 3;
    let entry = Checked::raw(6);
    let mut backend = RecordingBackend {
        fail_feed: Some(1),
        ..Default::default()
    };
    let mut sink = Sink::default();
    let mut buffer = [0; 2048];
    {
        let mut session = XmlSaxSession::new(&mut source, None, &mut backend);
        assert_eq!(
            session.open(&entry, &mut sink, resources(&mut buffer), false),
            Status::Ok
        );
        assert_eq!(session.feed_chunk(None), Status::Ok);
        let mut end = true;
        assert_eq!(session.feed_chunk(Some(&mut end)), Status::ParseError);
        assert!(!end);
        assert_eq!(session.bytes_consumed(), 3);
        assert!(session.is_open());
    }
    assert_eq!(sink.bytes, b"abcdef");
    assert_eq!(backend.ops.last(), Some(&Op::Close));
}
#[test]
fn io_error_never_feeds_parser() {
    let mut source = Source::new(b"x");
    source.fail = Some(0);
    let entry = Checked::raw(1);
    let mut backend = RecordingBackend::default();
    let mut sink = Sink::default();
    let mut buffer = [0; 2048];
    assert_eq!(
        parse_entry(
            &mut source,
            &entry,
            None,
            &mut backend,
            &mut sink,
            resources(&mut buffer),
            false
        ),
        Status::IoError
    );
    assert_eq!(backend.ops, [Op::Create, Op::Close]);
}
#[test]
fn create_and_typed_resource_failures() {
    for mode in 0..3 {
        let mut source = Source::new(b"x");
        let entry = Checked::raw(1);
        let mut backend = RecordingBackend {
            fail_create: true,
            ..Default::default()
        };
        let mut sink = Sink::default();
        let mut buffer = [0; 2048];
        let res = Resources {
            buffer: if mode == 0 { None } else { Some(&mut buffer) },
            filter: None,
        };
        assert_eq!(
            parse_entry(
                &mut source,
                &entry,
                None,
                &mut backend,
                &mut sink,
                res,
                mode == 1
            ),
            Status::OutOfMemory
        );
        assert_eq!(
            backend.ops,
            if mode == 2 { vec![Op::Create] } else { vec![] }
        );
        assert_eq!(source.calls, 0);
    }
}
#[test]
fn reopen_closes_old_before_failed_reader_open_and_resets_count() {
    let mut source = Source::new(b"x");
    let first = Checked::raw(1);
    let mut invalid = Entry::raw(1);
    invalid.method = 9;
    let invalid = Checked::unverified(invalid);
    let mut backend = RecordingBackend::default();
    let mut one = Sink::default();
    let mut two = Sink::default();
    let mut a = [0; 2048];
    let mut b = [0; 2048];
    {
        let mut session = XmlSaxSession::new(&mut source, None, &mut backend);
        assert_eq!(
            session.open(&first, &mut one, resources(&mut a), false),
            Status::Ok
        );
        assert_eq!(session.feed_chunk(None), Status::Ok);
        assert_eq!(session.bytes_consumed(), 1);
        assert_eq!(
            session.open(&invalid, &mut two, resources(&mut b), false),
            Status::Unsupported
        );
        assert_eq!(session.bytes_consumed(), 0);
        assert!(!session.is_open());
    }
    assert_eq!(
        backend.ops,
        [Op::Create, Op::Feed(b"x".to_vec(), false), Op::Close]
    );
}
#[test]
fn successful_reopen_routes_callbacks_to_new_handler() {
    let mut source = Source::new(b"x");
    let entry = Checked::raw(1);
    let mut backend = RecordingBackend::default();
    let mut one = Sink::default();
    let mut two = Sink::default();
    let mut a = [0; 2048];
    let mut b = [0; 2048];
    {
        let mut session = XmlSaxSession::new(&mut source, None, &mut backend);
        assert_eq!(
            session.open(&entry, &mut one, resources(&mut a), false),
            Status::Ok
        );
        assert_eq!(session.feed_chunk(None), Status::Ok);
        assert_eq!(
            session.open(&entry, &mut two, resources(&mut b), false),
            Status::Ok
        );
        assert_eq!(session.feed_chunk(None), Status::Ok);
    }
    assert_eq!(one.bytes, b"x");
    assert_eq!(two.bytes, b"x");
    assert_eq!(
        backend.ops,
        [
            Op::Create,
            Op::Feed(vec![b'x'], false),
            Op::Close,
            Op::Create,
            Op::Feed(vec![b'x'], false),
            Op::Close
        ]
    );
}
#[test]
fn one_shot_stop_initially_and_during_callback_is_between_chunks() {
    for initial in [false, true] {
        let mut source = Source::new(b"abcdef");
        source.max = 2;
        let entry = Checked::raw(6);
        let mut backend = RecordingBackend::default();
        let mut sink = Sink {
            stop: initial,
            stop_on_text: true,
            ..Default::default()
        };
        let mut buffer = [0; 2048];
        assert_eq!(
            parse_entry(
                &mut source,
                &entry,
                None,
                &mut backend,
                &mut sink,
                resources(&mut buffer),
                false
            ),
            Status::Ok
        );
        assert_eq!(source.calls, usize::from(!initial));
        assert_eq!(sink.bytes, if initial { b"".as_slice() } else { b"ab" });
    }
}
#[test]
fn stepped_feed_ignores_handler_stop() {
    let mut source = Source::new(b"x");
    let entry = Checked::raw(1);
    let mut backend = RecordingBackend::default();
    let mut sink = Sink {
        stop: true,
        ..Default::default()
    };
    let mut buffer = [0; 2048];
    {
        let mut session = XmlSaxSession::new(&mut source, None, &mut backend);
        assert_eq!(
            session.open(&entry, &mut sink, resources(&mut buffer), false),
            Status::Ok
        );
        assert_eq!(session.feed_chunk(None), Status::Ok);
    }
    assert_eq!(sink.bytes, b"x");
}
#[test]
fn real_filter_expansion_is_counted_and_filter_is_reset_at_open() {
    let mut source = Source::new(b"&nbsp;&amp;");
    source.max = 1;
    let entry = Checked::raw(11);
    let mut backend = RecordingBackend::default();
    let mut sink = Sink::default();
    let mut buffer = [0; 2048];
    let mut filter = EntityFilter::new();
    struct Seed(bool);
    impl newt_core::xml_input::BytePull for Seed {
        fn pull(&mut self, out: &mut [u8]) -> i32 {
            if self.0 {
                return -1;
            }
            self.0 = true;
            out[..4].copy_from_slice(b"&bad");
            4
        }
    }
    assert_eq!(filter.read(&mut Seed(false), &mut [0; 16]), -1);
    {
        let mut session = XmlSaxSession::new(&mut source, None, &mut backend);
        assert_eq!(
            session.open(
                &entry,
                &mut sink,
                Resources {
                    buffer: Some(&mut buffer),
                    filter: Some(&mut filter)
                },
                true
            ),
            Status::Ok
        );
        assert_eq!(session.feed_chunk(None), Status::Ok);
        assert_eq!(session.bytes_consumed(), 7);
        let mut end = false;
        assert_eq!(session.feed_chunk(Some(&mut end)), Status::Ok);
        assert!(end);
    }
    assert_eq!(sink.bytes, "\u{a0}&amp;".as_bytes());
}
#[test]
fn failure_on_final_empty_feed_does_not_finish() {
    let mut source = Source::new(b"");
    let entry = Checked::raw(0);
    let mut backend = RecordingBackend {
        fail_feed: Some(0),
        ..Default::default()
    };
    let mut sink = Sink::default();
    let mut buffer = [0; 2048];
    let mut session = XmlSaxSession::new(&mut source, None, &mut backend);
    assert_eq!(
        session.open(&entry, &mut sink, resources(&mut buffer), false),
        Status::Ok
    );
    let mut end = true;
    assert_eq!(session.feed_chunk(Some(&mut end)), Status::ParseError);
    assert!(!end);
    assert_eq!(session.bytes_consumed(), 0);
}
#[test]
fn real_reader_2048_chunk_boundary_and_final_empty_feed() {
    let input = [b'x'; 4097];
    let mut source = Source::new(&input);
    let entry = Checked::raw(4097);
    let mut backend = RecordingBackend::default();
    let mut sink = Sink::default();
    let mut buffer = [0; 2048];
    assert_eq!(
        parse_entry(
            &mut source,
            &entry,
            None,
            &mut backend,
            &mut sink,
            resources(&mut buffer),
            false
        ),
        Status::Ok
    );
    let feeds: Vec<_> = backend
        .ops
        .iter()
        .filter_map(|op| match op {
            Op::Feed(bytes, final_chunk) => Some((bytes.len(), *final_chunk)),
            _ => None,
        })
        .collect();
    assert_eq!(feeds, [(2048, false), (2048, false), (1, false), (0, true)]);
    assert_eq!(sink.bytes.len(), 4097);
}
#[test]
fn missing_decoder_fails_in_reader_before_parser_admission() {
    let mut local = [0; 30];
    local[..4].copy_from_slice(&0x04034b50u32.to_le_bytes());
    let mut source = Source::new(&local);
    let entry = Checked::unverified(Entry {
        compressed_size: 2,
        uncompressed_size: 1,
        method: 8,
        ..Default::default()
    });
    let mut backend = RecordingBackend::default();
    let mut sink = Sink::default();
    assert_eq!(
        parse_entry(
            &mut source,
            &entry,
            None,
            &mut backend,
            &mut sink,
            Resources {
                buffer: None,
                filter: None
            },
            false
        ),
        Status::OutOfMemory
    );
    assert_eq!(source.calls, 1);
    assert!(backend.ops.is_empty());
}
#[test]
fn zero_source_read_before_expected_end_is_io_error() {
    let mut source = Source::new(b"x");
    source.max = 0;
    let entry = Checked::raw(1);
    let mut backend = RecordingBackend::default();
    let mut sink = Sink::default();
    let mut buffer = [0; 2048];
    assert_eq!(
        parse_entry(
            &mut source,
            &entry,
            None,
            &mut backend,
            &mut sink,
            resources(&mut buffer),
            false
        ),
        Status::IoError
    );
    assert_eq!(backend.ops, [Op::Create, Op::Close]);
}

impl ProducerBackend for RecordingBackend {
    fn create_producer(&mut self) -> bool {
        self.create()
    }
    fn current_byte_index(&self) -> i64 {
        0
    }
}
struct CancelAt {
    poll: usize,
    at: usize,
}
impl ProcessingPolicy for CancelAt {
    fn cancelled(&mut self) -> bool {
        let old = self.poll;
        self.poll += 1;
        old == self.at
    }
}
#[test]
fn producer_reopen_cancellation_closes_before_poll_without_resetting_progress() {
    let mut source = Source::new(b"x");
    let entry = Checked::raw(1);
    let mut backend = RecordingBackend::default();
    let mut one = Sink::default();
    let mut two = Sink::default();
    let mut a = [0; 2048];
    let mut b = [0; 2048];
    let mut policy = CancelAt { poll: 0, at: 4 };
    {
        let mut session = XmlSaxSession::with_policy(
            &mut source,
            None,
            &mut backend,
            BorrowedPolicy(&mut policy),
        );
        assert_eq!(
            session.open(&entry, &mut one, resources(&mut a), false),
            Status::Ok
        );
        assert_eq!(session.feed_chunk(None), Status::Ok);
        assert_eq!(session.bytes_consumed(), 1);
        assert_eq!(
            session.open(&entry, &mut two, resources(&mut b), false),
            Status::IoError
        );
        assert_eq!(session.bytes_consumed(), 1);
        assert!(!session.is_open());
    }
    assert_eq!(
        backend.ops,
        [Op::Create, Op::Feed(vec![b'x'], false), Op::Close]
    );
    assert_eq!(policy.poll, 5);
    assert!(two.bytes.is_empty());
}
#[test]
fn ordinary_policy_is_zero_sized() {
    assert_eq!(core::mem::size_of::<NoPolicy>(), 0);
}
