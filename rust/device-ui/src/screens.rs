use crate::*;
use crate::{Buffer, cstr, para, text, width};
use core::fmt::Write;
use newt_core::time_zone::{TIME_ZONE_LABELS, TimeZone};
use newt_core::update_online::TestBuilds;
use newt_system_ui::*;
#[derive(Default, Clone, Copy)]
pub struct BuildIdentity<'a> {
    pub version: &'a [u8],
    pub id: Option<&'a [u8]>,
    pub date: Option<&'a [u8]>,
}
pub const FIRMWARE_BUILD: BuildIdentity<'static> = BuildIdentity {
    version: b"",
    id: None,
    date: None,
};
pub(crate) const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
pub fn format_build_line(build: BuildIdentity<'_>, out: &mut [u8; 48]) -> bool {
    out[0] = 0;
    let (Some(id), Some(date)) = (build.id, build.date) else {
        return false;
    };
    let id = cstr(id);
    let date = cstr(date);
    if !(id.len() == 12 || id.len() == 18 && &id[12..] == b"-dirty")
        || !id[..12]
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
    {
        return false;
    }
    if date.len() != 10 || date[4] != b'-' || date[7] != b'-' {
        return false;
    }
    fn decimal(s: &[u8]) -> Option<u32> {
        s.iter().try_fold(0, |v, b| {
            b.is_ascii_digit().then(|| v * 10 + u32::from(b - b'0'))
        })
    }
    let (Some(year), Some(month), Some(day)) = (
        decimal(&date[..4]),
        decimal(&date[5..7]),
        decimal(&date[8..]),
    ) else {
        return false;
    };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return false;
    }
    let mut b = Buffer::new(out);
    let _ = write!(b, "Build {}", core::str::from_utf8(id).unwrap_or(""));
    if id.len() == 12 {
        let _ = write!(b, " · {} {}, {}", MONTHS[month as usize - 1], day, year);
    }
    true
}
const ROWS: [SettingsRow; 10] = [
    SettingsRow::TimeZone,
    SettingsRow::TimeFormat,
    SettingsRow::SleepScreen,
    SettingsRow::Diagnostics,
    SettingsRow::ReadingStats,
    SettingsRow::FirmwareSwitch,
    SettingsRow::Update,
    SettingsRow::Wifi,
    SettingsRow::About,
    SettingsRow::QuickStart,
];
fn settings(
    frame: Option<&mut Frame>,
    bar: &SystemBar,
    state: &AppState,
    zone: TimeZone,
    info: &DeviceInfo<'_>,
) {
    let row = state.settings_row;
    let missing =
        row == SettingsRow::FirmwareSwitch && info.firmware_switch == FirmwareSwitch::Missing;
    let action = match row {
        SettingsRow::TimeZone => b"Edit".as_slice(),
        SettingsRow::TimeFormat => b"Switch",
        _ => b"Open",
    };
    let mut page = SystemPage::new(
        frame,
        bar,
        clamped_list_buttons(
            Some(b"Back"),
            (!missing).then_some(action),
            row as u32,
            ROWS.len() as u32,
        ),
        Some(b"Settings"),
    );
    let mut label = [0; 48];
    let mut note = [0; 128];
    for (i, r) in ROWS.iter().enumerate() {
        let mut b = Buffer::new(&mut label);
        let _ = match r {
            SettingsRow::TimeZone => write!(b, "Time zone: {}", TIME_ZONE_LABELS[zone as usize]),
            SettingsRow::TimeFormat => write!(
                b,
                "Time format: {}",
                newt_core::clock_format::clock_format_label(bar.format)
            ),
            // With no other firmware there is nothing to restart into; the note says how to add one.
            SettingsRow::FirmwareSwitch if info.firmware_switch == FirmwareSwitch::Missing => {
                b.write_str("Other firmware")
            }
            SettingsRow::FirmwareSwitch => {
                write!(b, "Restart into {}", info.other_firmware.title())
            }
            _ => b.write_str(match r {
                SettingsRow::SleepScreen => "Sleep screen",
                SettingsRow::Diagnostics => "Diagnostics",
                SettingsRow::ReadingStats => "Reading statistics",
                SettingsRow::Update => "Update",
                SettingsRow::Wifi => "Wi-Fi",
                SettingsRow::About => "About",
                SettingsRow::QuickStart => "Quick start",
                _ => "",
            }),
        };
        page.row(146, i as u32, Some(&label), *r == row);
    }
    let notice = state.settings_notice;
    if row == SettingsRow::FirmwareSwitch {
        switch_note(info.firmware_switch, info.other_firmware, &mut note);
    } else {
        let mut b = Buffer::new(&mut note);
        let _ = match row {
            SettingsRow::QuickStart if notice == SettingsNotice::QuickStartSaveFailed => {
                b.write_str("Could not confirm the save. After a restart it may change.")
            }
            SettingsRow::QuickStart => b.write_str("Walk through setup again."),
            SettingsRow::About => b.write_str("Newt's version and What's new."),
            SettingsRow::Update => b.write_str(if notice == SettingsNotice::UpdateNotStarted {
                "Save unconfirmed. Update not started. Open your book and go back to save again."
            } else {
                "Saves your place, then restarts to check for updates."
            }),
            SettingsRow::Wifi => b.write_str(if notice == SettingsNotice::WifiNotStarted {
                "Save unconfirmed. Wi-Fi not opened. Open your book and go back to save again."
            } else {
                "Saves your place, then restarts to set up Wi-Fi."
            }),
            SettingsRow::TimeZone | SettingsRow::TimeFormat => {
                let (failed, saved, value, help) = if row == SettingsRow::TimeZone {
                    (
                        SettingsNotice::SaveFailed,
                        SettingsNotice::Saved,
                        TIME_ZONE_LABELS[zone as usize],
                        "Whole device. Sets the Home clock.",
                    )
                } else {
                    (
                        SettingsNotice::FormatSaveFailed,
                        SettingsNotice::FormatSaved,
                        newt_core::clock_format::clock_format_label(bar.format),
                        "12-hour: 9:41 PM. 24-hour: 21:41.",
                    )
                };
                if notice == failed {
                    write!(
                        b,
                        "Could not confirm the save. Home keeps {}; after a restart it may change.",
                        value
                    )
                } else {
                    b.write_str(if notice == saved {
                        "Saved. Home shows this time."
                    } else {
                        help
                    })
                }
            }
            _ => return,
        };
    }
    let note_top = 146 + ROWS.len() as i32 * SYSTEM_ROW_PITCH + 26;
    para(page.canvas(), 50, note_top, &note, 21, 428, note_top + 129);
}
// Leave room for the longest prefix and all 23 version bytes.
fn version_label<'a>(line: &'a mut [u8; 34], prefix: &[u8], version: &[u8]) -> &'a [u8] {
    let (head, tail) = line.split_at_mut(prefix.len());
    head.copy_from_slice(prefix);
    let count = version.len().min(tail.len());
    tail[..count].copy_from_slice(&version[..count]);
    &line[..prefix.len() + count]
}

fn about(frame: Option<&mut Frame>, bar: &SystemBar, view: AboutView, build: BuildIdentity<'_>) {
    let selected = view == AboutView::WhatsNewRow;
    let mut page = SystemPage::new(
        frame,
        bar,
        clamped_list_buttons(
            Some(b"Back"),
            selected.then_some(b"Open".as_slice()),
            u32::from(selected),
            2,
        ),
        Some(b"About your X3"),
    );
    text(page.canvas(), 50, 160, b"Newt for Xteink X3", 24, true, 428);
    text(
        page.canvas(),
        50,
        211,
        b"Offline reading. Little apps.",
        21,
        false,
        428,
    );
    let mut version = [0; 34];
    page.row(284, 0, Some(version_label(&mut version, b"Version: ", build.version)), !selected);
    let mut line = [0; 48];
    if format_build_line(build, &mut line) {
        text(page.canvas(), 72, 331, &line, 21, false, 406);
    }
    page.row(284, 2, Some(b"What's new"), selected);
    para(
        page.canvas(),
        50,
        459,
        b"Built on official FreeInk. Third-party source and font licenses are preserved.",
        21,
        428,
        699,
    );
}
fn firmware(frame: Option<&mut Frame>, bar: &SystemBar, slot: FirmwareSwitch, name: FirmwareName) {
    let mut page = SystemPage::new(
        frame,
        bar,
        SystemButtons {
            back: Some(b"Back"),
            confirm: Some(if slot == FirmwareSwitch::Missing {
                b""
            } else {
                b"Restart"
            }),
            previous: Some(b""),
            next: Some(b""),
        },
        Some(b"Switch firmware"),
    );
    let mut buf = [0; 128];
    let mut b = Buffer::new(&mut buf);
    if slot == FirmwareSwitch::Switching {
        let _ = write!(b, "Switching to {}…", name.title());
        para(page.canvas(), 50, 160, &buf, 24, 428, 300);
        return;
    }
    let _ = write!(
        b,
        "Newt saves your place, then restarts into {}.",
        name.title()
    );
    para(page.canvas(), 50, 160, &buf, 24, 428, 240);
    // An unnamed firmware (such as the X3's own) may not install from the card.
    // Whole strings, not a write!: a format call costs more flash than the repeated words.
    let stays: &[u8] = match name {
        FirmwareName::Unknown => b"Newt stays on this X3 unless that firmware updates over it. To come back, install Newt from a computer.",
        _ => b"Newt stays on this X3 unless that firmware updates over it. To come back, install newt.bin from that firmware or a computer.",
    };
    para(page.canvas(), 50, 256, stays, 21, 428, 400);
    if slot != FirmwareSwitch::Available {
        switch_note(slot, name, &mut buf);
        para(page.canvas(), 50, 416, &buf, 21, 428, 540);
    }
}
pub struct WhatsNewUpdate<'a> {
    pub date: Option<&'a [u8]>,
    pub changes: [Option<&'a [u8]>; 6],
}
include!(concat!(env!("OUT_DIR"), "/whats_new.rs"));
pub fn draw_whats_new(c: &mut UiCanvas<'_>, updates: &[WhatsNewUpdate<'_>]) -> bool {
    let mut top = 146;
    for u in updates {
        let mut rows = [0; 6];
        let mut bottom = top + 34;
        let Some(date) = u.date else { return false };
        if u.changes[0].is_none() || width(date, 21, true) > 428 {
            return false;
        }
        for (i, s) in u.changes.iter().enumerate() {
            let Some(s) = s else { break };
            rows[i] = newt_ui_canvas::paragraph_rows(Some(s), 21, 406);
            if rows[i] <= 0 {
                return false;
            }
            bottom += rows[i] * 30 + 6;
        }
        if bottom - 10 > 699 {
            return false;
        }
        text(c, 50, top, date, 21, true, 428);
        top += 34;
        for (i, s) in u.changes.iter().enumerate() {
            let Some(s) = s else { break };
            c.rect(56, top + 11, 4, 1, true);
            c.rect(55, top + 12, 6, 4, true);
            c.rect(56, top + 16, 4, 1, true);
            para(c, 72, top, s, 21, 406, 699);
            top += rows[i] * 30 + 6;
        }
        top += 14;
    }
    true
}
fn diagnostics(frame: Option<&mut Frame>, bar: &SystemBar, info: &DeviceInfo<'_>) {
    let mut page = SystemPage::new(
        frame,
        bar,
        SystemButtons {
            back: Some(b"Back"),
            confirm: Some(b"About"),
            ..Default::default()
        },
        Some(b"Diagnostics"),
    );
    let c = page.canvas();
    #[cfg(feature = "dev-usb")]
    if let Some(evidence) = info.wake_evidence.filter(|e| e.available) {
        c.text(
            50,
            139,
            Some(b"Wake details (this boot)"),
            21,
            true,
            428,
            true,
        );
        let mut line_text = [0; 64];
        for line in 0..newt_wake_evidence::LINES {
            crate::format_wake_evidence_line(evidence, line, &mut line_text);
            c.text(
                50,
                176 + line as i32 * 30,
                Some(cstr(&line_text)),
                21,
                false,
                428,
                true,
            );
        }
        c.text(
            50,
            585,
            Some(if info.storage().is_some() {
                b"Storage"
            } else {
                b"Wake"
            }),
            17,
            true,
            428,
            true,
        );
        c.paragraph(50, 611, info.storage().or(info.wake), 21, 428, 699);
        return;
    }
    let mut value = [0; 64];
    for (y, label) in [
        (153, b"Battery".as_slice()),
        (236, b"Voltage"),
        (319, b"Charging"),
        (402, b"Display"),
        (485, b"Memory"),
    ] {
        let mut b = Buffer::new(&mut value);
        let _ = match y {
            153 => {
                if info.battery_known {
                    write!(b, "{}%", info.battery_percent.min(100))
                } else {
                    b.write_str("Not measured yet")
                }
            }
            236 => {
                if info.millivolts_known {
                    write!(b, "{} mV", info.millivolts)
                } else {
                    b.write_str("Not reported")
                }
            }
            319 => b.write_str(if !info.charging_known {
                "Not reported"
            } else if info.charging {
                "Yes"
            } else {
                "No"
            }),
            485 => {
                if info.free_heap != 0 {
                    write!(b, "{} bytes free", info.free_heap)
                } else {
                    b.write_str("Not reported")
                }
            }
            _ => Ok(()),
        };
        text(c, 50, y, label, 17, true, 428);
        text(
            c,
            50,
            y + 26,
            if y == 402 {
                info.panel.unwrap_or(b"Not reported")
            } else {
                &value
            },
            24,
            false,
            428,
        );
    }
    text(
        c,
        50,
        568,
        if info.storage().is_some() {
            b"Storage"
        } else {
            b"Wake"
        },
        17,
        true,
        428,
    );
    para(
        c,
        50,
        594,
        info.storage().or(info.wake).unwrap_or(b""),
        21,
        428,
        699,
    );
}
/// Callback is called once only on ReadingStats, even with a null frame.
/// Scratch is independently borrowed; aliasing the output cannot be expressed.
pub fn render_device_ui(
    state: &AppState,
    info: &DeviceInfo<'_>,
    bar: &SystemBar,
    mut frame: Option<&mut Frame>,
    zone: TimeZone,
    mut scratch: Option<&mut Frame>,
    mut stats: Option<&mut dyn FnMut(StatsGraphPosition, u32) -> stats::StatsScreenView>,
    build: BuildIdentity<'_>,
    sleep: SleepScreenChoices,
    test_builds: TestBuilds,
) {
    match device_page(state) {
        DevicePage::Settings => {
            if !state.zone_list_open {
                settings(frame, bar, state, zone, info);
                return;
            }
            if let Some(back) = scratch.as_deref_mut() {
                settings(Some(back), bar, state, zone, info);
            } else {
                settings(frame.as_deref_mut(), bar, state, zone, info);
            }
            let labels = TIME_ZONE_LABELS.map(|v| Some(v.as_bytes()));
            let list = ListOverlay::new(Some(b"Time zone"), &labels).expect("15 labels");
            render_list_overlay(
                &list,
                state.list_focus,
                scratch.as_deref(),
                frame.as_deref_mut(),
            );
            draw_frosted_system_header(bar, HomePlane::Monochrome, frame.as_deref_mut());
            draw_system_footer(
                frame,
                list_overlay_buttons(&list, state.list_focus),
                HomePlane::Monochrome,
            );
        }
        DevicePage::FirmwareSwitch => {
            firmware(frame, bar, info.firmware_switch, info.other_firmware)
        }
        DevicePage::SleepScreen => render_sleep_screen_settings(
            frame,
            bar,
            state.list_focus.into(),
            sleep,
            state.settings_notice,
        ),
        DevicePage::About => about(frame, bar, state.about, build),
        DevicePage::WhatsNew => {
            let mut version = [0; 34];
            let mut page = SystemPage::new(
                frame,
                bar,
                SystemButtons {
                    back: Some(b"Back"),
                    ..Default::default()
                },
                Some(if state.about == AboutView::Updated {
                    version_label(&mut version, b"Updated to ", build.version)
                } else {
                    b"What's new"
                }),
            );
            draw_whats_new(page.canvas(), &WHATS_NEW);
        }
        DevicePage::ReadingStats => {
            let shown = stats_graph_position(state.list_focus);
            let buttons = stats::stats_graph_buttons(shown.view);
            let mut page = SystemPage::new(frame, bar, buttons, Some(b"Reading statistics"));
            let view = shown.view;
            let data = stats
                .as_mut()
                .map(|source| source(shown, stats::stats_graph_days(428)))
                .unwrap_or_default();
            stats::render_stats_graph_view(page.canvas(), view, &data);
        }
        DevicePage::FirstNewt => crate::first_newt::first_newt(frame, bar),
        DevicePage::Developer => crate::developer::developer(frame, bar, test_builds, state.settings_notice),
        DevicePage::Diagnostics => diagnostics(frame, bar, info),
    }
}

#[cfg(test)]
#[path = "../../tests/device_ui/version_label.rs"]
mod tests;
