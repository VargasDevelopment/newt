// SPDX-License-Identifier: MIT
//! Publish physical reader and sleep composition through W1's display receipt.
use super::*;
use crate::{app_image::{Application, Events, Home, PressState},
    reader_book::{self, BookStep}, app_runtime::display};
use newt_core::{app::Screen, frame::{self, Frame}, preferences::Orientation};
use newt_c3_board::{Error, Refresh, grayscale::{GrayEncoding, GrayMode, GrayPlane}};
use newt_device_ui::{HomeData, HomePlane, HomeStyle, SleepExtras};

impl Runtime {
    pub(super) fn paint_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut Frame, events: &mut Events<'_, '_>, force: bool,
    ) -> Result<bool, Error> {
        if ui.app.screen == Screen::Sleeping { return self.paint_sleep_image(home, ui, logical, events); }
        if !matches!(ui.app.screen, Screen::EpubReading | Screen::Reading) { return Ok(false); }
        let started = display::refresh_start(&mut home.bus.board);
        let composition = display::Composition::new(events.action);
        if self.memory.has_sample() {
            #[cfg(feature = "embedded-sample")]
            if let Err(status) = self.compose_sample_image(ui, logical, millis()) {
                self.open_failed(reader_book::BookFailure::Open(status));
                self.paint_unavailable(ui, logical);
            }
        } else if self.memory.is_closed() {
            self.paint_unavailable(ui, logical);
        } else { match self.operate_image(home, ui, logical, events, None, None, true, None, None) {
            Ok(exit) => {
                self.observe_operation(&exit);
                // compose_native wrote the logical image in SDK polarity.
                // Return it before rotating into that same SDK frame.
                for (out, byte) in logical.iter_mut().zip(home.bus.board.frame()?) { *out = !*byte; }
            }
            Err(error) => {
                self.open_failed(error);
                self.publish_notice_image(ui);
                self.paint_unavailable(ui, logical);
            }
        } }
        let orientation = self.settings.orientation;
        let full = force || !ui.displayed.valid || ui.displayed.grayscale
            || ui.displayed.orientation() != orientation
            || self.reading_refresh.needs_full(u32::from(self.settings.refresh_pages));
        let native = home.bus.board.frame()?.try_into().map_err(|_| Error::Length)?;
        frame::to_native_frame(logical, native, orientation);
        drop(composition);
        let inverse = ui.displayed.valid && ui.displayed.overlay;
        let check = self.picture.check(ui.displayed.generation, logical);
        #[cfg(feature = "dev-usb")]
        if check == newt_core::resume_picture::Check::Differs { self.trace_picture_miss_image(home); }
        let covered = check == newt_core::resume_picture::Check::Covered;
        if !covered {
            let _display = events.action.scope(crate::reader_action::ActionStage::BlockingDisplay);
            if inverse { display::drive_from_inverse(&mut home.bus.board)?; }
            panel_binary(home, ui, if full && !inverse { Refresh::Full } else { Refresh::Fast }, "reader")?;
        }
        ui.displayed.completed(&mut home.bus.board, orientation, false, false, started, events.control);
        if ui.press == PressState::Accepted {
            ui.press = if full || inverse { PressState::AnsweredCleanup } else { PressState::AnsweredFast };
        } else if !covered { self.reading_refresh.painted(full || inverse); }
        ui.home_refresh.invalidate();
        ui.home_cover.release();
        #[cfg(feature = "dev-usb")]
        ui.runtime_lows.painted(ui.app.screen);
        Ok(true)
    }

    fn paint_unavailable(&mut self, ui: &Application<'_>, logical: &mut Frame) {
        #[cfg(feature = "embedded-sample")]
        if self.memory.has_sample() {
            self.compose_sample_unavailable(ui, logical);
            return;
        }
        if let Some(memory) = self.memory.external_mut() {
            memory.lend(|loan|{
                let book = &*loan.runtime;
                newt_reader_ui::compose_reader_ui(&book.reader, &book.owner.state, &self.reader_ui,
                    self.settings, &ui.info, &self.saved, false, logical,
                    book.owner.chapter_failure(), book.owner.last_chapter_failure(), None);
            });
        } else {
            newt_reader_ui::compose_unavailable_reader_ui(&self.reader_ui, self.settings,
                &ui.info, &self.saved, logical, None);
        }
    }

    fn paint_sleep_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut Frame, events: &mut Events<'_, '_>,
    ) -> Result<bool, Error> {
        let extras = self.sleep_extras_image(home, ui);
        let started = display::refresh_start(&mut home.bus.board);
        let mut gray = false;
        if home.bus.board.gray_encoding(GrayMode::Direct)? == GrayEncoding::AbsolutePlanes
            && home.bus.board.gray_base(GrayMode::Direct, false, Refresh::Full, false)? {
            let book_painted = self.opened()
                && ui.app.resume_screen == Screen::EpubReading;
            self.sleep_plane(home, ui, logical, events, &extras, HomePlane::Lsb)?;
            let orientation = self.sleep_orientation(ui);
            frame::to_native_selectors(logical, home.bus.board.frame()?.try_into().map_err(|_| Error::Length)?, orientation);
            home.bus.board.gray_copy_frame(GrayPlane::Lsb)?;
            self.sleep_plane(home, ui, logical, events, &extras, HomePlane::Msb)?;
            if !book_painted || self.opened() {
                frame::to_native_selectors(logical, home.bus.board.frame()?.try_into().map_err(|_| Error::Length)?, orientation);
                home.bus.board.gray_copy_frame(GrayPlane::Msb)?;
                home.bus.board.gray_display()?;
                gray = true;
            }
        }
        if !gray {
            self.sleep_plane(home, ui, logical, events, &extras, HomePlane::Monochrome)?;
            let orientation = self.sleep_orientation(ui);
            frame::to_native_frame(logical, home.bus.board.frame()?.try_into().map_err(|_| Error::Length)?, orientation);
            panel_binary(home, ui, Refresh::Full, "sleep-fallback")?;
        }
        let orientation = self.sleep_orientation(ui);
        ui.displayed.completed(&mut home.bus.board, orientation, gray, false, started, events.control);
        ui.ui_refresh.painted(true);
        ui.home_refresh.invalidate();
        Ok(true)
    }

    fn sleep_orientation(&self, ui: &Application<'_>) -> Orientation {
        if ui.app.resume_screen == Screen::EpubReading && self.opened() {
            self.settings.orientation
        } else { Orientation::Portrait }
    }

    fn sleep_plane(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>, logical: &mut Frame,
        events: &mut Events<'_, '_>, extras: &SleepExtras, plane: HomePlane,
    ) -> Result<(), Error> {
        #[cfg(feature = "embedded-sample")]
        if ui.app.resume_screen == Screen::EpubReading && self.opened() && self.memory.has_sample() {
            match self.compose_sample_backdrop(ui, logical, millis()) {
                Ok(true) => {
                    let g = newt_core::preferences::geometry(self.settings);
                    if let Some(memory) = self.memory.sample_mut() {
                        memory.with_layout_scratch(|scratch|
                            newt_device_ui::frost_sleep_backdrop_in_place(logical, scratch, g, plane))
                            .map_err(|_| Error::Length)?;
                    }
                    let data = HomeData { clock: None, book_title: Some(self.title.as_slice()),
                        chapter_title: self.home.chapter_title, percent: self.home.percent,
                        reading_available: self.home.reading_available,
                        progress_unavailable: self.home.progress_unavailable,
                        fixture_cover: self.home.fixture_cover, flags: self.home.flags,
                        device_description: self.home.device_description };
                    let selector = plane != HomePlane::Monochrome;
                    newt_device_ui::render_sleep_ui(&ui.info, Some(logical), g, selector);
                    newt_device_ui::render_sleep_extras(extras, &data, Some(logical), g, selector);
                    if newt_device_ui::sleep_inverted(extras) { for byte in logical { *byte = !*byte; } }
                    return Ok(());
                }
                Ok(false) => {}
                Err(status) => self.open_failed(reader_book::BookFailure::Open(status)),
            }
        }
        if ui.app.resume_screen == Screen::EpubReading
            && self.opened() && !self.memory.has_sample() {
            match self.operate_image(home, ui, logical, events, None, Some(BookStep::Sleep(plane)), false, None, Some(extras)) {
                Ok(exit) => {
                    self.observe_operation(&exit);
                    for (out, byte) in logical.iter_mut().zip(home.bus.board.frame()?) { *out = !*byte; }
                    return Ok(());
                }
                Err(error) => self.open_failed(error),
            }
        }
        let marker = home.clock.begin_paint();
        let data = HomeData { clock: home.clock.shown(marker), book_title: Some(self.title.as_slice()),
            chapter_title: self.home.chapter_title, percent: self.home.percent,
            reading_available: self.home.reading_available, progress_unavailable: self.home.progress_unavailable,
            fixture_cover: self.home.fixture_cover, flags: self.home.flags,
            device_description: self.home.device_description };
        let mut state = ui.app;
        state.screen = Screen::Home;
        let g = newt_core::preferences::geometry(newt_core::preferences::ViewSettings {
            orientation: Orientation::Portrait, ..self.settings });
        newt_device_ui::render_home(&state, &newt_device_ui::HeaderInfo {
            battery_known: ui.info.battery_known, battery_percent: ui.info.battery_percent },
            &data, HomePlane::Monochrome, Some(logical), &HomeStyle::default(), None);
        match &mut self.memory {
            #[cfg(feature = "embedded-sample")]
            ReaderBacking::Sample(memory) => memory.with_layout_scratch(|scratch|
                newt_device_ui::frost_sleep_backdrop_in_place(logical, scratch, g, plane))
                .map_err(|_| Error::Length)?,
            ReaderBacking::External(memory) => memory.lend(|memory| memory.storage.with_frame_scratch(|scratch|
                newt_device_ui::frost_sleep_backdrop_in_place(logical, scratch, g, plane)))
                .map_err(|_| Error::Length)?.map_err(|_| Error::Length)?,
            ReaderBacking::Closed | ReaderBacking::Interrupted => {
                let target = home.bus.board.frame()?.try_into().map_err(|_| Error::Length)?;
                newt_system_ui::frost_sleep_backdrop(Some(logical), Some(target), g, plane);
                logical.copy_from_slice(home.bus.board.frame()?);
            }
        }
        let selector = plane != HomePlane::Monochrome;
        newt_device_ui::render_sleep_ui(&ui.info, Some(logical), g, selector);
        newt_device_ui::render_sleep_extras(extras, &data, Some(logical), g, selector);
        if newt_device_ui::sleep_inverted(extras) { for byte in logical { *byte = !*byte; } }
        Ok(())
    }

    fn sleep_extras_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>) -> SleepExtras {
        let choices = home.clock.sleep_screen();
        let zone = home.clock.zone();
        let mut utc = 0;
        let today = crate::stats_clock::read_stats_session_clock(&mut home.bus.board, zone, Some(&mut utc));
        ui.info.hat = newt_device_ui::HolidayHat::on(today);
        let mut extras = SleepExtras { progress: choices.progress, ..SleepExtras::default() };
        if choices.week || choices.habit {
            let now = millis();
            let _ = self.with_store(home, |this, store| {
                newt_core::reading_stats_service::read_sleep_stats(store, today,
                    newt_core::reading_stats_service::SleepScreenChoices { week: choices.week, habit: choices.habit },
                    |ledger| {
                        let Some(memory) = this.memory.external_mut() else { return 0; };
                        let verified = this.selection.active() && this.session.as_ref().is_none_or(|session|
                            session.verification.state() == crate::reader_verification::Verification::Verified);
                        memory.lend(|loan| {
                        let book = &*loan.runtime;
                        let stats = reader_book::stats_book(&this.checkpoint, this.selection.digest(), verified,
                            &book.owner, &book.reader);
                        newt_core::reading_stats_live::unsaved_stats_seconds(ledger, today, utc, now,
                            false, zone, stats.as_ref(), &book.owner.tracker, this.statistics.fold_owed)
                        })
                    }, &mut extras);
            });
        }
        extras
    }
}

fn panel_binary(home: &mut Home<'_>, ui: &mut Application<'_>, refresh: Refresh, reason: &str) -> Result<(), Error> {
    #[cfg(feature = "dev-usb")]
    let token = ui.panels.begin(home.bus.board.millis(), if matches!(refresh, Refresh::Full) { "full" } else { "fast" }, reason);
    let result = home.bus.board.update(refresh, false);
    #[cfg(feature = "dev-usb")]
    ui.panels.finish(token, home.bus.board.millis(), if result.is_ok() {
        crate::dev_usb::PanelOutcome::Accepted } else { crate::dev_usb::PanelOutcome::Rejected });
    #[cfg(not(feature = "dev-usb"))]
    let _ = (ui, reason);
    result
}
