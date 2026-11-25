// SPDX-License-Identifier: MIT
//! Settings -> Sleep screen -> Holiday outfits: three opt-in holiday rows.
use crate::{Buffer, HolidayOutfits, para};
use core::fmt::Write;
use newt_core::app::SettingsNotice;
use newt_system_ui::{SYSTEM_BODY_TOP, SystemBar, SystemPage, clamped_list_buttons};
use newt_ui_canvas::Frame;

pub(crate) fn holiday_outfits(
    frame: Option<&mut Frame>,
    bar: &SystemBar,
    focus: u32,
    choices: HolidayOutfits,
    notice: SettingsNotice,
) {
    let pos = focus.min(2);
    let mut page = SystemPage::new(
        frame,
        bar,
        clamped_list_buttons(Some(b"Back"), Some(b"Switch"), pos, 3),
        Some(b"Holiday outfits"),
    );
    let on = [choices.halloween, choices.thanksgiving, choices.christmas];
    let names = ["Halloween", "Thanksgiving", "Christmas"];
    let mut buf = [0; 104];
    for i in 0..3 {
        let _ = write!(
            Buffer::new(&mut buf),
            "{}: {}",
            names[i],
            if on[i] { "On" } else { "Off" }
        );
        page.row(SYSTEM_BODY_TOP, i as u32, Some(&buf), i as u32 == pos);
    }
    let notes = [
        "Witch hat on October 31, when the clock is set.",
        "Pilgrim hat, scarf and pumpkin on US Thanksgiving, when the clock is set.",
        "Christmas hat on December 24 and 25, when the clock is set.",
    ];
    let failed = [
        SettingsNotice::HolidayHalloweenSaveFailed,
        SettingsNotice::HolidayThanksgivingSaveFailed,
        SettingsNotice::HolidayChristmasSaveFailed,
    ];
    let i = pos as usize;
    let note = if notice == failed[i] {
        let _ = write!(
            Buffer::new(&mut buf),
            "Could not confirm the save. {} stays {} for now and may change after a restart.",
            names[i],
            if on[i] { "On" } else { "Off" }
        );
        &buf[..]
    } else {
        notes[i].as_bytes()
    };
    para(page.canvas(), 50, 301, note, 21, 428, 430);
}
