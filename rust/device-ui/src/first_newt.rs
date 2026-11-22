// SPDX-License-Identifier: MIT
//! The first Newt: the character from the owner's first working X3, kept as
//! an Easter egg. A port of the awake `newtMascot` and its `Canvas::line`
//! and filled `Canvas::circle` from render.cpp at 44c454d
//! (assets/first-boot-newt), so the drawing matches the original pixel for pixel.
use newt_system_ui::{SystemBar, SystemButtons, SystemPage};
use newt_ui_canvas::{Frame, UiCanvas};

/// Body, toes and mouth, as (ax, ay, bx, by) in the original's units.
const STROKES: [[i32; 4]; 24] = [
    [8, 34, 15, 23], [15, 23, 32, 20], [32, 20, 46, 26], [46, 26, 51, 36],
    [51, 36, 62, 39], [62, 39, 74, 30], [74, 30, 74, 20], [74, 20, 67, 16],
    [8, 34, 10, 45], [10, 45, 25, 49], [25, 49, 46, 45], [46, 45, 62, 46],
    [62, 46, 79, 35], [79, 35, 82, 23], [82, 23, 76, 12], [76, 12, 67, 10],
    [18, 48, 16, 57], [16, 57, 9, 60], [16, 57, 20, 61],
    [39, 48, 44, 57], [44, 57, 38, 61], [44, 57, 51, 60],
    [24, 41, 31, 43], [31, 43, 36, 40],
];
/// The two eyes' centres; each is a filled circle of radius 3.
const EYES: [[i32; 2]; 2] = [[22, 30], [38, 31]];

/// The original page as a system page: its title, drawing and caption. Its
/// centred "Back to About" hint is the rail's Back button here.
pub(crate) fn first_newt(frame: Option<&mut Frame>, bar: &SystemBar) {
    let back = SystemButtons { back: Some(b"Back"), ..Default::default() };
    let mut page = SystemPage::new(frame, bar, back, Some(b"The first Newt"));
    draw_first_newt(page.canvas());
    page.canvas().center(565, Some(b"A little device. A little friend."), 21, false);
}

/// The original page's call: newtMascot(c, 124, 310, 3).
fn draw_first_newt(c: &mut UiCanvas<'_>) {
    let (x, y, scale) = (124, 310, 3);
    for [ax, ay, bx, by] in STROKES {
        line(c, x + ax * scale, y + ay * scale, x + bx * scale, y + by * scale, 2 * scale);
    }
    for [ex, ey] in EYES {
        disc(c, x + ex * scale, y + ey * scale, 3 * scale);
    }
}

// Canvas::line: Bresenham, stamping a weight-square at every step.
fn line(c: &mut UiCanvas<'_>, mut x0: i32, mut y0: i32, x1: i32, y1: i32, weight: i32) {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut error = dx + dy;
    loop {
        c.rect(x0, y0, weight, weight, true);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let twice = 2 * error;
        if twice >= dy {
            error += dy;
            x0 += sx;
        }
        if twice <= dx {
            error += dx;
            y0 += sy;
        }
    }
}

// Canvas::circle with filled = true.
fn disc(c: &mut UiCanvas<'_>, x: i32, y: i32, radius: i32) {
    for yy in -radius..=radius {
        for xx in -radius..=radius {
            if xx * xx + yy * yy <= radius * radius {
                c.pixel(x + xx, y + yy, true);
            }
        }
    }
}
