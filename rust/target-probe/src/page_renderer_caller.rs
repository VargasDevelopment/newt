//! Concrete synthetic adapters over caller-owned runtime data, not font/image engines.
use newt_core::page_renderer::*;

pub struct BorrowedFont<'a> {
    pub pixels: &'a [u8],
}
impl Font for BorrowedFont<'_> {
    fn covers(&mut self, cp: u32) -> bool {
        self.has_glyph(cp)
    }
    fn advance(&mut self, _: u32, _: u16, _: u8) -> i16 {
        2
    }
    fn line_height(&mut self, size: u16) -> i16 {
        size as i16
    }
    fn ascent(&mut self, size: u16) -> i16 {
        size as i16
    }
}
impl RasterFont for BorrowedFont<'_> {
    fn has_glyph(&mut self, cp: u32) -> bool {
        cp == 65
    }
    fn rasterize(&mut self, cp: u32, _: u16) -> Option<GlyphBitmap<'_>> {
        if !self.has_glyph(cp) {
            return None;
        }
        Some(GlyphBitmap {
            pixels: self.pixels,
            width: 1,
            height: 1,
            xoff: 0,
            yoff: 0,
            advance: 7,
        })
    }
}
struct BorrowedRows<'a, 'b> {
    rows: &'a [&'b [u8]],
    emitted: u32,
}
impl ImageDecoder for BorrowedRows<'_, '_> {
    fn render(
        &mut self,
        _: &PageImage<'_>,
        arena: &mut ByteArena<'_>,
        sink: &mut dyn ImageRows,
    ) -> Result<BookStatus, SafetyError> {
        let Some(bytes) = arena.allocate(3, 1) else {
            return Ok(BookStatus::OutOfMemory);
        };
        bytes.fill(0x5a);
        for (y, row) in self.rows.iter().enumerate() {
            let width = u16::try_from(row.len()).map_err(|_| SafetyError::RowSlice)?;
            let y = u16::try_from(y).map_err(|_| SafetyError::RowSlice)?;
            self.emitted += 1;
            if !sink.on_row(y, row, width)? {
                break;
            }
        }
        Ok(BookStatus::Ok)
    }
}
/// Caller supplies every backing buffer. Local values hold only descriptors/adapters.
/// Supported scenario: 8x4 padded mono frame, one-pixel sparse glyph, width-2 image.
#[inline(never)]
pub fn page_borrowed_caller_runtime(
    text: &[u8],
    pixels: &[u8],
    rows: &[&[u8]],
    frame: &mut [u8],
    err: &mut [i16],
    next: &mut [i16],
    arena: &mut [u8],
    output: &mut [u32; 6],
) -> Result<BookStatus, SafetyError> {
    let mut font = BorrowedFont { pixels };
    let mut bank: [&mut dyn RasterFont; 1] = [&mut font];
    let mut chain = FontChain::new(&mut bank);
    chain.add(0, 0);
    let runs = [PageTextRun {
        text,
        x: 0,
        baseline_y: 0,
        size_px: 16,
        style_flags: 0,
    }];
    let images = [PageImage {
        href: text,
        x: 0,
        y: 1,
        width: 2,
        height: 2,
    }];
    let page = Page {
        runs: &runs,
        images: &images,
        links: &[],
        page_index: 0,
        char_start: 0,
    };
    let mut target = FrameTarget {
        framebuffer: frame,
        width: 8,
        height: 4,
        width_bytes: 2,
        format: FrameFormat::Mono1Sharp,
        rotation: FrameRotation::None,
    };
    let mut scratch = ImageScratch {
        err,
        err_next: next,
        arena: ByteArena::new(arena),
    };
    // Nonzero caller reservation makes restoration observable independently of zero-init.
    if scratch.arena.allocate(1, 1).is_none() {
        return Ok(BookStatus::OutOfMemory);
    }
    output[2] = scratch.arena.mark() as u32;
    output[1] = 999;
    output[0] = render_text(&page, &mut chain, &mut target, Some(&mut output[1]))?;
    let mut decoder = BorrowedRows { rows, emitted: 0 };
    let status = render_images(&page, &mut decoder, &mut scratch, &mut target)?;
    output[3] = scratch.arena.mark() as u32;
    output[4] = decoder.emitted;
    let metrics: &mut dyn Font = &mut font;
    output[5] = u32::from(metrics.covers(66));
    Ok(status)
}
