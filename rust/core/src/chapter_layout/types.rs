// Derived from pinned FreeInk ChapterLayout.h and Font.h. MIT; see reference.
#![forbid(unsafe_code)]
use crate::css::{CssStylesheet, TextAlign};
pub const BOLD: u8 = 1;
pub const ITALIC: u8 = 2;
pub const UNDERLINE: u8 = 4;
pub const SUP: u8 = 8;
pub const SUB: u8 = 16;
pub use crate::page_renderer::Font;
#[derive(Clone, Copy)]
pub struct Params<'a> {
    pub width: i16,
    pub height: i16,
    pub left: i16,
    pub right: i16,
    pub top: i16,
    pub bottom: i16,
    pub base_size: u16,
    pub language: Option<&'a core::ffi::CStr>,
    pub line_spacing: u16,
    pub paragraph_spacing: u16,
    pub align: TextAlign,
    pub orphan: u8,
    pub widow: u8,
    pub embedded_styles: bool,
    pub focus: bool,
    pub inline_ascent: bool,
    pub stylesheet: Option<CssStylesheet<'a>>,
}
impl Default for Params<'_> {
    fn default() -> Self {
        Self {
            width: 800,
            height: 480,
            left: 24,
            right: 24,
            top: 20,
            bottom: 20,
            base_size: 16,
            language: Some(c"en"),
            line_spacing: 100,
            paragraph_spacing: 100,
            align: TextAlign::Left,
            orphan: 2,
            widow: 2,
            embedded_styles: true,
            focus: false,
            inline_ascent: false,
            stylesheet: None,
        }
    }
}
/// Offsets index the callback-local page byte pool; no pointers or self-borrows.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Bytes {
    pub start: u32,
    pub len: u16,
}
impl Bytes {
    pub fn get(self, pool: &[u8]) -> &[u8] {
        &pool[self.start as usize..self.start as usize + self.len as usize]
    }
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Run {
    pub text: Bytes,
    pub x: i16,
    pub baseline: i16,
    pub size: u16,
    pub flags: u8,
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Image {
    pub href: Bytes,
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Link {
    pub target: Bytes,
    pub fragment: Bytes,
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}
pub struct Page<'a> {
    pub bytes: &'a [u8],
    pub runs: &'a [Run],
    pub images: &'a [Image],
    pub links: &'a [Link],
    pub index: u32,
    pub char_start: u32,
}
pub trait Sink {
    fn anchor(&mut self, _hash: u32, _chars: u32) {}
    fn page(&mut self, page: Page<'_>) -> bool;
}
/// One owner lends font operations and consumes a page synchronously. A
/// rendering sink can reuse its own fonts without overlapping mutable loans.
pub trait Backend: Font + Sink {}
impl<T: Font + Sink> Backend for T {}
pub struct SplitBackend<'a> {
    pub font: &'a mut dyn Font,
    pub sink: &'a mut dyn Sink,
}
impl Font for SplitBackend<'_> {
    fn advance(&mut self, c: u32, s: u16, f: u8) -> i16 {
        self.font.advance(c, s, f)
    }
    fn line_height(&mut self, s: u16) -> i16 {
        self.font.line_height(s)
    }
    fn ascent(&mut self, s: u16) -> i16 {
        self.font.ascent(s)
    }
    fn ligature(&mut self, a: u32, b: u32, f: u8) -> u32 {
        self.font.ligature(a, b, f)
    }
    fn kerning(&mut self, a: u32, b: u32, s: u16, f: u8) -> i16 {
        self.font.kerning(a, b, s, f)
    }
    fn covers(&mut self, c: u32) -> bool {
        self.font.covers(c)
    }
    fn ink_extents(&mut self, c: u32, s: u16, f: u8) -> Option<(i16, i16)> {
        self.font.ink_extents(c, s, f)
    }
}
impl Sink for SplitBackend<'_> {
    fn anchor(&mut self, h: u32, c: u32) {
        self.sink.anchor(h, c);
    }
    fn page(&mut self, p: Page<'_>) -> bool {
        self.sink.page(p)
    }
}
/// This must use pinned libunibreak, not an approximate Rust classifier.
pub trait LineBreak {
    fn breaks(&mut self, text: &[u8], language: Option<&core::ffi::CStr>, out: &mut [u8]);
}
/// Resolved href and dimension probing use the accepted package/ZIP/probe ports.
/// None means missing/unknown; errors are intentionally nonfatal.
/// The implementation borrows a pre-scan table and falls back to inline probing.
/// An empty fragment buffer requests path-only resolution. A nonempty buffer
/// receives the available prefix; layout only publishes the first 47 bytes.
pub trait Images {
    fn resolve(
        &mut self,
        src: &[u8],
        path: &mut [u8],
        fragment: &mut [u8],
    ) -> Option<(usize, usize)>;
    fn probe(&mut self, path: &[u8]) -> Option<(u32, u32)>;
}
#[derive(Clone, Copy, Default)]
pub struct Span {
    pub start: u16,
    pub flags: u8,
    pub size: u16,
    pub margin: u16,
    pub link: u8,
}
#[derive(Clone, Copy, Default)]
pub struct Line {
    pub start: usize,
    pub end: usize,
    pub width: i32,
    pub spaces: u16,
    pub cjk: u16,
    pub hangul: u16,
    pub flags: u8,
}
#[derive(Clone, Copy)]
pub struct Element {
    pub size: u16,
    pub flags: u8,
    pub align: TextAlign,
    pub indent: i16,
    pub hidden: bool,
}
impl Default for Element {
    fn default() -> Self {
        Self {
            size: 100,
            flags: 0,
            align: TextAlign::Left,
            indent: 0,
            hidden: false,
        }
    }
}
#[derive(Clone, Copy, Default)]
pub struct Paragraph {
    pub size: u16,
    pub flags: u8,
    pub align: TextAlign,
    pub indent: i16,
    pub text_indent: i16,
    pub before: u16,
    pub after: u16,
}
#[derive(Clone, Copy)]
pub struct ParLink {
    pub path: [u8; 160],
    pub path_len: usize,
    pub fragment: [u8; 48],
    pub fragment_len: usize,
}
impl Default for ParLink {
    fn default() -> Self {
        Self {
            path: [0; 160],
            path_len: 0,
            fragment: [0; 48],
            fragment_len: 0,
        }
    }
}
/// All large buffers are borrowed. SMALL/standard/LARGE values are source policy,
/// not a RAM budget increase. Root selects the profile and proves RV32 frames.
pub struct Storage<'a> {
    pub text: &'a mut [u8],
    pub breaks: &'a mut [u8],
    pub levels: &'a mut [u8],
    pub spans: &'a mut [Span],
    pub lines: &'a mut [Line],
    pub elements: &'a mut [Element],
    pub runs: &'a mut [Run],
    pub images: &'a mut [Image],
    pub links: &'a mut [Link],
    pub page: &'a mut [u8],
    pub style_text: &'a mut [u8],
    pub paragraph_links: &'a mut [ParLink],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capacity {
    Text,
    Spans,
    Lines,
    Elements,
    Runs,
    Images,
    Links,
    Page,
    Style,
    ParagraphLinks,
}
impl Storage<'_> {
    pub fn validate(&self) -> Result<(), Capacity> {
        for (n, min, d) in [
            (self.text.len(), 2, Capacity::Text),
            (self.breaks.len(), self.text.len(), Capacity::Text),
            (self.levels.len(), self.text.len(), Capacity::Text),
            (self.spans.len(), 1, Capacity::Spans),
            (self.lines.len(), 2, Capacity::Lines),
            (self.elements.len(), 1, Capacity::Elements),
            (self.runs.len(), 1, Capacity::Runs),
            (self.images.len(), 1, Capacity::Images),
            (self.links.len(), 1, Capacity::Links),
            (self.page.len(), 1, Capacity::Page),
            (self.paragraph_links.len(), 1, Capacity::ParagraphLinks),
        ] {
            if n < min || n > u16::MAX as usize {
                return Err(d);
            }
        }
        Ok(())
    }
}
