// SPDX-License-Identifier: MIT
//! Cover JPEG validation pins (baseline, or progressive for its first scan)
//! and ordered cover placement. A cover PNG pins its header here too.
use super::read;
use crate::zip_window::source::BookSource;
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Status { Ready, Unsupported, Incomplete, Invalid, IoError }
#[repr(C)]
pub struct Layout {
    at: [u32; 18], length: [u8; 18], bytes: [u8; 85], pins: u32, used: u32,
    pub width: u16, pub height: u16,
    /// SOF2: only the first scan, which holds the DC values, is drawn.
    pub progressive: bool,
    /// The first component's sampling factors (high nibble horizontal).
    pub sampling: u8,
    /// A PNG's ring bytes (a filtered row and two pixels); zero for a JPEG.
    pub ring: u16,
}
impl Default for Layout {
    fn default() -> Self { Self { at: [0;18], length: [0;18], bytes: [0;85], pins: 0,
        used: 0, width: 0, height: 0, progressive: false, sampling: 0x11, ring: 0 } }
}
impl Layout {
    pub(super) fn pin(&mut self, at: u64, data: &[u8]) {
        self.at[self.pins as usize] = at as u32;
        self.length[self.pins as usize] = data.len() as u8;
        self.pins += 1;
        self.bytes[self.used as usize..self.used as usize + data.len()].copy_from_slice(data);
        self.used += data.len() as u32;
    }
    pub fn matches(&self, offset: u64, data: &[u8]) -> bool {
        let mut base = 0;
        for i in 0..self.pins as usize {
            let begin = offset.max(u64::from(self.at[i]));
            let end = (offset + data.len() as u64).min(u64::from(self.at[i]) + u64::from(self.length[i]));
            if begin < end && data[(begin-offset) as usize..(end-offset) as usize]
                != self.bytes[base + (begin-u64::from(self.at[i])) as usize..base + (end-u64::from(self.at[i])) as usize] {
                return false;
            }
            base += usize::from(self.length[i]);
        }
        true
    }
}
pub fn validate(source: &mut dyn BookSource, layout: &mut Layout) -> Status {
    use Status::*;
    let mut header = [0;4];
    if source.size() < 4 { return Incomplete; }
    if !read(source, 0, &mut header[..2]) { return IoError; }
    if header[..2] != [255,216] { return Unsupported; }
    layout.pin(0, &header[..2]);
    let (mut at, mut segments) = (2, 0);
    loop {
        if at >= 64 * 1024 { return Unsupported; }
        if at + 4 > source.size() { return Incomplete; }
        if !read(source, at, &mut header) { return IoError; }
        let marker = header[1]; let bytes = usize::from(u16::from_be_bytes([header[2],header[3]]));
        if header[0] != 255 || matches!(marker, 0 | 0xd9 | 0xda) { return Invalid; }
        if matches!(marker, 255 | 1 | 0xd8 | 0xd0..=0xd7) { return Unsupported; }
        if bytes < 2 { return Invalid; }
        if bytes == 2 { return Unsupported; }
        if at + 2 + bytes as u64 > source.size() { return Incomplete; }
        let frame = (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc);
        if frame && !matches!(marker, 0xc0 | 0xc2) { return Unsupported; }
        if !frame {
            if segments == 16 { return Unsupported; }
            layout.pin(at,&header); at += 2 + bytes as u64; segments += 1; continue;
        }
        if bytes < 8 { return Invalid; }
        if bytes > 17 { return Unsupported; }
        let mut sof = [0;19]; sof[..4].copy_from_slice(&header);
        if !read(source, at + 4, &mut sof[4..bytes+2]) { return IoError; }
        let components = usize::from(sof[9]);
        if sof[4] != 8 { return Invalid; }
        if components != 1 && components != 3 { return Unsupported; }
        if bytes != 8 + 3 * components { return Invalid; }
        if if components == 1 { sof[11] != 0x11 } else { !matches!(sof[11],0x11|0x21|0x22) } { return Unsupported; }
        for i in 0..components {
            if sof[12+3*i] > 3 { return Invalid; }
            if i != 0 && sof[11+3*i] != 0x11 { return Unsupported; }
        }
        layout.progressive = marker == 0xc2; layout.sampling = sof[11];
        layout.height = u16::from_be_bytes([sof[5],sof[6]]);
        layout.width = u16::from_be_bytes([sof[7],sof[8]]);
        if layout.width == 0 || layout.height == 0 || layout.width > 2048 || layout.height > 3072 { return Unsupported; }
        layout.pin(at,&sof[..bytes+2]); return Ready;
    }
}
pub fn placement(width: u16, height: u16) -> (u16,u16) {
    let mut w = u32::from(width).min(144); let mut h = (u32::from(height)*w/u32::from(width)).max(1);
    if h > 188 { h = 188; w = (u32::from(width)*h/u32::from(height)).max(1); }
    (w as u16,h as u16)
}
pub fn scale(frame: (u16,u16), target: (u16,u16)) -> u8 {
    let mut s = 0;
    while s < 3 && frame.0 >> (s+1) >= target.0 && frame.1 >> (s+1) >= target.1 { s += 1; }
    s
}
/// Decode scratch after the 1 KiB CRC skip buffer, with 128 B for alignment.
pub fn arena_bytes(layout: &Layout, target: (u16,u16)) -> usize {
    let frame = (layout.width, layout.height);
    if layout.progressive {
        // progressive::render: input, four tables, one MCU row of DC greys, three rows.
        let (h, v) = (usize::from(layout.sampling >> 4), usize::from(layout.sampling & 15));
        let grid = usize::from(layout.width).div_ceil(8 * h) * h * v;
        return 1024 + 512 + 4 * core::mem::size_of::<crate::jpeg::progressive::Huffman>()
            + grid + 3 * usize::from(target.0) + 128;
    }
    let scaled = usize::from(frame.0 >> scale(frame,target));
    1024 + 8192 + 16 * scaled + 7 * usize::from(target.0).min(scaled) + 128
}
pub fn dither_row(gray: &[u8], y: usize, bits: &mut [u8;18]) {
    const BAYER: [[u8;4];4] = [[15,135,45,165],[195,75,225,105],[60,180,30,150],[240,120,210,90]];
    bits.fill(0);
    for (x,&gray) in gray.iter().enumerate() {
        if gray < BAYER[y & 3][x & 3] { bits[x >> 3] |= 0x80 >> (x & 7); }
    }
}
