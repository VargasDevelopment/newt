// SPDX-License-Identifier: MIT
//! Quick Start screen rendering over Core's typed policy.
//! No AppState, dispatch or refresh authority.
use newt_core::{quick_start::{Facts, Notice, ORDER, State, Step}, time_zone::{TIME_ZONE_LABELS, time_zone_label}};
use newt_system_ui::{
    SystemBar, SystemPage, SYSTEM_CARD, SYSTEM_CONTENT_LEFT as LEFT,
    SYSTEM_CONTENT_WIDTH as WIDTH, SYSTEM_ROW_PITCH, SYSTEM_TITLE_TOP,
    ListOverlay, clamped_list_buttons, draw_frosted_system_header,
    draw_system_footer, list_overlay_buttons, render_list_overlay,
};
use newt_ui_canvas::{Frame, HomePlane, UiCanvas, paragraph_rows, text_width};

const SAVE_NOTE: &[u8] = b"Could not confirm the save. After a restart it may change.";
const BOTTOM: i32 = SYSTEM_CARD.y + SYSTEM_CARD.height - 18;
const BRAND_WIDTH: i32 = 304;
const BRAND_HEIGHT: i32 = 416;
const OPTION_HEIGHT: i32 = 116;
const OPTION_GAP: i32 = 14;

// Presentation choices only; all enter/dispatch/saved policy remains in Core.
#[derive(Clone, Copy)]
enum Row { Start, SkipAll, Upright, Sideways, Zone, SetUpWifi, ChangeWifi, Next, Skip, Home }
fn rows(step: Step, facts: &Facts) -> &'static [Row] {
    match step {
        Step::Welcome => &[Row::Start, Row::SkipAll],
        Step::Reading => &[Row::Upright, Row::Sideways, Row::Skip],
        Step::TimeZone => &[Row::Zone, Row::Skip],
        Step::Wifi if facts.wifi_saved => &[Row::Next, Row::ChangeWifi],
        Step::Wifi => &[Row::SetUpWifi, Row::Skip],
        Step::Books => &[Row::Next],
        Step::Done => &[Row::Home],
    }
}
fn paragraph(c: &mut UiCanvas<'_>, top: i32, text: &[u8]) -> i32 {
    c.paragraph(LEFT, top, Some(text), 21, WIDTH, BOTTOM);
    top + paragraph_rows(Some(text), 21, WIDTH).max(0) * 30
}
/// The pinned snprintf fields are local fixed scratch, never retained.
fn parts<'a>(out: &'a mut [u8], values: &[&[u8]]) -> &'a [u8] {
    out.fill(0);
    let mut used = 0;
    for value in values {
        for &byte in *value {
            if used + 1 >= out.len() { return out; }
            out[used] = byte;
            used += 1;
        }
    }
    out
}
fn label<'a>(row: Row, facts: &Facts, scratch: &'a mut [u8; 48]) -> &'a [u8] {
    match row {
        Row::Start => b"Start",
        Row::SkipAll => b"Skip quick start",
        Row::Upright => b"Upright",
        Row::Sideways => b"Sideways",
        Row::Zone => parts(scratch, &[b"Time zone: ", time_zone_label(facts.zone).as_bytes()]),
        Row::SetUpWifi => b"Set up Wi-Fi",
        Row::ChangeWifi => b"Change Wi-Fi",
        Row::Next => b"Next",
        Row::Skip => b"Skip",
        Row::Home => b"Go to Home",
    }
}
fn action(row: Row) -> &'static [u8] {
    match row {
        Row::Start => b"Start",
        Row::SkipAll | Row::Skip => b"Skip",
        Row::Upright | Row::Sideways | Row::Zone => b"Select",
        Row::SetUpWifi | Row::ChangeWifi => b"Open",
        Row::Next => b"Next",
        Row::Home => b"Done",
    }
}
fn counter(c: &mut UiCanvas<'_>, step: Step) {
    let index = ORDER.iter().position(|&candidate| candidate == step).unwrap_or(0);
    if index == 0 || index + 1 >= ORDER.len() { return; }
    let mut text = [0; 24];
    text[..11].copy_from_slice(b"Step 1 of 4");
    text[5] = b'0' + index as u8;
    c.text(LEFT + WIDTH - text_width(Some(&text), 17, false), SYSTEM_TITLE_TOP + 12,
        Some(&text), 17, false, 0, true);
}
fn page_preview(c: &mut UiCanvas<'_>, center_x: i32, center_y: i32, sideways: bool) {
    let (w, h) = if sideways { (92, 64) } else { (64, 92) };
    let (x, y) = (center_x - w / 2, center_y - h / 2);
    c.rect(x, y, w, 3, true);
    c.rect(x, y + h - 3, w, 3, true);
    c.rect(x, y, 3, h, true);
    c.rect(x + w - 3, y, 3, h, true);
    let mut top = y + 11;
    let mut line = 0;
    while top + 2 <= y + h - 10 {
        c.rect(x + 10, top, if line % 4 == 3 { (w - 20) * 3 / 5 } else { w - 20 }, 2, true);
        top += 7;
        line += 1;
    }
}
fn option(c: &mut UiCanvas<'_>, top: i32, sideways: bool, focused: bool) {
    let border = if focused { 4 } else { 1 };
    c.rect(LEFT, top, WIDTH, border, true);
    c.rect(LEFT, top + OPTION_HEIGHT - border, WIDTH, border, true);
    c.rect(LEFT, top, border, OPTION_HEIGHT, true);
    c.rect(LEFT + WIDTH - border, top, border, OPTION_HEIGHT, true);
    page_preview(c, LEFT + 66, top + OPTION_HEIGHT / 2, sideways);
    c.text(LEFT + 132, top + 22, Some(if sideways { b"Sideways" } else { b"Upright" }),
        24, true, WIDTH - 148, true);
    c.paragraph(LEFT + 132, top + 56,
        Some(if sideways { b"Wide pages, X3 on its side." } else { b"Tall pages, held like a book." }),
        21, WIDTH - 148, top + OPTION_HEIGHT - 4);
}
fn step_text(c: &mut UiCanvas<'_>, state: &State, facts: &Facts) -> i32 {
    match state.step {
        Step::Welcome => {
            // Reuse Sleep's existing asset and brand-only paint, not its scene.
            newt_device_ui::draw_brand_stamp(c, (newt_core::frame::WIDTH as i32 - BRAND_WIDTH) / 2, 84, false);
            paragraph(c, 84 + BRAND_HEIGHT + 12,
                if state.notice != Notice::None { SAVE_NOTE }
                else if state.focus == 1 { b"You can run it any time from Settings, Quick start." }
                else { b"Let's set up your X3 for reading. Every step can be skipped." }) + 16
        }
        Step::Reading => {
            let next = paragraph(c, 146, b"Only books turn; Home stays upright.") + 16;
            option(c, next, false, state.focus == 0);
            option(c, next + OPTION_HEIGHT + OPTION_GAP, true, state.focus == 1);
            next + 2 * (OPTION_HEIGHT + OPTION_GAP) + 4
        }
        Step::TimeZone => paragraph(c, 146,
            b"Newt shows the time in this zone.") + 20,
        Step::Wifi if !facts.wifi_saved => paragraph(c, 146,
            b"Newt sets the clock when it checks for updates. Reading never needs Wi-Fi.") + 20,
        Step::Wifi => paragraph(c, 146,
            if facts.clock_set { b"Wi-Fi is saved, and the clock is set." }
            else { b"Wi-Fi is saved. Newt sets the clock when it checks for updates." }) + 20,
        Step::Books => {
            let next = paragraph(c, 146,
                b"Copy EPUB books anywhere on the card from a computer.") + 14;
            paragraph(c, next, b"Open them from Books on Home.") + 20
        }
        Step::Done => {
            let mut next = paragraph(c, 146,
                b"Home is next. To run this quick start again, open Settings, Quick start.") + 14;
            if !facts.wifi_saved {
                next = paragraph(c, next, b"Newt sets the clock when it checks for updates.") + 14;
            }
            next + 6
        }
    }
}
fn note<'a>(state: &State, facts: &Facts, row: Row, scratch: &'a mut [u8; 128]) -> Option<&'a [u8]> {
    if state.notice != Notice::None {
        let text = if state.notice == Notice::ZoneSaveFailed {
            parts(scratch, &[b"Could not confirm the save. Newt keeps ",
                time_zone_label(facts.zone).as_bytes(), b"; after a restart it may change."])
        } else { parts(scratch, &[SAVE_NOTE]) };
        return (state.step != Step::Welcome).then_some(text);
    }
    match row {
        Row::Upright | Row::Sideways => Some(b"Change it later in a book's menu."),
        Row::SetUpWifi | Row::ChangeWifi => Some(b"Newt restarts to set up Wi-Fi, then comes back here."),
        Row::Skip if state.step == Step::Wifi => Some(b"Set it up later in Settings, Wi-Fi."),
        _ => None,
    }
}
fn title(step: Step) -> Option<&'static [u8]> {
    match step {
        Step::Welcome => None,
        Step::Reading => Some(b"How do you read?"),
        Step::TimeZone => Some(b"Your time zone"),
        Step::Wifi => Some(b"Wi-Fi"),
        Step::Books => Some(b"Your books"),
        Step::Done => Some(b"You're all set"),
    }
}
fn render_step(state: &State, facts: &Facts, bar: &SystemBar, frame: Option<&mut Frame>) {
    let rows = rows(state.step, facts);
    let focus = if usize::from(state.focus) < rows.len() { usize::from(state.focus) } else { 0 };
    let row = rows[focus];
    let backs = state.step != ORDER[0] || state.from_settings;
    let buttons = clamped_list_buttons(backs.then_some(b"Back"), Some(action(row)), focus as u32, rows.len() as u32);
    let mut page = SystemPage::new(frame, bar, buttons, title(state.step));
    counter(page.canvas(), state.step);
    let mut top = step_text(page.canvas(), state, facts);
    let mut label_scratch = [0; 48];
    let mut note_scratch = [0; 128];
    let first = if state.step == Step::Reading { 2 } else { 0 };
    for (index, &row) in rows.iter().enumerate().skip(first) {
        page.row(top, (index - first) as u32, Some(label(row, facts, &mut label_scratch)), index == focus);
    }
    top += (rows.len() - first) as i32 * SYSTEM_ROW_PITCH + 20;
    if let Some(text) = note(state, facts, row, &mut note_scratch) {
        page.canvas().paragraph(LEFT, top, Some(text), 21, WIDTH, BOTTOM);
    }
}
/// The existing logical frame and distinct native scratch are short loans.
/// With a list, repaint the ENTIRE scratch behind it before the frost reads it.
/// No frame reference escapes, no refresh/capture occurs and no buffer is owned.
pub fn render(state: &State, facts: &Facts, bar: &SystemBar,
              mut frame: Option<&mut Frame>, scratch: Option<&mut Frame>) {
    if state.zone_list_open {
        if let Some(scratch) = scratch {
            let behind = State { zone_list_open: false, focus: 0, ..*state };
            render_step(&behind, facts, bar, Some(scratch));
            let labels = TIME_ZONE_LABELS.map(|label| Some(label.as_bytes()));
            // The fixed 15-row source list satisfies ListOverlay's bound.
            let Some(list) = ListOverlay::new(Some(b"Time zone"), &labels) else { return; };
            render_list_overlay(&list, state.focus, Some(scratch), frame.as_deref_mut());
            draw_frosted_system_header(bar, HomePlane::Monochrome, frame.as_deref_mut());
            draw_system_footer(frame, list_overlay_buttons(&list, state.focus), HomePlane::Monochrome);
            return;
        }
    }
    render_step(state, facts, bar, frame);
}
