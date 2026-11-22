//! Reading glyph and kerning atlas (`NewtReadingAtlas`). MIT; font data OFL-1.1.
#![no_std]
#![forbid(unsafe_code)]
#![allow(non_upper_case_globals)]
use newt_core::{frame, page_renderer::Font};

#[repr(C)]
struct AtlasGlyph { width: u8, height: u8, advance16: u16, left: i8, top: i8, offset: u16 }
#[repr(C)]
struct AtlasKern { key: u16, value16: i8 }
#[repr(C)]
struct AtlasKernClasses { left: u8, right: u8 }
#[repr(C)]
struct AddedKernClasses { left: u16, right: u16 }
mod data { use super::{AtlasGlyph, AtlasKern, AtlasKernClasses, AddedKernClasses}; include!("data.rs"); }
use data::*;
const _: () = assert!(core::mem::size_of::<AtlasGlyph>() == 8);
const _: () = assert!(core::mem::size_of::<AtlasKern>() == 4);
const _: () = assert!(core::mem::size_of::<AtlasKernClasses>() == 2);
const _: () = assert!(core::mem::size_of::<AddedKernClasses>() == 4);
pub const PROFILE_SIZE: u16 = 30;
pub const IDENTITY: u32 = 0xce2bda74;
const ASCII: usize = 95;
const LEGACY: usize = ASCII + kReaderExtraCodepoints.len();
const COUNT: usize = LEGACY + kReaderAddedCodepoints.len();

mod faces;
use faces::{Face, AddedFace, face, added};

fn glyph_index(cp: u32) -> usize {
    if (32..=126).contains(&cp) { return (cp - 32) as usize; }
    let Ok(cp) = u16::try_from(cp) else { return COUNT; };
    if let Ok(i) = kReaderExtraCodepoints.binary_search(&cp) { return ASCII + i; }
    kReaderAddedCodepoints.binary_search(&cp).map_or(COUNT, |i| LEGACY + i)
}
fn glyph(f: &Face, a: &AddedFace, index: usize) -> &'static AtlasGlyph {
    if index < ASCII { &f.glyphs[index] }
    else if index < LEGACY { &f.extra[index - ASCII] }
    else { &a.glyphs[index - LEGACY] }
}
const fn range(first: u32, last: u32) -> u32 { (first << 12) | (last - first) }
const IGNORABLE: [u32; 17] = [range(0xad,0xad), range(0x34f,0x34f), range(0x61c,0x61c), range(0x115f,0x1160), range(0x17b4,0x17b5), range(0x180b,0x180f), range(0x200b,0x200f), range(0x202a,0x202e), range(0x2060,0x206f), range(0x3164,0x3164), range(0xfe00,0xfe0f), range(0xfeff,0xfeff), range(0xffa0,0xffa0), range(0xfff0,0xfff8), range(0x1bca0,0x1bca3), range(0x1d173,0x1d17a), range(0xe0000,0xe0fff)];
fn ignorable(cp: u32) -> bool {
    for &entry in &IGNORABLE { let first = entry >> 12; if cp < first { return false; } if cp - first <= (entry & 0xfff) { return true; } }
    false
}
fn semantic(f: &Face, a: &AddedFace, cp: u32) -> i32 {
    if (0x2000..=0x200a).contains(&cp) || cp == 0x202f || cp == 0x205f { return 3 * i32::from(glyph(f, a, glyph_index(cp)).advance16); }
    if cp == 0x3000 { return 1440; }
    if matches!(cp, 0x85 | 0x2028 | 0x2029) || ignorable(cp) { 0 } else { -1 }
}
pub const fn effective_size(px: u16) -> i32 { if px < 8 { 8 } else if px > 90 { 90 } else { px as i32 } }
fn rounded(value16: i32, px: i32) -> i32 { (value16 * px + 240) / 480 }
fn scaled(value: i32, px: i32) -> i32 { let n = (value.abs() * px + 15) / 30; if value < 0 { -n } else { n } }
fn cell(f: &Face, a: &AddedFace, index: usize, size: i32) -> i32 { if index < COUNT { rounded(i32::from(glyph(f,a,index).advance16), size) } else { size } }
fn kern(f: &Face, a: &AddedFace, left: usize, right: usize) -> i32 {
    if left >= LEGACY || right >= LEGACY {
        let row = usize::from(a.classes[left].left); let col = a.classes[right].right;
        let start = usize::from(a.rows[row]); let end = usize::from(a.rows[row+1]);
        return a.cols[start..end].binary_search(&col).map_or(0, |i| i32::from(a.values[start+i]));
    }
    if f.ascii.is_empty() || left >= ASCII || right >= ASCII {
        let row = usize::from(f.classes[left].left); let col = f.classes[right].right;
        if row == 0 || col == 0 { return 0; }
        for i in usize::from(f.rows[row-1])..usize::from(f.rows[row]) { if f.cols[i] == col-1 { return i32::from(f.values[i]); } }
        return 0;
    }
    let key = (left * ASCII + right) as u16;
    f.ascii.binary_search_by_key(&key, |k| k.key).map_or(0, |i| i32::from(f.ascii[i].value16))
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Geometry { pub width: i32, pub height: i32, pub margin: i32, pub content_bottom: i32, pub bar_y: i32, pub footer_y: i32 }
impl Geometry {
    pub const fn reader(landscape: bool) -> Self {
        let (portrait_width, portrait_height) = (frame::WIDTH as i32, frame::HEIGHT as i32);
        let (width,height) = if landscape { (portrait_height,portrait_width) } else { (portrait_width,portrait_height) };
        Self { width, height, margin: 16, content_bottom: height-48, bar_y: height-42, footer_y: height-33 }
    }
    pub fn valid(self) -> bool {
        frame::is_frame_size(self.width as usize, self.height as usize) && self.margin >= 0 && self.margin < self.width/2 && self.margin < self.height && self.content_bottom >= self.margin && self.content_bottom <= self.height
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DrawError { Geometry, Frame }
#[derive(Default)]
pub struct ReaderAtlas;
impl Font for ReaderAtlas {
    fn advance(&mut self, cp: u32, px: u16, style: u8) -> i16 {
        let f = face(style); let a = added(style); let size = effective_size(px);
        if (32..=126).contains(&cp) { return rounded(i32::from(f.glyphs[(cp-32) as usize].advance16), size) as i16; }
        let width = semantic(f,a,cp);
        if width >= 0 { ((width*size + 720)/1440) as i16 } else { cell(f,a,glyph_index(cp),size) as i16 }
    }
    fn line_height(&mut self, px: u16) -> i16 { scaled(36,effective_size(px)) as i16 }
    fn ascent(&mut self, px: u16) -> i16 { effective_size(px) as i16 }
    fn covers(&mut self, cp: u32) -> bool { glyph_index(cp) < COUNT }
    fn kerning(&mut self, left: u32, right: u32, px: u16, style: u8) -> i16 {
        let f = face(style); let a = added(style);
        if semantic(f,a,left) >= 0 || semantic(f,a,right) >= 0 { return 0; }
        let l = glyph_index(left); let r = glyph_index(right);
        if l >= COUNT || r >= COUNT { return 0; }
        let size = effective_size(px); let advance = i32::from(glyph(f,a,l).advance16);
        (rounded(advance+kern(f,a,l,r),size)-rounded(advance,size)) as i16
    }
}
fn nibble(value: u8) -> u8 { ((value>>4)&8) | ((value>>3)&4) | ((value>>2)&2) | ((value>>1)&1) }
impl ReaderAtlas {
    /// Caller owns the exact packed frame; rejection precedes every write.
    pub fn draw(&self, frame: &mut [u8], geometry: Geometry, pen_x: i32, baseline: i32, cp: u32, px: u16, style: u8) -> Result<(),DrawError> {
        if !geometry.valid() { return Err(DrawError::Geometry); }
        if frame.len() != (geometry.width * geometry.height / 8) as usize { return Err(DrawError::Frame); }
        let f = face(style); let a = added(style);
        if semantic(f,a,cp) >= 0 { return Ok(()); }
        let found = glyph_index(cp); let index = if found < COUNT { found } else { (b'?' - 32) as usize };
        let g = glyph(f,a,index);
        let bitmap = if index >= LEGACY { &kReaderAddedBitmap[a.offsets[index-LEGACY] as usize..] } else if index < ASCII { f.bitmap } else { f.extra_bitmap };
        if g.width == 0 || g.height == 0 { return Ok(()); }
        let size = effective_size(px);
        let width = scaled(i32::from(g.width),size).max(1); let height = scaled(i32::from(g.height),size).max(1);
        let left = i64::from(pen_x)+i64::from(scaled(i32::from(g.left),size));
        let top = i64::from(baseline)-i64::from(scaled(i32::from(g.top),size));
        let overhang = if pen_x >= geometry.margin && i64::from(pen_x)+i64::from(cell(f,a,found,size)) <= i64::from(geometry.width-geometry.margin) { geometry.margin/2 } else { 0 };
        let clip_left = i64::from(geometry.margin-overhang); let clip_right = i64::from(geometry.width-geometry.margin+overhang);
        let stride = (usize::from(g.width)+3)/4;
        if size == 30 && left >= clip_left && left+i64::from(width) <= clip_right && top >= i64::from(geometry.margin) && top+i64::from(height) <= i64::from(geometry.content_bottom) {
            for row in 0..usize::from(g.height) {
                let source = &bitmap[usize::from(g.offset)+row*stride..];
                for col in (0..usize::from(g.width)).step_by(8) {
                    let mut ink = nibble(source[col/4])<<4;
                    if col+4 < usize::from(g.width) { ink |= nibble(source[col/4+1]); }
                    let remaining = (usize::from(g.width)-col).min(8);
                    ink &= (0xffu16 << (8-remaining)) as u8;
                    let x = left as usize+col; let shift = x%8;
                    let target = (top as usize+row)*(geometry.width as usize/8)+x/8;
                    frame[target] |= ink>>shift;
                    let carry = ((u16::from(ink) << (8-shift)) & 255) as u8;
                    if carry != 0 { frame[target+1] |= carry; }
                }
            }
            return Ok(());
        }
        for row in 0..height {
            let y = top+i64::from(row);
            if y < i64::from(geometry.margin) || y >= i64::from(geometry.content_bottom) { continue; }
            let source_row = ((2*row+1)*i32::from(g.height)/(2*height)) as usize;
            for col in 0..width {
                let x = left+i64::from(col);
                if x < clip_left || x >= clip_right { continue; }
                let source_col = ((2*col+1)*i32::from(g.width)/(2*width)) as usize;
                let value = bitmap[usize::from(g.offset)+source_row*stride+source_col/4];
                if ((value>>(6-2*(source_col%4)))&3) < 2 { continue; }
                let bit = (y*i64::from(geometry.width)+x) as usize;
                frame[bit/8] |= 0x80>>(bit%8);
            }
        }
        Ok(())
    }
}
pub mod probe;
#[cfg(test)]
mod safety_tests;
