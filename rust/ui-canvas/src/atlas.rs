use crate::{cstr, data};
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Stamp {
    pub offset: u32,
    pub size: u16,
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
    pub packed: bool,
}
#[repr(C)]
pub struct Glyph {
    pub stamp: Stamp,
    pub advance: i16,
}
#[repr(C)]
pub struct Kern {
    pub face: u8,
    pub left: u8,
    pub right: u8,
    pub adjustment: i16,
}
const _: () = assert!(
    core::mem::size_of::<Stamp>() == 16
        && core::mem::size_of::<Glyph>() == 20
        && core::mem::size_of::<Kern>() == 6
);
#[repr(u8)]
#[derive(Clone, Copy)]
pub enum Face {
    R21,
    R24,
    R25,
    R28,
    R32,
    B24,
    B32,
    B36,
    B40,
}
pub const FACE_PIXELS: [u32; 9] = [21, 24, 25, 28, 32, 24, 32, 36, 40];
pub fn index(cp: u32) -> usize {
    match cp {
        32..=126 => (cp - 32) as usize,
        183 => 95,
        8217 => 96,
        8230 => 97,
        _ => 31,
    }
}
/// Home's deliberately narrow decoder is not the strict folding decoder.
pub fn next(text: &[u8], at: &mut usize) -> u32 {
    if *at >= text.len() || text[*at] == 0 {
        return 0;
    }
    let a = text[*at];
    *at += 1;
    if a < 128 {
        return a.into();
    }
    if a == 0xc2 && text.get(*at) == Some(&0xb7) {
        *at += 1;
        return 183;
    }
    if a == 0xe2 && text.get(*at) == Some(&0x80) && matches!(text.get(*at + 1), Some(0x99 | 0xa6)) {
        let cp = if text[*at + 1] == 0x99 { 8217 } else { 8230 };
        *at += 2;
        return cp;
    }
    while *at < text.len() && text[*at] & 0xc0 == 0x80 {
        *at += 1;
    }
    63
}
pub fn glyph(face: Face, cp: u32, phase: u8) -> &'static Glyph {
    assert!(phase < 8);
    &data::GLYPHS[(face as usize * 98 + index(cp)) * 8 + phase as usize]
}
pub fn kern(face: Face, left: u32, right: u32) -> i32 {
    if left == 0 || right == 0 {
        return 0;
    }
    let key = ((face as u32) << 16) | ((index(left) as u32) << 8) | index(right) as u32;
    let mut lo = 0;
    let mut hi = data::KERNS.len();
    while lo < hi {
        let m = lo + (hi - lo) / 2;
        let k = &data::KERNS[m];
        let observed = ((k.face as u32) << 16) | ((k.left as u32) << 8) | k.right as u32;
        if observed < key {
            lo = m + 1;
        } else {
            hi = m;
        }
    }
    if lo == data::KERNS.len() {
        return 0;
    }
    let k = &data::KERNS[lo];
    if k.face == face as u8 && k.left as usize == index(left) && k.right as usize == index(right) {
        k.adjustment.into()
    } else {
        0
    }
}
pub fn width64(text: Option<&[u8]>, face: Face) -> i32 {
    let t = cstr(text);
    let mut at = 0;
    let mut previous = 0;
    let mut width = 0;
    while at < t.len() {
        let cp = next(t, &mut at);
        width += glyph(face, cp, 0).advance as i32 + kern(face, previous, cp);
        previous = cp;
    }
    width
}
pub fn tones(stamp: Stamp, mut emit: impl FnMut(u32, u32)) {
    if stamp.packed {
        for pos in 0..u32::from(stamp.w) * u32::from(stamp.h) {
            emit(
                pos,
                u32::from(
                    (data::RUNS[stamp.offset as usize + pos as usize / 4] >> (6 - 2 * (pos % 4)))
                        & 3,
                ) * 85,
            );
        }
    } else {
        let mut pos = 0;
        for i in (0..stamp.size as usize).step_by(2) {
            for _ in 0..data::RUNS[stamp.offset as usize + i] {
                emit(pos, data::RUNS[stamp.offset as usize + i + 1].into());
                pos += 1;
            }
        }
    }
}
pub fn face(p: u32, bold: bool) -> Face {
    if bold {
        if p >= 40 {
            Face::B40
        } else if p >= 36 {
            Face::B36
        } else if p >= 32 {
            Face::B32
        } else {
            Face::B24
        }
    } else if p >= 32 {
        Face::R32
    } else if p >= 28 {
        Face::R28
    } else if p >= 25 {
        Face::R25
    } else if p >= 24 {
        Face::R24
    } else {
        Face::R21
    }
}
pub fn pixels(w: i32, f: Face, p: u32) -> i32 {
    (w * p as i32 + FACE_PIXELS[f as usize] as i32 * 64 - 1) / (FACE_PIXELS[f as usize] as i32 * 64)
        + 1
}
pub fn text_width(t: Option<&[u8]>, p: u32, bold: bool) -> i32 {
    let p = p.clamp(8, 40);
    let f = face(p, bold);
    pixels(width64(t, f), f, p)
}
