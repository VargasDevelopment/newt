//! Explicit primitive test double only; real rendering is tested through native.c.
use super::*;
use core::cell::Cell;
struct Model<'a> {
    closed: &'a Cell<u32>,
    slot: Slot,
    data: &'a [u8],
    error: GlyphFailure,
    flags: i32,
    shear: i32,
    strength: i32,
    axes: [Axis; 16],
    n: usize,
    coords: [i32; 16],
}
impl<'a> Model<'a> {
    fn new(c: &'a Cell<u32>, b: &'a [u8]) -> Self {
        Self {
            closed: c,
            slot: Slot {
                bitmap: true,
                width: 9,
                rows: 2,
                pitch: -2,
                mono: true,
                advance: 640,
                top: 2,
                ..Slot::default()
            },
            data: b,
            error: GlyphFailure::None,
            flags: 0,
            shear: 0,
            strength: 0,
            axes: [Axis::default(); 16],
            n: 0,
            coords: [0; 16],
        }
    }
}
impl<'a> Backend<'a> for Model<'a> {
    fn open(&mut self, _: Option<&'a [u8]>) -> Result<(), OpenError> {
        Ok(())
    }
    fn close(&mut self) {
        self.closed.set(self.closed.get() + 1);
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
    fn axes(&mut self, a: &mut [Axis; 16]) -> usize {
        *a = self.axes;
        self.n
    }
    fn set_coordinates(&mut self, c: &[i32]) {
        self.coords[..c.len()].copy_from_slice(c);
    }
    fn set_size(&mut self, _: u32) -> Result<(), i32> {
        if self.error == GlyphFailure::Size {
            Err(77)
        } else {
            Ok(())
        }
    }
    fn glyph_id(&self, cp: u32) -> u32 {
        cp
    }
    fn properties(&mut self, _: bool, _: u8) {}
    fn transform(&mut self, s: i32) {
        self.shear = s;
    }
    fn load(&mut self, _: u32, f: i32) -> Result<(), i32> {
        self.flags = f;
        if self.error == GlyphFailure::Load {
            Err(78)
        } else {
            Ok(())
        }
    }
    fn embolden(&mut self, s: i32) -> Result<(), i32> {
        self.strength = s;
        if self.error == GlyphFailure::Embolden {
            Err(79)
        } else {
            Ok(())
        }
    }
    fn render(&mut self, _: bool) -> Result<(), i32> {
        if self.error == GlyphFailure::Render {
            Err(80)
        } else {
            self.slot.bitmap = true;
            Ok(())
        }
    }
    fn slot(&self) -> Slot {
        self.slot
    }
    fn bitmap(&self) -> &[u8] {
        self.data
    }
    fn line_metrics(&self) -> LineMetrics {
        LineMetrics::default()
    }
    fn legacy_kern(&mut self, _: u32, _: u32) -> i32 {
        0
    }
    fn scale_x(&self, u: i32) -> i32 {
        u
    }
    fn table_length(&mut self, _: u32) -> Option<usize> {
        None
    }
    fn read_table(&mut self, _: u32, _: &mut [u8]) -> bool {
        false
    }
    fn describe(&mut self, _: &mut [u8]) -> Result<FaceInfo, InspectResult> {
        Ok(FaceInfo::default())
    }
    fn stream_failed(&self) -> bool {
        false
    }
}
#[test]
fn mono_negative_pitch_and_scratch_failure() {
    let closed = Cell::new(0);
    let data = [0x80, 0x80, 0x40, 0x00];
    let mut scratch = [0; 18];
    let mut f = FtFont::new(Model::new(&closed, &data), &mut scratch, &mut [], &mut []);
    assert!(f.init(None, 10, 400, false));
    let b = f.rasterize_glyph26_6(1, 640).unwrap();
    assert_eq!(
        b.pixels,
        &[0, 255, 0, 0, 0, 0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 255]
    );
    drop(f);
    let mut small = [0; 17];
    let mut f = FtFont::new(Model::new(&closed, &data), &mut small, &mut [], &mut []);
    f.init(None, 10, 400, false);
    assert!(f.rasterize_glyph26_6(1, 640).is_none());
    assert_eq!(f.last_glyph_failure, GlyphFailure::BitmapBuffer);
}
#[test]
fn error_stage_and_raw_error_survive() {
    for (stage, raw) in [
        (GlyphFailure::Load, 78),
        (GlyphFailure::Embolden, 79),
        (GlyphFailure::Render, 80),
    ] {
        let closed = Cell::new(0);
        let mut model = Model::new(&closed, &[]);
        model.error = stage;
        model.slot.outline = true;
        model.slot.bitmap = false;
        let mut f = FtFont::new(model, &mut [], &mut [], &mut []);
        assert!(f.init(None, 10, 700, false));
        assert!(f.rasterize_glyph26_6(1, 640).is_none());
        assert_eq!(f.last_glyph_failure, stage);
        assert_eq!(f.last_glyph_error, raw);
    }
}
#[test]
fn axes_clamp_and_synthetic_fallback() {
    let c = Cell::new(0);
    let mut b = Model::new(&c, &[]);
    b.n = 3;
    b.axes[0] = Axis {
        tag: u32::from_be_bytes(*b"wght"),
        minimum: 100 << 16,
        default: 400 << 16,
        maximum: 900 << 16,
    };
    b.axes[1] = Axis {
        tag: u32::from_be_bytes(*b"ital"),
        minimum: 0,
        default: 0,
        maximum: 65536,
    };
    b.axes[2] = Axis {
        tag: u32::from_be_bytes(*b"slnt"),
        minimum: -12 << 16,
        default: 0,
        maximum: 0,
    };
    let mut f = FtFont::new(b, &mut [], &mut [], &mut []);
    assert!(f.init(None, 10, 1200, true));
    assert_eq!(&f.backend.coords[..3], &[900 << 16, 65536, -12 << 16]);
    assert!(!f.bold);
    assert!(!f.oblique);
    f.backend.n = 0;
    assert!(f.init(None, 10, 700, true));
    assert!(f.bold && f.oblique);
    assert!(f.load(1, 640));
    assert_eq!(f.backend.shear, 13763);
    assert_eq!(f.backend.flags, 32776);
}
#[test]
fn bounds_and_missing_and_size() {
    let c = Cell::new(0);
    let mut b = Model::new(&c, &[]);
    b.slot.width = 65536;
    let mut f = FtFont::new(b, &mut [], &mut [], &mut []);
    assert!(f.init(None, 10, 400, false));
    assert!(f.rasterize_glyph26_6(1, 640).is_none());
    assert_eq!(f.last_glyph_failure, GlyphFailure::Bounds);
    assert!(!f.load(0, 640));
    assert_eq!(f.last_glyph_failure, GlyphFailure::MissingGlyph);
    assert!(!f.load(1, 63));
    assert_eq!(f.last_glyph_failure, GlyphFailure::Size);
}
#[test]
fn load_flags_are_independent() {
    let mut o = RenderOptions::default();
    assert_eq!(load_flags(o), 32776);
    o.hinting = HintingMode::None;
    assert_eq!(load_flags(o), 32778);
    o.hinting = HintingMode::Light;
    assert_eq!(load_flags(o), 65544);
    o.monochrome = true;
    assert_eq!(load_flags(o), 131112);
}
