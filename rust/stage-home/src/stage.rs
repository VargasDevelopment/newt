use crate::{
    canvas::{Anchor::*, Paper::*, StageCanvas},
    span::{Span, wrap},
    *,
};
use newt_core::app::{HomeSection, home_section, home_section_title};
use newt_ui_canvas::atlas::Face::{self, *};
fn text(
    c: &mut StageCanvas<'_>,
    x: i32,
    y: i32,
    t: &[u8],
    face: Face,
    w: i32,
    a: crate::canvas::Anchor,
) {
    c.text(x, y, Span::plain(t), face, w, a, 0, Backdrop);
}
fn white_text(
    c: &mut StageCanvas<'_>,
    x: i32,
    y: i32,
    t: &[u8],
    face: Face,
    w: i32,
    a: crate::canvas::Anchor,
    ink: u32,
) {
    c.text(x, y, Span::plain(t), face, w, a, ink, White);
}
fn ceil(t: &[u8], f: Face) -> i32 {
    (Span::plain(t).width(f) + 63) / 64
}
fn header(c: &mut StageCanvas<'_>, info: &HeaderInfo, d: &HomeData<'_>) {
    let mut clock = [0; newt_core::clock_format::CLOCK_TEXT_BYTES];
    text(c, 264, 31, d.clock_text(&mut clock), R24, 112, Middle);
    let mut value = [0; 32];
    let mut w = crate::words::LineWriter::new(&mut value, 0);
    w.put("Offline · ".as_bytes());
    if info.battery_known && info.battery_percent <= 100 {
        w.number(u32::from(info.battery_percent));
    } else {
        w.put(b"--");
    }
    w.put(b"%");
    text(c, 500, 31, &value, R21, 180, End);
}
fn mat(c: &mut StageCanvas<'_>, x: i32, y: i32, w: i32, h: i32) {
    c.rrect(x - 6, y - 6, w + 12, h + 12, 0, 3);
    c.frame(x - 6, y - 6, w + 12, h + 12, 0, 1, 1);
}
fn empty_slot(c: &mut StageCanvas<'_>, label: Option<&[u8]>) {
    c.rrect(174, 56, 180, 264, 0, 3);
    c.frame(180, 62, 168, 252, 0, 1, 1);
    if let Some(label) = label {
        white_text(c, 264, 196, label, R21, 168, Middle, 1);
    }
}
fn cover(c: &mut StageCanvas<'_>, cover: &PreparedHomeCover<'_>) {
    mat(c, 192, 94, 144, 188);
    for y in 0..188 {
        for x in 0..144 {
            let bits = cover.pixels[y * 18 + x / 8];
            c.pixel(
                192 + x as i32,
                94 + y as i32,
                if bits & (0x80 >> (x % 8)) != 0 { 0 } else { 3 },
            );
        }
    }
}
fn missing_cover(c: &mut StageCanvas<'_>, title: &[u8]) {
    mat(c, 180, 62, 168, 252);
    c.frame(180, 62, 168, 252, 0, 1, 0);
    let (lines, n) = wrap::<4>(title, B24, 144);
    for (i, line) in lines[..n].iter().enumerate() {
        c.text(192, 96 + i as i32 * 26, *line, B24, 144, Start, 0, White);
    }
    white_text(c, 192, 300, b"No cover", R21, 144, Start, 1);
}
fn book(c: &mut StageCanvas<'_>, d: &HomeData<'_>, w: &HomeText, no_card: bool) {
    let (lines, n) = wrap::<2>(&w.title, B32, 440);
    let mut y = 364;
    for line in &lines[..n] {
        c.text(264, y, *line, B32, 440, Middle, 0, Backdrop);
        y += 38;
    }
    text(c, 264, y + 4, &w.progress, R24, 440, Middle);
    let percent = d.shown_percent();
    if !no_card && d.reading_available && !d.progress_unavailable && (0..=100).contains(&percent) {
        c.frame(154, y + 18, 220, 6, 2, 1, 0);
        c.rrect(154, y + 18, (220 * percent + 50) / 100, 6, 2, 0);
    }
    if d.flags & (CARD_FLAGS | SAVE_WARNING) != 0 {
        text(c, 264, y + 68, w.hint, R24, 440, Middle);
        return;
    }
    let tail: &[u8] = if d.ready_to_begin() {
        b"to begin"
    } else {
        b"to carry on"
    };
    let key = ceil(b"Read", R21) + 20;
    let left = 264 - (key + 10 + ceil(tail, R24)) / 2;
    c.frame(left, y + 46, key, 30, 6, 2, 0);
    text(c, left + key / 2, y + 68, b"Read", R21, 1056, Middle);
    text(c, left + key + 10, y + 68, tail, R24, 1056, Start);
}
#[inline(never)]
fn stage_book(
    c: &mut StageCanvas<'_>,
    d: &HomeData<'_>,
    prepared: Option<&PreparedHomeCover<'_>>,
    no_card: bool,
) {
    let w = stage_text(d);
    if no_card {
        empty_slot(c, None);
    } else if let Some(p) = prepared.filter(|p| p.ready && d.flags & CARD_FLAGS == 0) {
        cover(c, p);
    } else {
        missing_cover(c, &w.title);
    }
    book(c, d, &w, no_card);
}
fn icon(c: &mut StageCanvas<'_>, section: HomeSection, cx: i32, cy: i32) {
    match section {
        HomeSection::Books => {
            for side in [-1, 1] {
                c.triangle(
                    cx + side * 24,
                    cy - 14,
                    cx + side * 3,
                    cy - 10,
                    cx + side * 3,
                    cy + 16,
                    0,
                );
                c.triangle(
                    cx + side * 24,
                    cy - 14,
                    cx + side * 3,
                    cy + 16,
                    cx + side * 24,
                    cy + 12,
                    0,
                );
            }
        }
        HomeSection::Apps => {
            for dy in [-19, 3] {
                for dx in [-19, 3] {
                    c.rrect(cx + dx, cy + dy, 16, 16, 4, 0);
                }
            }
        }
        HomeSection::Settings => {
            c.frame(cx - 15, cy - 21, 30, 42, 4, 3, 0);
            c.frame(cx - 9, cy - 14, 18, 20, 0, 2, 0);
            c.rrect(cx - 6, cy + 11, 12, 3, 0, 0);
        }
    }
}
fn dock(c: &mut StageCanvas<'_>, focused: HomeSection, focus: HomeFocusStyle) {
    let quiet = focus == HomeFocusStyle::BinaryCursor;
    for i in 0..3 {
        let x = 36 + i * 164;
        let section = home_section(i as u32);
        let on = section == focused;
        if on && !quiet {
            c.halo(x, 566, 128, 132);
        }
        c.rrect(x, 566, 128, 132, 10, 3);
        c.frame(x, 566, 128, 132, 10, if on && !quiet { 4 } else { 2 }, 0);
        if quiet && on {
            c.rrect(x + 10, 568, 108, 8, 0, 0);
        }
        icon(c, section, x + 64, 620);
        white_text(
            c,
            x + 64,
            676,
            home_section_title(section).as_bytes(),
            if on && !quiet { B24 } else { R24 },
            116,
            Middle,
            0,
        );
    }
}
fn rail(c: &mut StageCanvas<'_>, read: bool) {
    for (i, cx) in [95, 201, 331, 437].into_iter().enumerate() {
        if i == 0 && !read {
            continue;
        }
        c.rrect(cx - 47, 749, 94, 42, 7, 3);
        c.frame(cx - 47, 749, 94, 42, 7, 2, 0);
        if i == 2 {
            c.triangle(cx + 8, 760, cx + 8, 780, cx - 9, 770, 0);
        } else if i == 3 {
            c.triangle(cx - 8, 760, cx - 8, 780, cx + 9, 770, 0);
        } else {
            let label: &[u8] = if i == 0 { b"Read" } else { b"Select" };
            let face = if ceil(label, R28) <= 86 {
                R28
            } else if ceil(label, R24) <= 86 {
                R24
            } else {
                R21
            };
            white_text(c, cx, 779, label, face, 90, Middle, 0);
        }
    }
}
pub const STAGE_FOCUS_X: i32 = 46;
pub const STAGE_FOCUS_Y: i32 = 568;
pub const STAGE_FOCUS_STEP: i32 = 164;
pub const STAGE_FOCUS_WIDTH: i32 = 108;
pub const STAGE_FOCUS_HEIGHT: i32 = 8;
/// None mirrors the null frame no-op. Full-sized mutable borrow excludes short/aliased frames.
pub fn render_stage_home_focus(selected: u32, plane: HomePlane, frame: Option<&mut Frame>) -> bool {
    let Some(frame) = frame.filter(|_| selected < 3) else {
        return false;
    };
    for i in 0..3 {
        let set = if plane == HomePlane::Monochrome {
            i == selected
        } else {
            i != selected
        };
        let left = 46 + i as usize * 164;
        for y in 568..576 {
            for x in left..left + 108 {
                let at = y * newt_core::frame::WIDTH + x;
                let mask = 0x80 >> (at % 8);
                if set {
                    frame[at / 8] |= mask;
                } else {
                    frame[at / 8] &= !mask;
                }
            }
        }
    }
    true
}
pub fn render_stage_home_focused(
    state: &AppState,
    info: &HeaderInfo,
    data: &HomeData<'_>,
    plane: HomePlane,
    frame: Option<&mut Frame>,
    backdrop: &StageBackdrop,
    cover: Option<&PreparedHomeCover<'_>>,
    focus: HomeFocusStyle,
) {
    let Some(frame) = frame else {
        return;
    };
    let mut c = StageCanvas::new(plane, frame, backdrop);
    header(&mut c, info, data);
    let no_card = data.flags & NO_CARD != 0;
    let no_book = !no_card && data.book_title.is_none();
    if no_book {
        empty_slot(&mut c, Some(b"No book"));
        text(&mut c, 264, 380, b"Nothing to resume", B32, 440, Middle);
        text(
            &mut c,
            264,
            420,
            b"Choose a book in Books.",
            R24,
            440,
            Middle,
        );
    } else {
        stage_book(&mut c, data, cover, no_card);
    }
    dock(&mut c, home_section(state.selected), focus);
    rail(&mut c, !no_book);
}
pub fn render_stage_home(
    state: &AppState,
    info: &HeaderInfo,
    data: &HomeData<'_>,
    plane: HomePlane,
    frame: Option<&mut Frame>,
    backdrop: &StageBackdrop,
    cover: Option<&PreparedHomeCover<'_>>,
) {
    render_stage_home_focused(
        state,
        info,
        data,
        plane,
        frame,
        backdrop,
        cover,
        HomeFocusStyle::Approved,
    );
}
pub fn render_home(
    state: &AppState,
    info: &HeaderInfo,
    data: &HomeData<'_>,
    plane: HomePlane,
    frame: Option<&mut Frame>,
    style: &HomeStyle<'_>,
    cover: Option<&PreparedHomeCover<'_>>,
) {
    render_stage_home_focused(
        state,
        info,
        data,
        plane,
        frame,
        style.backdrop.unwrap_or(APPROVED_BACKDROP),
        cover,
        style.focus,
    );
}
