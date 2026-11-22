// SPDX-License-Identifier: MIT
//! Existing Settings platform effects over the same reader and boot owners.
use crate::{app_image::{Events, Home, PressState, Reader}, dev_usb::PanelRing, device_lifecycle::Platform,
    firmware_switch, home_startup::FirmwareCard, sdk::Sdk552, serial_console::SerialConsole};
use super::display::DisplayReceipt;
use newt_c3_board::{Board, hardware::HardwareReader};
use newt_core::{app::{Action, AppState},
    frame::Frame, refresh::UiRefresh, save_notice::SaveOutcome};
use newt_device_ui::DeviceInfo;

pub(crate) struct DevicePlatform<'a, 'events, 'task, 'sdk, R> {
    pub reader: &'a mut R,
    pub info: &'a mut DeviceInfo<'static>,
    pub displayed: &'a mut DisplayReceipt,
    pub budget: &'a mut UiRefresh,
    pub switch: &'a mut firmware_switch::State,
    pub logical: &'a mut Frame,
    pub panels: Option<&'a mut PanelRing>,
    pub press: &'a mut PressState,
    pub events: &'a mut Events<'events, 'task>,
    pub hardware: &'a mut HardwareReader<'sdk>,
    #[cfg(feature = "dev-usb")] pub bridge: &'a mut crate::dev_usb::Bridge,
    #[cfg(feature = "dev-usb")] pub facts: super::publication::Facts,
}
impl<'sdk, R: Reader> Platform<'sdk, SerialConsole, Sdk552, FirmwareCard> for DevicePlatform<'_, '_, '_, 'sdk, R> {
    fn firmware_switch(&mut self, home: &mut Home<'sdk>, app: &mut AppState, action: Action) -> bool {
        let mut ports = SwitchPorts { home, reader: self.reader, displayed: self.displayed,
            budget: self.budget, logical: self.logical, panels: self.panels.as_deref_mut(), press: self.press,
            events: self.events, hardware: self.hardware,
            #[cfg(feature = "dev-usb")] bridge: self.bridge,
            #[cfg(feature = "dev-usb")] facts: self.facts };
        self.switch.dispatch(&mut ports, app, action, self.info)
    }
    fn update_entry(&mut self, home: &mut Home<'sdk>, app: &mut AppState, action: Action) -> bool {
        #[cfg(feature = "dev-usb")]
        let panels = match self.panels.as_deref() { Some(panels) => panels, None => crate::app_power::stop() };
        #[cfg(not(feature = "dev-usb"))]
        let _ = &self.hardware;
        let mut ports = super::restart::Exit { home, reader: self.reader, logical: self.logical,
            events: self.events, displayed: self.displayed,
            #[cfg(feature = "dev-usb")] hardware: self.hardware,
            #[cfg(feature = "dev-usb")] bridge: self.bridge,
            #[cfg(feature = "dev-usb")] panels,
            #[cfg(feature = "dev-usb")] facts: self.facts };
        match crate::update_entry::dispatch_update_entry(&mut ports, app, action) {
            Ok(handled) => handled, Err(_) => crate::app_power::stop(),
        }
    }
}
struct SwitchPorts<'a, 'events, 'task, 'sdk, R> {
    home: &'a mut Home<'sdk>,
    reader: &'a mut R,
    displayed: &'a mut DisplayReceipt,
    budget: &'a mut UiRefresh,
    logical: &'a mut Frame,
    panels: Option<&'a mut PanelRing>,
    press: &'a mut PressState,
    events: &'a mut Events<'events, 'task>,
    hardware: &'a mut HardwareReader<'sdk>,
    #[cfg(feature = "dev-usb")] bridge: &'a mut crate::dev_usb::Bridge,
    #[cfg(feature = "dev-usb")] facts: super::publication::Facts,
}
impl<'sdk, R: Reader> firmware_switch::Ports<'sdk> for SwitchPorts<'_, '_, '_, 'sdk, R> {
    fn board(&mut self) -> &mut Board<'sdk> { &mut self.home.bus.board }
    fn save(&mut self) -> SaveOutcome {
        let now = self.home.bus.board.millis();
        self.reader.save_exit(self.home, now, self.events.action)
    }
    fn show(&mut self, app: &AppState, info: &DeviceInfo<'_>) {
        let started = super::display::refresh_start(&mut self.home.bus.board);
        let composition = super::display::Composition::new(self.events.action);
        let bar = self.home.clock.read_system_bar(&mut self.home.bus, info, false);
        *self.press = PressState::Idle;
        newt_device_ui::render_device_ui(app, info, &bar, Some(self.logical), self.home.clock.zone(),
            None, None, crate::app_dev::build_identity(),
            self.home.clock.sleep_screen(), self.home.clock.test_builds());
        if self.displayed.binary(&mut self.home.bus.board, self.logical, self.budget, self.panels.as_deref_mut(), false, started, composition).is_err() {
            crate::app_power::stop();
        }
        self.reader.note_paint(app.screen, self.displayed.generation, true, self.home.bus.board.millis());
    }
    fn restart(&mut self) {
        #[cfg(feature = "dev-usb")]
        let panels = match self.panels.as_deref() { Some(panels) => panels, None => crate::app_power::stop() };
        #[cfg(not(feature = "dev-usb"))]
        let _ = &self.hardware;
        super::restart::Exit { home: self.home, reader: self.reader, logical: self.logical,
            events: self.events, displayed: self.displayed,
            #[cfg(feature = "dev-usb")] hardware: self.hardware,
            #[cfg(feature = "dev-usb")] bridge: self.bridge,
            #[cfg(feature = "dev-usb")] panels,
            #[cfg(feature = "dev-usb")] facts: self.facts }.restart("the other firmware")
    }
}
