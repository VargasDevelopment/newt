use crate::{
    FRAME_BYTES,
    atlas::{self, Glyph},
    cstr,
};
#[derive(Clone, Copy)]
pub struct Clip {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}
/// The original permits any positive geometry with exactly one frame's pixels.
pub fn draw_text(
    frame: Option<&mut [u8]>,
    width: i32,
    height: i32,
    x: i32,
    baseline: i32,
    text: Option<&[u8]>,
    pixels: u32,
    bold: bool,
    black: bool,
    clip: Clip,
) {
    let (Some(frame), Some(text)) = (frame, text) else {
        return;
    };
    if width <= 0 || height <= 0 || i64::from(width) * i64::from(height) != (FRAME_BYTES * 8) as i64
    {
        return;
    }
    assert!(frame.len() >= FRAME_BYTES);
    let p = pixels.clamp(8, 40);
    let face = atlas::face(p, bold);
    let s = atlas::FACE_PIXELS[face as usize] as i32;
    let p = p as i32;
    let scaled = |v: i32| (v * p).div_euclid(s);
    let end = |v: i32| (v * p - 1).div_euclid(s) + 1;
    let mut plot = |xx: i32, yy: i32| {
        if xx < 0.max(clip.left)
            || yy < 0.max(clip.top)
            || xx >= width.min(clip.right)
            || yy >= height.min(clip.bottom)
        {
            return;
        }
        let byte = &mut frame[(yy * width + xx) as usize / 8];
        let bit = 0x80u8 >> (xx & 7);
        if black {
            *byte |= bit;
        } else {
            *byte &= !bit;
        }
    };
    let mut paint = |g: &Glyph, origin: i32| {
        let st = g.stamp;
        let mut painted = false;
        if p < s {
            let mut darkness = [[0u32; 24]; 2];
            let left = scaled(st.x.into());
            let right = end(i32::from(st.x) + i32::from(st.w));
            assert!(right - left <= 24);
            let mut row = scaled(st.y.into());
            let flush = |row: &mut i32,
                         cells: &mut [[u32; 24]; 2],
                         plot: &mut dyn FnMut(i32, i32),
                         painted: &mut bool| {
                let line = &mut cells[(*row & 1) as usize];
                for cell in 0..right - left {
                    let ink = 2 * line[cell as usize] > 255 * (s * s) as u32;
                    if ink {
                        plot(origin + left + cell, baseline + *row);
                    }
                    *painted |= ink;
                }
                line.fill(0);
                *row += 1;
            };
            atlas::tones(st, |position, tone| {
                let sx = i32::from(st.x) + (position % u32::from(st.w)) as i32;
                let sy = i32::from(st.y) + (position / u32::from(st.w)) as i32;
                let top = scaled(sy);
                while row < top {
                    flush(&mut row, &mut darkness, &mut plot, &mut painted);
                }
                if tone >= 255 {
                    return;
                }
                let column = scaled(sx);
                for ty in top..=top + 1 {
                    let down = ((sy + 1) * p).min((ty + 1) * s) - (sy * p).max(ty * s);
                    if down <= 0 {
                        continue;
                    }
                    for tx in column..=column + 1 {
                        let across = ((sx + 1) * p).min((tx + 1) * s) - (sx * p).max(tx * s);
                        if across > 0 {
                            darkness[(ty & 1) as usize][(tx - left) as usize] +=
                                (down * across) as u32 * (255 - tone);
                        }
                    }
                }
            });
            while row <= scaled(i32::from(st.y) + i32::from(st.h) - 1) + 1 {
                flush(&mut row, &mut darkness, &mut plot, &mut painted);
            }
        }
        if !painted {
            atlas::tones(st, |position, tone| {
                if tone >= 128 {
                    return;
                }
                let sx = i32::from(st.x) + (position % u32::from(st.w)) as i32;
                let sy = i32::from(st.y) + (position / u32::from(st.w)) as i32;
                let px = origin + scaled(sx);
                let py = baseline + scaled(sy);
                for yy in py..py + 1.max(scaled(sy + 1) - scaled(sy)) {
                    for xx in px..px + 1.max(scaled(sx + 1) - scaled(sx)) {
                        plot(xx, yy);
                    }
                }
            });
        }
    };
    let text = cstr(Some(text));
    let mut at = 0;
    let mut previous = 0;
    let mut pen = 0;
    while at < text.len() {
        let cp = atlas::next(text, &mut at);
        pen += atlas::kern(face, previous, cp);
        let g = atlas::glyph(face, cp, 0);
        let origin = x + pen * p / (s * 64);
        if g.stamp.w != 0 {
            paint(g, origin);
        }
        pen += i32::from(g.advance);
        previous = cp;
    }
}
