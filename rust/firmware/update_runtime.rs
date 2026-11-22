// SPDX-License-Identifier: MIT
//! Required effects for the separate update boot. No reading owner, input
//! task, radio, TLS heap or credential backing is constructed by this module.
#![forbid(unsafe_code)]
use core::ffi::CStr;
use crate::wifi_secrets::{SecretBytes, Wipe};
use newt_core::{durable::Backend, update_mode_request::UpdateModeRequest,
    wifi_network_store::{PASSWORD_BYTES, workflow::{self, Cipher, KeyBackend, NativeWipe,
        Random, ScopedSecrets, Secret, WifiChangeReport, WifiRestoreReport, Work}}};
use newt_device_ui::update::{self, UpdateIntent};

pub type UpdateView<'a> = update::UpdateView<'a, SecretBytes<PASSWORD_BYTES>, SecretBytes<32>>;

/// Called once before Root places the owner in its final no-move scope.
/// All fills, mixing and UI calls occur only after that placement.
pub fn empty_view(running: &CStr) -> UpdateView<'_> {
    UpdateView::new(running, SecretBytes::empty(), SecretBytes::empty())
}

/// A Wi-Fi-only boot skips card offers; Quick Start Wi-Fi still reads them.
pub fn needs_card_offers(request: UpdateModeRequest) -> bool {
    matches!(request, UpdateModeRequest::Update | UpdateModeRequest::QuickStartWifi)
}
pub fn open_requested_page(view: &mut UpdateView<'_>, request: UpdateModeRequest) -> Option<UpdateIntent> {
    if request == UpdateModeRequest::None { return None; }
    view.quick_start = request == UpdateModeRequest::QuickStartWifi;
    Some(if request == UpdateModeRequest::Update { UpdateIntent::Repaint } else { update::open_networks(view) })
}

/// Frozen update_mode.h::scanUpdate. Caller supplies actual short card and
/// partition loans; Core keeps file ordering, classification and OTA policy.
pub fn scan_offers(view: &mut UpdateView<'_>, card_ready: bool,
    files: &mut dyn newt_core::update_card::Files, directory: &mut dyn newt_core::update_card::Directory,
    slots: &mut newt_c3_board::image::ImagePartitions<'_, '_>,
    scratch: &mut [u8; newt_core::update_manifest::MAX_BYTES + 1],
) {
    use newt_core::update_card;
    view.card_ready = card_ready;
    if card_ready { update_card::read_card_offers(files, directory, &mut view.offers, &mut view.more_files, scratch); }
    scan_target(view, slots);
}

/// The same slot observations also run when no mounted card capability exists.
pub fn scan_target(view: &mut UpdateView<'_>, slots: &mut newt_c3_board::image::ImagePartitions<'_, '_>) {
    use newt_core::update_card::{self, slot_installer::{classify_slot, install_target_slot,
        identity::{OtaRecord, ota_rolled_back, OTA_SECTOR_BYTES}, selection::read_ota_records}};
    let running = slots.running_slot();
    let mut head = [0; 512];
    let read = running >= 0 && slots.read_app(install_target_slot(running) as usize, 0, &mut head).is_ok();
    view.target = if read { classify_slot(&head) } else { update_card::SlotImage::Invalid };
    view.target_name = if read { update_card::firmware_name(&head) } else { update_card::FirmwareName::Unknown };
    let mut records = [OtaRecord::default(); 2];
    view.rolled_back = running >= 0 && slots.ota_bytes().is_ok_and(|size| size >= 2 * OTA_SECTOR_BYTES)
        && read_ota_records(slots, &mut records) && ota_rolled_back(&records, running);
}

/// The actual native NVS flash fence is queried at the save boundary; its
/// backend independently enforces the same fence on every mutation.
pub trait FlashFence { fn flash_fenced_for_wifi(&self) -> bool; }

pub fn restore_network(view: &mut UpdateView<'_>, card: &mut dyn Backend,
    keys: &mut dyn KeyBackend, cipher: &mut dyn Cipher, scopes: &dyn ScopedSecrets, work: &mut Work<'_>,
) -> WifiRestoreReport {
    workflow::restore_wifi_network(card, keys, cipher, scopes, &Wipe, &mut view.saved, None, work)
}

/// Caller first displays the actual committing frame. This performs the
/// existing Core Save/Forget, clears the SAME password in every outcome and
/// publishes only Core's verified choice. Paint this result before invoking
/// check_after_save for the main9c Quick Start continuation.
pub fn change_network(view: &mut UpdateView<'_>, card: &mut dyn Backend, keys: &mut dyn KeyBackend,
    cipher: &mut dyn Cipher, ports: &mut (impl EntropyPorts + FlashFence), scopes: &dyn ScopedSecrets,
    work: &mut Work<'_>,
) -> WifiChangeReport {
    let report = if ports.flash_fenced_for_wifi() { WifiChangeReport::default() }
    else if view.forgetting { workflow::forget_wifi_network(card, keys, scopes, &Wipe, &mut view.saved, work) }
    else {
        let mut random = WifiRandom { entropy: &mut view.entropy, ports };
        workflow::save_wifi_network(card, keys, cipher, &mut random, scopes, &Wipe,
            CStr::from_bytes_until_nul(&view.joining.name).ok(), &view.password, &mut view.saved, work)
    };
    view.password.clear(&Wipe);
    view.commit = report.record;
    view.committing = false;
    report
}

/// Exact SDK teardown primitives, all attempted before restart. Ordinary
/// scan/join paths must separately prove deinitialization before any save.
pub trait ExitPorts {
    fn clear_station_config(&mut self) -> i32;
    fn stop_station(&mut self) -> i32;
    fn deinit_station(&mut self) -> i32;
    fn restart(&mut self, into: &'static str) -> !;
}
pub fn leave_update_mode(view: &mut UpdateView<'_>, ports: &mut impl ExitPorts, into: &'static str) -> ! {
    let _ = ports.clear_station_config();
    let _ = ports.stop_station();
    let _ = ports.deinit_station();
    view.clear_secrets(&Wipe);
    ports.restart(into)
}

/// The radio-time seed is the only entropy authority. There is no RNG
/// fallback before a scan. Root implements these same SDK scalar operations.
pub trait EntropyPorts {
    fn random_word(&mut self) -> u32;
    fn sha256(&mut self, input: &[u8], output: &mut [u8; 32]) -> bool;
}
pub struct WifiRandom<'a, P> { pub entropy: &'a mut update::WifiEntropy<SecretBytes<32>>, pub ports: &'a mut P }
impl<P: EntropyPorts> Random for WifiRandom<'_, P> {
    fn fill(&mut self, out: &mut [u8]) -> bool {
        if self.entropy.seed.size() == 0 { return false; }
        let mut input = [0; 40]; let mut block = [0; 32]; let mut ok = true; let mut at = 0;
        while ok && at < out.len() {
            input[..32].copy_from_slice(self.entropy.seed.bytes());
            input[32..36].copy_from_slice(&self.entropy.counter.to_le_bytes());
            self.entropy.counter = self.entropy.counter.wrapping_add(1);
            input[36..].copy_from_slice(&self.ports.random_word().to_le_bytes());
            ok = self.ports.sha256(&input, &mut block);
            if ok { let count = (out.len() - at).min(block.len()); out[at..at + count].copy_from_slice(&block[..count]); }
            at += block.len();
        }
        Wipe.wipe(&mut input); Wipe.wipe(&mut block);
        ok
    }
}
