// SPDX-License-Identifier: MIT
//! Update-mode entry guard and reboot request handoff. Product callers lend the actual
//! reader exit and restart ports to the separate update boot.
#![forbid(unsafe_code)]
use newt_c3_board::retained::RetainedError;
use newt_core::{app::{self, Action, AppState, DevicePage, Screen, SettingsNotice, SettingsRow},
    update_mode_request::UpdateModeRequest};
use newt_core::save_notice::SaveOutcome;

/// Actual reader exit, existing retained-word request and nonreturning restart.
/// No default implementation and no durable/selection/controller owner here.
pub trait Ports {
    fn save(&mut self) -> SaveOutcome;
    fn request(&mut self, page: UpdateModeRequest) -> Result<(), RetainedError>;
    fn restart(&mut self, into: &'static str) -> !;
}
pub fn enter_update_mode(ports: &mut impl Ports, page: UpdateModeRequest,
    into: &'static str,
) -> Result<bool, RetainedError> {
    #[cfg(feature = "wifi-update")]
    {
        if ports.save() == SaveOutcome::Unconfirmed { return Ok(false); }
        ports.request(page)?;
        ports.restart(into)
    }
    #[cfg(not(feature = "wifi-update"))]
    {
        // Explicit reading-only bench cut: do not save/request/restart into
        // an update mode which this image does not contain.
        let _ = (ports, page, into);
        Ok(false)
    }
}
pub fn dispatch_update_entry(ports: &mut impl Ports, app: &mut AppState, action: Action) -> Result<bool, RetainedError> {
    let wifi = app.settings_row == SettingsRow::Wifi;
    if app.screen != Screen::Device || app.zone_list_open || action != Action::Activate
        || app::device_page(app) != DevicePage::Settings || (app.settings_row != SettingsRow::Update && !wifi) {
        return Ok(false);
    }
    if !enter_update_mode(ports, if wifi { UpdateModeRequest::Wifi } else { UpdateModeRequest::Update }, "update mode")? {
        app.settings_notice = if wifi { SettingsNotice::WifiNotStarted } else { SettingsNotice::UpdateNotStarted };
    }
    Ok(true)
}
