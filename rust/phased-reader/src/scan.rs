// MIT. CSS span scanner and token serials for phased reader styles.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Token {
    Code,
    Comment,
    String,
    Group,
    Escape,
}
pub trait Sink {
    fn lexeme(&mut self, id: u32, trivia: bool);
    fn token(&mut self, c: u8, kind: Token);
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Token,
    Ident,
    Number,
    Comment,
    String,
    UrlStart,
    Url,
    UrlSpace,
    BadUrl,
    Escape,
}
#[derive(Default)]
pub struct Scanner {
    mode: Mode,
    back: Mode,
    escape_kind: Option<Token>,
    value: u32,
    serial: u32,
    size: usize,
    depth: usize,
    hex_count: u8,
    name_length: u8,
    number_part: u8,
    input: [u8; 8],
    blocks: Blocks,
    quote: u8,
    pub invalid: bool,
    overflow: bool,
    url_name: bool,
    only_name: bool,
}
struct Blocks([u8; 64]);
impl Default for Blocks {
    fn default() -> Self {
        Self([0; 64])
    }
}
fn ws(c: u8) -> bool {
    crate::css::is_space(c)
}
fn newline(c: u8) -> bool {
    matches!(c, b'\n' | b'\r' | 12)
}
fn start(c: u32) -> bool {
    matches!(c,65..=90|97..=122|95|0xb7|0xc0..=0xd6|0xd8..=0xf6|0xf8..=0x37d|0x37f..=0x1fff|0x200c|0x200d|0x203f|0x2040|0x2070..=0x218f|0x2c00..=0x2fef|0x3001..=0xd7ff|0xf900..=0xfdcf|0xfdf0..=0xfffd|0x10000..=0x10ffff)
}
fn name(c: u32) -> bool {
    start(c) || matches!(c, 48..=57 | 45)
}
fn hex(c: u8) -> Option<u32> {
    match c {
        b'0'..=b'9' => Some((c - b'0') as u32),
        b'a'..=b'f' => Some((c - b'a' + 10) as u32),
        b'A'..=b'F' => Some((c - b'A' + 10) as u32),
        _ => None,
    }
}
impl Scanner {
    pub fn feed(&mut self, c: u8, s: &mut impl Sink) {
        self.input[self.size] = c;
        self.size += 1;
        while self.size == 8 {
            self.step(s);
        }
    }
    pub fn finish(&mut self, s: &mut impl Sink) {
        while self.size != 0 {
            self.step(s);
        }
        if self.mode == Mode::Escape {
            if self.hex_count == 0 {
                self.invalid = true;
            }
            self.end_escape();
        }
        if !matches!(self.mode, Mode::Token | Mode::Ident | Mode::Number) || self.depth != 0 {
            self.invalid = true;
        }
    }
    fn peek(&self, n: usize) -> u8 {
        if n < self.size { self.input[n] } else { 0 }
    }
    fn point(&self, at: usize) -> (u32, usize) {
        if at >= self.size {
            return (0, 0);
        }
        let c = self.peek(at);
        if c == 0 {
            return (0xfffd, 1);
        }
        if c < 128 {
            return (c as u32, 1);
        }
        let len = match c {
            0xf0..=0xf4 => 4,
            0xe0..=0xef => 3,
            0xc2..=0xdf => 2,
            _ => 1,
        };
        if len == 1 {
            return (0xfffd, 1);
        }
        let mut value = (c & (0x7f >> len)) as u32;
        for i in 1..len {
            if at + i == self.size {
                return (0xfffd, i);
            }
            let n = self.peek(at + i);
            if n & 0xc0 != 0x80
                || (i == 1
                    && ((c == 0xe0 && n < 0xa0)
                        || (c == 0xed && n >= 0xa0)
                        || (c == 0xf0 && n < 0x90)
                        || (c == 0xf4 && n >= 0x90)))
            {
                return (0xfffd, i);
            }
            value = value * 64 + (n & 63) as u32;
        }
        (value, len)
    }
    fn escape(&self, n: usize) -> bool {
        self.peek(n) == b'\\' && (n + 1 == self.size || !newline(self.peek(n + 1)))
    }
    fn ident(&self, n: usize) -> bool {
        let (a, bytes) = self.point(n);
        let (b, bbytes) = self.point(n + 1);
        (bytes != 0 && start(a))
            || self.escape(n)
            || (a == 45 && ((bbytes != 0 && start(b)) || b == 45 || self.escape(n + 1)))
    }
    fn number(&self) -> bool {
        let n = usize::from(matches!(self.peek(0), b'+' | b'-'));
        self.peek(n).is_ascii_digit() || (self.peek(n) == b'.' && self.peek(n + 1).is_ascii_digit())
    }
    fn notice(&self, s: &mut impl Sink, trivia: bool) {
        s.lexeme(self.serial, trivia);
    }
    fn lexeme(&mut self, s: &mut impl Sink, trivia: bool) {
        self.serial = self.serial.wrapping_add(1);
        self.notice(s, trivia);
    }
    fn emit(&mut self, s: &mut impl Sink, k: Token, n: usize) {
        for i in 0..n {
            s.token(self.input[i], k);
        }
        self.input.copy_within(n..self.size, 0);
        self.size -= n;
    }
    fn plain(&self) -> Token {
        if self.depth != 0 || self.overflow {
            Token::Group
        } else {
            Token::Code
        }
    }
    fn decoded(&mut self, mut c: u32) {
        if (65..=90).contains(&c) {
            c += 32;
        }
        if self.name_length >= 3 || c != b"url"[self.name_length as usize] as u32 {
            self.url_name = false;
        }
        self.name_length = (self.name_length + 1).min(4);
    }
    fn begin_ident(&mut self, only: bool) {
        self.mode = Mode::Ident;
        self.only_name = only;
        self.name_length = 0;
        self.url_name = true;
    }
    fn begin_escape(&mut self, s: &mut impl Sink, k: Token) {
        self.back = self.mode;
        self.mode = Mode::Escape;
        self.hex_count = 0;
        self.value = 0;
        self.escape_kind = Some(k);
        self.emit(s, k, 1);
    }
    fn end_escape(&mut self) {
        if self.value == 0 || self.value > 0x10ffff || (0xd800..=0xdfff).contains(&self.value) {
            self.value = 0xfffd;
        }
        if self.back == Mode::Ident {
            self.decoded(self.value);
        }
        self.mode = self.back;
    }
    fn escaped(&mut self, s: &mut impl Sink) {
        let k = self.escape_kind.unwrap_or(Token::Escape);
        if let Some(h) = hex(self.peek(0)).filter(|_| self.hex_count < 6) {
            self.value = self.value * 16 + h;
            self.hex_count += 1;
            self.emit(s, k, 1);
            return;
        }
        if self.hex_count != 0 {
            if ws(self.peek(0)) {
                self.emit(
                    s,
                    k,
                    if self.peek(0) == b'\r' && self.peek(1) == b'\n' {
                        2
                    } else {
                        1
                    },
                );
            }
            self.end_escape();
            return;
        }
        let (v, n) = self.point(0);
        self.value = v;
        self.emit(s, k, n);
        self.end_escape();
    }
    fn block(&mut self, s: &mut impl Sink) {
        let c = self.peek(0);
        let open = c == b'(' || c == b'[' || (self.depth != 0 && c == b'{');
        let k = if open || self.depth != 0 {
            Token::Group
        } else {
            Token::Code
        };
        if open {
            if self.depth == 64 {
                self.invalid = true;
                self.overflow = true;
            } else {
                self.blocks.0[self.depth] = match c {
                    b'(' => b')',
                    b'[' => b']',
                    _ => b'}',
                };
                self.depth += 1;
            }
        } else if self.depth != 0 && c == self.blocks.0[self.depth - 1] {
            self.depth -= 1;
        }
        self.emit(s, k, self.point(0).1);
    }
    fn step(&mut self, s: &mut impl Sink) {
        let c = self.peek(0);
        if self.overflow {
            self.emit(s, Token::Group, 1);
            return;
        }
        match self.mode {
            Mode::Escape => {
                self.escaped(s);
                return;
            }
            Mode::Comment => {
                if c == b'*' && self.peek(1) == b'/' {
                    self.emit(s, Token::Comment, 2);
                    self.mode = Mode::Token;
                } else {
                    self.emit(s, Token::Comment, 1);
                }
                return;
            }
            Mode::String => {
                if c == self.quote {
                    self.emit(s, Token::String, 1);
                    self.mode = Mode::Token;
                } else if newline(c) {
                    self.invalid = true;
                    self.mode = Mode::Token;
                } else if c == b'\\' && self.size > 1 {
                    if newline(self.peek(1)) {
                        self.emit(
                            s,
                            Token::String,
                            if self.peek(1) == b'\r' && self.peek(2) == b'\n' {
                                3
                            } else {
                                2
                            },
                        );
                    } else {
                        self.begin_escape(s, Token::String);
                    }
                } else {
                    self.emit(s, Token::String, 1);
                }
                return;
            }
            Mode::UrlStart => {
                if ws(c) {
                    self.notice(s, true);
                    self.emit(s, Token::Group, 1);
                    return;
                }
                self.notice(s, false);
                if matches!(c, b'"' | b'\'') {
                    if self.depth == 64 {
                        self.invalid = true;
                        self.overflow = true;
                        return;
                    }
                    self.blocks.0[self.depth] = b')';
                    self.depth += 1;
                    self.mode = Mode::Token;
                } else {
                    self.mode = Mode::Url;
                }
                return;
            }
            Mode::Url | Mode::UrlSpace | Mode::BadUrl => {
                if c == b')' {
                    self.notice(s, false);
                    self.emit(s, Token::Group, 1);
                    self.mode = Mode::Token;
                    return;
                }
                if self.mode == Mode::UrlSpace && !ws(c) {
                    self.invalid = true;
                    self.mode = Mode::BadUrl;
                }
                self.notice(s, self.mode == Mode::UrlSpace && ws(c));
                if c == b'\\' && self.escape(0) {
                    self.begin_escape(s, Token::Group);
                    return;
                }
                if self.mode == Mode::Url {
                    if ws(c) {
                        self.mode = Mode::UrlSpace;
                        self.notice(s, true);
                    } else if matches!(c, b'"' | b'\'' | b'(' | b'\\' | 127)
                        || (c < 32 && c != 0 && c != b'\t')
                    {
                        self.invalid = true;
                        self.mode = Mode::BadUrl;
                    }
                }
                self.emit(s, Token::Group, 1);
                return;
            }
            Mode::Ident => {
                let (v, n) = self.point(0);
                if name(v) {
                    self.decoded(v);
                    self.emit(s, self.plain(), n);
                    return;
                }
                if self.escape(0) {
                    self.begin_escape(s, Token::Escape);
                    return;
                }
                self.mode = Mode::Token;
                if !self.only_name && c == b'(' {
                    if self.url_name && self.name_length == 3 {
                        self.emit(s, Token::Group, 1);
                        self.mode = Mode::UrlStart;
                    } else {
                        self.block(s);
                    }
                }
                return;
            }
            Mode::Number => {
                if c.is_ascii_digit() {
                    self.emit(s, self.plain(), 1);
                    return;
                }
                if self.number_part == 0 && c == b'.' && self.peek(1).is_ascii_digit() {
                    self.number_part = 1;
                    self.emit(s, self.plain(), 1);
                    return;
                }
                if self.number_part < 2
                    && matches!(c, b'e' | b'E')
                    && (self.peek(1).is_ascii_digit()
                        || (matches!(self.peek(1), b'+' | b'-') && self.peek(2).is_ascii_digit()))
                {
                    self.number_part = 2;
                    self.emit(
                        s,
                        self.plain(),
                        if matches!(self.peek(1), b'+' | b'-') {
                            2
                        } else {
                            1
                        },
                    );
                    return;
                }
                if self.ident(0) {
                    self.begin_ident(true);
                } else {
                    self.mode = Mode::Token;
                    if c == b'%' {
                        self.emit(s, self.plain(), 1);
                    }
                }
                return;
            }
            Mode::Token => {}
        }
        if ws(c) {
            self.lexeme(s, true);
            self.emit(s, self.plain(), 1);
        } else if c == b'/' && self.peek(1) == b'*' {
            self.lexeme(s, true);
            self.emit(s, Token::Comment, 2);
            self.mode = Mode::Comment;
        } else if matches!(c, b'"' | b'\'') {
            self.lexeme(s, false);
            self.quote = c;
            self.emit(s, Token::String, 1);
            self.mode = Mode::String;
        } else if c == b'<' && self.peek(1) == b'!' && self.peek(2) == b'-' && self.peek(3) == b'-'
        {
            self.lexeme(s, false);
            self.emit(s, self.plain(), 4);
        } else if c == b'-' && self.peek(1) == b'-' && self.peek(2) == b'>' {
            self.lexeme(s, false);
            self.emit(s, self.plain(), 3);
        } else if (c == b'#' && ((self.size > 1 && name(self.point(1).0)) || self.escape(1)))
            || (c == b'@' && self.ident(1))
        {
            self.lexeme(s, false);
            self.emit(s, self.plain(), 1);
            self.begin_ident(true);
        } else if self.number() {
            self.lexeme(s, false);
            self.mode = Mode::Number;
            self.number_part = 0;
            if matches!(c, b'+' | b'-') {
                self.emit(s, self.plain(), 1);
            }
        } else if self.ident(0) {
            self.lexeme(s, false);
            self.begin_ident(false);
        } else {
            self.lexeme(s, false);
            if c == b'\\' {
                self.invalid = true;
            }
            self.block(s);
        }
    }
}
