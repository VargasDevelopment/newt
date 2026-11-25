// MIT. Phased reader stylesheet collection; reuses Core's CSS parser.
use crate::{
    css::{CssDecl, CssRule, CssStylesheet, CssStylesheetBuilder, TextAlign},
    scan::{Scanner, Sink, Token},
    zip_container::{InflateScratch, Status},
    zip_window::{arena::Arena, source::BookSource},
};
pub const ID: u32 = 0x4e53540a;
pub const MAX_RULES: usize = 64;
pub const SDK_BATCH: usize = 512;
pub const EMBEDDED: usize = 4096;
pub const RULE_BYTES: usize = 8192;
pub const EXTERNAL_BYTES: u32 = 256 * 1024;
pub const HEAD_BYTES: u64 = 16384;
pub const HEAD_WINDOW: u32 = 2048;
pub const BODY_DEPTH: u32 = 14;
pub const LIMITED: u16 = 1;
pub const SOURCE_LIMIT: u16 = 1 << 1;
pub const TEXT_LIMIT: u16 = 1 << 2;
pub const RULE_LIMIT: u16 = 1 << 3;
pub const UNSUPPORTED: u16 = 1 << 4;
pub const NUMERIC: u16 = 1 << 5;
pub const VISIBILITY: u16 = 1 << 6;
pub const RESOURCE: u16 = 1 << 7;
pub const HEAD_LIMIT: u16 = 1 << 8;
pub const NESTING: u16 = 1 << 9;
pub const MALFORMED: u16 = 1 << 10;
pub const DAMAGED: u16 = 1 << 11;
pub const TRUNCATED: u16 = 1 << 12;
pub const SELECTOR: u16 = 1 << 13;
pub const COMBINATOR: u16 = 1 << 14;
pub const SYNTAX: u16 = 1 << 15;
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    #[default]
    Empty,
    Deferred,
    Pending,
    Settled,
}
pub struct Slot<'a> {
    pub rules: &'a mut [CssRule; 64],
    pub count: usize,
    pub hash: u32,
    pub spine: u32,
    pub issues: u16,
    pub state: State,
}
impl Slot<'_> {
    pub fn reset(&mut self, spine: u32, state: State, issues: u16) {
        self.count = 0;
        self.hash = ID;
        self.spine = spine;
        self.issues = issues;
        self.state = state;
    }
    pub fn sheet(&self) -> CssStylesheet<'_> {
        CssStylesheet {
            rules: &self.rules[..self.count],
            content_hash: self.hash,
        }
    }
    pub fn limit_nesting(&mut self) {
        self.reset(self.spine, State::Settled, LIMITED | NESTING);
    }
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Link {
    #[default]
    Unsupported,
    Resolved,
    TextLimit,
    ResourceLimit,
    Damaged,
    WorkLimit,
}
/// The resolved record outlives its metadata Book; names and hashes do not.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct Member {
    record: CheckedRecord,
    pub link: Link,
    /// Set only when a resolved stylesheet passes the open-time check.
    checked: bool,
}
// Four sit in every style source record inside the shared bank.
const _: () = assert!(core::mem::size_of::<Member>() == 16);
impl Member {
    fn checked(&self) -> Option<Checked> {
        (self.link == Link::Resolved && self.checked).then(|| self.record.member())
    }
}

pub struct Sources<'a> {
    pub links: [Member; 4],
    pub start: [u16; 8],
    pub length: [u16; 8],
    pub issues: u16,
    pub used: usize,
    pub events: usize,
    pub external_mask: u8,
    pub text: &'a mut [u8],
}
impl Sources<'_> {
    pub fn clear(&mut self) {
        self.links.fill(Member::default());
        self.start.fill(0);
        self.length.fill(0);
        self.issues = 0;
        self.used = 0;
        self.events = 0;
        self.external_mask = 0;
    }
}
/// All large objects are borrowed. No shadow byte-arena reservations.
pub struct BuildScratch<'a> {
    pub text: &'a mut [u8; RULE_BYTES],
    pub marks: &'a mut [u8; RULE_BYTES / 4],
    pub batch: &'a mut [u8; RULE_BYTES],
    pub sdk: &'a mut [CssRule; SDK_BATCH],
    pub pending: &'a mut [CssRule; MAX_RULES],
    pub chunk: &'a mut [u8; 256],
}
fn priority(d: &CssDecl) -> u8 {
    if d.text_indent_pct >= 0 {
        2
    } else if d.size_pct != 0
        || d.weight_bold >= 0
        || d.style_italic >= 0
        || d.align != TextAlign::Inherit
        || d.margin_left_pct >= 0
        || d.margin_top_pct >= 0
        || d.margin_bottom_pct >= 0
        || d.underline >= 0
        || d.vert_align >= 0
    {
        1
    } else {
        0
    }
}
fn fold(hash: &mut u32, n: u32) {
    for b in n.to_le_bytes() {
        *hash = (*hash ^ b as u32).wrapping_mul(16777619);
    }
}
#[derive(Default)]
struct Mark {
    id: u32,
    last: u32,
    trivia: bool,
}
impl Mark {
    fn next(&mut self) -> u8 {
        if self.trivia {
            0
        } else {
            let v = if self.id == self.last { 1 } else { 3 };
            self.last = self.id;
            v
        }
    }
}
struct Check<'a> {
    marks: &'a [u8],
    mark: Mark,
    at: usize,
    equal: bool,
}
impl Sink for Check<'_> {
    fn lexeme(&mut self, id: u32, trivia: bool) {
        self.mark.id = id;
        self.mark.trivia = trivia;
    }
    fn token(&mut self, _: u8, _: Token) {
        self.equal &= ((self.marks[self.at / 4] >> ((self.at % 4) * 2)) & 3) == self.mark.next();
        self.at += 1;
    }
}
struct Admission<'a, 'b, 'c> {
    slot: &'a mut Slot<'b>,
    scratch: &'a mut BuildScratch<'c>,
    scanner: Scanner,
    mark: Mark,
    hash: u32,
    used: usize,
    depth: usize,
    brace: usize,
    first: usize,
    decl_start: usize,
    digits: usize,
    pending: usize,
    minimum: u8,
    bom: usize,
    legacy: bool,
    compacted: bool,
    projection: bool,
    oversized: bool,
    nested: bool,
    at: bool,
    in_list: bool,
    decl_drop: bool,
    seen: bool,
    gap: bool,
    rejected: bool,
    combinator: bool,
}
impl<'a, 'b, 'c> Admission<'a, 'b, 'c> {
    fn new(slot: &'a mut Slot<'b>, scratch: &'a mut BuildScratch<'c>) -> Self {
        Self {
            slot,
            scratch,
            scanner: Scanner::default(),
            mark: Mark::default(),
            hash: ID,
            used: 0,
            depth: 0,
            brace: 0,
            first: 0,
            decl_start: 0,
            digits: 0,
            pending: 0,
            minimum: 0,
            bom: 0,
            legacy: false,
            compacted: false,
            projection: false,
            oversized: false,
            nested: false,
            at: false,
            in_list: false,
            decl_drop: false,
            seen: false,
            gap: false,
            rejected: false,
            combinator: false,
        }
    }
    fn truncate(&mut self, why: u16) {
        self.slot.issues |= TRUNCATED | why;
    }
    fn flush(&mut self, force: bool) {
        if self.pending == 0 && !force {
            return;
        }
        let count = self.slot.count;
        if !self.compacted && count + self.pending <= 64 {
            self.slot.rules[count..count + self.pending]
                .copy_from_slice(&self.scratch.pending[..self.pending]);
            self.slot.count += self.pending;
        } else {
            self.compacted = true;
            let (mut ones, mut twos) = (0usize, 0usize);
            for i in 0..count + self.pending {
                let r = if i < count {
                    self.slot.rules[i]
                } else {
                    self.scratch.pending[i - count]
                };
                match priority(&r.decl) {
                    1 => ones += 1,
                    2 => twos += 1,
                    _ => {}
                }
            }
            if ones + twos > 64 {
                self.truncate(RULE_LIMIT);
            }
            let keep2 = twos.min(64);
            let keep1 = ones.min(64 - keep2);
            ones -= keep1;
            twos -= keep2;
            let mut kept = 0;
            for i in 0..count + self.pending {
                let r = if i < count {
                    self.slot.rules[i]
                } else {
                    self.scratch.pending[i - count]
                };
                let p = priority(&r.decl);
                if p == 0 {
                    continue;
                }
                if p == 1 && ones != 0 {
                    ones -= 1;
                    continue;
                }
                if p == 2 && twos != 0 {
                    twos -= 1;
                    continue;
                }
                self.slot.rules[kept] = r;
                kept += 1;
            }
            self.slot.count = kept;
            self.minimum = if kept == 64 {
                if keep1 != 0 { 1 } else { 2 }
            } else {
                0
            };
            self.pending = 0;
            return;
        }
        self.pending = 0;
        self.minimum = if self.slot.count == 64 {
            self.slot
                .rules
                .iter()
                .map(|r| priority(&r.decl))
                .min()
                .unwrap_or(0)
        } else {
            0
        };
    }
    fn accept(&mut self, mut r: CssRule) {
        if r.decl.display_none >= 0 {
            self.slot.issues |= VISIBILITY;
        }
        let p = priority(&r.decl);
        if p == 0 && (self.compacted || self.slot.count == 64) {
            if !self.compacted {
                self.compacted = true;
                self.flush(true);
            }
            return;
        }
        if self.slot.count == 64 && p < self.minimum {
            self.truncate(RULE_LIMIT);
            return;
        }
        r.decl.display_none = -1;
        self.scratch.pending[self.pending] = r;
        self.pending += 1;
        if self.pending == 64 {
            self.flush(false);
        }
    }
    fn parse_batch(&mut self, bytes: usize, legacy: bool) -> bool {
        let count = {
            let mut b = CssStylesheetBuilder::default();
            if !b.begin(self.scratch.sdk) {
                self.slot.issues |= LIMITED | RESOURCE;
                return true;
            }
            b.add_text(&self.scratch.batch[..bytes]);
            b.finish().rule_count()
        };
        if legacy && count > 64 {
            return false;
        }
        for i in 0..count {
            self.accept(self.scratch.sdk[i]);
        }
        true
    }
    fn parse(&mut self, bytes: usize, legacy: bool, selectors: usize) -> bool {
        if legacy {
            self.scratch.batch[..bytes].copy_from_slice(&self.scratch.text[..bytes]);
            return self.parse_batch(bytes, true);
        }
        let mut first = 0;
        while first < selectors {
            let mut last = first;
            for _ in 0..512 {
                if last >= selectors {
                    break;
                }
                last = self.scratch.text[last..selectors]
                    .iter()
                    .position(|b| *b == b',')
                    .map_or(selectors, |p| last + p + 1);
            }
            let len = last - first - usize::from(last < selectors);
            self.scratch.batch[..len].copy_from_slice(&self.scratch.text[first..first + len]);
            self.scratch.batch[len..len + bytes - selectors]
                .copy_from_slice(&self.scratch.text[selectors..bytes]);
            self.parse_batch(len + bytes - selectors, false);
            first = last;
        }
        true
    }
    fn begin(&mut self, external: bool, bytes: u32) {
        self.scanner = Scanner::default();
        self.used = 0;
        self.digits = 0;
        self.projection = false;
        self.legacy = bytes <= RULE_BYTES as u32;
        self.unit();
        if !self.legacy && !self.compacted {
            self.compacted = true;
            self.flush(true);
        }
        fold(&mut self.hash, u32::from(external));
        fold(&mut self.hash, bytes);
    }
    fn scan_byte(&mut self, c: u8) {
        let mut scanner = core::mem::take(&mut self.scanner);
        scanner.feed(c, self);
        self.scanner = scanner;
    }
    fn feed(&mut self, c: u8) {
        self.hash = (self.hash ^ c as u32).wrapping_mul(16777619);
        if c.is_ascii_digit() {
            self.digits += 1;
        } else {
            self.digits = 0;
        }
        if c == 0 || self.digits > 6 {
            self.slot.issues |= LIMITED | NUMERIC;
        }
        if self.legacy {
            self.append(c);
        }
        self.scan_byte(c);
    }
    fn scan_finish(&mut self) -> bool {
        let mut scanner = core::mem::take(&mut self.scanner);
        scanner.finish(self);
        let invalid = scanner.invalid;
        self.scanner = scanner;
        invalid
    }
    fn end(&mut self) {
        if self.scan_finish() {
            self.truncate(SYNTAX);
            self.projection = true;
        }
        if self.legacy {
            let bytes = self.used;
            if self.slot.issues & LIMITED == 0 && !self.projection && self.parse(bytes, true, 0) {
                return;
            }
            if self.slot.issues & LIMITED != 0 {
                return;
            }
            self.scanner = Scanner::default();
            self.legacy = false;
            self.used = 0;
            self.unit();
            for i in 0..bytes {
                let c = self.scratch.text[i];
                self.scan_byte(c);
            }
            self.scan_finish();
        }
        if self.used != 0 && self.brace != 0 {
            self.declaration();
            self.rule();
        } else if self.oversized {
            self.truncate(TEXT_LIMIT);
        }
    }
    fn finish(&mut self) {
        self.flush(false);
        fold(&mut self.hash, self.slot.issues as u32);
        fold(&mut self.hash, self.slot.count as u32);
        self.slot.hash = self.hash;
    }
    fn unit(&mut self) {
        if !self.legacy {
            self.used = 0;
        }
        self.depth = 0;
        self.brace = 0;
        self.first = 0;
        self.decl_start = 0;
        self.decl_drop = false;
        self.mark.last = 0;
        self.oversized = false;
        self.nested = false;
        self.at = false;
        self.in_list = false;
        self.reset_member();
    }
    fn reset_member(&mut self) {
        self.seen = false;
        self.gap = false;
        self.rejected = false;
        self.combinator = false;
        self.bom = 0;
    }
    fn selector(&mut self, c: u8, k: Token) {
        if crate::css::is_space(c) {
            if self.bom == 3 {
                self.seen = false;
                self.bom = 0;
            }
            self.gap = self.seen;
            return;
        }
        if self.seen && self.gap {
            self.rejected = true;
            if k == Token::Code {
                self.combinator = true;
            }
        }
        self.gap = false;
        self.seen = true;
        if self.bom < 3 && c == [0xef, 0xbb, 0xbf][self.bom] {
            self.bom += 1;
        } else {
            self.bom = 4;
        }
        if matches!(c, b'+' | b'>' | b'~') {
            self.rejected = true;
            if k == Token::Code {
                self.combinator = true;
            }
        }
        if matches!(c, b'#' | b':' | b'[' | b']') {
            self.rejected = true;
        }
        if matches!(k, Token::String | Token::Group)
            || (k == Token::Escape && matches!(c, b'{' | b'}' | b',' | b';' | b'.'))
        {
            if k == Token::Escape {
                self.projection = true;
            }
            if !self.legacy {
                self.rejected = true;
            }
        }
    }
    fn matches(&self, first: usize, last: usize) -> bool {
        let mut scanner = Scanner::default();
        let mut check = Check {
            marks: self.scratch.marks,
            mark: Mark {
                trivia: true,
                ..Mark::default()
            },
            at: first,
            equal: true,
        };
        for c in &self.scratch.text[first..last] {
            scanner.feed(*c, &mut check);
        }
        scanner.finish(&mut check);
        check.equal && check.at == last
    }
    fn member(&mut self) -> bool {
        if !self.legacy && self.seen && !self.oversized && !self.matches(self.first, self.used) {
            self.rejected = true;
        }
        let kept = self.seen && !self.rejected;
        if self.rejected {
            self.projection = true;
            if !self.legacy {
                self.slot.issues |= SELECTOR;
                if self.combinator {
                    self.slot.issues |= COMBINATOR;
                }
            }
        }
        if !self.legacy && !kept {
            self.used = self.first;
        }
        self.reset_member();
        kept
    }
    fn declaration(&mut self) -> bool {
        if self.legacy || self.oversized {
            return false;
        }
        let kept = !self.decl_drop && self.matches(self.decl_start, self.used);
        if !kept {
            self.used = self.decl_start;
            self.truncate(SYNTAX);
        }
        self.decl_drop = false;
        kept && self.used > self.decl_start
    }
    fn append(&mut self, c: u8) {
        if self.used < RULE_BYTES {
            if !self.legacy {
                let shift = (self.used % 4) * 2;
                let mark = self.mark.next();
                self.scratch.marks[self.used / 4] =
                    (self.scratch.marks[self.used / 4] & !(3 << shift)) | (mark << shift);
            }
            self.scratch.text[self.used] = c;
            self.used += 1;
        } else {
            self.oversized = true;
        }
    }
    fn rule(&mut self) {
        if self.oversized || self.nested {
            self.truncate(TEXT_LIMIT);
            return;
        }
        if self.brace != 0 && self.brace < self.used {
            self.parse(self.used, false, self.brace);
        }
    }
}
impl Sink for Admission<'_, '_, '_> {
    fn lexeme(&mut self, id: u32, trivia: bool) {
        self.mark.id = id;
        self.mark.trivia = trivia;
    }
    fn token(&mut self, c: u8, k: Token) {
        let syntax = k == Token::Code;
        if !syntax && matches!(c, b'{' | b'}' | b',' | b';') {
            self.projection = true;
        }
        if self.depth == 0 && !self.at {
            if k == Token::Comment {
                if self.seen || self.in_list {
                    self.projection = true;
                }
                return;
            }
            if syntax && matches!(c, b'{' | b',') {
                let kept = self.member();
                if !self.legacy {
                    if c == b',' {
                        if kept {
                            self.append(c);
                        }
                        self.first = self.used;
                    } else {
                        if !kept && self.used != 0 && self.scratch.text[self.used - 1] == b',' {
                            self.used -= 1;
                        }
                        self.brace = self.used;
                        self.append(c);
                        self.decl_start = self.used;
                    }
                    self.mark.last = 0;
                }
                if c == b'{' {
                    self.depth += 1;
                } else {
                    self.in_list = true;
                }
                return;
            }
            if syntax && c == b'}' {
                self.unit();
                return;
            }
            if !self.seen && syntax && c == b'@' {
                self.at = true;
            }
            if !self.at {
                self.selector(c, k);
                if !self.legacy && (self.seen || !crate::css::is_space(c)) {
                    self.append(c);
                }
                return;
            }
        }
        if !self.legacy && !self.at {
            if k == Token::Comment {
                return;
            }
            if !syntax && matches!(c, b'{' | b'}' | b';') {
                self.decl_drop = true;
            }
            if syntax && self.depth == 1 && matches!(c, b';' | b'}') {
                let kept = self.declaration();
                if c == b'}' || kept {
                    self.append(c);
                }
                self.decl_start = self.used;
                self.mark.last = 0;
            } else {
                self.append(c);
            }
        }
        if !syntax {
            return;
        }
        if c == b'{' {
            if self.depth != 0 && !self.at {
                self.nested = true;
            }
            self.depth += 1;
        }
        let closed = if c == b'}' {
            if self.depth == 0 {
                true
            } else {
                self.depth -= 1;
                self.depth == 0
            }
        } else {
            false
        };
        if closed || (self.at && self.depth == 0 && c == b';') {
            if !self.legacy && !self.at {
                self.rule();
            }
            self.unit();
        }
    }
}
fn link_issue(link: Link) -> u16 {
    match link {
        Link::Resolved => 0,
        Link::WorkLimit => TRUNCATED | RESOURCE,
        Link::TextLimit => LIMITED | TEXT_LIMIT,
        Link::ResourceLimit => LIMITED | RESOURCE,
        Link::Damaged => LIMITED | DAMAGED,
        Link::Unsupported => LIMITED | UNSUPPORTED,
    }
}
include!("style_source.rs");

fn reborrow<'b>(d: &'b mut Option<InflateScratch<'_>>) -> Option<InflateScratch<'b>> {
    d.as_mut().map(|d| InflateScratch {
        state: &mut *d.state,
        dictionary: &mut *d.dictionary,
        input: &mut *d.input,
    })
}
/// None scratch is the preflight allocation failure, before any member reads.
pub fn build<S: BookSource + ?Sized>(
    source: &mut S,
    sources: &Sources<'_>,
    scratch: Option<&mut BuildScratch<'_>>,
    mut decoder: Option<InflateScratch<'_>>,
    on_yield: newt_core::admission::Yield<'_>,
    slot: &mut Slot<'_>,
    spine: u32,
    policy: Option<&dyn crate::xml_session::SourcePolicy>,
) -> Status {
    slot.reset(spine, State::Empty, sources.issues);
    if sources.issues & LIMITED == 0 && sources.events != 0 {
        let Some(scratch) = scratch else {
            slot.reset(spine, State::Empty, 0);
            return Status::OutOfMemory;
        };
        let mut admission = Admission::new(slot, scratch);
        let mut issues = sources.issues;
        let mut link = 0;
        for i in 0..sources.events {
            if (issues | admission.slot.issues) & LIMITED != 0 {
                break;
            }
            if sources.external_mask & (1 << i) == 0 {
                admission.begin(false, sources.length[i] as u32);
                let first = sources.start[i] as usize;
                for &c in &sources.text[first..first + sources.length[i] as usize] {
                    admission.feed(c);
                }
                admission.end();
                continue;
            }
            let m = &sources.links[link];
            link += 1;
            issues |= link_issue(m.link);
            let Some(entry) = m.checked() else {
                continue;
            };
            let bytes = source.size();
            let mut tracked = FaultTrack {
                source,
                bytes,
                fault: false,
            };
            let status = {
                match crate::admission::ChapterSource::open(
                    &mut tracked,
                    &entry,
                    reborrow(&mut decoder),
                    on_yield,
                    policy,
                ) {
                    Err(e) => e,
                    Ok(mut chapter) => {
                        admission.begin(true, entry.uncompressed_size);
                        let mut at = 0;
                        let mut status = Status::Ok;
                        while at < entry.uncompressed_size {
                            let want = (entry.uncompressed_size - at).min(256) as usize;
                            let n =
                                chapter.read_at(at as u64, &mut admission.scratch.chunk[..want]);
                            if n <= 0 {
                                status = Status::IoError;
                                break;
                            }
                            for j in 0..n as usize {
                                let c = admission.scratch.chunk[j];
                                admission.feed(c);
                            }
                            at += n as u32;
                        }
                        if status == Status::Ok && !chapter.finish() {
                            status = Status::Truncated;
                        }
                        status
                    }
                }
            };
            if status != Status::Ok {
                if status != Status::OutOfMemory && tracked.damaged() {
                    issues |= LIMITED | DAMAGED;
                } else {
                    admission.slot.reset(spine, State::Empty, 0);
                    return status;
                }
            } else {
                admission.end();
            }
        }
        admission.slot.issues |= issues;
        if admission.slot.issues & LIMITED != 0 {
            let why = admission.slot.issues;
            admission.slot.reset(spine, State::Empty, why);
        } else {
            admission.finish();
        }
    }
    slot.state = if slot.count != 0 {
        State::Pending
    } else {
        State::Settled
    };
    Status::Ok
}
use crate::{
    package_parsers::ManifestItem,
    xml_input::{c_bytes, dir_name, local_is, resolve_href},
    xml_session::{Attributes, Handler, ParserBackend, Resources, XmlStream},
};
use newt_core::canonical::CatalogView;
use newt_core::member_check::{Checked, CheckedRecord};
pub struct Book<'a, 's> {
    pub manifest: &'a [ManifestItem],
    pub arena: &'a Arena<'s>,
    pub catalog: Option<CatalogView<'a, 's>>,
}
impl Book<'_, '_> {
    pub fn declares_stylesheet(&self) -> bool {
        self.manifest
            .iter()
            .any(|m| m.media_type.and_then(|r| self.arena.c_string(r)) == Some(b"text/css"))
    }
}
fn optional(value: Option<&[u8]>, expected: &[u8]) -> bool {
    value.is_none_or(|v| {
        let mut v = v;
        while v.first().is_some_and(|c| crate::css::is_space(*c)) {
            v = &v[1..];
        }
        while v.last().is_some_and(|c| crate::css::is_space(*c)) {
            v = &v[..v.len() - 1];
        }
        v.is_empty() || v.eq_ignore_ascii_case(expected)
    })
}
fn token(value: Option<&[u8]>, word: &[u8]) -> bool {
    value.is_some_and(|v| {
        v.split(|c| crate::css::is_space(*c))
            .any(|v| v.eq_ignore_ascii_case(word))
    })
}
pub fn local_href(href: Option<&[u8]>) -> bool {
    let Some(href) = href else {
        return false;
    };
    if href.is_empty() || href.len() > 511 || href.starts_with(b"//") {
        return false;
    }
    let mut i = 0;
    while i < href.len() {
        let mut c = href[i];
        if c == b'%' {
            let Some(pair) = href.get(i + 1..i + 3) else {
                return false;
            };
            let mut v = 0;
            for b in pair {
                let Some(n) = (*b as char).to_digit(16) else {
                    return false;
                };
                v = v * 16 + n;
            }
            c = v as u8;
            i += 2;
        }
        if c <= 32 || matches!(c, 127 | b':' | b'?' | b'#' | b'\\') {
            return false;
        }
        i += 1;
    }
    true
}
struct SourceScan<'a, 'b, 'c, 'd, 's> {
    out: &'a mut Sources<'b>,
    book: &'a Book<'c, 's>,
    directory: Option<&'a [u8]>,
    arena: &'a mut Arena<'d>,
    status: Status,
    depth: u32,
    style_depth: u32,
    head: u8,
    links: usize,
    /// Catalog index of each resolved link, checked after the scan.
    members: [u16; 4],
    stop: bool,
}
impl SourceScan<'_, '_, '_, '_, '_> {
    fn limit(&mut self, why: u16) {
        self.out.issues |= LIMITED | why;
    }
    fn check(&mut self, href: &[u8]) -> Member {
        let mut out = Member::default();
        let Some(dir) = self.directory else {
            return out;
        };
        let mark = self.arena.mark();
        if self.arena.alloc(512, 1).is_none() {
            self.status = Status::OutOfMemory;
            return out;
        }
        self.arena.release(mark);
        let path = resolve_href(self.arena, Some(dir), Some(href), None);
        if let Some(path) = path.and_then(|r| self.arena.view(r)) {
            let path = c_bytes(path);
            let (mut css, mut contradictory) = (false, false);
            for item in self.book.manifest {
                if item.href.and_then(|r| self.book.arena.c_string(r)) == Some(path) {
                    if item.media_type.and_then(|r| self.book.arena.c_string(r))
                        == Some(b"text/css")
                    {
                        css = true;
                    } else {
                        contradictory = true;
                    }
                }
            }
            if css && !contradictory {
                if let Some((index, entry)) = self.book.catalog.and_then(|z| z.find(path)) {
                    if matches!(entry.method, 0 | 8) {
                        out.link = if entry.method == 0
                            && entry.compressed_size != entry.uncompressed_size
                        {
                            Link::Damaged
                        } else if entry.uncompressed_size > EXTERNAL_BYTES {
                            Link::WorkLimit
                        } else if !crate::admission::inflate_input_bounded(&entry) {
                            Link::ResourceLimit
                        } else {
                            Link::Resolved
                        };
                        if out.link == Link::Resolved {
                            self.members[self.links] = index as u16;
                        }
                    }
                }
            }
        } else {
            out.link = Link::ResourceLimit;
        }
        self.arena.release(mark);
        out
    }
}
impl Handler for SourceScan<'_, '_, '_, '_, '_> {
    fn stop_parse(&self) -> bool {
        self.stop
    }
    fn start_element(&mut self, name: &[u8], a: &mut dyn Attributes) {
        if self.stop {
            return;
        }
        self.depth += 1;
        if self.head == 2 && !local_is(name, b"head") && !local_is(name, b"body") {
            return;
        }
        if a.exact(b"xml:base").is_some() {
            self.limit(UNSUPPORTED);
        }
        if self.style_depth != 0 {
            self.limit(UNSUPPORTED);
            return;
        }
        if local_is(name, b"body") {
            self.stop = true;
            return;
        }
        if local_is(name, b"head") {
            if self.head != 0 {
                self.limit(UNSUPPORTED);
            }
            self.head = 1;
            return;
        }
        if self.head != 1 {
            return;
        }
        if local_is(name, b"base") {
            self.limit(UNSUPPORTED);
        }
        if local_is(name, b"link") {
            let rel = a.exact(b"rel");
            if !token(rel, b"stylesheet") {
                return;
            }
            let href = a.exact(b"href");
            if token(rel, b"alternate")
                || a.exact(b"disabled").is_some()
                || !optional(a.exact(b"type"), b"text/css")
                || !optional(a.exact(b"media"), b"all")
                || !local_href(href)
            {
                self.limit(UNSUPPORTED);
                return;
            }
            if self.out.events == 8 || self.links == 4 {
                self.limit(SOURCE_LIMIT);
                return;
            }
            let m = self.check(href.unwrap_or(b""));
            self.out.links[self.links] = m;
            if self.status != Status::Ok {
                self.stop = true;
                return;
            }
            self.links += 1;
            self.out.external_mask |= 1 << self.out.events;
            self.out.events += 1;
        } else if local_is(name, b"style") {
            if !optional(a.exact(b"type"), b"text/css") || !optional(a.exact(b"media"), b"all") {
                self.limit(UNSUPPORTED);
            }
            if self.out.events == 8 {
                self.limit(SOURCE_LIMIT);
                return;
            }
            self.out.start[self.out.events] = self.out.used as u16;
            self.out.events += 1;
            self.style_depth = self.depth;
        }
    }
    fn end_element(&mut self, name: &[u8]) {
        if self.stop {
            return;
        }
        if self.style_depth == self.depth && local_is(name, b"style") {
            self.style_depth = 0;
        }
        if local_is(name, b"head") {
            self.head = 2;
        }
        self.depth = self.depth.saturating_sub(1);
    }
    fn text(&mut self, text: &[u8]) {
        if self.stop || self.style_depth == 0 || text.is_empty() {
            return;
        }
        if text.len() > EMBEDDED - self.out.used {
            self.limit(TEXT_LIMIT);
            return;
        }
        if text.len() > self.out.text.len() - self.out.used {
            self.limit(RESOURCE);
            return;
        }
        self.out.text[self.out.used..self.out.used + text.len()].copy_from_slice(text);
        self.out.used += text.len();
        self.out.length[self.out.events - 1] += text.len() as u16;
    }
}
/// The two decoder loans are distinct: compressed chapter -> sequential decoded
/// source -> raw XML session. The raw session needs no inflater storage.
pub struct ResolveScratch<'a, 'storage> {
    pub directory: &'a mut [u8; 512],
    pub paths: &'a mut Arena<'storage>,
    pub decoder: Option<InflateScratch<'a>>,
    pub xml: Resources<'a>,
}
pub fn resolve<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    book: &Book<'_, '_>,
    source: &mut S,
    chapter: &Checked,
    href: Option<&[u8]>,
    backend: &mut B,
    scratch: ResolveScratch<'_, '_>,
    on_yield: newt_core::admission::Yield<'_>,
    out: &mut Sources<'_>,
    policy: Option<&dyn crate::xml_session::SourcePolicy>,
) -> Status {
    out.clear();
    let mark = scratch.paths.mark();
    let located = href.is_some_and(|h| dir_name(h, scratch.directory));
    let bytes = source.size();
    let mut tracked = FaultTrack {
        source,
        bytes,
        fault: false,
    };
    let mut handler = SourceScan {
        out,
        book,
        directory: if located {
            Some(c_bytes(scratch.directory))
        } else {
            None
        },
        arena: scratch.paths,
        status: Status::Ok,
        depth: 0,
        style_depth: 0,
        head: 0,
        links: 0,
        members: [0; 4],
        stop: false,
    };
    let mut status = match crate::admission::ChapterSource::open(
        &mut tracked,
        chapter,
        scratch.decoder,
        on_yield,
        policy,
    ) {
        Err(e) => e,
        Ok(mut decoded) => {
            let raw = Checked::raw(chapter.uncompressed_size);
            let mut session = XmlStream::with_policy(&mut decoded, None, backend, crate::xml_session::OptionalPolicy(policy));
            let mut status = session.open(&raw, scratch.xml, true);
            let mut end = false;
            while status == Status::Ok && !end && !handler.stop {
                if session.bytes_consumed() >= HEAD_BYTES {
                    handler.limit(HEAD_LIMIT);
                    break;
                }
                status = session.feed_chunk(&mut handler, Some(&mut end));
                if session.bytes_consumed() > HEAD_BYTES {
                    handler.limit(HEAD_LIMIT);
                }
                if status == Status::Ok && !end && !handler.stop {
                    if let Some(f) = on_yield {
                        f.yield_now();
                    }
                }
            }
            status
        }
    };
    // The scan cannot read members while the chapter streams, so stylesheets
    // are checked here. A damaged one is skipped, as a missing one is; a read
    // failure fails the scan, as it did when the stylesheet was opened.
    if let Some(catalog) = book.catalog {
        let SourceScan { out, members, links, .. } = &mut handler;
        for (member, &index) in out.links[..*links].iter_mut().zip(members.iter()) {
            if member.link != Link::Resolved {
                continue;
            }
            match catalog.entry(usize::from(index)).map(|entry| catalog.check(&mut tracked, entry)) {
                Some(Ok(checked)) => (member.record, member.checked) = (CheckedRecord::from(&checked), true),
                Some(Err(Status::IoError)) => {
                    status = Status::IoError;
                    break;
                }
                None | Some(Err(_)) => *member = Member { link: Link::Damaged, ..Member::default() },
            }
        }
    }
    if handler.status != Status::Ok {
        status = handler.status;
    } else if status == Status::ParseError
        || (status != Status::Ok && status != Status::OutOfMemory && tracked.damaged())
    {
        handler.limit(MALFORMED);
        status = Status::Ok;
    }
    if status != Status::Ok {
        handler.out.clear();
    } else if handler.out.issues & LIMITED == 0 && handler.out.events != 0 && !located {
        handler.limit(UNSUPPORTED);
    }
    handler.arena.release(mark);
    status
}
/// Recapture when open metadata constrained embedded storage; preserve link
/// facts, reject changed event topology before using the rebuilt profile.
pub fn reconcile_capture(old: &Sources<'_>, again: &mut Sources<'_>) -> Status {
    if old.events != again.events || old.external_mask != again.external_mask {
        return Status::Stale;
    }
    again.links = old.links;
    Status::Ok
}
