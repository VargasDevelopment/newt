use crate::*;
use crate::{Buffer, assets::*, cstr, para, valid_geometry, width};
use core::fmt::Write;
use newt_system_ui::clamped_list_buttons;
use newt_ui_canvas::{book_title, fold_text};
pub fn sleep_inverted(e: &SleepExtras) -> bool {
    e.habit == ReadingHabit::Night
}
pub fn sleep_extras_shown(e: &SleepExtras) -> bool {
    e.progress || e.week || e.habit != ReadingHabit::None
}
fn mark(c: &mut UiCanvas<'_>, left: i32, top: i32, m: &Mark, selector: bool) {
    let mut at = 0;
    for &run in m.runs {
        for _ in 0..(run & 63) + 1 {
            let kind = run >> 6;
            if kind != 0 {
                c.pixel(
                    left + m.x + at % m.width,
                    top + m.y + at / m.width,
                    (kind == 2) != selector,
                );
            }
            at += 1;
        }
    }
    debug_assert_eq!(at, m.width * m.height);
}
pub fn draw_christmas_hat(c: &mut UiCanvas<'_>, left: i32, top: i32, selector: bool) {
    mark(c, left, top, &kChristmasHat, selector);
}
#[derive(Clone, Copy)]
struct HatRow {
    il: i32,
    ir: i32,
    bl: i32,
    br: i32,
}
const fn hat_rows() -> [HatRow; 74] {
    let mut rows = [HatRow {
        il: 255,
        ir: 0,
        bl: 255,
        br: 0,
    }; 74];
    let mut row = 0;
    while row < 74 {
        let mut col = 0;
        while col < 90 {
            let dx = (183 + col - 228) as i64;
            let dy = (-32 + row - 31) as i64;
            let u = 63 * dx + 11 * dy;
            let v = -11 * dx + 63 * dy;
            let t = -v;
            let brim = u * u * 49 + v * v * 1681 <= 4096 * 1681 * 49;
            let half = 21 * (59 * 64 - t) / 59 + 64;
            let bend = -38 * t * t * t / (59 * 59 * 59 * 64 * 64);
            let cone = t >= 0 && t <= 59 * 64 && u - bend <= half && bend - u <= half;
            let s = &mut rows[row as usize];
            if brim || cone {
                if col < s.il {
                    s.il = col;
                }
                s.ir = col;
            }
            if cone && !brim && t >= 15 * 64 && t <= 20 * 64 {
                if col < s.bl {
                    s.bl = col;
                }
                s.br = col;
            }
            col += 1;
        }
        row += 1;
    }
    rows
}
static HAT: [HatRow; 74] = hat_rows();
fn witch(c: &mut UiCanvas<'_>, left: i32, top: i32, selector: bool) {
    let left = left + 183;
    let top = top - 32;
    let reach = [2, 2, 2, 2, 2, 3, 2, 2, 0];
    for row in -3..79 {
        let mut from = 90;
        let mut to = -1;
        for dy in -5..=3 {
            let at = row + dy;
            if !(0..74).contains(&at) || (dy < -3 && at < 56) {
                continue;
            }
            let s = HAT[at as usize];
            if s.il > s.ir {
                continue;
            }
            from = from.min(s.il - reach[(dy + 5) as usize]);
            to = to.max(s.ir + reach[(dy + 5) as usize]);
        }
        for x in from..=to {
            c.pixel(left + x, top + row, selector);
        }
    }
    for (row, s) in HAT.iter().enumerate() {
        for x in s.il..=s.ir {
            c.pixel(
                left + x,
                top + row as i32,
                (x >= s.bl && x <= s.br) == selector,
            );
        }
    }
}
pub fn render_storage_warning(
    info: &DeviceInfo<'_>,
    frame: Option<&mut Frame>,
    g: Geometry,
    selector: bool,
) {
    let (Some(frame), Some(storage)) = (frame, info.storage()) else {
        return;
    };
    if !valid_geometry(g) {
        return;
    }
    let w = i32::from(g.width);
    let h = i32::from(g.height);
    let top = h - 150;
    let bottom = h - 55;
    {
        let mut c = UiCanvas::new(Some(frame), w, h);
        c.rect(24, top, w - 48, bottom - top, false);
        para(&mut c, 36, top + 8, storage, 21, w - 72, bottom - 4);
    }
    if selector {
        for y in top..bottom {
            for x in 3..(w - 24) / 8 {
                frame[(y * w / 8 + x) as usize] ^= 255;
            }
        }
    }
}
#[path = "sleep_brand.rs"]
mod brand;
pub use brand::draw_brand_stamp;
pub fn render_sleep_ui(
    info: &DeviceInfo<'_>,
    frame: Option<&mut Frame>,
    g: Geometry,
    selector: bool,
) {
    let Some(frame) = frame else { return };
    if !valid_geometry(g) {
        return;
    }
    let w = i32::from(g.width);
    let h = i32::from(g.height);
    {
        let mut c = UiCanvas::new(Some(frame), w, h);
        c.clip(16, 16, w - 32, h - 32);
        let left = (w - 304) / 2;
        let top = (h - 416) / 2;
        draw_brand_stamp(&mut c, left, top, selector);
        match info.hat {
            HolidayHat::None => (),
            HolidayHat::Witch => witch(&mut c, left, top, selector),
            HolidayHat::Christmas => draw_christmas_hat(&mut c, left, top, selector),
        }
        c.text(
            (w - width(b"Power to wake", 21, false)) / 2,
            h - 38,
            Some(b"Power to wake"),
            21,
            false,
            0,
            !selector,
        );
    }
    render_storage_warning(info, Some(frame), g, selector);
}
struct Ink<'a, 'b> {
    c: &'a mut UiCanvas<'b>,
    selector: bool,
}
impl Ink<'_, '_> {
    fn text(&mut self, x: i32, y: i32, s: &[u8], p: u32, b: bool) {
        self.c.text(x, y, Some(s), p, b, 0, !self.selector);
    }
    fn slip(&mut self, x: i32, y: i32, w: i32, h: i32) {
        for yy in 0..h {
            for xx in 0..w {
                let cx = if xx < 6 {
                    6 - xx
                } else if xx >= w - 6 {
                    xx - (w - 7)
                } else {
                    0
                };
                let cy = if yy < 6 {
                    6 - yy
                } else if yy >= h - 6 {
                    yy - (h - 7)
                } else {
                    0
                };
                if cx * cx + cy * cy <= 36 {
                    self.c.pixel(x + xx, y + yy, self.selector);
                }
            }
        }
    }
    fn dots(&mut self, days: u8, left: i32, top: i32) {
        for i in 0..7 {
            let cx = left + 8 + i * 26;
            let cy = top + 8;
            let ring = days >> (6 - i) & 1 == 0;
            for y in -9..=9 {
                for x in -9..=9 {
                    let d = (2 * x + 1) * (2 * x + 1) + (2 * y + 1) * (2 * y + 1);
                    if d <= 256 && (!ring || d > 134) {
                        self.c.pixel(cx + x, cy + y, !self.selector);
                    }
                }
            }
            if i == 6 {
                self.c.rect(cx - 7, cy + 14, 14, 3, !self.selector);
            }
        }
    }
}
fn title(home: &HomeData<'_>, out: &mut [u8; 96]) -> bool {
    if home.flags & newt_stage_home::NO_CARD != 0
        || home.book_title.is_none_or(|s| cstr(s).is_empty())
    {
        return false;
    }
    book_title(home.book_title, out);
    true
}
fn progress(home: &HomeData<'_>, w: i32, out: &mut [u8; 96]) {
    let percent = home.shown_percent();
    let known = (0..=100).contains(&percent);
    let label = if fold_text(Some(&home.chapter_title), &mut out[..87]) {
        cstr(out).len()
    } else {
        0
    };
    if known {
        let mut b = Buffer::new(&mut out[label..]);
        let _ = write!(b, "{}{}%", if label != 0 { " · " } else { "" }, percent);
    }
    if width(out, 19, false) <= w {
        return;
    }
    if known {
        let _ = write!(Buffer::new(out), "{percent}%");
    } else {
        out[0] = 0;
    }
}
fn count(e: &SleepExtras, out: &mut [u8; 16]) -> bool {
    if !e.week || e.days != 127 || e.streak == 0 {
        return false;
    }
    let _ = write!(Buffer::new(out), "{} days", e.streak);
    true
}
// In-place tail capacity matters for landscape's second row.
fn cut_fits(s: &mut [u8], n: usize, p: u32, b: bool, w: i32) -> bool {
    if n + 4 > s.len() {
        return false;
    }
    let saved: [u8; 4] = s[n..n + 4].try_into().unwrap();
    s[n..n + 4].copy_from_slice("…\0".as_bytes());
    let fits = width(s, p, b) <= w;
    s[n..n + 4].copy_from_slice(&saved);
    fits
}
fn cut(s: &mut [u8], p: u32, b: bool, w: i32) {
    if width(s, p, b) <= w {
        return;
    }
    let end = cstr(s).len();
    let mut n = end;
    while n > 0 {
        n -= 1;
        if s[n] == b' ' && !b" ,;:-".contains(&s[n - 1]) && cut_fits(s, n, p, b, w) {
            s[n..n + 4].copy_from_slice("…\0".as_bytes());
            return;
        }
    }
    n = end;
    while n > 0 && (s[n] & 0xc0 == 0x80 || !cut_fits(s, n, p, b, w)) {
        n -= 1;
    }
    if n + 4 <= s.len() {
        s[n..n + 4].copy_from_slice("…\0".as_bytes());
    }
}
fn portrait(ink: &mut Ink<'_, '_>, e: &SleepExtras, home: &HomeData<'_>, w: i32, top: i32) {
    let mut t = [0; 96];
    let mut r = [0; 96];
    let mut n = [0; 16];
    let line = e.progress && title(home, &mut t);
    if !line && !e.week {
        return;
    }
    let mut bw = 0;
    let mut h = 0;
    let mut rh = 0;
    if line {
        cut(&mut t, 21, true, w - 136);
        progress(home, w - 136, &mut r);
        rh = if r[0] != 0 { 26 } else { 0 };
        bw = width(&t, 21, true).max(width(&r, 19, false));
        h = 26 + rh;
    }
    let counted = count(e, &mut n);
    if e.week {
        bw = bw.max(172);
        h += if line { 12 } else { 0 };
        h += 26 + if counted { 22 } else { 0 };
    }
    let sw = bw + 40;
    ink.slip((w - sw) / 2, top, sw, h + 20);
    let mut y = top + 9;
    if line {
        ink.text((w - width(&t, 21, true)) / 2, y, &t, 21, true);
        if r[0] != 0 {
            ink.text((w - width(&r, 19, false)) / 2, y + 27, &r, 19, false);
        }
        y += 26 + rh + 14;
    } else {
        y += 2;
    }
    if e.week {
        ink.dots(e.days, (w - 172) / 2, y);
        if counted {
            ink.text((w - width(&n, 17, false)) / 2, y + 30, &n, 17, false);
        }
    }
}
fn landscape_title(ink: &mut Ink<'_, '_>, home: &HomeData<'_>, column: i32, bottom: i32) {
    let inner = column - 24;
    let mut t = [0; 96];
    let mut r = [0; 96];
    if !title(home, &mut t) {
        return;
    }
    let mut tail = None;
    if width(&t, 21, true) > inner {
        let end = cstr(&t).len();
        for at in (1..end).rev() {
            if t[at] != b' ' {
                continue;
            }
            t[at] = 0;
            if width(&t, 21, true) <= inner {
                tail = Some(at + 1);
                break;
            }
        }
        if let Some(at) = tail {
            for b in &mut t[at..end] {
                if *b == 0 {
                    *b = b' ';
                }
            }
            cut(&mut t[at..], 21, true, inner);
        } else {
            cut(&mut t, 21, true, inner);
        }
    }
    let second = tail.map(|at| cstr(&t[at..])).unwrap_or(b"");
    progress(home, inner, &mut r);
    let rows = if second.is_empty() { 1 } else { 2 };
    let w = width(&t, 21, true)
        .max(width(second, 21, true))
        .max(width(&r, 19, false))
        + 32;
    let h = rows * 28 + if r[0] != 0 { 26 } else { 0 } + 18;
    let x = 20 + (column - w) / 2;
    let y = bottom - h;
    ink.slip(x, y, w, h);
    let cx = x + w / 2;
    ink.text(cx - width(&t, 21, true) / 2, y + 9, &t, 21, true);
    if rows == 2 {
        ink.text(cx - width(second, 21, true) / 2, y + 37, second, 21, true);
    }
    if r[0] != 0 {
        ink.text(
            cx - width(&r, 19, false) / 2,
            y + 9 + rows * 28,
            &r,
            19,
            false,
        );
    }
}
#[inline]
pub fn render_sleep_extras(
    e: &SleepExtras,
    home: &HomeData<'_>,
    frame: Option<&mut Frame>,
    g: Geometry,
    selector: bool,
) {
    let Some(frame) = frame else { return };
    if !valid_geometry(g) || !sleep_extras_shown(e) {
        return;
    }
    paint_sleep_extras(e, home, frame, g, selector);
}
fn paint_sleep_extras(
    e: &SleepExtras,
    home: &HomeData<'_>,
    frame: &mut Frame,
    g: Geometry,
    selector: bool,
) {
    let w = i32::from(g.width);
    let h = i32::from(g.height);
    let left = (w - 304) / 2;
    let top = (h - 416) / 2;
    let mut c = UiCanvas::new(Some(frame), w, h);
    c.clip(16, 16, w - 32, h - 32);
    let m = match e.habit {
        ReadingHabit::None => None,
        ReadingHabit::Morning => Some(&kMug),
        ReadingHabit::Afternoon => Some(&kSun),
        ReadingHabit::Evening => Some(&kSunset),
        ReadingHabit::Night => Some(&kMoon),
    };
    if let Some(m) = m {
        mark(&mut c, left, top, m, selector);
    }
    let mut ink = Ink {
        c: &mut c,
        selector,
    };
    if w <= h {
        portrait(&mut ink, e, home, w, top + 420);
        return;
    }
    let column = left - 28;
    if e.progress {
        landscape_title(&mut ink, home, column, top + 404);
    }
    if e.week {
        let mut n = [0; 16];
        let counted = count(e, &mut n);
        let sw = 204;
        let sh = 48;
        let x = left + 312 + (column - sw) / 2;
        let y = top + 404 - sh;
        ink.slip(x, y, sw, sh + if counted { 22 } else { 0 });
        ink.dots(e.days, x + 16, y + 11);
        if counted {
            ink.text(x + (sw - width(&n, 17, false)) / 2, y + 41, &n, 17, false);
        }
    }
}
pub fn render_sleep_screen_settings(
    frame: Option<&mut Frame>,
    bar: &SystemBar,
    focus: u32,
    choices: SleepScreenChoices,
    notice: SettingsNotice,
) {
    let pos = focus.min(2);
    let mut page = SystemPage::new(
        frame,
        bar,
        clamped_list_buttons(Some(b"Back"), Some(b"Switch"), pos, 3),
        Some(b"Sleep screen"),
    );
    let on = [choices.progress, choices.week, choices.habit];
    let names = ["Book progress", "Last 7 days", "When you read"];
    let mut buf = [0; 96];
    for i in 0..3 {
        let _ = write!(
            Buffer::new(&mut buf),
            "{}: {}",
            names[i],
            if on[i] { "On" } else { "Off" }
        );
        page.row(146, i as u32, Some(&buf), i as u32 == pos);
    }
    let notes = [
        "Shows your book's title and progress. Anyone who sees the device can read it.",
        "Dots for the last 7 days, filled on days you read. A full week also shows your streak.",
        "A mug, sun or moon for the time of day you read most, once you've read 30 minutes. Night readers get a dark screen.",
    ];
    let failed = [
        SettingsNotice::SleepProgressSaveFailed,
        SettingsNotice::SleepWeekSaveFailed,
        SettingsNotice::SleepHabitSaveFailed,
    ];
    let i = pos as usize;
    let note = if notice == failed[i] {
        let _ = write!(
            Buffer::new(&mut buf),
            "Could not confirm the save. {} stays {}; after a restart it may change.",
            names[i],
            if on[i] { "On" } else { "Off" }
        );
        &buf[..]
    } else {
        notes[i].as_bytes()
    };
    para(page.canvas(), 50, 301, note, 21, 428, 430);
}
