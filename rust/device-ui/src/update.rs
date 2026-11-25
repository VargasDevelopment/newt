// SPDX-License-Identifier: MIT
//! Separate-boot update screen dispatch and rendering.
//! Offers, installer consent, network choices and sealed bytes stay in Core.
#![forbid(unsafe_code)]
use core::ffi::CStr;
use newt_core::{update_card::{OFFERS, UpdateOffer, SlotImage, FirmwareName},
    update_card::slot_installer::InstallIssue, update_online::OnlineCheck,
    wifi_network_store::{NAME_BYTES, PASSWORD_BYTES, workflow::{Secret, WifiChoice, WifiCommit}}};
mod dispatch;
mod keyboard;
mod paint;
mod status;
pub use dispatch::{add_wifi_network, check_after_save, check_focused, dispatch_update,
    focused_offer, open_networks, update_choice, update_consent};
pub use paint::render_update;
pub use status::{OnlineStatus, render_installing, render_online};
#[cfg(feature = "dev-usb")]
pub use status::card_step_line;

pub const NETWORK_ROWS: usize = 10;
pub const PASSWORD_MINIMUM: usize = 8;

/// The actual noncopyable Secret owner supplies its original editing methods.
/// Painting uses size only; password bytes never become display text.
pub trait Password: Secret<PASSWORD_BYTES> {
    fn append(&mut self, text: &[u8]) -> bool;
    fn erase(&mut self) -> bool;
}

pub struct WifiEntropy<S> { pub seed: S, pub counter: u32 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum UpdatePage { List, Confirm, Installing, Result, Networks, Password, Network, Join, Online }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WifiScan { Scanning, Done, Failed }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WifiJoin { Joining, Connected, WrongPassword, NotFound, NoAddress, TimedOut, Failed }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum UpdateChoice { None, Install, Confirm }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum UpdateIntent { None, Repaint, Install, Exit, Scan, Network, Join, Check }

#[derive(Clone, Copy)]
pub struct WifiNetwork { pub name: [u8; NAME_BYTES + 1], pub rssi: i8, pub open: bool }
impl Default for WifiNetwork {
    fn default() -> Self { Self { name: [0; NAME_BYTES + 1], rssi: 0, open: false } }
}

#[derive(Clone, Copy, Debug, Default)]
pub enum ReturnOffer {
    #[default] None,
    Offered(newt_core::official_return::ReturnAuthorization),
    Confirmed(newt_core::official_return::ConfirmedReturn),
}
impl ReturnOffer {
    pub fn authorization(self) -> Option<newt_core::official_return::ReturnAuthorization> {
        match self { Self::None => None, Self::Offered(value) => Some(value), Self::Confirmed(value) => Some(value.authorization()) }
    }
    fn confirm(&mut self) {
        if let Self::Offered(value) = *self { *self = Self::Confirmed(value.confirm()); }
    }
    fn cancel_confirmation(&mut self) {
        if let Self::Confirmed(value) = *self { *self = Self::Offered(value.authorization()); }
    }
}

/// One actual update-boot owner. P and S are the existing required Secret
/// capabilities, occupying their original fields, never copied or formatted.
/// The runtime must construct these owners in its native-wiping scope before
/// passing the view to any operation; reading boots construct none of this.
pub struct UpdateView<'a, P, S> {
    pub page: UpdatePage,
    pub offers: [UpdateOffer; OFFERS],
    pub more_files: bool,
    pub focus: u8,
    pub running: &'a CStr,
    pub target: SlotImage,
    pub official_return: ReturnOffer,
    pub target_name: FirmwareName,
    pub card_ready: bool,
    pub rolled_back: bool,
    pub checking: bool,
    pub percent: u8,
    pub issue: InstallIssue,
    pub scan: WifiScan,
    pub network_count: u8,
    pub networks: [WifiNetwork; NETWORK_ROWS],
    pub network_focus: u8,
    pub entropy: WifiEntropy<S>,
    pub saved: WifiChoice,
    pub joining: WifiNetwork,
    pub password: P,
    pub key_row: u8,
    pub key_column: u8,
    pub shifted: bool,
    pub symbols: bool,
    pub forgetting: bool,
    pub committing: bool,
    pub commit: WifiCommit,
    pub join: WifiJoin,
    pub online: OnlineCheck,
    #[cfg(feature = "dev-usb")]
    pub card_failure: CardFailure,
    pub password_on_panel: bool,
    pub quick_start: bool,
}
impl<'a, P: Password, S: Secret<32>> UpdateView<'a, P, S> {
    /// Only fresh empty owners may cross this constructor. Root places the
    /// whole view in its no-move scope before any fill/append/mix; afterwards
    /// pages and scalars change in place and the live view is never reassigned.
    pub fn new(running: &'a CStr, empty_password: P, empty_seed: S) -> Self {
        Self { page: UpdatePage::List, offers: [UpdateOffer::default(); OFFERS], more_files: false, focus: 0,
            running, target: SlotImage::Empty, official_return: ReturnOffer::None, target_name: FirmwareName::Unknown,
            card_ready: false, rolled_back: false, checking: false, percent: 0, issue: InstallIssue::None,
            scan: WifiScan::Scanning, network_count: 0, networks: [WifiNetwork::default(); NETWORK_ROWS],
            network_focus: 0, entropy: WifiEntropy { seed: empty_seed, counter: 0 }, saved: WifiChoice::default(),
            joining: WifiNetwork::default(), password: empty_password, key_row: 0, key_column: 0, shifted: false, symbols: false,
            forgetting: false, committing: false, commit: WifiCommit::NotSaved,
            join: WifiJoin::Joining, online: OnlineCheck::Joining,
            #[cfg(feature = "dev-usb")]
            card_failure: CardFailure::default(),
            password_on_panel: false, quick_start: false }
    }
    pub fn clear_secrets(&mut self, wipe: &dyn newt_core::wifi_network_store::workflow::NativeWipe) {
        self.password.clear(wipe);
        self.entropy.seed.clear(wipe);
        self.entropy.counter = 0;
    }
}

pub fn update_frame_private<P, S>(view: &UpdateView<'_, P, S>) -> bool {
    matches!(view.page, UpdatePage::Networks | UpdatePage::Password)
}

#[derive(Clone, Copy)]
pub struct UpdateBar { pub left: i32, pub top: i32, pub width: i32, pub height: i32, pub filled: i32 }
pub fn update_bar(percent: u32) -> UpdateBar {
    UpdateBar { left: newt_system_ui::SYSTEM_CONTENT_LEFT, top: 214,
        width: newt_system_ui::SYSTEM_CONTENT_WIDTH, height: 18,
        filled: newt_system_ui::SYSTEM_CONTENT_WIDTH * percent.min(100) as i32 / 100 }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum CardStep { #[default] None, MakeFolder, RemoveManifest, OpenImage, WriteImage,
    SaveImage, CloseImage, OpenManifest, WriteManifest, SaveManifest, CloseManifest }
#[cfg(feature = "dev-usb")]
#[derive(Clone, Copy, Default)]
pub struct CardFailure { pub step: CardStep, pub error: i32, pub at: u32, pub tries: u8 }
