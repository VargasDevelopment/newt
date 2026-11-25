// SPDX-License-Identifier: MIT
use super::*;
use super::dispatch::{can_check, connect_focused, firmware, forget_focused, network_rows, order, present_count,
    sets_up_wifi};
use crate::{Buffer, cstr};
use core::fmt::Write;
use newt_core::update_card::FILE_BYTES;
use newt_system_ui::*;
use newt_ui_canvas::{Frame, folded_name, paragraph_rows, text_width};

/// snprintf's byte-preserving concatenation into the caller's original buffer.
fn joined(out: &mut [u8], parts: &[&[u8]]) {
    if out.is_empty() { return; }
    let mut used = 0;
    for part in parts {
        let part = cstr(part);
        let n = part.len().min(out.len() - 1 - used);
        out[used..used + n].copy_from_slice(&part[..n]);
        used += n;
    }
    out[used] = 0;
}
pub(super) fn firmware_title(name: FirmwareName) -> &'static [u8] {
    match name { FirmwareName::Unknown => b"the other firmware", FirmwareName::CrossInk => b"CrossInk",
        FirmwareName::CrossPoint => b"CrossPoint" }
}
fn offer_label(offer: &UpdateOffer, out: &mut [u8; 48]) {
    if firmware(offer) && offer.readable && offer.name != FirmwareName::Unknown {
        joined(out, &[firmware_title(offer.name), b" ", &offer.manifest.version[..31]]);
    } else if firmware(offer) { joined(out, &[&offer.file]); }
    else if !offer.readable { joined(out, &[b"Newt: unreadable file"]); }
    else { joined(out, &[b"Newt ", &offer.manifest.version]); }
}
fn offer_note<P, S>(view: &UpdateView<'_, P, S>, offer: &UpdateOffer, note: &mut [u8; 160]) {
    if firmware(offer) {
        if offer.readable && offer.name != FirmwareName::Unknown {
            joined(note, &[b"From ", &offer.file, b". Newt stays installed beside it."]);
        } else if offer.readable {
            joined(note, &[b"Version ", &offer.manifest.version[..31], b". Newt stays installed beside it."]);
        } else if offer.image == SlotImage::Newt {
            joined(note, &[b"This file is Newt. Newt updates need newt.bin with its newt.manifest."]);
        } else { joined(note, &[b"This file is not X3 firmware."]); }
        return;
    }
    if !offer.readable { joined(note, &[b"Its manifest is not a Newt update file."]); return; }
    let newer = order(view, offer);
    joined(note, &[b"Build ", &offer.manifest.build, b". This X3 has ", view.running.to_bytes(), b".",
        if newer < 0 { b" Use USB to install an older version." } else if newer == 0 { b" It is installed." } else { b"" }]);
}
fn render_list<P, S>(view: &UpdateView<'_, P, S>, bar: &SystemBar, frame: Option<&mut Frame>) {
    let offer = focused_offer(view);
    let count = present_count(view);
    let install = offer.is_some_and(|index| update_choice(view, index) != UpdateChoice::None);
    let check = check_focused(view);
    let wifi = sets_up_wifi(view);
    let confirm = if install { Some(b"Install".as_slice()) } else if check && can_check(view) { Some(b"Check".as_slice()) }
        else if check && wifi { Some(b"Open".as_slice()) } else { None };
    let mut page = SystemPage::new(frame, bar, clamped_list_buttons(Some(b"Back"), confirm,
        u32::from(view.focus), count + 1), Some(b"Update"));
    let mut label = [0; 48]; let mut note = [0; 160]; let mut row = 0;
    for each in &view.offers {
        if !each.present { continue; }
        offer_label(each, &mut label);
        page.row(SYSTEM_BODY_TOP, row, Some(&label), row == u32::from(view.focus)); row += 1;
    }
    page.row(SYSTEM_BODY_TOP, row, Some(if wifi { b"Set up Wi-Fi".as_slice() } else { b"Check GitHub for updates" }), check);
    row += 1;
    let mut top = SYSTEM_BODY_TOP + row as i32 * SYSTEM_ROW_PITCH + 26;
    let c = page.canvas();
    let mut paragraph = |text: &[u8]| {
        let rows = paragraph_rows(Some(text), 21, SYSTEM_CONTENT_WIDTH);
        c.paragraph(SYSTEM_CONTENT_LEFT, top, Some(text), 21, SYSTEM_CONTENT_WIDTH, 699);
        top += rows.max(1) * 30 + 16;
    };
    if let Some(index) = offer { offer_note(view, &view.offers[index], &mut note); paragraph(&note); }
    else if !view.card_ready { paragraph(b"No card found. Insert the card, then open Update again."); }
    else if wifi { paragraph(b"Save a Wi-Fi network to check GitHub for updates."); }
    else { paragraph(b"Downloads a newer Newt from GitHub to the card with the saved Wi-Fi network."); }
    if check && view.card_ready && count == 0 {
        paragraph(b"Or copy newt.bin and newt.manifest to /Newt/Update/.");
        paragraph(b"To install another firmware, copy its .bin file there.");
    }
    if view.more_files { paragraph(b"Newt lists the first three .bin files in /Newt/Update/ by name."); }
    if view.rolled_back {
        joined(&mut note, &[b"The last update did not start. Newt went back to ", view.running.to_bytes(), b"."]);
        paragraph(&note);
    }
}
fn render_networks<P, S>(view: &UpdateView<'_, P, S>, bar: &SystemBar, frame: Option<&mut Frame>) {
    let confirm = if view.network_focus == 0 { (view.scan != WifiScan::Scanning).then_some(b"Scan".as_slice()) }
        else { Some(if forget_focused(view) { b"Forget".as_slice() } else if connect_focused(view) { b"Test" } else { b"Pick" }) };
    let mut page = SystemPage::new(frame, bar, clamped_list_buttons(Some(b"Back"), confirm,
        u32::from(view.network_focus), network_rows(view)), Some(b"Wi-Fi networks"));
    let c = page.canvas();
    let mut name = [0; 64]; let mut label = [0; 80]; let mut row = 0;
    draw_system_row(c, SYSTEM_CONTENT_LEFT, SYSTEM_BODY_TOP, row, SYSTEM_CONTENT_WIDTH, Some(b"Scan again"), view.network_focus == 0);
    for network in view.networks.iter().take(usize::from(view.network_count)) {
        row += 1;
        folded_name(Some(&network.name), b"Unnamed network", &mut name);
        draw_system_row(c, SYSTEM_CONTENT_LEFT, SYSTEM_BODY_TOP, row, SYSTEM_CONTENT_WIDTH - 96,
            Some(&name), u32::from(view.network_focus) == row);
        let signal = if network.rssi >= -60 { b"Strong".as_slice() } else if network.rssi >= -70 { b"Good" } else { b"Weak" };
        c.text(SYSTEM_CONTENT_LEFT + SYSTEM_CONTENT_WIDTH - text_width(Some(signal), 21, false),
            SYSTEM_BODY_TOP + row as i32 * SYSTEM_ROW_PITCH + 2, Some(signal), 21, false, 96, true);
    }
    if view.saved.saved {
        folded_name(Some(&view.saved.name), b"Unnamed network", &mut name);
        for verb in [b"Test".as_slice(), b"Forget"] {
            row += 1; joined(&mut label, &[verb, b" ", &name]);
            draw_system_row(c, SYSTEM_CONTENT_LEFT, SYSTEM_BODY_TOP, row, SYSTEM_CONTENT_WIDTH,
                Some(&label), u32::from(view.network_focus) == row);
        }
    }
    let note = match view.scan { WifiScan::Scanning => Some(b"Looking for networks...".as_slice()),
        WifiScan::Failed => Some(b"Newt could not look for networks.".as_slice()),
        WifiScan::Done if view.network_count == 0 => Some(b"No networks found.".as_slice()), _ => None };
    if let Some(note) = note { c.paragraph(SYSTEM_CONTENT_LEFT, SYSTEM_BODY_TOP + (row + 1) as i32 * SYSTEM_ROW_PITCH + 26,
        Some(note), 21, SYSTEM_CONTENT_WIDTH, 699); }
}
fn render_password<P: Password, S>(view: &UpdateView<'_, P, S>, bar: &SystemBar, frame: Option<&mut Frame>) {
    let (row, column) = keyboard::focus(view.symbols, view.shifted, view.key_row, view.key_column);
    let key = keyboard::key(view.symbols, view.shifted, row, column);
    let complete = view.password.size() >= PASSWORD_MINIMUM;
    let confirm = key.and_then(|key| match key.kind {
        keyboard::Kind::Text | keyboard::Kind::Space => Some(b"Type".as_slice()),
        keyboard::Kind::Delete => Some(b"Delete"), keyboard::Kind::Submit => complete.then_some(b"Save".as_slice()),
        keyboard::Kind::Shift | keyboard::Kind::Mode => Some(b"Switch"),
    });
    let mut page = SystemPage::new(frame, bar, SystemButtons { back: Some(b"Back"), confirm,
        previous: Some(b"Left"), next: Some(b"Right") }, Some(b"Wi-Fi password"));
    let c = page.canvas(); let mut text = [0; PASSWORD_BYTES + 1];
    folded_name(Some(&view.joining.name), b"Unnamed network", &mut text);
    c.text(SYSTEM_CONTENT_LEFT, SYSTEM_BODY_TOP, Some(&text), 24, true, SYSTEM_CONTENT_WIDTH, true);
    c.outline(SYSTEM_CONTENT_LEFT, 190, SYSTEM_CONTENT_WIDTH, 52);
    let typed = view.password.size(); text[..typed].fill(b'*'); text[typed] = 0;
    c.text(SYSTEM_CONTENT_LEFT + 12, 203, Some(&text), 24, false, SYSTEM_CONTENT_WIDTH - 24, true);
    let _ = write!(Buffer::new(&mut text), "{} {}.{}", typed, if typed == 1 { "character" } else { "characters" },
        if complete { "" } else { " At least 8." });
    c.text(SYSTEM_CONTENT_LEFT, 262, Some(&text), 21, false, SYSTEM_CONTENT_WIDTH, true);
    c.paragraph(SYSTEM_CONTENT_LEFT, 300, Some(b"The side buttons move between rows."), 21, SYSTEM_CONTENT_WIDTH, 360);
    const UNIT: i32 = SYSTEM_CONTENT_WIDTH / 20;
    const KEYS_TOP: i32 = 64 + 654 - 12 - 4 * 54 - 50;
    for r in 0..keyboard::rows(view.symbols) {
        let count = keyboard::columns(view.symbols, view.shifted, r);
        let units: i32 = (0..count).filter_map(|k| keyboard::key(view.symbols, view.shifted, r, k)).map(|key| key.width).sum();
        let mut x = SYSTEM_CONTENT_LEFT + (SYSTEM_CONTENT_WIDTH - units * UNIT) / 2;
        let top = KEYS_TOP + i32::from(r) * 54;
        for k in 0..count {
            let Some(key) = keyboard::key(view.symbols, view.shifted, r, k) else { continue; };
            let width = key.width * UNIT; let focused = r == row && k == column;
            let bold = key.kind == keyboard::Kind::Shift && view.shifted && !view.symbols;
            let byte = [key.byte];
            let label: &[u8] = match key.kind { keyboard::Kind::Text => byte.as_slice(), keyboard::Kind::Space => b"Space",
                keyboard::Kind::Delete => b"Del", keyboard::Kind::Submit => b"Save",
                keyboard::Kind::Mode => if view.symbols { b"ABC" } else { b"?123" },
                keyboard::Kind::Shift => if view.symbols { if view.shifted { b"123" } else { b"#+=" } } else { b"Shift" } };
            if focused { c.rect(x + 2, top, width - 4, 50, true); } else { c.outline(x + 2, top, width - 4, 50); }
            c.text(x + (width - text_width(Some(label), 21, bold)) / 2, top + 13, Some(label), 21, bold, width - 6, !focused);
            x += width;
        }
    }
}
fn render_confirm<P, S>(view: &UpdateView<'_, P, S>, bar: &SystemBar, frame: Option<&mut Frame>) {
    let mut page = SystemPage::new(frame, bar, SystemButtons { back: Some(b"Back"), confirm: Some(b"Install"),
        ..SystemButtons::default() }, Some(b"Update"));
    let Some(index) = focused_offer(view) else { return; };
    let offer = &view.offers[index]; let c = page.canvas(); let mut top = 160;
    let mut paragraph = |text: &[u8], size: u32| {
        let rows = paragraph_rows(Some(text), size, SYSTEM_CONTENT_WIDTH);
        c.paragraph(SYSTEM_CONTENT_LEFT, top, Some(text), size, SYSTEM_CONTENT_WIDTH, 699);
        top += rows.max(1) * (size as i32 + 9) + 16;
    };
    let mut text = [0; 160]; let other = firmware_title(view.target_name);
    if !firmware(offer) {
        if view.target == SlotImage::Other { joined(&mut text, &[b"This replaces ", other,
            b" on this X3. To install it again later, copy its .bin file to /Newt/Update/."]); }
        else { joined(&mut text, &[b"This version is already installed. Install it again?"]); }
        paragraph(&text, 24); return;
    }
    let mut label = [0; 48]; offer_label(offer, &mut label);
    joined(&mut text, &[b"Install ", &label, b"?"]); paragraph(&text, 24);
    match view.target {
        SlotImage::Other => joined(&mut text, &[b"It replaces ", other, b" on this X3."]),
        SlotImage::Newt => joined(&mut text, &[b"It replaces the spare copy of Newt on this X3."]),
        SlotImage::Empty => joined(&mut text, &[b"No other firmware is installed, so nothing is replaced."]),
        SlotImage::Invalid => joined(&mut text, &[b"It replaces the damaged spare firmware on this X3."]),
    }
    paragraph(&text, 21);
    // An unnamed firmware (such as the X3's own) may not install from the card.
    joined(&mut text, &[b"Newt stays installed. The X3 then restarts into the new firmware. To come back to Newt, install ",
        if offer.name == FirmwareName::Unknown { b"it from a computer." } else { b"newt.bin from that firmware or a computer." }]);
    paragraph(&text, 21);
}
pub fn render_update<P: Password, S>(view: &UpdateView<'_, P, S>, bar: &SystemBar, frame: Option<&mut Frame>) {
    match view.page {
        UpdatePage::List => render_list(view, bar, frame), UpdatePage::Confirm => render_confirm(view, bar, frame),
        UpdatePage::Networks => render_networks(view, bar, frame), UpdatePage::Password => render_password(view, bar, frame),
        _ => status::render(view, bar, frame),
    }
}
const _: () = assert!(b"CrossPoint ".len() + 32 + 1 <= 48 && FILE_BYTES <= 48);
