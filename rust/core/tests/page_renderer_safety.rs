use newt_core::page_renderer::*;
struct Short;
impl Font for Short {
    fn covers(&mut self, cp: u32) -> bool {
        self.has_glyph(cp)
    }
    fn advance(&mut self, _: u32, _: u16, _: u8) -> i16 {
        1
    }
    fn line_height(&mut self, _: u16) -> i16 {
        1
    }
    fn ascent(&mut self, _: u16) -> i16 {
        1
    }
}
impl RasterFont for Short {
    fn has_glyph(&mut self, _: u32) -> bool {
        true
    }
    fn rasterize(&mut self, _: u32, _: u16) -> Option<GlyphBitmap<'_>> {
        Some(GlyphBitmap {
            pixels: &[255],
            width: 2,
            height: 2,
            xoff: 0,
            yoff: 0,
            advance: 1,
        })
    }
}
struct BadRow;
impl ImageDecoder for BadRow {
    fn render(
        &mut self,
        _: &PageImage<'_>,
        _: &mut ByteArena<'_>,
        rows: &mut dyn ImageRows,
    ) -> Result<BookStatus, SafetyError> {
        rows.on_row(0, &[0], 2)?;
        Ok(BookStatus::Ok)
    }
}
fn frame(bytes: &mut [u8]) -> FrameTarget<'_> {
    FrameTarget {
        framebuffer: bytes,
        width: 8,
        height: 2,
        width_bytes: 1,
        format: FrameFormat::Mono1Sharp,
        rotation: FrameRotation::None,
    }
}
#[test]
fn descriptor_and_glyph_rejection() {
    let mut bytes = [255; 2];
    let mut target = frame(&mut bytes);
    target.width_bytes = 0;
    assert_eq!(target.validate(), Err(SafetyError::Frame));
    target.width_bytes = 1;
    let runs = [PageTextRun {
        text: b"a",
        x: 0,
        baseline_y: 0,
        size_px: 16,
        style_flags: 0,
    }];
    let page = Page {
        runs: &runs,
        images: &[],
        links: &[],
        page_index: 0,
        char_start: 0,
    };
    let mut f = Short;
    let mut bank: [&mut dyn RasterFont; 1] = [&mut f];
    let mut chain = FontChain::new(&mut bank);
    assert!(!chain.add(1, 0));
    assert!(chain.add(0, 0));
    assert_eq!(
        render_text(&page, &mut chain, &mut target, None),
        Err(SafetyError::GlyphSlice)
    );
    assert_eq!(bytes, [255; 2]);
}
#[test]
fn row_rejection_releases_arena() {
    let images = [PageImage {
        href: b"x",
        x: 0,
        y: 0,
        width: 2,
        height: 1,
    }];
    let page = Page {
        runs: &[],
        images: &images,
        links: &[],
        page_index: 0,
        char_start: 0,
    };
    let mut bytes = [255; 2];
    let mut target = frame(&mut bytes);
    let mut arena = [0; 32];
    let mut err = [42; 4];
    let mut next = [42; 4];
    let mut scratch = ImageScratch {
        err: &mut err,
        err_next: &mut next,
        arena: ByteArena::new(&mut arena),
    };
    assert_eq!(
        render_images(&page, &mut BadRow, &mut scratch, &mut target),
        Err(SafetyError::RowSlice)
    );
    assert_eq!(scratch.arena.mark(), 0);
}
#[test]
fn typed_rows_and_cpp_arena_are_separate_admissions() {
    let images = [PageImage {
        href: b"x",
        x: 0,
        y: 0,
        width: 2,
        height: 1,
    }];
    let page = Page {
        runs: &[],
        images: &images,
        links: &[],
        page_index: 0,
        char_start: 0,
    };
    let mut bytes = [255; 2];
    let mut target = frame(&mut bytes);
    let mut arena = [0; 64];
    let mut err = [0; 3];
    let mut next = [0; 4];
    let mut scratch = ImageScratch {
        err: &mut err,
        err_next: &mut next,
        arena: ByteArena::new(&mut arena),
    };
    assert_eq!(
        render_images(&page, &mut BadRow, &mut scratch, &mut target),
        Ok(BookStatus::OutOfMemory)
    );
    assert_eq!(scratch.arena.mark(), 0);
}
#[path = "../../target-probe/src/page_renderer_caller.rs"]
mod borrowed_caller;

#[test]
fn sparse_raster_coverage_through_metrics_font() {
    let pixels = [255u8];
    let mut sparse = borrowed_caller::BorrowedFont { pixels: &pixels };
    let metrics: &mut dyn Font = &mut sparse;
    assert!(metrics.covers(65));
    assert!(!metrics.covers(66));
}

#[test]
fn full_borrowed_frame_and_scratch_probe() {
    // Values live in the host caller; the shared target caller owns no buffers.
    let mut text = [0u8; 2];
    text.copy_from_slice(b"AA");
    let mut pixels = [0u8; 1];
    pixels[0] = 255;
    let row0 = [100u8, 100];
    let row1 = [128u8, 128];
    let rows = [&row0[..], &row1[..]];
    let mut frame = [255u8; 8];
    let mut err = [123i16; 4];
    let mut next = [-123i16; 4];
    let mut arena = [0xa5u8; 64];
    let mut output = [0u32; 6];
    assert_eq!(
        borrowed_caller::page_borrowed_caller_runtime(
            &text,
            &pixels,
            &rows,
            &mut frame,
            &mut err,
            &mut next,
            &mut arena,
            &mut output
        ),
        Ok(BookStatus::Ok)
    );
    assert_eq!(frame, [0x5f, 0xff, 0x7f, 0xff, 0xbf, 0xff, 0xff, 0xff]);
    assert_eq!(output, [0, 999, 1, 1, 2, 0]);
    assert_eq!(&arena[18..21], &[0x5a; 3]);
}
