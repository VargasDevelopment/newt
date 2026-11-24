//! Shared non-reader shell, system bar, buttons and sleep frost. MIT; Inter OFL-1.1.
#![no_std]
#![forbid(unsafe_code)]
mod data;
mod shell;
mod sleep_frost;
pub use sleep_frost::{SLEEP_FROST_SCRATCH_BYTES, SleepFrostError, frost_sleep_backdrop_in_place};
use core::ffi::CStr;
pub use newt_core::{
    app::{Action, AppState, Screen},
    clock_format::{CLOCK_TEXT_BYTES, ClockFormat, format_clock_text},
    frame::{FRAME_BYTES, Frame, HEIGHT, WIDTH},
    preferences::Geometry,
};
pub use newt_ui_canvas::{HomePlane, UiCanvas};
pub use shell::*;

#[derive(Clone, Copy)]
pub struct SystemBar {
    pub clock: [u8; 6],
    pub format: ClockFormat,
    pub battery_known: bool,
    pub battery_percent: u16,
    pub radio_on: bool,
}
impl Default for SystemBar {
    fn default() -> Self {
        Self::new(None, ClockFormat::TwentyFourHour, false, 0)
    }
}
impl SystemBar {
    pub fn new(
        clock: Option<&CStr>,
        format: ClockFormat,
        battery_known: bool,
        battery_percent: u16,
    ) -> Self {
        let mut snapshot = *b"--:--\0";
        if let Some(clock) = clock {
            snapshot.fill(0);
            let n = clock.to_bytes().len().min(5);
            snapshot[..n].copy_from_slice(&clock.to_bytes()[..n]);
        }
        Self {
            clock: snapshot,
            format,
            battery_known: battery_known && battery_percent <= 100,
            battery_percent,
            radio_on: false,
        }
    }
}
#[derive(Clone, Copy, Default)]
pub struct SystemButtons<'a> {
    pub back: Option<&'a [u8]>,
    pub confirm: Option<&'a [u8]>,
    pub previous: Option<&'a [u8]>,
    pub next: Option<&'a [u8]>,
}
pub fn clamped_list_buttons<'a>(
    back: Option<&'a [u8]>,
    confirm: Option<&'a [u8]>,
    position: u32,
    count: u32,
) -> SystemButtons<'a> {
    SystemButtons {
        back,
        confirm,
        previous: (position > 0).then_some(b"Up"),
        next: (position.wrapping_add(1) < count).then_some(b"Down"),
    }
}
pub const SYSTEM_ROW_PITCH: i32 = 43;
// The card sits under the 64 px system bar and above the footer, inset from
// the frame's sides, so system pages follow the device's screen size.
const SYSTEM_BAR_HEIGHT: i32 = 64;
const CARD_SIDE_INSET: i32 = 28;
/// The 44 px footer plus a 30 px gap above it.
const CARD_BOTTOM_INSET: i32 = 74;
const CONTENT_INSET: i32 = 22;
pub const SYSTEM_CARD: Card = Card {
    x: CARD_SIDE_INSET,
    y: SYSTEM_BAR_HEIGHT,
    width: WIDTH as i32 - 2 * CARD_SIDE_INSET,
    height: HEIGHT as i32 - SYSTEM_BAR_HEIGHT - CARD_BOTTOM_INSET,
};
pub const SYSTEM_CONTENT_LEFT: i32 = SYSTEM_CARD.x + CONTENT_INSET;
pub const SYSTEM_CONTENT_WIDTH: i32 = SYSTEM_CARD.width - 2 * CONTENT_INSET;
pub const SYSTEM_TITLE_TOP: i32 = SYSTEM_CARD.y + 20;
/// Where a page's rows or text start under its one-line title.
pub const SYSTEM_BODY_TOP: i32 = SYSTEM_TITLE_TOP + 62;
// Pinned to the X3 values every reviewed frame was drawn with.
const _: () = assert!(
    SYSTEM_CARD.x == 28
        && SYSTEM_CARD.y == 64
        && SYSTEM_CARD.width == 472
        && SYSTEM_CARD.height == 654
        && SYSTEM_CONTENT_LEFT == 50
        && SYSTEM_CONTENT_WIDTH == 428
        && SYSTEM_TITLE_TOP == 84
        && SYSTEM_BODY_TOP == 146
);
pub struct SystemPage<'a> {
    canvas: UiCanvas<'a>,
}
impl<'a> SystemPage<'a> {
    pub fn new(
        mut frame: Option<&'a mut Frame>,
        bar: &SystemBar,
        buttons: SystemButtons<'_>,
        title: Option<&[u8]>,
    ) -> Self {
        draw_frosted_system_base(bar, HomePlane::Monochrome, frame.as_deref_mut());
        draw_system_footer(frame.as_deref_mut(), buttons, HomePlane::Monochrome);
        let mut canvas = UiCanvas::portrait(frame);
        let c = SYSTEM_CARD;
        canvas.panel(c.x, c.y, c.width, c.height);
        canvas.clip(c.x, c.y, c.width, c.height);
        canvas.text(
            SYSTEM_CONTENT_LEFT,
            SYSTEM_TITLE_TOP,
            title,
            32,
            true,
            SYSTEM_CONTENT_WIDTH,
            true,
        );
        Self { canvas }
    }
    pub fn canvas(&mut self) -> &mut UiCanvas<'a> {
        &mut self.canvas
    }
    pub fn row(&mut self, top: i32, index: u32, label: Option<&[u8]>, focused: bool) {
        draw_system_row(
            &mut self.canvas,
            SYSTEM_CONTENT_LEFT,
            top,
            index,
            SYSTEM_CONTENT_WIDTH,
            label,
            focused,
        );
    }
}
pub fn draw_system_row(
    c: &mut UiCanvas<'_>,
    left: i32,
    top: i32,
    index: u32,
    width: i32,
    label: Option<&[u8]>,
    focused: bool,
) {
    c.focus(
        left,
        top + index as i32 * SYSTEM_ROW_PITCH,
        width,
        label,
        focused,
    );
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}
/// Constructor enforces the original template's 1..=255 row invariant.
pub struct ListOverlay<'a> {
    pub title: Option<&'a [u8]>,
    labels: &'a [Option<&'a [u8]>],
}
impl<'a> ListOverlay<'a> {
    pub fn new(title: Option<&'a [u8]>, labels: &'a [Option<&'a [u8]>]) -> Option<Self> {
        (1..=255)
            .contains(&labels.len())
            .then_some(Self { title, labels })
    }
    pub fn count(&self) -> u8 {
        self.labels.len() as u8
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListOverlayResult {
    Unhandled,
    Clamped,
    Moved,
    Picked,
    Cancelled,
}
pub fn dispatch_list_overlay(
    list: &ListOverlay<'_>,
    focus: &mut u8,
    action: Action,
) -> ListOverlayResult {
    let last = list.count() - 1;
    *focus = (*focus).min(last);
    match action {
        Action::Previous if *focus == 0 => ListOverlayResult::Clamped,
        Action::Previous => {
            *focus -= 1;
            ListOverlayResult::Moved
        }
        Action::Next if *focus == last => ListOverlayResult::Clamped,
        Action::Next => {
            *focus += 1;
            ListOverlayResult::Moved
        }
        Action::Activate => ListOverlayResult::Picked,
        Action::Back => ListOverlayResult::Cancelled,
        Action::Sleep | Action::Read => ListOverlayResult::Unhandled,
    }
}
pub fn list_overlay_buttons(list: &ListOverlay<'_>, focus: u8) -> SystemButtons<'static> {
    clamped_list_buttons(
        Some(b"Cancel"),
        Some(b"Save"),
        focus.into(),
        list.count().into(),
    )
}
pub fn list_overlay_card(list: &ListOverlay<'_>) -> Card {
    let height = 76 + i32::from(list.count().min(9)) * SYSTEM_ROW_PITCH + 28;
    Card {
        x: 40,
        y: (HEIGHT as i32 - height) / 2,
        width: 448,
        height,
    }
}
pub fn render_list_overlay(
    list: &ListOverlay<'_>,
    focus: u8,
    backdrop: Option<&Frame>,
    frame: Option<&mut Frame>,
) {
    let Some(frame) = frame else { return };
    frost_sleep_backdrop(
        backdrop,
        Some(frame),
        Geometry {
            width: newt_core::frame::WIDTH as u16,
            height: newt_core::frame::HEIGHT as u16,
            margin: 0,
            content_bottom: 0,
            bar_y: 0,
            footer_y: 0,
        },
        HomePlane::Monochrome,
    );
    let focus = focus.min(list.count() - 1);
    let shown = list.count().min(9);
    let first = focus.saturating_sub(shown / 2).min(list.count() - shown);
    let card = list_overlay_card(list);
    let mut c = UiCanvas::portrait(Some(frame));
    c.panel(card.x, card.y, card.width, card.height);
    c.text(62, card.y + 24, list.title, 24, true, 404, true);
    for i in 0..shown {
        draw_system_row(
            &mut c,
            62,
            card.y + 76,
            i.into(),
            382,
            list.labels[(first + i) as usize],
            first + i == focus,
        );
    }
    if list.count() > shown {
        let track = i32::from(shown) * SYSTEM_ROW_PITCH - 8;
        c.rect(470, card.y + 80, 1, track, true);
        c.rect(
            468,
            card.y + 80 + track * i32::from(first) / i32::from(list.count()),
            5,
            track * i32::from(shown) / i32::from(list.count()),
            true,
        );
    }
}
