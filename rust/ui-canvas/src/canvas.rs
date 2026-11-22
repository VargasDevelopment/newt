use crate::{
    Frame, HEIGHT, WIDTH, atlas, cstr,
    paint::{Clip, draw_text},
    text::{copy_cstr, rows},
};
/// A borrowed logical frame. No framebuffer, cache or retained scratch is owned.
pub struct UiCanvas<'a> {
    frame: Option<&'a mut Frame>,
    width: i32,
    height: i32,
    clip: Clip,
    #[cfg(feature = "host-probe")]
    pub incomplete_paragraphs: u32,
}
impl<'a> UiCanvas<'a> {
    pub fn portrait(frame: Option<&'a mut Frame>) -> Self {
        Self::new(frame, WIDTH as i32, HEIGHT as i32)
    }
    pub fn new(frame: Option<&'a mut Frame>, width: i32, height: i32) -> Self {
        if width > 0 && height > 0 {
            let bits = i64::from(width) * i64::from(height);
            assert!(bits <= i64::from(i32::MAX));
            if frame.is_some() {
                assert!(bits <= (crate::FRAME_BYTES * 8) as i64);
            }
        }
        Self {
            frame,
            width,
            height,
            clip: Clip {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            },
            #[cfg(feature = "host-probe")]
            incomplete_paragraphs: 0,
        }
    }
    pub fn clip(&mut self, x: i32, y: i32, width: i32, height: i32) {
        self.clip = Clip {
            left: 0.max(x),
            top: 0.max(y),
            right: self.width.min(x + width),
            bottom: self.height.min(y + height),
        };
    }
    pub fn pixel(&mut self, x: i32, y: i32, black: bool) {
        let c = self.clip;
        if x < c.left || y < c.top || x >= c.right || y >= c.bottom {
            return;
        }
        let Some(frame) = self.frame.as_deref_mut() else {
            return;
        };
        let byte = &mut frame[(y * self.width + x) as usize / 8];
        let bit = 0x80u8 >> (x & 7);
        if black {
            *byte |= bit;
        } else {
            *byte &= !bit;
        }
    }
    pub fn rect(&mut self, x: i32, y: i32, width: i32, height: i32, black: bool) {
        for yy in y.max(self.clip.top)..(y + height).min(self.clip.bottom) {
            for xx in x.max(self.clip.left)..(x + width).min(self.clip.right) {
                self.pixel(xx, yy, black);
            }
        }
    }
    pub fn outline(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.rect(x, y, w, 1, true);
        self.rect(x, y + h - 1, w, 1, true);
        self.rect(x, y, 1, h, true);
        self.rect(x + w - 1, y, 1, h, true);
    }
    pub fn panel(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.rect(x, y, w, h, false);
        self.rect(x + 4, y, w - 8, 1, true);
        self.rect(x + 4, y + h - 1, w - 8, 1, true);
        self.rect(x, y + 4, 1, h - 8, true);
        self.rect(x + w - 1, y + 4, 1, h - 8, true);
        for offset in 1..4 {
            let inset = if offset == 1 { 2 } else { 1 };
            self.pixel(x + inset, y + offset, true);
            self.pixel(x + w - 1 - inset, y + offset, true);
            self.pixel(x + inset, y + h - 1 - offset, true);
            self.pixel(x + w - 1 - inset, y + h - 1 - offset, true);
        }
    }
    pub fn text(
        &mut self,
        x: i32,
        top: i32,
        value: Option<&[u8]>,
        pixels: u32,
        bold: bool,
        max_width: i32,
        black: bool,
    ) {
        let Some(value) = value else { return };
        let mut fitted = [0; 256];
        copy_cstr(value, &mut fitted);
        let available = if max_width > 0 {
            max_width.min(self.clip.right - x)
        } else {
            self.clip.right - x
        };
        if available <= 0 {
            return;
        }
        if atlas::text_width(Some(&fitted), pixels, bold) > available {
            let mut len = cstr(Some(&fitted)).len();
            while len != 0
                && atlas::text_width(Some(&fitted), pixels, bold)
                    + atlas::text_width(Some("…".as_bytes()), pixels, bold)
                    > available
            {
                len -= 1;
                while len != 0 && fitted[len] & 0xc0 == 0x80 {
                    len -= 1;
                }
                fitted[len] = 0;
            }
            if len + 4 <= fitted.len() {
                fitted[len..len + 4].copy_from_slice("…\0".as_bytes());
            }
        }
        draw_text(
            self.frame.as_deref_mut().map(|frame| &mut frame[..]),
            self.width,
            self.height,
            x,
            top + pixels as i32,
            Some(&fitted),
            pixels,
            bold,
            black,
            Clip {
                left: self.clip.left.max(x),
                top: self.clip.top,
                right: self.clip.right.min(x + available),
                bottom: self.clip.bottom,
            },
        );
    }
    pub fn center(&mut self, top: i32, value: Option<&[u8]>, pixels: u32, bold: bool) {
        let width = atlas::text_width(value, pixels, bold).min(self.clip.right - self.clip.left);
        self.text(
            self.clip.left + (self.clip.right - self.clip.left - width) / 2,
            top,
            value,
            pixels,
            bold,
            width,
            true,
        );
    }
    fn counted(&mut self, whole: bool) -> bool {
        #[cfg(feature = "host-probe")]
        {
            self.incomplete_paragraphs = self.incomplete_paragraphs.wrapping_add(u32::from(!whole));
        }
        whole
    }
    pub fn paragraph(
        &mut self,
        x: i32,
        mut top: i32,
        value: Option<&[u8]>,
        pixels: u32,
        width: i32,
        bottom: i32,
    ) -> bool {
        let whole = rows(value, pixels, width, true, |row| {
            if top + (pixels.wrapping_add(pixels / 4)) as i32 > bottom.min(self.clip.bottom) {
                return false;
            }
            self.text(x, top, Some(row), pixels, false, width, true);
            top += pixels as i32 + 9;
            true
        });
        self.counted(whole)
    }
    pub fn fixed_complete_paragraph(
        &mut self,
        x: i32,
        mut top: i32,
        value: Option<&[u8]>,
        pixels: u32,
        width: i32,
        bottom: i32,
    ) -> i32 {
        if !self.counted(pixels != 0 && pixels <= 40) {
            return -1;
        }
        let width = width.min(self.clip.right - x);
        let bottom = bottom.min(self.clip.bottom);
        for words in [true, false] {
            let mut measured_top = top;
            let fits = rows(value, pixels, width, words, |_| {
                let fits = measured_top + (pixels.wrapping_add(pixels / 4)) as i32 <= bottom;
                measured_top += pixels as i32 + 9;
                fits
            });
            if !fits {
                continue;
            }
            rows(value, pixels, width, words, |row| {
                self.text(x, top, Some(row), pixels, false, width, true);
                top += pixels as i32 + 9;
                true
            });
            return top;
        }
        self.counted(false);
        -1
    }
    pub fn complete_paragraph(
        &mut self,
        x: i32,
        top: i32,
        value: Option<&[u8]>,
        pixels: u32,
        width: i32,
        bottom: i32,
    ) -> bool {
        let mut pixels = pixels.clamp(8, 40);
        loop {
            #[cfg(feature = "host-probe")]
            let before = self.incomplete_paragraphs;
            if self.fixed_complete_paragraph(x, top, value, pixels, width, bottom) >= 0 {
                return true;
            }
            if pixels == 8 {
                return false;
            }
            #[cfg(feature = "host-probe")]
            {
                self.incomplete_paragraphs = before;
            }
            pixels -= 1;
        }
    }
    pub fn focus(&mut self, x: i32, top: i32, width: i32, value: Option<&[u8]>, selected: bool) {
        if selected {
            for yy in 0..17 {
                self.rect(x, top + 8 + yy, 1 + yy.min(16 - yy), 1, true);
            }
            self.rect(x + 22, top + 34, width - 22, 2, true);
        }
        self.text(x + 22, top, value, 24, selected, width - 22, true);
    }
}
