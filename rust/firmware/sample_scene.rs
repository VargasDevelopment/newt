// SPDX-License-Identifier: MIT
//! Three-page Reading fallback scene painter.
//! renderSampleScene delegates this screen to render; the EPUB fixture engine
//! is a separate path. AppState owns the page; this painter owns no live state.
#![forbid(unsafe_code)]
mod font;
use newt_core::{app::{AppState, Screen}, frame::{self, Frame}};
use newt_ui_canvas::UiCanvas;

const MARGIN: i32 = 40;
const WIDTH: i32 = frame::WIDTH as i32;
const CONTENT_WIDTH: i32 = WIDTH - 2 * MARGIN;
struct ReadingPage {
    heading: &'static [u8],
    first: &'static [u8],
    second: &'static [u8],
}
const PAGES: [ReadingPage; 3] = [
    ReadingPage {
        heading: b"The lantern keeper",
        first: concat!("Every evening, Ada carried a lantern down to the pond. It was a small light, ",
            "but the path was a small path, and that was enough.").as_bytes(),
        second: concat!("One night she found a newt waiting on the lowest stone. It looked at the ",
            "lantern, then at the water, as if it had a very important question.").as_bytes(),
    },
    ReadingPage {
        heading: b"A light to borrow",
        first: concat!("Ada set the lantern beside the reeds. In its warm circle, the pond became ",
            "a whole little world: a leaf, a ripple, a silver thread of rain.").as_bytes(),
        second: concat!("The newt climbed onto her boot. Together they watched the first star appear. ",
            "Neither of them had anywhere else to be.").as_bytes(),
    },
    ReadingPage {
        heading: b"Enough for tonight",
        first: concat!("When Ada stood up, the newt slipped back into the pond. She left the ",
            "lantern on the stone for one more minute.").as_bytes(),
        second: concat!("Then she took her small light home. The path was still a small path. ",
            "The world was still a very large world. And, for tonight, this was enough.").as_bytes(),
    },
];

/// The original three-page composition, including its bitmap font and bounds.
/// Navigation remains Core app::dispatch, using the existing AppState.page.
pub fn paint(state: &AppState, frame: Option<&mut Frame>) -> bool {
    if state.screen != Screen::Reading { return false; }
    let Some(frame) = frame else { return false; };
    frame.fill(0);
    let c = &mut UiCanvas::portrait(Some(frame));
    let page = state.page.min(PAGES.len() as u32 - 1) as usize;
    let content = &PAGES[page];
    font::text(c, MARGIN, 45, b"NEWT / A SHORT READ", 2);
    font::text(c, MARGIN, 98, content.heading, 3);
    c.rect(MARGIN, 144, CONTENT_WIDTH, 2, true);
    let y = paragraph(c, MARGIN, 183, content.first);
    paragraph(c, MARGIN, y + 28, content.second);
    if page == 2 { center(c, 657, b"The end.", 2); }
    let mut progress = [0; 24];
    progress[..5].copy_from_slice(&[b'1' + page as u8, b' ', b'/', b' ', b'3']);
    center(c, 691, &progress, 2);
    c.rect(MARGIN, 719, CONTENT_WIDTH, 2, true);
    font::text(c, MARGIN, 740, b"BACK HOME", 2);
    let right = b"< > PAGE";
    font::text(c, WIDTH - MARGIN - font::width(right, 2), 740, right, 2);
    true
}

fn center(c: &mut UiCanvas<'_>, y: i32, value: &[u8], scale: i32) {
    font::text(c, (WIDTH - font::width(value, scale)) / 2, y, value, scale);
}

/// Canvas::paragraph's original row/word/candidate workspaces are call-local.
fn paragraph(c: &mut UiCanvas<'_>, x: i32, mut y: i32, value: &[u8]) -> i32 {
    const SCALE: i32 = 3;
    const BOTTOM: i32 = 690;
    let mut row = [0; 128];
    let mut used = 0;
    let mut at = 0;
    while at < value.len() && value[at] != 0 && at < 2048 {
        if value[at] == b' ' { at += 1; continue; }
        let mut word = [0; 64];
        let mut length = 0;
        while at < value.len() && value[at] != 0 && value[at] != b' '
            && length < word.len() - 1 && at < 2048 {
            word[length] = value[at];
            length += 1;
            at += 1;
        }
        let mut candidate = [0; 128];
        let separator = if used != 0 { b" ".as_slice() } else { b"".as_slice() };
        let mut copied = 0;
        for part in [&row[..used], separator, &word[..length]] {
            let count = part.len().min(candidate.len() - 1 - copied);
            candidate[copied..copied + count].copy_from_slice(&part[..count]);
            copied += count;
        }
        if used != 0 && (font::width(&candidate, SCALE) > CONTENT_WIDTH
            || used + length + 1 >= row.len()) {
            if y + 7 * SCALE > BOTTOM { return y; }
            font::text(c, x, y, &row, SCALE);
            y += SCALE * 10;
            row[0] = 0;
            used = 0;
        }
        while length != 0 && font::width(&word, SCALE) > CONTENT_WIDTH {
            length -= 1;
            word[length] = 0;
        }
        if used != 0 { row[used] = b' '; used += 1; }
        for &byte in &word[..length] {
            if used == row.len() - 1 { break; }
            row[used] = byte;
            used += 1;
        }
        row[used] = 0;
    }
    if used != 0 && y + 7 * SCALE <= BOTTOM {
        font::text(c, x, y, &row, SCALE);
        y += SCALE * 10;
    }
    y
}
