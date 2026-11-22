// SPDX-License-Identifier: MIT
//! Frozen firmware_switch.h and EspAppSlots policy, over short SDK loans.
#![forbid(unsafe_code)]
use newt_c3_board::{Board, image::ImagePartitions};
use newt_core::{app::{self, Action, AppState, DevicePage, Screen, SettingsRow},
    update_card::{self, SlotImage, slot_installer::{classify_slot,
        identity::{app_image_intact, other_firmware_slot, OTA_SECTOR_BYTES},
        selection::{commit_ota_selection, OtaCommit, SaveOutcome}}}};
use newt_device_ui::{DeviceInfo, FirmwareName, FirmwareSwitch};

/// Required actual reader exit/save, paint and restart operations. Reborrow
/// one existing Board sequentially; no callback can retain a partition loan.
pub trait Ports<'sdk> {
    fn board(&mut self) -> &mut Board<'sdk>;
    fn save(&mut self) -> SaveOutcome;
    fn show(&mut self, app: &AppState, info: &DeviceInfo<'_>);
    fn restart(&mut self);
}
/// Boot-lifetime uncertainty, surviving page exit/re-entry as in EspAppSlots.
pub struct State { selection_unconfirmed: bool }
const _: () = assert!(core::mem::size_of::<State>() == 1);
impl State {
    pub const fn new() -> Self { Self { selection_unconfirmed: false } }

    #[inline(never)]
    pub fn dispatch<'sdk>(&mut self, ports: &mut impl Ports<'sdk>, app: &mut AppState,
        action: Action, info: &mut DeviceInfo<'_>,
    ) -> bool {
        if app.screen != Screen::Device || app.zone_list_open || action != Action::Activate { return false; }
        let page = app::device_page(app);
        let row = page == DevicePage::Settings && app.settings_row == SettingsRow::FirmwareSwitch;
        if !row && page != DevicePage::FirmwareSwitch { return false; }
        if row {
            let present = found(ports.board(), &mut info.other_firmware);
            info.firmware_switch = if !present { FirmwareSwitch::Missing }
                else if self.selection_unconfirmed { FirmwareSwitch::SwitchUnconfirmed }
                else { FirmwareSwitch::Available };
            app.row_open = present;
            return true;
        }
        if ports.save() == SaveOutcome::Unconfirmed {
            info.firmware_switch = FirmwareSwitch::SaveUnconfirmed;
            return true;
        }
        if !found(ports.board(), &mut info.other_firmware) {
            info.firmware_switch = FirmwareSwitch::Missing;
            return true;
        }
        info.firmware_switch = FirmwareSwitch::Switching;
        ports.show(app, info);
        let outcome = if verify(ports.board()) { select(ports.board(), self.selection_unconfirmed) }
            else if self.selection_unconfirmed { SaveOutcome::Unconfirmed } else { SaveOutcome::Unchanged };
        match outcome {
            SaveOutcome::Verified => {
                self.selection_unconfirmed = false;
                ports.restart();
                return true;
            }
            SaveOutcome::Unconfirmed => {
                self.selection_unconfirmed = true;
                info.firmware_switch = FirmwareSwitch::SwitchUnconfirmed;
                return true;
            }
            SaveOutcome::Unchanged => self.selection_unconfirmed = false,
        }
        info.firmware_switch = FirmwareSwitch::Failed;
        true
    }
}

#[inline(never)]
fn read(parts: &mut ImagePartitions<'_, '_>, slot: usize, name: &mut FirmwareName) -> SlotImage {
    let mut head = [0; 512];
    if parts.read_app(slot, 0, &mut head).is_err() { return SlotImage::Invalid; }
    *name = match update_card::firmware_name(&head) {
        update_card::FirmwareName::Unknown => FirmwareName::Unknown,
        update_card::FirmwareName::CrossInk => FirmwareName::CrossInk,
        update_card::FirmwareName::CrossPoint => FirmwareName::CrossPoint,
    };
    classify_slot(&head)
}
fn locate(parts: &mut ImagePartitions<'_, '_>, name: &mut FirmwareName) -> Option<usize> {
    *name = FirmwareName::Unknown;
    let running = parts.running_slot();
    if !parts.app_present(0) || !parts.app_present(1) || running < 0 { return None; }
    let mut names = [FirmwareName::Unknown; 2];
    let app0 = read(parts, 0, &mut names[0]);
    let app1 = read(parts, 1, &mut names[1]);
    let slot = other_firmware_slot(app0, app1, running);
    if slot < 0 { return None; }
    *name = names[slot as usize];
    Some(slot as usize)
}
pub fn found(board: &mut Board<'_>, name: &mut FirmwareName) -> bool {
    board.with_image_partitions(false, |parts| locate(parts, name).is_some())
}
#[inline(never)]
fn verify(board: &mut Board<'_>) -> bool {
    board.with_image_partitions(false, |parts| {
        let Some(slot) = locate(parts, &mut FirmwareName::Unknown) else { return false; };
        let Ok(bytes) = parts.app_bytes(slot) else { return false; };
        matches!(parts.with_hash(|parts, hash| app_image_intact(
            &mut |at, out| parts.read_app(slot, at, out).is_ok(), bytes, hash)), Ok(true))
    })
}
#[inline(never)]
fn select(board: &mut Board<'_>, unconfirmed: bool) -> SaveOutcome {
    board.with_image_partitions(true, |parts| {
        let refused = if unconfirmed { SaveOutcome::Unconfirmed } else { SaveOutcome::Unchanged };
        let Some(slot) = locate(parts, &mut FirmwareName::Unknown) else { return refused; };
        if !matches!(parts.ota_bytes(), Ok(bytes) if bytes >= 2 * OTA_SECTOR_BYTES) {
            return refused;
        }
        // The existing Core operation owns erase/write/readback decisions,
        // including a failed write which nevertheless selected the new slot.
        commit_ota_selection(parts, slot as i32, OtaCommit::Switch)
    })
}
