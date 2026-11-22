//! FtFont wrapper port, MIT, pinned FreeInk cd6f5b8; native FreeType is not ported.
//! Backend owns native handles; caller supplies all Rust scratch. No allocator here.
#![forbid(unsafe_code)]
use crate::{
    font_layout,
    page_renderer::{Font, GlyphBitmap, RasterFont},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GlyphMetrics {
    pub advance26_6: i32,
    pub left: i16,
    pub top: i16,
    pub width: u16,
    pub height: u16,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct LineMetrics {
    pub ascender26_6: i32,
    pub descender26_6: i32,
    pub height26_6: i32,
}
#[derive(Clone, Copy, Debug)]
pub struct FaceInfo {
    pub family_length: usize,
    pub weight: u16,
    pub italic: bool,
}
impl Default for FaceInfo {
    fn default() -> Self {
        Self {
            family_length: 0,
            weight: 400,
            italic: false,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectResult {
    Ok,
    Unsupported,
    Unavailable,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum GlyphFailure {
    #[default]
    None,
    MissingGlyph,
    Size,
    Load,
    Embolden,
    Render,
    Bounds,
    BitmapBuffer,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum InitFailure {
    #[default]
    None,
    Library,
    Source,
    Allocation,
    OpenFace,
    SetSize,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum HintingMode {
    #[default]
    Default,
    None,
    Auto,
    Light,
    Native,
}
#[derive(Clone, Copy, Debug)]
pub struct RenderOptions {
    pub hinting: HintingMode,
    pub interpreter_version: u8,
    pub monochrome: bool,
    pub stem_darkening: bool,
    pub embolden26_6: i32,
    pub slant16_16: i32,
}
impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            hinting: HintingMode::Default,
            interpreter_version: 40,
            monochrome: false,
            stem_darkening: false,
            embolden26_6: 0,
            slant16_16: 0,
        }
    }
}
#[derive(Clone, Copy, Default)]
pub struct Capabilities {
    pub auto: bool,
    pub native: bool,
    pub mono: bool,
}
#[derive(Clone, Copy, Default)]
pub struct Axis {
    pub tag: u32,
    pub minimum: i32,
    pub default: i32,
    pub maximum: i32,
}
#[derive(Clone, Copy, Default)]
pub struct Slot {
    pub outline: bool,
    pub bitmap: bool,
    pub linear_advance: i64,
    pub advance: i64,
    pub left: i64,
    pub top: i64,
    pub width: u32,
    pub rows: u32,
    pub pitch: i32,
    pub mono: bool,
    pub xmin: i64,
    pub xmax: i64,
    pub ymin: i64,
    pub ymax: i64,
}
#[derive(Clone, Copy)]
pub struct OpenError {
    pub stage: InitFailure,
    pub raw: i32,
}
/// Implementations must close a prior face before opening and retain borrowed sources
/// until close. Stream contexts live in caller storage, not in a movable font.
/// Primitive operations only: no wrapper style/cache/metrics policy behind this trait.
pub trait Backend<'a> {
    fn open(&mut self, memory: Option<&'a [u8]>) -> Result<(), OpenError>;
    fn close(&mut self);
    fn capabilities(&self) -> Capabilities;
    fn axes(&mut self, axes: &mut [Axis; 16]) -> usize;
    fn set_coordinates(&mut self, coordinates: &[i32]);
    fn set_size(&mut self, size: u32) -> Result<(), i32>;
    fn glyph_id(&self, cp: u32) -> u32;
    fn properties(&mut self, darkening: bool, interpreter: u8);
    fn transform(&mut self, shear: i32);
    fn load(&mut self, glyph: u32, flags: i32) -> Result<(), i32>;
    fn embolden(&mut self, strength: i32) -> Result<(), i32>;
    fn render(&mut self, mono: bool) -> Result<(), i32>;
    fn slot(&self) -> Slot;
    fn bitmap(&self) -> &[u8];
    fn line_metrics(&self) -> LineMetrics;
    fn legacy_kern(&mut self, left: u32, right: u32) -> i32;
    fn scale_x(&self, units: i32) -> i32;
    fn table_length(&mut self, tag: u32) -> Option<usize>;
    fn read_table(&mut self, tag: u32, output: &mut [u8]) -> bool;
    /// Describes an already-open face, including scalable/Unicode validation.
    fn describe(&mut self, family: &mut [u8]) -> Result<FaceInfo, InspectResult>;
    fn stream_failed(&self) -> bool;
}
const MAX_TABLE: usize = 1024 * 1024;
const GSUB: u32 = u32::from_be_bytes(*b"GSUB");
const GPOS: u32 = u32::from_be_bytes(*b"GPOS");
struct Table<'a> {
    memory: Option<&'a [u8]>,
    len: usize,
    attempted: bool,
    budget: usize,
}
impl<'a> Table<'a> {
    fn new() -> Self {
        Self {
            memory: None,
            len: 0,
            attempted: false,
            budget: MAX_TABLE,
        }
    }
    fn clear(&mut self) {
        self.memory = None;
        self.len = 0;
        self.attempted = false;
    }
    fn release(&mut self) {
        self.clear();
        self.attempted = true;
    }
    fn budget(&mut self, n: usize) {
        self.budget = n;
        if self.memory.is_none() && self.len > n {
            self.release();
        }
    }
    fn ensure<B: Backend<'a>>(
        &mut self,
        b: &mut B,
        source: Option<&'a [u8]>,
        scratch: &mut [u8],
        tag: u32,
    ) {
        if self.attempted {
            return;
        }
        self.attempted = true;
        if let Some(bytes) = source {
            self.memory = sfnt_table(bytes, tag).filter(|t| t.len() <= MAX_TABLE);
            return;
        }
        if let Some(n) = b.table_length(tag) {
            if n > 0
                && n <= MAX_TABLE
                && n <= self.budget
                && n <= scratch.len()
                && b.read_table(tag, &mut scratch[..n])
            {
                self.len = n;
            }
        }
    }
    fn bytes<'s>(&'s self, scratch: &'s [u8]) -> &'s [u8] {
        self.memory.unwrap_or(&scratch[..self.len])
    }
}
fn be16(b: &[u8], p: usize) -> Option<usize> {
    Some(u16::from_be_bytes(b.get(p..p.checked_add(2)?)?.try_into().ok()?) as usize)
}
fn be32(b: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        b.get(p..p.checked_add(4)?)?.try_into().ok()?,
    ))
}
pub fn sfnt_table(b: &[u8], tag: u32) -> Option<&[u8]> {
    if b.len() < 12 {
        return None;
    }
    let mut d = 0usize;
    if be32(b, 0)? == u32::from_be_bytes(*b"ttcf") {
        let n = be32(b, 8)? as usize;
        if n == 0 || n > (b.len() - 12) / 4 {
            return None;
        }
        d = be32(b, 12)? as usize;
    }
    let records = d.checked_add(12)?;
    if records > b.len() {
        return None;
    }
    let count = be16(b, d.checked_add(4)?)?;
    if count > (b.len() - records) / 16 {
        return None;
    }
    for i in 0..count {
        let r = records + i * 16;
        if be32(b, r)? == tag {
            let at = be32(b, r + 8)? as usize;
            let n = be32(b, r + 12)? as usize;
            if n == 0 {
                return None;
            }
            return b.get(at..at.checked_add(n)?);
        }
    }
    None
}
pub fn load_flags(o: RenderOptions) -> i32 {
    let mut f = 8; // FT_LOAD_NO_BITMAP
    if o.monochrome {
        f |= 2 << 16;
    } else if o.hinting == HintingMode::Light {
        f |= 1 << 16;
    }
    match o.hinting {
        HintingMode::Auto => f |= 32,
        HintingMode::None => f |= 2 | 32768,
        HintingMode::Light => {
            if o.monochrome {
                f |= 32;
            }
        }
        _ => f |= 32768,
    }
    f
}
fn sat(v: i64) -> i32 {
    v.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}
fn pixel(v: i32) -> i16 {
    (v >> 6).clamp(i16::MIN as i32, i16::MAX as i32) as i16
}
fn linear(v: i64) -> i32 {
    sat(if v >= 0 {
        (v.saturating_add(512)) >> 10
    } else {
        -((v.saturating_neg().saturating_add(512)) >> 10)
    })
}
/// One mutable font borrow protects backend bitmap and scratch until released.
pub struct FtFont<'a, 's, B: Backend<'a>> {
    backend: B,
    source: Option<&'a [u8]>,
    mono: &'s mut [u8],
    gsub_scratch: &'s mut [u8],
    gpos_scratch: &'s mut [u8],
    gsub: Table<'a>,
    gpos: Table<'a>,
    ready: bool,
    opened: bool,
    size: u32,
    oblique: bool,
    bold: bool,
    pub options: RenderOptions,
    pub last_glyph_failure: GlyphFailure,
    pub last_glyph_error: i32,
    pub last_init_failure: InitFailure,
    pub last_init_error: i32,
}
impl<'a, 's, B: Backend<'a>> FtFont<'a, 's, B> {
    pub fn new(backend: B, mono: &'s mut [u8], gsub: &'s mut [u8], gpos: &'s mut [u8]) -> Self {
        Self {
            backend,
            source: None,
            mono,
            gsub_scratch: gsub,
            gpos_scratch: gpos,
            gsub: Table::new(),
            gpos: Table::new(),
            ready: false,
            opened: false,
            size: 0,
            oblique: false,
            bold: false,
            options: RenderOptions::default(),
            last_glyph_failure: GlyphFailure::None,
            last_glyph_error: 0,
            last_init_failure: InitFailure::None,
            last_init_error: 0,
        }
    }
    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn deinit(&mut self) {
        self.backend.close();
        self.opened = false;
        self.ready = false;
        self.size = 0;
        self.oblique = false;
        self.bold = false;
        self.options = RenderOptions::default();
        self.last_glyph_failure = GlyphFailure::None;
        self.last_glyph_error = 0;
        self.last_init_failure = InitFailure::None;
        self.last_init_error = 0;
        self.gsub.clear();
        self.gpos.clear();
        self.source = None;
    }
    /// None selects the stream borrowed by the backend. Some borrows whole bytes.
    pub fn init(&mut self, source: Option<&'a [u8]>, px: u16, weight: i32, italic: bool) -> bool {
        self.deinit();
        if let Err(e) = self.backend.open(source) {
            self.last_init_failure = e.stage;
            self.last_init_error = e.raw;
            return false;
        }
        self.opened = true;
        self.source = source;
        self.variation(weight, italic);
        if !self.ensure_size(u32::from(px) * 64) {
            self.last_init_failure = InitFailure::SetSize;
            self.last_init_error = self.last_glyph_error;
            self.backend.close();
            self.opened = false;
            return false;
        }
        self.ready = true;
        true
    }
    fn variation(&mut self, weight: i32, italic: bool) {
        let mut axes = [Axis::default(); 16];
        let n = self.backend.axes(&mut axes).min(16);
        let mut coords = [0i32; 16];
        let (mut have_w, mut have_i) = (false, false);
        for i in 0..n {
            let a = axes[i];
            coords[i] = a.default;
            if a.tag == u32::from_be_bytes(*b"wght") {
                coords[i] = sat(i64::from(weight) * 65536).clamp(a.minimum, a.maximum);
                have_w = true;
            } else if italic && a.tag == u32::from_be_bytes(*b"ital") {
                coords[i] = a.maximum;
                have_i = true;
            } else if italic && a.tag == u32::from_be_bytes(*b"slnt") {
                coords[i] = a.minimum;
                have_i = true;
            }
        }
        if n > 0 {
            self.backend.set_coordinates(&coords[..n]);
        }
        self.oblique = italic && !have_i;
        self.bold = weight >= 600 && !have_w;
    }
    pub fn set_render_options(&mut self, o: RenderOptions) -> bool {
        self.options = o;
        let c = self.backend.capabilities();
        (!o.monochrome || c.mono)
            && (!matches!(o.hinting, HintingMode::Auto | HintingMode::Light) || c.auto)
            && (o.hinting != HintingMode::Native
                || (c.native && matches!(o.interpreter_version, 35 | 40)))
    }
    fn ensure_size(&mut self, n: u32) -> bool {
        if !self.opened || !(64..=0xffffff).contains(&n) {
            return false;
        }
        if n == self.size {
            return true;
        }
        match self.backend.set_size(n) {
            Ok(()) => {
                self.size = n;
                true
            }
            Err(e) => {
                self.last_glyph_error = e;
                false
            }
        }
    }
    pub fn glyph_id(&self, cp: u32) -> u32 {
        if self.ready {
            self.backend.glyph_id(cp)
        } else {
            0
        }
    }
    pub fn has_glyph(&self, cp: u32) -> bool {
        self.glyph_id(cp) != 0
    }
    fn fail(&mut self, stage: GlyphFailure, e: i32) -> bool {
        self.last_glyph_failure = stage;
        self.last_glyph_error = e;
        false
    }
    fn load(&mut self, g: u32, n: u32) -> bool {
        self.last_glyph_failure = GlyphFailure::None;
        self.last_glyph_error = 0;
        if g == 0 {
            return self.fail(GlyphFailure::MissingGlyph, 0);
        }
        if !self.ensure_size(n) {
            self.last_glyph_failure = GlyphFailure::Size;
            return false;
        }
        if !self.ready {
            return self.fail(GlyphFailure::Load, 0);
        }
        let o = self.options;
        self.backend.properties(
            o.stem_darkening,
            if o.hinting == HintingMode::Native {
                o.interpreter_version
            } else {
                40
            },
        );
        self.backend.transform(sat(
            i64::from(o.slant16_16) + if self.oblique { 13763 } else { 0 }
        ));
        if let Err(e) = self.backend.load(g, load_flags(o)) {
            return self.fail(GlyphFailure::Load, e);
        }
        let strength =
            sat(i64::from(o.embolden26_6) + if self.bold { i64::from(n) / 26 } else { 0 });
        if strength != 0 && self.backend.slot().outline {
            if let Err(e) = self.backend.embolden(strength) {
                return self.fail(GlyphFailure::Embolden, e);
            }
        }
        true
    }
    fn render(&mut self) -> bool {
        if !self.backend.slot().bitmap {
            if let Err(e) = self.backend.render(self.options.monochrome) {
                return self.fail(GlyphFailure::Render, e);
            }
        }
        true
    }
    pub fn metrics26_6(&mut self, cp: u32, n: u32) -> Option<GlyphMetrics> {
        self.metrics_glyph26_6(self.glyph_id(cp), n)
    }
    pub fn metrics_glyph26_6(&mut self, g: u32, n: u32) -> Option<GlyphMetrics> {
        if !self.load(g, n) {
            return None;
        }
        let mut s = self.backend.slot();
        let (l, r, b, t) = if self.options.monochrome || !s.outline {
            if !self.render() {
                return None;
            }
            s = self.backend.slot();
            (
                s.left,
                s.left + i64::from(s.width),
                s.top - i64::from(s.rows),
                s.top,
            )
        } else {
            (
                s.xmin >> 6,
                (s.xmax + 63) >> 6,
                s.ymin >> 6,
                (s.ymax + 63) >> 6,
            )
        };
        if r < l
            || t < b
            || r - l > 65535
            || t - b > 65535
            || l < -32768
            || l > 32767
            || t < -32768
            || t > 32767
        {
            self.fail(GlyphFailure::Bounds, 0);
            return None;
        }
        Some(GlyphMetrics {
            advance26_6: linear(s.linear_advance),
            left: l as i16,
            top: t as i16,
            width: (r - l) as u16,
            height: (t - b) as u16,
        })
    }
    pub fn line_metrics26_6(&mut self, n: u32) -> Option<LineMetrics> {
        if self.ensure_size(n) {
            Some(self.backend.line_metrics())
        } else {
            None
        }
    }
    pub fn kerning26_6(&mut self, l: u32, r: u32, n: u32) -> i32 {
        self.kerning_glyphs26_6(self.glyph_id(l), self.glyph_id(r), n)
    }
    pub fn kerning_glyphs26_6(&mut self, l: u32, r: u32, n: u32) -> i32 {
        if l == 0 || r == 0 || !self.ensure_size(n) {
            return 0;
        }
        let legacy = self.backend.legacy_kern(l, r);
        if legacy != 0 {
            return legacy;
        }
        self.gpos
            .ensure(&mut self.backend, self.source, self.gpos_scratch, GPOS);
        let units = font_layout::pair_kern_adjustment(self.gpos.bytes(self.gpos_scratch), l, r);
        self.backend.scale_x(units)
    }
    pub fn set_gsub_byte_budget(&mut self, n: usize) {
        self.gsub.budget(n);
    }
    pub fn set_gpos_byte_budget(&mut self, n: usize) {
        self.gpos.budget(n);
    }
    pub fn release_ligature_table(&mut self) {
        self.gsub.release();
    }
    pub fn release_kerning_table(&mut self) {
        self.gpos.release();
    }
    pub fn ligature_glyph_id(&mut self, cps: &[u32]) -> u32 {
        if !self.ready || !(2..=3).contains(&cps.len()) {
            return 0;
        }
        self.gsub
            .ensure(&mut self.backend, self.source, self.gsub_scratch, GSUB);
        let mut ids = [0; 3];
        for (i, &cp) in cps.iter().enumerate() {
            ids[i] = self.glyph_id(cp);
            if ids[i] == 0 {
                return 0;
            }
        }
        font_layout::ligature_glyph_id(self.gsub.bytes(self.gsub_scratch), &ids[..cps.len()])
    }
    pub fn ligature_codepoint(&mut self, l: u32, r: u32) -> u32 {
        let (seq, n, candidate) = match (l, r) {
            (0xfb00, 105) => ([102, 102, 105], 3, 0xfb03),
            (0xfb00, 108) => ([102, 102, 108], 3, 0xfb04),
            (102, 102) => ([102, 102, 0], 2, 0xfb00),
            (102, 105) => ([102, 105, 0], 2, 0xfb01),
            (102, 108) => ([102, 108, 0], 2, 0xfb02),
            _ => return 0,
        };
        let g = self.ligature_glyph_id(&seq[..n]);
        if g != 0 && self.glyph_id(candidate) == g {
            candidate
        } else {
            0
        }
    }
    pub fn rasterize26_6(&mut self, cp: u32, n: u32) -> Option<GlyphBitmap<'_>> {
        self.rasterize_glyph26_6(self.glyph_id(cp), n)
    }
    pub fn rasterize_glyph26_6(&mut self, g: u32, n: u32) -> Option<GlyphBitmap<'_>> {
        if !self.load(g, n) || !self.render() {
            return None;
        }
        let s = self.backend.slot();
        let adv = s.advance >> 6;
        if s.width > 65535
            || s.rows > 65535
            || s.left < -32768
            || s.left > 32767
            || s.top < -32768
            || s.top > 32767
            || adv < -32768
            || adv > 32767
        {
            self.fail(GlyphFailure::Bounds, 0);
            return None;
        }
        let w = s.width as usize;
        let h = s.rows as usize;
        let Some(count) = w.checked_mul(h) else {
            self.fail(GlyphFailure::Bounds, 0);
            return None;
        };
        // Normalize mono/negative or padded rows to the accepted tightly packed contract.
        let packed = s.mono || s.pitch < 0 || s.pitch as usize != w;
        if packed && count > self.mono.len() {
            self.fail(GlyphFailure::BitmapBuffer, 0);
            return None;
        }
        let rowbytes = if s.mono { w.div_ceil(8) } else { w };
        let pitch = s.pitch.unsigned_abs() as usize;
        let needed = if h == 0 || w == 0 {
            0
        } else {
            match (h - 1)
                .checked_mul(pitch)
                .and_then(|x| x.checked_add(rowbytes))
            {
                Some(n) => n,
                None => {
                    self.fail(GlyphFailure::Bounds, 0);
                    return None;
                }
            }
        };
        if (h > 0 && pitch < rowbytes) || self.backend.bitmap().len() < needed {
            self.fail(GlyphFailure::BitmapBuffer, 0);
            return None;
        }
        let raw = self.backend.bitmap();
        let pixels = if packed {
            for y in 0..h {
                let row = if s.pitch >= 0 { y } else { h - 1 - y } * pitch;
                for x in 0..w {
                    self.mono[y * w + x] = if s.mono {
                        if raw[row + x / 8] & (128 >> (x % 8)) != 0 {
                            255
                        } else {
                            0
                        }
                    } else {
                        raw[row + x]
                    };
                }
            }
            &self.mono[..count]
        } else {
            &raw[..count]
        };
        Some(GlyphBitmap {
            pixels,
            width: s.width as u16,
            height: s.rows as u16,
            xoff: s.left as i16,
            yoff: (-s.top) as i16,
            advance: adv as i16,
        })
    }
}
impl<'a, 's, B: Backend<'a>> Drop for FtFont<'a, 's, B> {
    fn drop(&mut self) {
        self.backend.close();
    }
}
impl<'a, 's, B: Backend<'a>> Font for FtFont<'a, 's, B> {
    fn advance(&mut self, cp: u32, size: u16, _: u8) -> i16 {
        self.metrics26_6(cp, u32::from(size) * 64)
            .map_or(0, |m| pixel(m.advance26_6))
    }
    fn line_height(&mut self, size: u16) -> i16 {
        self.line_metrics26_6(u32::from(size) * 64)
            .map_or(size as i16, |m| pixel(m.height26_6))
    }
    fn ascent(&mut self, size: u16) -> i16 {
        self.line_metrics26_6(u32::from(size) * 64)
            .map_or(size as i16, |m| pixel(m.ascender26_6))
    }
    fn kerning(&mut self, l: u32, r: u32, size: u16, _: u8) -> i16 {
        if !self.ready {
            return 0;
        }
        let v = self.kerning26_6(l, r, u32::from(size) * 64);
        sat((i64::from(v) + 32) >> 6).clamp(-32768, 32767) as i16
    }
    fn ligature(&mut self, l: u32, r: u32, _: u8) -> u32 {
        self.ligature_codepoint(l, r)
    }
    fn covers(&mut self, cp: u32) -> bool {
        self.has_glyph(cp)
    }
}
impl<'a, 's, B: Backend<'a>> RasterFont for FtFont<'a, 's, B> {
    fn has_glyph(&mut self, cp: u32) -> bool {
        FtFont::has_glyph(self, cp)
    }
    fn rasterize(&mut self, cp: u32, size: u16) -> Option<GlyphBitmap<'_>> {
        self.rasterize26_6(cp, u32::from(size) * 64)
    }
}
/// Inspection borrows the same backend allocator/source; always closes the temporary face.
pub fn inspect<'a, B: Backend<'a>>(
    backend: &mut B,
    source: Option<&'a [u8]>,
    family: &mut [u8],
) -> (InspectResult, FaceInfo) {
    family.fill(0);
    if let Err(e) = backend.open(source) {
        return (
            if e.stage == InitFailure::Library
                || e.stage == InitFailure::Allocation
                || e.raw == 0x40
                || backend.stream_failed()
            {
                InspectResult::Unavailable
            } else {
                InspectResult::Unsupported
            },
            FaceInfo::default(),
        );
    }
    let result = backend.describe(family);
    let failed = backend.stream_failed();
    backend.close();
    match result {
        Ok(i) => (
            if failed {
                InspectResult::Unavailable
            } else {
                InspectResult::Ok
            },
            i,
        ),
        Err(e) => (
            if failed {
                InspectResult::Unavailable
            } else {
                e
            },
            FaceInfo::default(),
        ),
    }
}
#[cfg(test)]
#[path = "ft_font_tests.rs"]
mod tests;
