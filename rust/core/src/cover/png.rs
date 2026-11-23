// SPDX-License-Identifier: MIT
//! Cover PNG: the header policy and a driver that feeds pngle the whole
//! stream, keeping each feed's unconsumed tail for the next.
use super::{read, jpeg::{Layout, Status}};
use crate::{image_renderer::{PngState, RowScaler}, inflate, jpeg::{Input, renderer::RenderError},
    page_renderer::{BookStatus, ImageRows}, pngle::{self, Decoder, Math}, zip_window::source::BookSource};
pub const SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
/// The image row and pngle's ring together stay within this, so the decode
/// block fits cover Memory beside the Session and the raster.
pub const ROW_BYTES: usize = 10 * 1024;
/// The feed window. pngle waits for at most a chunk header, IHDR or CRC.
pub const READ: usize = 1024;
pub const PALETTE: usize = 768;
pub const TRANSPARENCY: usize = 256;
/// Every complete PNG ends with this IEND chunk.
const IEND: [u8; 12] = [0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82];
/// The decode arena at the row cap and the widest slot.
pub const MAX_ARENA_BYTES: usize = fixed_bytes() + ROW_BYTES + 7 * 144;
const fn fixed_bytes() -> usize {
    READ + core::mem::size_of::<inflate::State>() + inflate::DICTIONARY_SIZE + PALETTE + TRANSPARENCY + 128
}
/// Arena after the feed window (also the CRC skip buffer), with 128 B for alignment.
pub fn arena_bytes(layout: &Layout, target: (u16, u16)) -> usize {
    fixed_bytes() + usize::from(layout.ring) + usize::from(layout.width) + 7 * usize::from(target.0)
}
fn be(bytes: &[u8]) -> u32 { u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) }
/// Pins the signature and IHDR, so a later change to them reads as drift.
pub fn validate(source: &mut dyn BookSource, layout: &mut Layout) -> Status {
    let mut head = [0; 33];
    if source.size() < (head.len() + IEND.len()) as u64 { return Status::Incomplete; }
    if !read(source, 0, &mut head) { return Status::IoError; }
    if head[..8] != SIGNATURE { return Status::Unsupported; }
    if head[8..16] != *b"\0\0\0\x0dIHDR" { return Status::Invalid; }
    let (width, height, depth, color) = (be(&head[16..]), be(&head[20..]), head[24], head[25]);
    let channels = match (color, depth) {
        (0, 1 | 2 | 4 | 8 | 16) | (3, 1 | 2 | 4 | 8) => 1,
        (4, 8 | 16) => 2, (2, 8 | 16) => 3, (6, 8 | 16) => 4,
        _ => return Status::Invalid,
    };
    if width == 0 || height == 0 || head[26] != 0 || head[27] != 0 || head[28] > 1 { return Status::Invalid; }
    let bits = usize::from(depth) * channels;
    let ring = (width as usize).saturating_mul(bits).div_ceil(8) + 2 * bits.div_ceil(8);
    // Adam7 (interlace 1) gives no row in order until its last pass.
    if head[28] == 1 || width > 0xffff || height > 0xffff || width as usize + ring > ROW_BYTES {
        return Status::Unsupported;
    }
    let mut end = [0; 12];
    let at = source.size() - 12;
    if !read(source, at, &mut end) { return Status::IoError; }
    if end != IEND { return Status::Incomplete; }
    layout.pin(0, &head);
    (layout.width, layout.height, layout.ring) = (width as u16, height as u16, ring as u16);
    Status::Ready
}
pub struct Buffers<'a> {
    pub read: &'a mut [u8],
    pub inflate: &'a mut inflate::State,
    pub dictionary: &'a mut [u8],
    pub palette: &'a mut [u8],
    pub transparency: &'a mut [u8],
    pub ring: &'a mut [u8],
    pub src: &'a mut [u8],
    pub dst: &'a mut [u8],
    pub sums: &'a mut [u32],
    pub counts: &'a mut [u16],
}
/// Covers ignore gAMA (display gamma stays 0), so pngle never asks for a power.
struct NoGamma;
impl Math for NoGamma { fn pow(&self, base: f64, _exponent: f64) -> f64 { base } }
/// Draws a validated PNG's rows, scaled to `target`, to `sink`.
pub fn render(input: &mut dyn Input, b: Buffers<'_>, size: (u16, u16), target: (u16, u16),
    sink: &mut dyn ImageRows) -> Result<(), RenderError> {
    let status = RenderError::Status;
    let width = usize::from(size.0);
    if b.read.len() < READ || b.src.len() < width { return Err(status(BookStatus::OutOfMemory)); }
    let scaler = RowScaler::new(size.0, size.1, target.0, target.1, b.dst, b.sums, b.counts).map_err(status)?;
    let mut png = pngle::Buffers { ring: b.ring, palette: b.palette, transparency: b.transparency,
        gamma: &mut [], dictionary: b.dictionary };
    let mut decoder = Decoder::new();
    decoder.reset(b.inflate);
    b.src[..width].fill(255);
    let mut state = PngState { scaler, src: &mut b.src[..width], current_y: 0, unsupported: false, sink, safety: None };
    let mut held = 0;
    loop {
        let got = input.read(&mut b.read[held..READ]).map_err(|_| status(BookStatus::IoError))?;
        let fed = held + got;
        let used = decoder.feed(&b.read[..fed], &mut png, b.inflate, &NoGamma, &mut state)
            .map_err(|_| state.safety.map_or(status(BookStatus::ParseError), RenderError::Safety))?;
        if state.unsupported { return Err(status(BookStatus::Unsupported)); }
        if state.scaler.stopped { return Ok(()); }
        b.read.copy_within(used..fed, 0);
        held = fed - used;
        if got == 0 { break; }
        if held == READ { return Err(status(BookStatus::ParseError)); }
    }
    if !decoder.eof() { return Err(status(BookStatus::Truncated)); }
    state.scaler.feed(state.current_y, state.src, state.sink).map_err(RenderError::Safety)?;
    Ok(())
}
