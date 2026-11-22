use super::{ASSET_URL_BYTES, ReleaseAssets};

const DEPTH: usize = 16;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Expect { Value, Key, Colon, Next, Done }
#[derive(Clone, Copy, PartialEq, Eq)]
enum Token { None, String, Number, Literal }
// Tags track the active output field and literal cursor without self-referential pointers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Destination { None, Key, Name, Url }
#[derive(Clone, Copy)]
enum Literal { True, False, Null }

pub struct ReleaseJson {
    object: [bool; DEPTH],
    depth: u8,
    // The release sits inside a top-level one-element list (`releases?per_page=1`).
    listed: bool,
    assets_at: u8,
    expect: Expect,
    token: Token,
    failed: bool,
    opened: bool,
    key: bool,
    escape: bool,
    hex_left: u8,
    code: u16,
    into: Destination,
    length: usize,
    usable: bool,
    key_text: [u8; 24],
    key_usable: bool,
    literal: Literal,
    literal_at: u8,
    size_number: bool,
    size_usable: bool,
    digits: u8,
    number: i64,
    name: [u8; 16],
    url: [u8; ASSET_URL_BYTES],
    name_usable: bool,
    url_usable: bool,
    size: i64,
    assets: ReleaseAssets,
}
impl Default for ReleaseJson {
    fn default() -> Self {
        Self {
            object: [false; DEPTH], depth: 0, listed: false, assets_at: 0, expect: Expect::Value, token: Token::None,
            failed: false, opened: false, key: false, escape: false, hex_left: 0, code: 0,
            into: Destination::None, length: 0, usable: true, key_text: [0;24], key_usable: false,
            literal: Literal::True, literal_at: 0, size_number: false, size_usable: false,
            digits: 0, number: 0, name: [0;16], url: [0;ASSET_URL_BYTES],
            name_usable: false, url_usable: false, size: -1, assets: ReleaseAssets::default(),
        }
    }
}
fn space(c: u8) -> bool { matches!(c, b' ' | b'\t' | b'\n' | b'\r') }
fn number_char(c: u8) -> bool { c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.' | b'e' | b'E') }
fn hex_value(c: u8) -> Option<u16> {
    match c { b'0'..=b'9' => Some(u16::from(c-b'0')), b'a'..=b'f' => Some(u16::from(c-b'a'+10)),
        b'A'..=b'F' => Some(u16::from(c-b'A'+10)), _ => None }
}
impl ReleaseJson {
    /// Every call reports the sticky parser result; empty feeds consume nothing.
    /// The orchestration's body-size cap is deliberately not a parser counter.
    pub fn feed(&mut self, bytes: &[u8]) -> bool {
        let mut i = 0;
        while i < bytes.len() && !self.failed {
            if self.step(bytes[i]) { i += 1; }
        }
        !self.failed
    }
    pub fn complete(&self) -> bool {
        !self.failed && self.expect == Expect::Done && self.token == Token::None
    }
    pub fn assets(&self) -> &ReleaseAssets { &self.assets }
    fn in_asset(&self) -> bool { self.assets_at != 0 && self.depth == self.assets_at + 1 }
    /// The depth of the release object's own keys.
    fn release_depth(&self) -> u8 { 1 + u8::from(self.listed) }
    /// Inside the top-level list, outside its release.
    fn in_list(&self) -> bool { self.listed && self.depth == 1 }
    fn fail(&mut self) -> bool { self.failed = true; true }
    fn value_done(&mut self) { self.expect = if self.depth != 0 { Expect::Next } else { Expect::Done }; }
    fn step(&mut self, c: u8) -> bool {
        match self.token {
            Token::String => { self.string(c); return true; }
            Token::Literal => {
                let word: &[u8] = match self.literal { Literal::True => b"rue", Literal::False => b"alse", Literal::Null => b"ull" };
                if c != word[usize::from(self.literal_at)] { return self.fail(); }
                self.literal_at += 1;
                if usize::from(self.literal_at) == word.len() { self.token = Token::None; self.value_done(); }
                return true;
            }
            Token::Number => {
                if !number_char(c) {
                    self.token = Token::None;
                    if self.size_number && self.size_usable && self.digits != 0 { self.size = self.number; }
                    self.value_done();
                    return false;
                }
                if !c.is_ascii_digit() || self.digits == 12 { self.size_usable = false; }
                else { self.number = self.number * 10 + i64::from(c-b'0'); self.digits += 1; }
                return true;
            }
            Token::None => {}
        }
        if space(c) { return true; }
        let fresh = self.opened;
        self.opened = false;
        match self.expect {
            Expect::Done => self.fail(),
            Expect::Colon => {
                if c != b':' { return self.fail(); }
                self.expect = Expect::Value; true
            }
            Expect::Key => {
                if c == b'"' { self.start_string(true); }
                else if c == b'}' && fresh { self.close(); }
                else { self.fail(); }
                true
            }
            Expect::Next => {
                if c == b',' && self.in_list() { self.fail(); }
                else if c == b',' { self.expect = if self.object[usize::from(self.depth)-1] { Expect::Key } else { Expect::Value }; }
                else if c == if self.object[usize::from(self.depth)-1] { b'}' } else { b']' } { self.close(); }
                else { self.fail(); }
                true
            }
            Expect::Value => {
                if c == b']' && fresh && !self.in_list() { self.close(); }
                else if !self.begin(c) { self.fail(); }
                true
            }
        }
    }
    fn begin(&mut self, c: u8) -> bool {
        // The top level is the release, or a list holding exactly one release.
        if self.depth == 0 && c == b'[' { self.listed = true; }
        else if (self.depth == 0 || self.in_list()) && c != b'{' { return false; }
        match c {
            b'{' => { self.open(true); return true; }
            b'[' => { self.open(false); return true; }
            b'"' => { self.start_string(false); return true; }
            b't' => self.literal = Literal::True,
            b'f' => self.literal = Literal::False,
            b'n' => self.literal = Literal::Null,
            _ => {
                if c != b'-' && !c.is_ascii_digit() { return false; }
                self.token = Token::Number;
                self.size_number = self.in_asset() && self.key_is(b"size");
                self.size_usable = c != b'-';
                self.digits = u8::from(c != b'-');
                self.number = if c == b'-' { 0 } else { i64::from(c-b'0') };
                return true;
            }
        }
        self.literal_at = 0;
        self.token = Token::Literal;
        true
    }
    fn open(&mut self, object: bool) {
        if usize::from(self.depth) == DEPTH { self.fail(); return; }
        if !object && self.depth == self.release_depth() && self.key_is(b"assets") && self.assets_at == 0 { self.assets_at = self.depth + 1; }
        if object && self.assets_at != 0 && self.depth == self.assets_at {
            self.name[0] = 0; self.url[0] = 0;
            self.name_usable = false; self.url_usable = false; self.size = -1;
        }
        self.object[usize::from(self.depth)] = object;
        self.depth += 1;
        self.opened = true;
        self.expect = if object { Expect::Key } else { Expect::Value };
    }
    fn close(&mut self) {
        if self.in_asset() && self.object[usize::from(self.depth)-1] {
            let manifest = self.name_usable && c_string_is(&self.name, b"newt.manifest");
            let image = self.name_usable && c_string_is(&self.name, b"newt.bin");
            let url = if manifest { Some(&mut self.assets.manifest) } else if image { Some(&mut self.assets.image) } else { None };
            if let Some(url) = url {
                if url[0] != 0 { self.assets.repeated = true; }
                else if self.url_usable {
                    // Frozen memcpy copies the entire region, including stale tails.
                    url.copy_from_slice(&self.url);
                    if image { self.assets.image_size = self.size; }
                }
            }
        }
        self.depth -= 1;
        if self.depth + 1 == self.assets_at { self.assets_at = 0; }
        self.value_done();
    }
    fn start_string(&mut self, key: bool) {
        self.token = Token::String; self.key = key; self.escape = false; self.hex_left = 0;
        self.length = 0; self.usable = true;
        self.into = if key { Destination::Key }
            else if self.in_asset() && self.key_is(b"name") { Destination::Name }
            else if self.in_asset() && self.key_is(b"browser_download_url") { Destination::Url }
            else { Destination::None };
        match self.into { Destination::Key => self.key_text[0] = 0, Destination::Name => self.name[0] = 0,
            Destination::Url => self.url[0] = 0, Destination::None => {} }
    }
    fn string(&mut self, c: u8) {
        if self.hex_left != 0 {
            let Some(value) = hex_value(c) else { self.fail(); return };
            self.code = self.code * 16 + value;
            self.hex_left -= 1;
            if self.hex_left == 0 { self.put(self.code as u8, self.code >= 0x20 && self.code < 0x7f); }
            return;
        }
        if self.escape {
            self.escape = false;
            match c {
                b'"' | b'\\' | b'/' => self.put(c, true),
                b'b' | b'f' | b'n' | b'r' | b't' => self.put(c, false),
                b'u' => { self.hex_left = 4; self.code = 0; }
                _ => { self.fail(); }
            }
            return;
        }
        if c == b'\\' { self.escape = true; }
        else if c == b'"' { self.end_string(); }
        else if c < 0x20 { self.fail(); }
        else { self.put(c, c < 0x7f); }
    }
    fn put(&mut self, c: u8, usable: bool) {
        let buffer: &mut [u8] = match self.into {
            Destination::None => return,
            Destination::Key => &mut self.key_text,
            Destination::Name => &mut self.name,
            Destination::Url => &mut self.url,
        };
        if !usable || self.length + 1 >= buffer.len() { self.usable = false; return; }
        buffer[self.length] = c;
        self.length += 1;
        buffer[self.length] = 0;
    }
    fn end_string(&mut self) {
        self.token = Token::None;
        if self.key { self.key_usable = self.usable; self.expect = Expect::Colon; return; }
        if self.into == Destination::Name { self.name_usable = self.usable; }
        if self.into == Destination::Url { self.url_usable = self.usable && self.length != 0; }
        self.value_done();
    }
    fn key_is(&self, word: &[u8]) -> bool { self.key_usable && c_string_is(&self.key_text, word) }
}
fn c_string_is(buffer: &[u8], word: &[u8]) -> bool {
    buffer.get(..word.len()) == Some(word) && buffer.get(word.len()) == Some(&0)
}
