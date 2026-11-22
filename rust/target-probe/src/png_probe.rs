//! Real runtime image bytes and output storage are borrowed across the RV32 seam.
use newt_core::{
    image_renderer::*,
    inflate,
    page_renderer::{BookStatus, ImageRows, SafetyError},
    pngle::*,
};
#[unsafe(no_mangle)]
#[inline(never)]
pub fn png_feed_runtime(
    bytes: &[u8],
    decoder: &mut Decoder,
    state: &mut inflate::State,
    buffers: &mut Buffers<'_>,
    math: &dyn Math,
    output: &mut dyn Callbacks,
) -> Result<usize, Error> {
    decoder.feed(bytes, buffers, state, math, output)
}
struct BorrowedInput<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl EntryInput for BorrowedInput<'_> {
    fn read(&mut self, out: &mut [u8]) -> i32 {
        let n = out.len().min(self.bytes.len() - self.at);
        out[..n].copy_from_slice(&self.bytes[self.at..self.at + n]);
        self.at += n;
        n as i32
    }
}
#[inline(never)]
pub fn png_rows_runtime(
    bytes: &[u8],
    src: (u16, u16),
    dst: (u16, u16),
    decoder: &mut Decoder,
    state: &mut inflate::State,
    png: &mut Buffers<'_>,
    work: RenderBuffers<'_>,
    math: &dyn Math,
    output: &mut dyn ImageRows,
) -> Result<BookStatus, SafetyError> {
    let mut input = BorrowedInput { bytes, at: 0 };
    render_png(
        &mut input, src.0, src.1, dst.0, dst.1, decoder, state, png, work, math, output,
    )
}
struct MemoryRows<'a> {
    output: &'a mut [u8],
    stride: usize,
    height: u16,
    next: u16,
}
impl ImageRows for MemoryRows<'_> {
    fn on_row(&mut self, y: u16, gray: &[u8], width: u16) -> Result<bool, SafetyError> {
        if y != self.next
            || y >= self.height
            || gray.len() != usize::from(width)
            || self.stride < usize::from(width)
        {
            return Err(SafetyError::RowSlice);
        }
        let start = usize::from(y)
            .checked_mul(self.stride)
            .ok_or(SafetyError::RowSlice)?;
        let end = start
            .checked_add(usize::from(width))
            .ok_or(SafetyError::RowSlice)?;
        self.output
            .get_mut(start..end)
            .ok_or(SafetyError::RowSlice)?
            .copy_from_slice(gray);
        self.next += 1;
        Ok(true)
    }
}
/// Writes actual decoded/scaled rows directly into runtime-owned output memory.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn png_gray_memory_runtime(
    bytes: &[u8],
    src: (u16, u16),
    dst: (u16, u16),
    decoder: &mut Decoder,
    state: &mut inflate::State,
    png: &mut Buffers<'_>,
    work: RenderBuffers<'_>,
    math: &dyn Math,
    output: &mut [u8],
    stride: usize,
) -> Result<BookStatus, SafetyError> {
    let mut sink = MemoryRows {
        output,
        stride,
        height: dst.1,
        next: 0,
    };
    png_rows_runtime(bytes, src, dst, decoder, state, png, work, math, &mut sink)
}
