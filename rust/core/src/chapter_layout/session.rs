#![forbid(unsafe_code)]
use crate::member_check::Checked;
use super::{engine::{Engine, EngineState}, types::Backend};
use crate::{
    xml_session::{
        Attributes, Handler, NoPolicy, ParserBackend, Resources, SessionPolicy, XmlStream,
    },
    zip_container::Status,
    zip_window::source::BookSource,
};
/// Engine loan for the ordinary session methods.
pub struct LayoutHandler<'e, 'a> {
    pub engine: &'e mut Engine<'a>,
}
/// Exactly one synchronous parser feed or final layout flush.
pub enum LayoutOperation<'a> {
    Feed { bytes: &'a [u8], last: bool },
    Finish,
}
/// SAX adapter for a backend borrowed only during this operation.
pub struct OperationHandler<'e, 'a, 'b> {
    pub engine: &'e mut EngineState<'a>,
    pub backend: &'b mut dyn Backend,
}
impl Handler for OperationHandler<'_, '_, '_> {
    fn start_element(&mut self, n: &[u8], atts: &mut dyn Attributes) {
        self.engine.start(self.backend, n, atts.local(b"class"), atts.local(b"style"),
            atts.local(b"id"), atts.local(b"href"), atts.local(b"src"))
    }
    fn end_element(&mut self, n: &[u8]) { self.engine.end(self.backend, n) }
    fn text(&mut self, t: &[u8]) { self.engine.text(self.backend, t) }
    fn stop_parse(&self) -> bool { self.engine.stopped }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Idle,
    Parsing,
    Done,
    Failed,
}
pub struct ChapterSession<
    'a,
    S: BookSource + ?Sized,
    B: ParserBackend + ?Sized,
    P: SessionPolicy<B> = NoPolicy,
> {
    pub stream: XmlStream<'a, S, B, P>,
    state: State,
    total: u64,
}
impl<'a, S: BookSource + ?Sized, B: ParserBackend + ?Sized, P: SessionPolicy<B>>
    ChapterSession<'a, S, B, P>
{
    pub fn new(stream: XmlStream<'a, S, B, P>) -> Self {
        Self {
            stream,
            state: State::Idle,
            total: 0,
        }
    }
    /// Engine and pre-scanned image resources are constructed by the caller before
    /// begin; no owning session contains a reference into itself.
    pub fn begin(&mut self, entry: &'a Checked, res: Resources<'a>, engine: &Engine<'_>) -> Status {
        self.begin_state(entry, res, &engine.state)
    }
    pub fn begin_state(&mut self, entry: &'a Checked, res: Resources<'a>, engine: &EngineState<'_>) -> Status {
        self.abort();
        if engine.params.width <= 0 || engine.params.height <= 0 {
            return Status::Unsupported;
        }
        self.total = u64::from(entry.uncompressed_size);
        let st = self.stream.open(entry, res, true);
        self.state = if st == Status::Ok {
            State::Parsing
        } else {
            State::Failed
        };
        st
    }
    pub fn state(&self) -> State {
        self.state
    }
    pub fn done(&self) -> bool {
        self.state == State::Done
    }
    pub fn bytes_total(&self) -> u64 {
        self.total
    }
    pub fn bytes_consumed(&self) -> u64 {
        self.stream.bytes_consumed()
    }
    pub fn step(&mut self, h: &mut LayoutHandler<'_, '_>, min_pages: u32) -> Status {
        self.step_bounded(h, min_pages, u32::MAX)
    }
    pub fn step_bounded(
        &mut self,
        h: &mut LayoutHandler<'_, '_>,
        min_pages: u32,
        max_feeds: u32,
    ) -> Status {
        let Engine { state, backend } = &mut *h.engine;
        self.step_with(state, min_pages, max_feeds, |_, parser, state, operation| {
            match operation {
                LayoutOperation::Feed { bytes, last } => parser.feed(bytes, last,
                    &mut OperationHandler { engine: state, backend: &mut **backend }),
                LayoutOperation::Finish => state.finish(&mut **backend),
            }
        })
    }
    /// The source loan exists after reads and during SAX/finish only. The callback
    /// must perform the indicated operation synchronously; it cannot retain loans.
    pub fn step_with(
        &mut self,
        engine: &mut EngineState<'_>,
        min_pages: u32,
        max_feeds: u32,
        mut operation: impl FnMut(&mut S, &mut B, &mut EngineState<'_>, LayoutOperation<'_>) -> bool,
    ) -> Status {
        if self.state == State::Done {
            return Status::Ok;
        }
        if self.state != State::Parsing {
            return Status::Unsupported;
        }
        let start = engine.pages;
        let mut end = false;
        let mut status = Status::Ok;
        let mut feeds = 0;
        while !end
            && !engine.stopped
            && engine.pages.wrapping_sub(start) < min_pages
            && feeds < max_feeds
        {
            feeds += 1;
            status = self.stream.feed_chunk_with(Some(&mut end),
                |source, parser, bytes, last| operation(source, parser, engine,
                    LayoutOperation::Feed { bytes, last }));
            if status != Status::Ok {
                break;
            }
        }
        if status == Status::Ok && (end || engine.stopped) {
            let finished = self.stream.with_source(|source, parser|
                operation(source, parser, engine, LayoutOperation::Finish));
            if !finished || engine.failed {
                status = Status::OutOfMemory
            }
            self.stream.close();
            self.state = if status == Status::Ok {
                State::Done
            } else {
                State::Failed
            };
            return status;
        }
        if status != Status::Ok {
            if status != Status::OutOfMemory && engine.failed {
                status = Status::OutOfMemory
            }
            self.stream.close();
            self.state = State::Failed
        }
        status
    }
    pub fn abort(&mut self) {
        self.stream.close();
        self.state = State::Idle;
        self.total = 0
    }
    pub fn run(&mut self, h: &mut LayoutHandler<'_, '_>) -> Status {
        let mut status = Status::Ok;
        while status == Status::Ok && !self.done() {
            status = self.step(h, u32::MAX)
        }
        status
    }
}
