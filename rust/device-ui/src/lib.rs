// SPDX-License-Identifier: MIT
#![no_std]
#![forbid(unsafe_code)]
#![allow(non_upper_case_globals)]
mod assets;
mod developer;
mod first_newt;
pub mod home_clock;
mod screens;
mod sleep;
pub mod stats;
pub mod update;
use core::fmt::{self, Write};
pub use newt_core::reading_stats_service::SleepExtras;
pub use newt_core::sleep_stats::ReadingHabit;
pub use newt_core::{app::*, preferences::Geometry};
pub use newt_stage_home::{HeaderInfo, HomeData, HomeStyle, render_home};
pub use newt_system_ui::{HomePlane, SLEEP_FROST_SCRATCH_BYTES, frost_sleep_backdrop_in_place};
pub use newt_system_ui::{SystemBar, SystemButtons, SystemPage};
pub use newt_ui_canvas::{Frame, UiCanvas};
pub use screens::*;
pub use sleep::*;
pub(crate) fn cstr(s: &[u8]) -> &[u8] {
    &s[..s.iter().position(|&v| v == 0).unwrap_or(s.len())]
}
/// snprintf's bounded writes retain the unused tail. Capacity zero is valid.
pub(crate) struct Buffer<'a> {
    bytes: &'a mut [u8],
    used: usize,
}
impl<'a> Buffer<'a> {
    pub fn new(bytes: &'a mut [u8]) -> Self {
        if !bytes.is_empty() {
            bytes[0] = 0;
        }
        Self { bytes, used: 0 }
    }
}
impl Write for Buffer<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let n = s.len().min(self.bytes.len().saturating_sub(self.used + 1));
        self.bytes[self.used..self.used + n].copy_from_slice(&s.as_bytes()[..n]);
        self.used += n;
        if self.used < self.bytes.len() {
            self.bytes[self.used] = 0;
        }
        Ok(())
    }
}
/// Bounded diagnostic prefix using the same writer as ordinary Device rows.
#[cfg(feature = "dev-usb")]
pub fn format_wake_evidence_line(e: &newt_wake_evidence::Evidence, line: u32, out: &mut [u8]) {
    newt_wake_evidence::write_line(e, line, &mut Buffer::new(out));
}
pub(crate) fn width(s: &[u8], pixels: u32, bold: bool) -> i32 {
    newt_ui_canvas::text_width(Some(s), pixels, bold)
}
pub(crate) fn text(c: &mut UiCanvas<'_>, x: i32, y: i32, s: &[u8], p: u32, b: bool, w: i32) {
    c.text(x, y, Some(s), p, b, w, true);
}
pub(crate) fn para(c: &mut UiCanvas<'_>, x: i32, y: i32, s: &[u8], p: u32, w: i32, bottom: i32) {
    c.paragraph(x, y, Some(s), p, w, bottom);
}
/// Published Sleep screen choices; Core's statistics service choices omit progress.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SleepScreenChoices {
    pub progress: bool,
    pub week: bool,
    pub habit: bool,
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum FirmwareSwitch {
    #[default]
    Missing,
    Available,
    Failed,
    Switching,
    SaveUnconfirmed,
    SwitchUnconfirmed,
}
#[derive(Clone, Copy, Default)]
pub enum FirmwareName {
    #[default]
    Unknown,
    CrossInk,
    CrossPoint,
}
impl FirmwareName {
    pub fn title(self) -> &'static str {
        match self {
            Self::Unknown => "the other firmware",
            Self::CrossInk => "CrossInk",
            Self::CrossPoint => "CrossPoint",
        }
    }
}
/// The sleeping newt's holiday outfit, chosen from the local date.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HolidayHat {
    #[default]
    None,
    Witch,
    Christmas,
    /// A pilgrim hat, a striped scarf and a pumpkin.
    Thanksgiving,
}
impl HolidayHat {
    pub fn on(today: newt_core::stats_clock::StatsClock) -> Self {
        use newt_core::stats_clock::{christmas, halloween, thanksgiving};
        if halloween(today) {
            Self::Witch
        } else if thanksgiving(today) {
            Self::Thanksgiving
        } else if christmas(today) {
            Self::Christmas
        } else {
            Self::None
        }
    }
}
/// Borrowed presentation snapshot. Storage and save publication remain owner-controlled.
pub struct DeviceInfo<'a> {
    pub battery_known: bool,
    pub battery_percent: u16,
    pub millivolts_known: bool,
    pub millivolts: u16,
    pub charging_known: bool,
    pub charging: bool,
    pub firmware_switch: FirmwareSwitch,
    pub other_firmware: FirmwareName,
    pub hat: HolidayHat,
    pub free_heap: u32,
    pub panel: Option<&'a [u8]>,
    pub wake: Option<&'a [u8]>,
    #[cfg(feature = "dev-usb")]
    pub wake_evidence: Option<&'a newt_wake_evidence::Evidence>,
    storage: Option<&'a [u8]>,
    page_save_unconfirmed: bool,
}
impl Default for DeviceInfo<'_> {
    fn default() -> Self {
        Self {
            battery_known: false,
            battery_percent: 0,
            millivolts_known: false,
            millivolts: 0,
            charging_known: false,
            charging: false,
            firmware_switch: FirmwareSwitch::Missing,
            other_firmware: FirmwareName::Unknown,
            hat: HolidayHat::None,
            free_heap: 0,
            panel: Some(b"X3 / unverified"),
            wake: Some(b"cold boot"),
            #[cfg(feature = "dev-usb")]
            wake_evidence: None,
            storage: None,
            page_save_unconfirmed: false,
        }
    }
}
impl<'a> DeviceInfo<'a> {
    pub fn storage(&self) -> Option<&'a [u8]> {
        self.storage
    }
    pub fn page_save_unconfirmed(&self) -> bool {
        self.page_save_unconfirmed
    }
    /// Adapter for the existing ReadingSaveNotice owner's already published snapshot.
    pub fn with_storage_notice(mut self, storage: Option<&'a [u8]>, unconfirmed: bool) -> Self {
        self.storage = storage;
        self.page_save_unconfirmed = unconfirmed;
        self
    }
}
/// Only firmware-supported physical frame geometries are accepted by this adapter.
// Compare as u16: widening to usize for frame::is_frame_size grew the image by 112 B.
pub(crate) fn valid_geometry(g: Geometry) -> bool {
    const W: u16 = newt_core::frame::WIDTH as u16;
    const H: u16 = newt_core::frame::HEIGHT as u16;
    matches!((g.width, g.height), (W, H) | (H, W))
}

pub(crate) fn switch_note(slot: FirmwareSwitch, name: FirmwareName, out: &mut [u8; 128]) {
    let mut b = Buffer::new(out);
    let title = name.title();
    let _ = match slot {
        FirmwareSwitch::Missing => b.write_str(
            "No other firmware is on this X3. To add one, copy its .bin file to /Newt/Update/ and open Update."
        ),
        FirmwareSwitch::Failed => write!(b, "Could not switch to {title}. Newt stays open."),
        FirmwareSwitch::SaveUnconfirmed => b.write_str(
            "Save unconfirmed. Newt stays open. Open your book and go back to save again."
        ),
        FirmwareSwitch::SwitchUnconfirmed => write!(
            b,
            "Switch unconfirmed. Newt stays open; the next restart may open {title}."
        ),
        FirmwareSwitch::Available | FirmwareSwitch::Switching => {
            write!(b, "Saves your place, then restarts into {title}.")
        }
    };
}
