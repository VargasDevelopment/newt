//! Link into the actual target-probe. Runtime data are borrowed, never copied to local frames.
use newt_core::page_renderer::*;
#[unsafe(no_mangle)]
#[inline(never)]
pub fn page_text_runtime(
    page: &Page<'_>,
    fonts: &mut FontChain<'_, '_>,
    target: &mut FrameTarget<'_>,
    first: &mut u32,
) -> Result<u32, SafetyError> {
    render_text(page, fonts, target, Some(first))
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn page_images_runtime(
    page: &Page<'_>,
    decoder: &mut dyn ImageDecoder,
    scratch: &mut ImageScratch<'_>,
    target: &mut FrameTarget<'_>,
) -> Result<BookStatus, SafetyError> {
    render_images(page, decoder, scratch, target)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn page_combined_runtime(
    page: &Page<'_>,
    fonts: &mut FontChain<'_, '_>,
    decoder: &mut dyn ImageDecoder,
    scratch: &mut ImageScratch<'_>,
    target: &mut FrameTarget<'_>,
) -> Result<BookStatus, SafetyError> {
    render(page, fonts, decoder, scratch, target)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn page_chain_runtime(
    fonts: &mut FontChain<'_, '_>,
    cp: u32,
    right: u32,
    size: u16,
    style: u8,
    output: &mut [i32; 6],
) {
    output[0] = i32::from(fonts.advance(cp, size, style));
    output[1] = i32::from(fonts.kerning(cp, right, size, style));
    output[2] = fonts.ligature(cp, right, style) as i32;
    output[3] = i32::from(fonts.line_height(size));
    output[4] = i32::from(fonts.ascent(size));
    output[5] = i32::from(fonts.covers(cp));
}

#[path = "page_renderer_caller.rs"]
mod borrowed_caller;
/// Exported root keeps the concrete borrowed path live through staticlib LTO.
#[unsafe(no_mangle)]
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
    borrowed_caller::page_borrowed_caller_runtime(
        text, pixels, rows, frame, err, next, arena, output,
    )
}
