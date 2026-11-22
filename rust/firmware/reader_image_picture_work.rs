// SPDX-License-Identifier: MIT
//! The held press borrows the same clock/panel between source operations.
use crate::{app_image::{Application, Home, PressState}, app_runtime::display,
    home_startup::{ReaderScope, FirmwareCard}, reader_action::Trace, sdk::Sdk552,
    serial_console::SerialConsole};
use newt_core::frame::Frame;
use newt_reader_ui::{SlowWork, ViewSettings};

// The source's logical-frame loan is reborrowed only while no source call runs.
pub(super) fn answer_slow(scope: &mut ReaderScope<'_, '_, '_, '_, SerialConsole, Sdk552, FirmwareCard>,
    ui: &mut Application<'_>, logical: &mut Frame, settings: ViewSettings, key: &str,
    control: &newt_input_policy::UiControl<'_>, trace: Trace<'_>,
    idle: &mut super::cover_idle::CoverIdleState, wake: &mut newt_wake_evidence::Evidence,
) {
    if ui.press != PressState::Deferred { return; }
    ui.press = PressState::Accepted;
    let work = if super::hints::receipt_present_on(scope.io, key) { SlowWork::Opening } else { SlowWork::FirstOpen };
    let started = scope.bus.panel.millis();
    ui.home_refresh.invalidate();
    ui.home_cover.release();
    let bar = if matches!(work, SlowWork::FirstOpen) {
        scope.clock.read_system_bar(&mut scope.bus, &ui.info, false)
    } else {
        newt_system_ui::SystemBar::new(None, scope.clock.format(), ui.info.battery_known, ui.info.battery_percent)
    };
    #[cfg(feature = "dev-usb")]
    let panels = Some(&mut *ui.panels);
    #[cfg(not(feature = "dev-usb"))]
    let panels = None;
    let cleanup = match display::working_panel(scope.bus.panel, &mut ui.displayed, work,
        &bar, settings, logical, panels, started, trace) {
        Ok(cleanup) => cleanup, Err(_) => crate::app_power::stop(),
    };
    ui.press = if cleanup { PressState::AnsweredCleanup } else { PressState::AnsweredFast };
    let now = scope.bus.panel.millis();
    idle.painted(ui.app.screen, false, now);
    #[cfg(feature = "dev-usb")]
    wake.completed_paint(ui.app.screen, ui.displayed.generation);
    #[cfg(not(feature = "dev-usb"))]
    let _ = wake;
    control.restart(now);
}

impl super::Runtime {
    pub(super) fn answer_picture_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut Frame, control: &newt_input_policy::UiControl<'_>, trace: Trace<'_>,
    ) {
        if ui.press != PressState::Deferred { return; }
        ui.press = PressState::Accepted;
        let shown = match self.show_picture_image(home, ui, logical, trace) {
            Ok(shown) => shown, Err(_) => crate::app_power::stop(),
        };
        if !shown {
            let name = self.selection.identity().filename;
            let present = core::ffi::CStr::from_bytes_until_nul(&name).ok()
                .is_some_and(|name| self.receipt_present_image(home, name));
            self.acknowledge_work_image(home, ui, logical,
                if self.selection.active() && !present { SlowWork::FirstOpen } else { SlowWork::Opening }, trace);
        }
        control.restart(home.bus.board.millis());
    }
}
