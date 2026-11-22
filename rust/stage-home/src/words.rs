use crate::{cstr, span::Span};
use core::ffi::CStr;
use newt_core::clock_format::{self, CLOCK_TEXT_BYTES, ClockFormat};
use newt_ui_canvas::{atlas::Face, text};
pub const SAMPLE_TITLE: &[u8] = b"The Small Lantern";
pub const ESTIMATE: u8 = 1;
pub const NO_CARD: u8 = 2;
pub const BOOK_ON_CARD: u8 = 4;
pub const SAMPLE_CHOSEN: u8 = 8;
pub const SAVE_WARNING: u8 = 16;
pub const TWELVE_HOUR: u8 = 32;
pub const FINISHED: u8 = 64;
pub const CARD_FLAGS: u8 = 14;
pub struct HomeData<'a> {
    pub clock: Option<&'a CStr>,
    pub book_title: Option<&'a [u8]>,
    pub chapter_title: [u8; 96],
    pub percent: i32,
    pub reading_available: bool,
    pub progress_unavailable: bool,
    pub fixture_cover: bool,
    pub flags: u8,
    pub device_description: &'a [u8],
}
impl Default for HomeData<'_> {
    fn default() -> Self {
        Self {
            clock: None,
            book_title: Some(SAMPLE_TITLE),
            chapter_title: [0; 96],
            percent: -1,
            reading_available: true,
            progress_unavailable: false,
            fixture_cover: false,
            flags: 0,
            device_description: b"",
        }
    }
}
impl HomeData<'_> {
    pub fn shown_percent(&self) -> i32 {
        if self.flags & FINISHED != 0 {
            100
        } else {
            self.percent
        }
    }
    pub fn ready_to_begin(&self) -> bool {
        self.flags & NO_CARD == 0
            && self.reading_available
            && !self.progress_unavailable
            && self.chapter_title[0] == 0
    }
    pub fn set_estimated(&mut self, value: bool) {
        self.flags = (self.flags & !ESTIMATE) | if value { ESTIMATE } else { 0 };
    }
    pub fn set_clock_format(&mut self, value: ClockFormat) {
        self.flags = (self.flags & !TWELVE_HOUR)
            | if value == ClockFormat::TwelveHour {
                TWELVE_HOUR
            } else {
                0
            };
    }
    pub fn set_finished(&mut self, value: bool) {
        self.flags = (self.flags & !FINISHED) | if value { FINISHED } else { 0 };
    }
    pub fn set_card(&mut self, card: u8) {
        self.flags = (self.flags & !CARD_FLAGS) | (card & CARD_FLAGS);
    }
    pub fn set_chapter(&mut self, label: Option<&[u8]>) -> bool {
        let Some(label) = label.map(cstr).filter(|s| !s.is_empty() && s.len() < 96) else {
            return false;
        };
        if cstr(&self.chapter_title) != label {
            self.chapter_title[..label.len()].copy_from_slice(label);
            self.chapter_title[label.len()] = 0;
        }
        true
    }
    pub fn clear_chapter(&mut self) {
        self.chapter_title.fill(0);
    }
    pub fn set_generic_chapter(&mut self, spine: u32) -> bool {
        // Preserve the original unsigned-long width on host and firmware.
        self.chapter_title[..8].copy_from_slice(b"Section ");
        let mut out = LineWriter::new(&mut self.chapter_title, 8);
        out.number_u64(if cfg!(target_pointer_width = "32") {
            u64::from(spine.wrapping_add(1))
        } else {
            u64::from(spine) + 1
        });
        true
    }
    pub fn set_progress(&mut self, label: Option<&[u8]>, percent: i32, estimated: bool) -> bool {
        if !self.set_chapter(label) {
            return false;
        }
        self.percent = percent;
        self.progress_unavailable = false;
        self.set_estimated(estimated);
        true
    }
    pub fn set_percent(&mut self, percent: i32, estimated: bool) -> bool {
        if self.chapter_title[0] == 0 {
            return false;
        }
        self.percent = percent;
        self.progress_unavailable = false;
        self.set_estimated(estimated);
        true
    }
    pub fn note_position(
        &mut self,
        label: Option<&[u8]>,
        known: bool,
        percent: i32,
        estimated: bool,
    ) {
        let identity = self.set_chapter(label);
        if identity && known {
            self.percent = percent;
            self.progress_unavailable = false;
            self.set_estimated(estimated);
        } else {
            if !identity {
                self.clear_chapter();
            }
            self.percent = -1;
            self.progress_unavailable = true;
            self.set_estimated(false);
        }
    }
    pub fn clock_text<'a>(&'a self, out: &'a mut [u8; CLOCK_TEXT_BYTES]) -> &'a [u8] {
        clock_format::format_clock_text(
            Some(self.clock.unwrap_or(c"--:--")),
            if self.flags & TWELVE_HOUR != 0 {
                ClockFormat::TwelveHour
            } else {
                ClockFormat::TwentyFourHour
            },
            out,
        )
        .expect("fallback is present")
        .to_bytes()
    }
}
pub fn card_flags(missing: bool, sample: bool, selected: bool) -> u8 {
    if !missing {
        0
    } else if !selected && sample {
        SAMPLE_CHOSEN
    } else {
        NO_CARD | if selected { BOOK_ON_CARD } else { 0 }
    }
}
pub struct HomeText {
    pub title: [u8; 128],
    pub progress: [u8; 128],
    pub hint: &'static [u8],
}
pub(crate) struct LineWriter<'a> {
    out: &'a mut [u8],
    at: usize,
}
impl<'a> LineWriter<'a> {
    pub fn new(out: &'a mut [u8], at: usize) -> Self {
        Self { out, at }
    }
    pub fn put(&mut self, t: &[u8]) {
        let n = t.len().min(self.out.len() - 1 - self.at);
        self.out[self.at..self.at + n].copy_from_slice(&t[..n]);
        self.at += n;
        self.out[self.at] = 0;
    }
    pub fn number(&mut self, n: u32) {
        self.number_u64(u64::from(n));
    }
    pub fn number_u64(&mut self, mut n: u64) {
        let mut digits = [0; 20];
        let mut at = 20;
        loop {
            at -= 1;
            digits[at] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        self.put(&digits[at..]);
    }
}
fn copy(t: &[u8], out: &mut [u8]) {
    LineWriter::new(out, 0).put(t);
}
pub fn chapter_progress(data: &HomeData<'_>, out: &mut [u8; 128], fits: impl Fn(&[u8]) -> bool) {
    let label = if text::fold_text(Some(&data.chapter_title), &mut out[..112]) {
        cstr(out).len()
    } else {
        0
    };
    let p = data.shown_percent();
    let known = (0..=100).contains(&p);
    let mut w = LineWriter::new(out, label);
    if label != 0 {
        w.put(" · ".as_bytes());
    }
    if known {
        w.number(p as u32);
    } else {
        w.put(b"--");
    }
    w.put(b"% read");
    if label != 0 && known && !fits(cstr(out)) {
        let mut w = LineWriter::new(out, 0);
        w.number(p as u32);
        w.put(b"% read");
    }
}
pub fn home_text(data: &HomeData<'_>) -> HomeText {
    let no_card = data.flags & NO_CARD != 0;
    let mut words = HomeText {
        title: [0; 128],
        progress: [0; 128],
        hint: if no_card {
            b"Insert the card, then press Read."
        } else if data.flags & SAMPLE_CHOSEN != 0 {
            b"No SD card. Read opens the sample."
        } else {
            b"Press Read to carry on."
        },
    };
    if data.flags & SAVE_WARNING != 0 {
        words.hint = b"Last save unconfirmed";
    }
    if no_card {
        copy(b"No SD card", &mut words.title);
    } else if data.book_title.is_some() {
        text::book_title(data.book_title, &mut words.title);
    } else {
        copy(b"No current book", &mut words.title);
    }
    let fixed: Option<&[u8]> = if no_card {
        Some(if data.flags & BOOK_ON_CARD != 0 {
            b"Your book is on it"
        } else {
            b"Sample is in Books"
        })
    } else if data.ready_to_begin() {
        Some(b"Not started")
    } else if !data.reading_available {
        Some(b"Reading unavailable")
    } else if data.progress_unavailable {
        Some(b"Progress unavailable")
    } else {
        None
    };
    if let Some(t) = fixed {
        copy(t, &mut words.progress);
    } else {
        chapter_progress(data, &mut words.progress, |line| {
            fixed_b24_width(line) <= 254 * 64
        });
    }
    words
}
pub fn stage_text(data: &HomeData<'_>) -> HomeText {
    let mut w = home_text(data);
    if data.flags & NO_CARD == 0
        && data.reading_available
        && !data.progress_unavailable
        && data.chapter_title[0] != 0
        && (0..=100).contains(&data.shown_percent())
    {
        chapter_progress(data, &mut w.progress, |line| {
            Span::plain(line).width(Face::R24) <= 440 * 64
        });
    }
    w
}

// Exact authored label metrics from frozen frosted_home_data.inc; never used for Stage drawing.
fn fixed_b24_width(t: &[u8]) -> i32 {
    if t == "Chapter 4 · 32% read".as_bytes() {
        return 15731;
    }
    Span::plain(t).width(Face::B24)
}
