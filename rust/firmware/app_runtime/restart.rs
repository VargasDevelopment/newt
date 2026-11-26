// SPDX-License-Identifier: MIT
//! Frozen firmwareSwitch.restart shared by switch and update entry. This is
//! reader exit/close, never the sleep checkpoint/arm-wake policy.
#![forbid(unsafe_code)]
use crate::{app_image::{Events, Home, Reader}, update_entry};
use super::display::DisplayReceipt;
use core::fmt::Write;
use newt_core::{frame::Frame, save_notice::SaveOutcome, update_mode_request::UpdateModeRequest};
#[cfg(feature = "dev-usb")]
use super::publication::{Facts, Published};

pub struct Exit<'a, 'events, 'task, 'sdk, R> {
    pub home: &'a mut Home<'sdk>,
    pub reader: &'a mut R,
    pub logical: &'a Frame,
    pub events: &'a mut Events<'events, 'task>,
    pub displayed: &'a DisplayReceipt,
    #[cfg(feature = "dev-usb")] pub facts: Facts,
    #[cfg(feature = "dev-usb")] pub hardware: &'a mut newt_c3_board::hardware::HardwareReader<'sdk>,
    #[cfg(feature = "dev-usb")] pub bridge: &'a mut crate::dev_usb::Bridge,
    #[cfg(feature = "dev-usb")] pub panels: &'a crate::dev_usb::PanelRing,
}
impl<R: Reader> Exit<'_, '_, '_, '_, R> {
    pub fn restart(&mut self, into: &'static str) -> ! {
        #[cfg(feature = "dev-usb")]
        {
            let started = self.home.bus.board.millis();
            // A fresh watch: this drain lasts no longer than its 250 ms link
            // window, and the restart drops the reader's totals anyway.
            let mut link = newt_core::dev_link::LinkWatch::default();
            let mut published = Published { facts: self.facts, displayed: self.displayed,
                hardware: self.hardware, bridge: self.bridge, link: &mut link, panels: self.panels, writes: false };
            while (published.bridge.output_pending() || published.bridge.request_pending())
                && self.home.bus.board.millis().wrapping_sub(started) < 250 {
                if published.poll(self.home, self.reader, self.logical, self.events).is_err() { crate::app_power::stop(); }
                crate::app_power::delay_ms(1);
            }
        }
        #[cfg(not(feature = "dev-usb"))]
        let _ = (&self.events, self.logical, self.displayed);
        if self.home.bus.board.with_retained(|mut words| {
            words.clear_sleep_magic(); words.clear_sd_resume_magic();
        }).is_err() { crate::app_power::stop(); }
        if self.reader.close_source(self.home).is_err() { crate::app_power::stop(); }
        if self.home.bus.board.panel_sleep().is_err() { crate::app_power::stop(); }
        let _ = writeln!(self.home.bus.log, "Newt restarting into {}", into);
        self.home.bus.log.flush();
        self.home.bus.board.restart();
        crate::app_power::stop()
    }
}
impl<R: Reader> update_entry::Ports for Exit<'_, '_, '_, '_, R> {
    fn save(&mut self) -> SaveOutcome {
        let now = self.home.bus.board.millis();
        self.reader.save_exit(self.home, now, self.events.action)
    }
    fn request(&mut self, page: UpdateModeRequest) -> Result<(), newt_c3_board::retained::RetainedError> {
        crate::update_boot::request(&mut self.home.bus.board, page)
    }
    fn restart(&mut self, into: &'static str) -> ! { Exit::restart(self, into) }
}
