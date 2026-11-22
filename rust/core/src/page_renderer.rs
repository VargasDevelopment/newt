//! FreeInk PageRenderer and FontChain port. See provenance.json and CONTRACT.md.
//! No font engine or compressed-image decoder is supplied by this module.
#![forbid(unsafe_code)]

pub const STYLE_BOLD: u8 = 1;
pub const STYLE_ITALIC: u8 = 2;
pub const STYLE_UNDERLINE: u8 = 4;
pub const STYLE_SUPERSCRIPT: u8 = 8;
pub const STYLE_SUBSCRIPT: u8 = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SafetyError {
    Frame,
    PageCount,
    TextLength,
    GlyphSlice,
    Coordinates,
    RowSlice,
    BankIndex,
}
#[derive(Clone, Copy)]
pub struct GlyphBitmap<'a> {
    pub pixels: &'a [u8],
    pub width: u16,
    pub height: u16,
    pub xoff: i16,
    pub yoff: i16,
    pub advance: i16,
}
pub trait Font {
    fn advance(&mut self, cp: u32, size: u16, style: u8) -> i16;
    fn line_height(&mut self, size: u16) -> i16;
    fn ascent(&mut self, size: u16) -> i16;
    fn ligature(&mut self, _left: u32, _right: u32, _style: u8) -> u32 {
        0
    }
    fn kerning(&mut self, _left: u32, _right: u32, _size: u16, _style: u8) -> i16 {
        0
    }
    // Raster implementations must delegate to has_glyph; metrics fakes may return true.
    fn covers(&mut self, cp: u32) -> bool;
}
/// The returned bitmap borrows the font until the next mutable font call.
pub trait RasterFont: Font {
    fn has_glyph(&mut self, cp: u32) -> bool;
    fn rasterize(&mut self, cp: u32, size: u16) -> Option<GlyphBitmap<'_>>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Selection {
    pub bank_index: usize,
    pub face_flags: u8,
}
#[derive(Clone, Copy, Default)]
struct Entry {
    index: usize,
    flags: u8,
}
/// Each font object has exactly one bank slot; registrations may repeat its index.
pub struct FontChain<'a, 'f> {
    bank: &'a mut [&'f mut dyn RasterFont],
    entries: [Entry; 8],
    count: usize,
    coverage: u8,
}
impl<'a, 'f> FontChain<'a, 'f> {
    pub fn new(bank: &'a mut [&'f mut dyn RasterFont]) -> Self {
        Self {
            bank,
            entries: [Entry::default(); 8],
            count: 0,
            coverage: 0,
        }
    }
    pub fn add(&mut self, index: usize, flags: u8) -> bool {
        if index >= self.bank.len() || self.count >= 8 {
            return false;
        }
        let flags = flags & 3;
        self.entries[self.count] = Entry { index, flags };
        self.count += 1;
        self.coverage |= if flags == 0 { 4 } else { flags };
        true
    }
    pub fn style_coverage(&self) -> u8 {
        self.coverage
    }
    pub fn font_for(&mut self, cp: u32, style: u8) -> Option<Selection> {
        let want = style & 3;
        let mut best = None;
        let mut best_score = -1;
        for entry in &self.entries[..self.count] {
            if !self.bank[entry.index].has_glyph(cp) {
                continue;
            }
            let have = entry.flags;
            let score = if have == want {
                3
            } else if have != 0 && have & want == have {
                2
            } else if have == 0 {
                1
            } else {
                0
            };
            if score > best_score {
                best_score = score;
                best = Some(Selection {
                    bank_index: entry.index,
                    face_flags: have,
                });
            }
        }
        if best.is_none() && self.count > 0 {
            let e = self.entries[0];
            best = Some(Selection {
                bank_index: e.index,
                face_flags: e.flags,
            });
        }
        best
    }
    pub fn rasterize(
        &mut self,
        selected: Selection,
        cp: u32,
        size: u16,
    ) -> Result<Option<GlyphBitmap<'_>>, SafetyError> {
        let font = self
            .bank
            .get_mut(selected.bank_index)
            .ok_or(SafetyError::BankIndex)?;
        Ok(font.rasterize(cp, size))
    }
}
impl Font for FontChain<'_, '_> {
    fn advance(&mut self, cp: u32, size: u16, style: u8) -> i16 {
        match self.font_for(cp, style) {
            Some(s) => self.bank[s.bank_index].advance(cp, size, style),
            None => 0,
        }
    }
    fn line_height(&mut self, size: u16) -> i16 {
        if self.count == 0 {
            size as i16
        } else {
            self.bank[self.entries[0].index].line_height(size)
        }
    }
    fn ascent(&mut self, size: u16) -> i16 {
        if self.count == 0 {
            size as i16
        } else {
            self.bank[self.entries[0].index].ascent(size)
        }
    }
    fn kerning(&mut self, left: u32, right: u32, size: u16, style: u8) -> i16 {
        let a = self.font_for(left, style);
        let b = self.font_for(right, style);
        match (a, b) {
            (Some(a), Some(b)) if a.bank_index == b.bank_index => {
                self.bank[a.bank_index].kerning(left, right, size, style)
            }
            _ => 0,
        }
    }
    fn ligature(&mut self, left: u32, right: u32, style: u8) -> u32 {
        let a = self.font_for(left, style);
        let b = self.font_for(right, style);
        let (Some(a), Some(b)) = (a, b) else {
            return 0;
        };
        if a.bank_index != b.bank_index {
            return 0;
        }
        let lig = self.bank[a.bank_index].ligature(left, right, style);
        if lig != 0 && self.font_for(lig, style).map(|s| s.bank_index) == Some(a.bank_index) {
            lig
        } else {
            0
        }
    }
    fn covers(&mut self, cp: u32) -> bool {
        for e in &self.entries[..self.count] {
            if self.bank[e.index].has_glyph(cp) {
                return true;
            }
        }
        false
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameFormat {
    Mono1Dithered,
    Mono1Sharp,
    Gray8,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameRotation {
    None,
    Portrait,
    PortraitInverted,
    UpsideDown,
}
pub struct FrameTarget<'a> {
    pub framebuffer: &'a mut [u8],
    pub width: i16,
    pub height: i16,
    pub width_bytes: i16,
    pub format: FrameFormat,
    pub rotation: FrameRotation,
}
impl FrameTarget<'_> {
    pub fn validate(&self) -> Result<(), SafetyError> {
        if self.width <= 0 || self.height <= 0 || self.width_bytes <= 0 {
            return Err(SafetyError::Frame);
        }
        let row = if self.format == FrameFormat::Gray8 {
            self.width as usize
        } else {
            (self.width as usize + 7) / 8
        };
        if (self.width_bytes as usize) < row
            || self.framebuffer.len() < self.height as usize * self.width_bytes as usize
        {
            return Err(SafetyError::Frame);
        }
        Ok(())
    }
    fn panel(&self, x: i32, y: i32) -> Option<(usize, usize)> {
        let (w, h) = (i32::from(self.width), i32::from(self.height));
        let lw = if matches!(
            self.rotation,
            FrameRotation::Portrait | FrameRotation::PortraitInverted
        ) {
            h
        } else {
            w
        };
        // Preserve the original equality branch, including square panels.
        let lh = if lw == h { w } else { h };
        if x < 0 || y < 0 || x >= lw || y >= lh {
            return None;
        }
        let (px, py) = match self.rotation {
            FrameRotation::Portrait => (y, h - 1 - x),
            FrameRotation::PortraitInverted => (w - 1 - y, x),
            FrameRotation::UpsideDown => (w - 1 - x, h - 1 - y),
            FrameRotation::None => (x, y),
        };
        Some((px as usize, py as usize))
    }
    fn black(&mut self, x: i32, y: i32) {
        if let Some((px, py)) = self.panel(x, y) {
            self.framebuffer[py * self.width_bytes as usize + (px >> 3)] &= !(0x80 >> (px & 7));
        }
    }
    fn ink(&mut self, x: i32, y: i32, c: u8) {
        if c == 0 {
            return;
        }
        match self.format {
            FrameFormat::Gray8 => {
                if let Some((px, py)) = self.panel(x, y) {
                    let p = &mut self.framebuffer[py * self.width_bytes as usize + px];
                    *p = (*p).min(255 - c);
                }
            }
            FrameFormat::Mono1Sharp => {
                if c >= 120 {
                    self.black(x, y);
                }
            }
            FrameFormat::Mono1Dithered => {
                const BAYER: [[u8; 4]; 4] = [
                    [15, 135, 45, 165],
                    [195, 75, 225, 105],
                    [60, 180, 30, 150],
                    [240, 120, 210, 90],
                ];
                if c >= 140 {
                    self.black(x, y);
                } else if c >= 40
                    && (u32::from(c - 40) * 255) / 100
                        >= u32::from(BAYER[(y & 3) as usize][(x & 3) as usize])
                {
                    self.black(x, y);
                }
            }
        }
    }
}
#[derive(Clone, Copy)]
pub struct PageTextRun<'a> {
    pub text: &'a [u8],
    pub x: i16,
    pub baseline_y: i16,
    pub size_px: u16,
    pub style_flags: u8,
}
#[derive(Clone, Copy)]
pub struct PageImage<'a> {
    pub href: &'a [u8],
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}
#[derive(Clone, Copy)]
pub struct PageLink<'a> {
    pub target: &'a [u8],
    pub fragment: &'a [u8],
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}
pub struct Page<'a> {
    pub runs: &'a [PageTextRun<'a>],
    pub images: &'a [PageImage<'a>],
    pub links: &'a [PageLink<'a>],
    pub page_index: u32,
    pub char_start: u32,
}
/// A callback-scoped view also accepts offset-backed layout pages without a
/// second descriptor array or a reference into its own owner.
pub trait PageView {
    fn run_count(&self) -> usize;
    fn image_count(&self) -> usize;
    fn link_count(&self) -> usize;
    fn run(&self, index: usize) -> Result<PageTextRun<'_>, SafetyError>;
    fn image(&self, index: usize) -> Result<PageImage<'_>, SafetyError>;
}
impl PageView for Page<'_> {
    fn run_count(&self) -> usize {
        self.runs.len()
    }
    fn image_count(&self) -> usize {
        self.images.len()
    }
    fn link_count(&self) -> usize {
        self.links.len()
    }
    fn run(&self, index: usize) -> Result<PageTextRun<'_>, SafetyError> {
        self.runs.get(index).copied().ok_or(SafetyError::PageCount)
    }
    fn image(&self, index: usize) -> Result<PageImage<'_>, SafetyError> {
        self.images
            .get(index)
            .copied()
            .ok_or(SafetyError::PageCount)
    }
}
fn validate_page<P: PageView + ?Sized>(page: &P) -> Result<(), SafetyError> {
    if page.run_count() > 65535 || page.image_count() > 65535 || page.link_count() > 65535 {
        return Err(SafetyError::PageCount);
    }
    for index in 0..page.run_count() {
        if page.run(index)?.text.len() > 65535 {
            return Err(SafetyError::TextLength);
        }
    }
    for index in 0..page.image_count() {
        page.image(index)?;
    }
    Ok(())
}
// Intentionally not the accepted scalar UTF-8 helper: this is byte-tolerant.
fn decode_utf8(text: &[u8], i: &mut usize) -> u32 {
    let b = text[*i];
    let (mut cp, mut extra) = if b >= 0xf0 {
        (u32::from(b & 7), 3)
    } else if b >= 0xe0 {
        (u32::from(b & 15), 2)
    } else if b >= 0xc0 {
        (u32::from(b & 31), 1)
    } else {
        (u32::from(b), 0)
    };
    *i += 1;
    while extra > 0 && *i < text.len() && text[*i] & 0xc0 == 0x80 {
        cp = (cp << 6) | u32::from(text[*i] & 63);
        *i += 1;
        extra -= 1;
    }
    cp
}
pub fn render_text<P: PageView + ?Sized>(
    page: &P,
    fonts: &mut FontChain<'_, '_>,
    target: &mut FrameTarget<'_>,
    mut first_missing: Option<&mut u32>,
) -> Result<u32, SafetyError> {
    validate_page(page)?;
    target.validate()?;
    let mut missing = 0u32;
    for index in 0..page.run_count() {
        let run = page.run(index)?;
        let mut pen = i32::from(run.x);
        let mut i = 0;
        let mut prev = 0;
        while i < run.text.len() {
            let cp = decode_utf8(run.text, &mut i);
            if prev != 0 {
                pen = pen
                    .checked_add(i32::from(fonts.kerning(
                        prev,
                        cp,
                        run.size_px,
                        run.style_flags,
                    )))
                    .ok_or(SafetyError::Coordinates)?;
            }
            let selected = fonts.font_for(cp, run.style_flags);
            let glyph = match selected {
                Some(s) => fonts.rasterize(s, cp, run.size_px)?,
                None => None,
            };
            if glyph.is_none() && cp != 32 && cp != 0xa0 {
                if missing == 0 {
                    if let Some(out) = first_missing.as_deref_mut() {
                        *out = cp;
                    }
                }
                missing += 1;
            }
            if let Some(g) = glyph {
                if g.pixels.len() < usize::from(g.width) * usize::from(g.height) {
                    return Err(SafetyError::GlyphSlice);
                }
                let strikes = if run.style_flags & STYLE_BOLD != 0
                    && selected.map_or(0, |s| s.face_flags) & STYLE_BOLD == 0
                {
                    2
                } else {
                    1
                };
                for s in 0..strikes {
                    for gy in 0..g.height {
                        for gx in 0..g.width {
                            let x = pen
                                .checked_add(i32::from(g.xoff) + i32::from(gx) + s)
                                .ok_or(SafetyError::Coordinates)?;
                            let y = i32::from(run.baseline_y) + i32::from(g.yoff) + i32::from(gy);
                            target.ink(
                                x,
                                y,
                                g.pixels[usize::from(gy) * usize::from(g.width) + usize::from(gx)],
                            );
                        }
                    }
                }
            }
            pen = pen
                .checked_add(i32::from(fonts.advance(cp, run.size_px, run.style_flags)))
                .ok_or(SafetyError::Coordinates)?;
            prev = cp;
        }
        if run.style_flags & STYLE_UNDERLINE != 0 {
            let y = i32::from(run.baseline_y) + if run.size_px >= 24 { 3 } else { 2 };
            let thickness = if run.size_px >= 28 { 2 } else { 1 };
            for t in 0..thickness {
                for x in i32::from(run.x)..pen {
                    target.ink(x, y + t, 255);
                }
            }
        }
    }
    Ok(missing)
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum BookStatus {
    Ok,
    IoError,
    NotZip,
    NotEpub,
    Encrypted,
    Truncated,
    Unsupported,
    OutOfMemory,
    ParseError,
    NotFound,
    Stale,
}
/// Byte-arena admission is independent of typed error-row capacity.
pub struct ByteArena<'a> {
    bytes: &'a mut [u8],
    used: usize,
}
impl<'a> ByteArena<'a> {
    pub fn new(bytes: &'a mut [u8]) -> Self {
        Self { bytes, used: 0 }
    }
    pub fn mark(&self) -> usize {
        self.used
    }
    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.used
    }
    pub fn allocate(&mut self, len: usize, align: usize) -> Option<&mut [u8]> {
        if !align.is_power_of_two() {
            return None;
        }
        let start = self.used.checked_add(align - 1)? & !(align - 1);
        let end = start.checked_add(len)?;
        if end > self.bytes.len() {
            return None;
        }
        self.used = end;
        Some(&mut self.bytes[start..end])
    }
    fn release(&mut self, mark: usize) {
        self.used = mark;
    }
}
pub trait ImageRows {
    fn on_row(&mut self, y: u16, gray: &[u8], width: u16) -> Result<bool, SafetyError>;
}
/// Implementations own source/catalog access and decoder admission; no decoder is implied.
pub trait ImageDecoder {
    fn render(
        &mut self,
        image: &PageImage<'_>,
        scratch: &mut ByteArena<'_>,
        rows: &mut dyn ImageRows,
    ) -> Result<BookStatus, SafetyError>;
}
pub struct ImageScratch<'a> {
    pub err: &'a mut [i16],
    pub err_next: &'a mut [i16],
    pub arena: ByteArena<'a>,
}
struct ImageBlit<'a, 'b> {
    target: &'a mut FrameTarget<'b>,
    x: i16,
    y: i16,
    err: &'a mut [i16],
    next: &'a mut [i16],
    width: usize,
}
impl ImageRows for ImageBlit<'_, '_> {
    fn on_row(&mut self, y: u16, gray: &[u8], width: u16) -> Result<bool, SafetyError> {
        if gray.len() < usize::from(width) {
            return Err(SafetyError::RowSlice);
        }
        if self.target.format == FrameFormat::Gray8 {
            for i in 0..usize::from(width) {
                if let Some((px, py)) = self.target.panel(
                    i32::from(self.x) + i as i32,
                    i32::from(self.y) + i32::from(y),
                ) {
                    let offset = py * self.target.width_bytes as usize + px;
                    self.target.framebuffer[offset] = gray[i];
                }
            }
            return Ok(true);
        }
        let width = usize::from(width).min(self.width);
        for (i, &gray) in gray[..width].iter().enumerate() {
            let v = (i32::from(gray) + i32::from(self.err[i + 1])).clamp(0, 255);
            let black = v < 128;
            if black {
                self.target.black(
                    i32::from(self.x) + i as i32,
                    i32::from(self.y) + i32::from(y),
                );
            }
            let residual = v - if black { 0 } else { 255 };
            self.err[i + 2] = (i32::from(self.err[i + 2]) + residual * 7 / 16) as i16;
            self.next[i] = (i32::from(self.next[i]) + residual * 3 / 16) as i16;
            self.next[i + 1] = (i32::from(self.next[i + 1]) + residual * 5 / 16) as i16;
            self.next[i + 2] = (i32::from(self.next[i + 2]) + residual / 16) as i16;
        }
        self.err[..self.width + 2].copy_from_slice(&self.next[..self.width + 2]);
        self.next[..self.width + 2].fill(0);
        Ok(true)
    }
}
pub fn render_images<P: PageView + ?Sized>(
    page: &P,
    decoder: &mut dyn ImageDecoder,
    scratch: &mut ImageScratch<'_>,
    target: &mut FrameTarget<'_>,
) -> Result<BookStatus, SafetyError> {
    validate_page(page)?;
    target.validate()?;
    let mut worst = BookStatus::Ok;
    for index in 0..page.image_count() {
        let image = page.image(index)?;
        let n = usize::from(image.width) + 2;
        // Typed scratch capacity is the admission boundary.
        if scratch.err.len() < n || scratch.err_next.len() < n {
            return Ok(BookStatus::OutOfMemory);
        }
        let mark = scratch.arena.mark();
        let first_admitted = scratch.arena.allocate(n * 2, 2).is_some();
        let second_admitted = scratch.arena.allocate(n * 2, 2).is_some();
        if !first_admitted || !second_admitted {
            scratch.arena.release(mark);
            return Ok(BookStatus::OutOfMemory);
        }
        scratch.err[..n].fill(0);
        scratch.err_next[..n].fill(0);
        let result = {
            let mut blit = ImageBlit {
                target,
                x: image.x,
                y: image.y,
                err: &mut scratch.err[..n],
                next: &mut scratch.err_next[..n],
                width: usize::from(image.width),
            };
            decoder.render(&image, &mut scratch.arena, &mut blit)
        };
        scratch.arena.release(mark);
        let status = result?;
        if status != BookStatus::Ok && status != BookStatus::Unsupported {
            worst = status;
        }
    }
    Ok(worst)
}
pub fn render<P: PageView + ?Sized>(
    page: &P,
    fonts: &mut FontChain<'_, '_>,
    decoder: &mut dyn ImageDecoder,
    scratch: &mut ImageScratch<'_>,
    target: &mut FrameTarget<'_>,
) -> Result<BookStatus, SafetyError> {
    render_text(page, fonts, target, None)?;
    render_images(page, decoder, scratch, target)
}
