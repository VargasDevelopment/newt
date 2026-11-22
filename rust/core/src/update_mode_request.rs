// SPDX-License-Identifier: MIT
//! Separate-boot update mode request record policy.
//! The owner lends separate fields of its existing empty SD resume record.
//! This module neither owns retained storage nor authorizes a save or restart.
#![forbid(unsafe_code)]

const UPDATE_MAGIC: u32 = 0x5544_574e;
const WIFI_MAGIC: u32 = 0x5744_574e;
const QUICK_START_WIFI_MAGIC: u32 = 0x5144_574e;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum UpdateModeRequest {
    #[default]
    None,
    Update,
    Wifi,
    QuickStartWifi,
}

/// Write in source order, on fields lent only after the owner's exit guard.
/// `None` maps to `Update` as the fallback magic.
pub fn request(
    magic: &mut u32,
    version: &mut u32,
    checksum: &mut u32,
    page: UpdateModeRequest,
) {
    let requested = match page {
        UpdateModeRequest::Wifi => WIFI_MAGIC,
        UpdateModeRequest::QuickStartWifi => QUICK_START_WIFI_MAGIC,
        _ => UPDATE_MAGIC,
    };
    *magic = 0;
    *version = requested;
    *checksum = !requested;
}

/// Consume a valid empty-record request on every reset; honor only software.
/// Invalid requests and nonempty resume records leave all three fields intact.
pub fn take(
    magic: &mut u32,
    version: &mut u32,
    checksum: &mut u32,
    software_restart: bool,
) -> UpdateModeRequest {
    let page = match *version {
        UPDATE_MAGIC => UpdateModeRequest::Update,
        WIFI_MAGIC => UpdateModeRequest::Wifi,
        QUICK_START_WIFI_MAGIC => UpdateModeRequest::QuickStartWifi,
        _ => UpdateModeRequest::None,
    };
    if *magic != 0 || page == UpdateModeRequest::None || *checksum != !*version {
        return UpdateModeRequest::None;
    }
    *version = 0;
    *checksum = 0;
    if software_restart { page } else { UpdateModeRequest::None }
}
