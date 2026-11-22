// MIT. XML style watcher consuming decoded chapter bytes.
use crate::{
    styles::{BODY_DEPTH, HEAD_WINDOW, MALFORMED, Slot, State},
    zip_container::Status,
    zip_window::source::BookSource,
};
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Lex {
    #[default]
    Text,
    Open,
    Bang,
    Comment,
    CData,
    Pi,
    Doctype,
    EndName,
    EndTail,
    StartName,
    Tag,
    AttrName,
    Quoted,
    Slash,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Off,
    Head,
    Nesting,
}
#[derive(Clone, Copy, Debug)]
pub struct Verdict {
    pub state: State,
    pub stopped: bool,
    pub cleared: bool,
}
pub struct Watch<'a, S: BookSource + ?Sized> {
    source: &'a mut S,
    state: State,
    mode: Mode,
    seen: u64,
    depth: u64,
    body_depth: u64,
    lex: Lex,
    quote: u8,
    keyword: &'static [u8],
    run: usize,
    name_length: usize,
    attr_run: usize,
    name: [u8; 5],
    xml_base: bool,
    root_html: bool,
    seen_head: bool,
    seen_body: bool,
    stopped: bool,
    cleared: bool,
}
fn space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n')
}
impl<'a, S: BookSource + ?Sized> Watch<'a, S> {
    /// A synchronous loan between watched reads does not advance the watch.
    pub fn with_source<R>(&mut self, operation: impl FnOnce(&mut S) -> R) -> R {
        operation(self.source)
    }
    pub fn begin(source: &'a mut S, state: State) -> Self {
        Self {
            source,
            state,
            mode: match state {
                State::Deferred => Mode::Head,
                State::Pending => Mode::Nesting,
                _ => Mode::Off,
            },
            seen: 0,
            depth: 0,
            body_depth: 0,
            lex: Lex::Text,
            quote: 0,
            keyword: b"",
            run: 0,
            name_length: 0,
            attr_run: 0,
            name: [0; 5],
            xml_base: false,
            root_html: false,
            seen_head: false,
            seen_body: false,
            stopped: false,
            cleared: false,
        }
    }
    pub fn verdict(&self) -> Verdict {
        Verdict {
            state: self.state,
            stopped: self.stopped,
            cleared: self.cleared,
        }
    }
    pub fn settled(&self) -> bool {
        self.mode == Mode::Off || self.cleared
    }
    fn stop(&mut self) -> i32 {
        self.stopped = true;
        -1
    }
    fn name(&mut self, c: u8) {
        if c == b':' {
            self.name_length = 0;
            return;
        }
        if self.name_length < 5 {
            self.name[self.name_length] = c;
        }
        if self.name_length <= 5 {
            self.name_length += 1;
        }
    }
    fn is(&self, name: &[u8]) -> bool {
        self.name_length == name.len() && &self.name[..name.len()] == name
    }
    fn start(&mut self) -> bool {
        if self.mode == Mode::Head {
            if self.xml_base {
                return false;
            }
            if self.is(b"body") {
                self.cleared = true;
                return true;
            }
            if self.is(b"head") {
                if self.seen_head {
                    return false;
                }
                self.seen_head = true;
                return true;
            }
            return self.is(b"html") || self.is(b"title") || self.is(b"meta");
        }
        self.depth += 1;
        if self.depth == 1 {
            self.root_html = self.is(b"html");
        }
        if self.is(b"body") {
            if self.seen_body || !self.root_html || self.depth != 2 {
                return false;
            }
            self.seen_body = true;
            self.body_depth = self.depth;
            return true;
        }
        self.body_depth == 0 || self.depth.wrapping_sub(self.body_depth) <= BODY_DEPTH as u64
    }
    fn end(&mut self) {
        if self.mode != Mode::Nesting {
            return;
        }
        if self.is(b"body") {
            self.body_depth = 0;
        }
        self.depth = self.depth.saturating_sub(1);
    }
    fn tag(&mut self, c: u8) -> bool {
        if space(c) || c == b'=' {
            return true;
        }
        match c {
            b'"' | b'\'' => {
                self.quote = c;
                self.lex = Lex::Quoted;
            }
            b'>' => {
                self.lex = Lex::Text;
                return self.start();
            }
            b'/' => self.lex = Lex::Slash,
            b'<' => return false,
            _ => {
                self.lex = Lex::AttrName;
                self.attr_run = if c == b'x' { 1 } else { 9 };
            }
        }
        true
    }
    fn step(&mut self, c: u8) -> bool {
        if c == 0 {
            return false;
        }
        match self.lex {
            Lex::Text => {
                if c == b'<' {
                    self.lex = Lex::Open;
                }
                true
            }
            Lex::Open => {
                self.name_length = 0;
                match c {
                    b'!' => {
                        self.lex = Lex::Bang;
                        self.keyword = b"";
                        self.run = 0;
                    }
                    b'?' => {
                        self.lex = Lex::Pi;
                        self.run = 0;
                    }
                    b'/' => self.lex = Lex::EndName,
                    _ => {
                        if space(c) || matches!(c, b'>' | b'<') {
                            return false;
                        }
                        self.lex = Lex::StartName;
                        self.xml_base = false;
                        self.name(c);
                    }
                }
                true
            }
            Lex::Bang => {
                if self.keyword.is_empty() {
                    self.keyword = match c {
                        b'-' => b"--",
                        b'[' => b"[CDATA[",
                        b'D' => b"DOCTYPE",
                        _ => return false,
                    };
                }
                if self.keyword.get(self.run) != Some(&c) {
                    return false;
                }
                self.run += 1;
                if self.run < self.keyword.len() {
                    return true;
                }
                self.lex = match self.keyword[0] {
                    b'-' => Lex::Comment,
                    b'[' => Lex::CData,
                    _ => Lex::Doctype,
                };
                self.run = 0;
                self.quote = 0;
                true
            }
            Lex::Comment | Lex::CData => {
                let marker = if self.lex == Lex::Comment { b'-' } else { b']' };
                if c == b'>' && self.run == 2 {
                    self.lex = Lex::Text;
                }
                self.run = if c == marker {
                    (self.run + 1).min(2)
                } else {
                    0
                };
                true
            }
            Lex::Pi => {
                if c == b'>' && self.run != 0 {
                    self.lex = Lex::Text;
                }
                self.run = usize::from(c == b'?');
                true
            }
            Lex::Doctype => {
                if self.quote != 0 {
                    if c == self.quote {
                        self.quote = 0;
                    }
                    return true;
                }
                if matches!(c, b'"' | b'\'') {
                    self.quote = c;
                } else if c == b'[' {
                    return false;
                } else if c == b'>' {
                    self.lex = Lex::Text;
                }
                true
            }
            Lex::EndName => {
                if c == b'>' {
                    self.lex = Lex::Text;
                    self.end();
                    return true;
                }
                if space(c) {
                    self.lex = Lex::EndTail;
                    return true;
                }
                if matches!(c, b'<' | b'/') {
                    return false;
                }
                self.name(c);
                true
            }
            Lex::EndTail => {
                if c == b'>' {
                    self.lex = Lex::Text;
                    self.end();
                    true
                } else {
                    space(c)
                }
            }
            Lex::StartName => {
                if space(c) {
                    self.lex = Lex::Tag;
                    return true;
                }
                match c {
                    b'>' => {
                        self.lex = Lex::Text;
                        self.start()
                    }
                    b'/' => {
                        self.lex = Lex::Slash;
                        true
                    }
                    b'<' => false,
                    _ => {
                        self.name(c);
                        true
                    }
                }
            }
            Lex::AttrName => {
                if !space(c) && !matches!(c, b'=' | b'>' | b'/' | b'"' | b'\'') {
                    if c == b'<' {
                        return false;
                    }
                    self.attr_run = if self.attr_run < 8 && c == b"xml:base"[self.attr_run] {
                        self.attr_run + 1
                    } else {
                        9
                    };
                    return true;
                }
                self.xml_base |= self.attr_run == 8;
                self.lex = Lex::Tag;
                self.tag(c)
            }
            Lex::Tag => self.tag(c),
            Lex::Quoted => {
                if c == self.quote {
                    self.lex = Lex::Tag;
                }
                true
            }
            Lex::Slash => {
                if c != b'>' {
                    return false;
                }
                self.lex = Lex::Text;
                if !self.start() {
                    return false;
                }
                self.end();
                true
            }
        }
    }
}
impl<S: BookSource + ?Sized> BookSource for Watch<'_, S> {
    fn size(&self) -> u64 {
        self.source.size()
    }
    fn read_at(&mut self, offset: u64, out: &mut [u8]) -> i32 {
        if self.stopped {
            return -1;
        }
        let got = self.source.read_at(offset, out);
        if self.mode == Mode::Off || got <= 0 || self.cleared {
            return got;
        }
        if offset != self.seen || got as usize > out.len() {
            return self.stop();
        }
        for &c in &out[..got as usize] {
            if self.cleared {
                break;
            }
            if self.mode == Mode::Head && self.seen >= HEAD_WINDOW as u64 {
                return self.stop();
            }
            if !self.step(c) {
                return self.stop();
            }
            self.seen += 1;
        }
        if !self.cleared && self.seen == self.source.size() {
            if self.lex != Lex::Text || (self.mode == Mode::Nesting && !self.seen_body) {
                return self.stop();
            }
            self.cleared = true;
        }
        got
    }
}
/// Deferred stop requires caller's ordinary resolve/build without a Book;
/// never reuse a stopped pass's page/cache output. At most two restarts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Settlement {
    Return(Status),
    ResolveAndBuild,
    Restyle,
}
pub fn settle(slot: &mut Slot<'_>, v: Verdict, status: Status) -> Settlement {
    if !v.stopped {
        if status == Status::Ok && slot.issues & MALFORMED != 0 {
            return Settlement::Return(Status::OutOfMemory);
        }
        if status == Status::Ok && v.cleared {
            slot.state = State::Settled;
        }
        return Settlement::Return(status);
    }
    if v.state == State::Pending {
        slot.limit_nesting();
        Settlement::Restyle
    } else {
        Settlement::ResolveAndBuild
    }
}
pub fn require_advance(previous: State, slot: &Slot<'_>, status: Status) -> Status {
    if status == Status::Ok && slot.state <= previous {
        Status::Unsupported
    } else {
        status
    }
}
