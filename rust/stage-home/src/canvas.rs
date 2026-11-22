use crate::{
    FRAME_BYTES, Frame, HomePlane, StageBackdrop, cstr,
    span::{Span, fit},
};
use newt_ui_canvas::atlas::{self, Face};

// The portrait frame, in the i32 pixel coordinates this canvas draws with.
const WIDTH: i32 = newt_core::frame::WIDTH as i32;
const HEIGHT: i32 = newt_core::frame::HEIGHT as i32;
#[derive(Clone, Copy)]
pub(crate) enum Anchor {
    Start,
    Middle,
    End,
}
#[derive(Clone, Copy)]
pub(crate) enum Paper {
    Backdrop,
    White,
}
pub(crate) struct StageCanvas<'a> {
    plane: HomePlane,
    frame: &'a mut Frame,
    backdrop: &'a StageBackdrop,
}
fn inside(px: i32, py: i32, x: i32, y: i32, w: i32, h: i32, r: i32) -> bool {
    if w <= 0 || h <= 0 || px < x || py < y || px >= x + w || py >= y + h {
        return false;
    }
    let cx = if px < x + r {
        x + r
    } else if px >= x + w - r {
        x + w - r - 1
    } else {
        px
    };
    let cy = if py < y + r {
        y + r
    } else if py >= y + h - r {
        y + h - r - 1
    } else {
        py
    };
    r == 0 || (px - cx) * (px - cx) + (py - cy) * (py - cy) <= r * r
}
impl<'a> StageCanvas<'a> {
    pub fn new(plane: HomePlane, frame: &'a mut Frame, backdrop: &'a StageBackdrop) -> Self {
        let offset = if plane == HomePlane::Lsb {
            0
        } else {
            FRAME_BYTES
        };
        frame.copy_from_slice(&backdrop[offset..offset + FRAME_BYTES]);
        if plane == HomePlane::Monochrome {
            for b in frame.iter_mut() {
                *b = !*b;
            }
        }
        Self {
            plane,
            frame,
            backdrop,
        }
    }
    fn backdrop_tone(&self, x: i32, y: i32) -> u32 {
        let at = (y * WIDTH + x) as usize;
        let mask = 0x80 >> (at % 8);
        u32::from(self.backdrop[at / 8] & mask != 0)
            | (u32::from(self.backdrop[FRAME_BYTES + at / 8] & mask != 0) << 1)
    }
    pub fn pixel(&mut self, x: i32, y: i32, tone: u32) {
        if !(0..WIDTH).contains(&x) || !(0..HEIGHT).contains(&y) {
            return;
        }
        let at = (y * WIDTH + x) as usize;
        let mask = 0x80 >> (at % 8);
        let set = match self.plane {
            HomePlane::Monochrome => tone < 2,
            HomePlane::Lsb => tone & 1 != 0,
            HomePlane::Msb => tone & 2 != 0,
        };
        if set {
            self.frame[at / 8] |= mask;
        } else {
            self.frame[at / 8] &= !mask;
        }
    }
    pub fn rrect(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, tone: u32) {
        for py in y..y + h {
            for px in x..x + w {
                if inside(px, py, x, y, w, h, r) {
                    self.pixel(px, py, tone);
                }
            }
        }
    }
    pub fn frame(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, t: i32, tone: u32) {
        for py in y..y + h {
            for px in x..x + w {
                if inside(px, py, x, y, w, h, r)
                    && !inside(px, py, x + t, y + t, w - 2 * t, h - 2 * t, 0.max(r - t))
                {
                    self.pixel(px, py, tone);
                }
            }
        }
    }
    pub fn triangle(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, x2: i32, y2: i32, tone: u32) {
        let edge = |ax, ay, bx, by, px, py| (bx - ax) * (py - ay) - (by - ay) * (px - ax);
        let area = if edge(x0, y0, x1, y1, x2, y2) < 0 {
            -1
        } else {
            1
        };
        for y in y0.min(y1).min(y2)..=y0.max(y1).max(y2) {
            for x in x0.min(x1).min(x2)..=x0.max(x1).max(x2) {
                let px = 2 * x + 1;
                let py = 2 * y + 1;
                if area * edge(2 * x1, 2 * y1, 2 * x2, 2 * y2, px, py) >= 0
                    && area * edge(2 * x2, 2 * y2, 2 * x0, 2 * y0, px, py) >= 0
                    && area * edge(2 * x0, 2 * y0, 2 * x1, 2 * y1, px, py) >= 0
                {
                    self.pixel(x, y, tone);
                }
            }
        }
    }
    pub fn text(
        &mut self,
        x: i32,
        baseline: i32,
        source: Span<'_>,
        face: Face,
        max: i32,
        anchor: Anchor,
        ink: u32,
        paper: Paper,
    ) {
        let mut text = source.copy();
        fit(&mut text, face, max);
        let text = cstr(&text);
        let left = 0.max(
            x - match anchor {
                Anchor::End => max,
                Anchor::Middle => max / 2,
                Anchor::Start => 0,
            },
        );
        let right = WIDTH.min(left + max);
        let width = Span::plain(text).width(face);
        let mut pen = x * 64
            - match anchor {
                Anchor::Middle => width / 2,
                Anchor::End => width,
                Anchor::Start => 0,
            };
        let mut at = 0;
        let mut previous = 0;
        while at < text.len() {
            let cp = atlas::next(text, &mut at);
            pen += atlas::kern(face, previous, cp);
            // Signed integer division truncates toward zero.
            let snapped = ((pen + 4) / 8) * 8;
            let stamp = atlas::glyph(face, cp, (((snapped % 64 + 64) % 64) / 8) as u8).stamp;
            atlas::tones(stamp, |pos, coverage| {
                let px = snapped / 64 + i32::from(stamp.x) + (pos % u32::from(stamp.w)) as i32;
                let py = baseline + i32::from(stamp.y) + (pos / u32::from(stamp.w)) as i32;
                if coverage == 255 || px < left || px >= right || !(0..HEIGHT).contains(&py) {
                    return;
                }
                let under = match paper {
                    Paper::White => 3,
                    Paper::Backdrop => self.backdrop_tone(px, py) as i32,
                };
                let mix = (under - ink as i32) * 85 * coverage as i32;
                let gray = ink as i32 * 85 + (if mix < 0 { mix - 127 } else { mix + 127 }) / 255;
                self.pixel(px, py, 3.min((gray + 42) / 85) as u32);
            });
            pen += i32::from(atlas::glyph(face, cp, 0).advance);
            previous = cp;
        }
    }
    pub fn halo(&mut self, x0: i32, y0: i32, w: i32, h: i32) {
        for y in 0.max(y0 - 19)..HEIGHT.min(y0 + h + 19) {
            for x in 0.max(x0 - 19)..WIDTH.min(x0 + w + 19) {
                let dx = 0.max((2 * x + 1 - (2 * x0 + w)).abs() - (w - 20));
                let dy = 0.max((2 * y + 1 - (2 * y0 + h)).abs() - (h - 20));
                let s = dx * dx + dy * dy;
                if s > 400
                    && s < i32::from(HALO_LIMIT[BAYER[(y & 7) as usize][(x & 7) as usize] as usize])
                {
                    self.pixel(x, y, 0);
                }
            }
        }
    }
}
const BAYER: [[u8; 8]; 8] = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
];
const HALO_LIMIT: [u16; 64] = [
    2932, 2738, 2595, 2475, 2371, 2276, 2190, 2110, 2036, 1966, 1899, 1836, 1777, 1720, 1665, 1613,
    1562, 1514, 1467, 1422, 1379, 1337, 1296, 1257, 1219, 1182, 1146, 1111, 1077, 1044, 1012, 981,
    951, 921, 893, 865, 838, 811, 785, 760, 736, 712, 689, 666, 644, 622, 601, 581, 561, 541, 522,
    504, 486, 468, 451, 434, 418, 402, 401, 401, 401, 401, 401, 401,
];

/// Host-only observation of repeated production painting over immutable paper.
#[cfg(feature = "host-probe")]
pub fn observe_repeated_text(
    plane: HomePlane,
    frame: &mut Frame,
    backdrop: &StageBackdrop,
    text: &[u8],
    first: &mut Frame,
) {
    let mut canvas = StageCanvas::new(plane, frame, backdrop);
    canvas.text(
        96,
        220,
        Span::plain(text),
        Face::R21,
        428,
        Anchor::Start,
        0,
        Paper::Backdrop,
    );
    first.copy_from_slice(canvas.frame);
    canvas.text(
        96,
        220,
        Span::plain(text),
        Face::R21,
        428,
        Anchor::Start,
        0,
        Paper::Backdrop,
    );
}
