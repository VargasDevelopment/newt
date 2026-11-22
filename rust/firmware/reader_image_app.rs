// SPDX-License-Identifier: MIT
//! Application view of the actual persistent reader; physical operations stay
//! in the sibling helpers and every observation borrows the existing owners.
#![forbid(unsafe_code)]
use super::{ReaderBacking, Runtime};
use crate::{
    app_image::{Application, Events, Home, HomeView, Reader, ReaderFacts},
    dev_usb::Diagnostics,
    reader_verification::Verification,
    rest_app::Work,
    retained_runtime::record::{BootSnapshot, SleepRecord},
};
use newt_c3_board::Error;
use newt_core::{
    app::{Action, Screen},
    frame::Frame,
    preferences::ViewSettings,
    save_notice::SaveOutcome,
    selection_authority::flag,
    zip_container::Status,
};

impl Reader for Runtime {
    fn boot(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>, boot: &BootSnapshot,
        _logical: &mut Frame, trace: crate::reader_action::Trace<'_>,
    ) -> bool {
        self.boot_restore(home, ui, boot, trace)
    }

    fn open_boot(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>, logical: &mut Frame,
        events: &mut Events<'_, '_>,
    ) {
        self.open_boot_image(home, ui, logical, events);
    }

    fn work(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>, work: Work,
        logical: &mut Frame, events: &mut Events<'_, '_>,
    ) -> bool {
        self.work_image(home, ui, work, logical, events)
    }

    fn paint(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>, logical: &mut Frame,
        events: &mut Events<'_, '_>, force: bool,
    ) -> Result<bool, Error> {
        self.paint_image(home, ui, logical, events, force)
    }

    fn visit_home(&mut self, paint: &mut dyn FnMut(HomeView<'_>)) {
        let flags = self.selection.flags();
        self.home.set_card(newt_stage_home::card_flags(flags & flag::CARD_MISSING != 0,
            flags & flag::SAMPLE_CHOSEN != 0, self.selection.active()));
        self.home.flags = self.saves.project(self.home.flags).flags;
        self.home.reading_available = self.progress_ready();
        let verified = self.selection.selection_verified(&mut || match self.session.as_ref() {
            // Frozen sourceVerified() explicitly accepts the absent session;
            // an existing session must attest its actual bound source.
            None => true,
            Some(session) => session.verification.state() == Verification::Verified
                && self.source.as_ref().is_some_and(|source| source.bound()),
        });
        let title = if !self.selection.active() { Some(newt_stage_home::SAMPLE_TITLE) }
            else if self.title[0] == 0 { Some(b"Your SD book".as_slice()) }
            else { Some(self.title.as_slice()) };
        paint(HomeView {
            data: newt_stage_home::HomeData {
                clock: None, book_title: title, chapter_title: self.home.chapter_title,
                percent: self.home.percent, reading_available: self.home.reading_available,
                progress_unavailable: self.home.progress_unavailable, fixture_cover: self.home.fixture_cover,
                flags: self.home.flags, device_description: self.home.device_description,
            },
            identity: self.selection.active().then(|| self.selection.identity()),
            cover_eligible: self.selection.active() && verified
                && self.home.flags & newt_stage_home::CARD_FLAGS == 0,
            cover_generation: self.cover_generation(),
        });
    }

    fn facts(&self) -> ReaderFacts {
        let (opened, spine, character, book) = match self.reading() {
            Some(reading) => {
                let page = reading.page;
                // DEV uses the active selection's identity, as UsbDevice;
                // an unselected owner retains its own actual progress identity.
                let identity = if !reading.has_progress { 0 }
                    else if self.selection.active() { self.selection.book_id() }
                    else { reading.tracker.snapshot(0).book };
                (reading.opened, page.spine, page.char_start, identity)
            }
            // Frozen pageInfo() without either storage owner is the empty
            // page, while the distinct actual status scalar remains intact.
            None => (false, 0, 0, 0),
        };
        ReaderFacts { status: self.status() as u32, opened, progress_ready: self.progress_ready(),
            spine, character, book, reopen_owed: self.reopen_owed() }
    }

    fn diagnostics(&self, out: &mut Diagnostics) {
        out.available = self.wake.available;
        out.reset = self.wake.reset;
        out.cause = self.wake.cause;
        out.record = u32::from(self.wake.record);
        out.requested = u32::from(self.wake.requested);
        out.restored = self.wake.restored;
        out.checkpoint_saved = self.wake.checkpoint_saved;
        out.book = self.wake.book;
        out.spine = self.wake.spine;
        out.character = self.wake.character;
        out.first_input = u32::from(self.wake.first_physical.action);
        let facts = self.facts();
        out.reader_status = facts.status;
        out.reader_open = facts.opened;
        out.progress_ready = facts.progress_ready;
        out.actual_spine = facts.spine;
        out.actual_character = facts.character;
    }

    fn idle(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>, logical: &mut Frame,
        events: &mut Events<'_, '_>,
    ) -> (bool, u32) {
        let mut delay = if ui.app.screen == Screen::EpubReading { 10 } else { 1 };
        let repaint = self.idle_image(home, ui, logical, events, &mut delay);
        (repaint, delay)
    }

    fn save_exit(&mut self, home: &mut Home<'_>, now: u32, trace: crate::reader_action::Trace<'_>) -> SaveOutcome {
        Runtime::save_exit(self, home, now, trace)
    }

    fn close_source(&mut self, home: &mut Home<'_>) -> Result<(), Error> {
        self.close_runtime(home)
    }

    fn preferences_result(&mut self, saved: bool, settings: ViewSettings) {
        self.saves.set_reader_preference_unavailable(!saved);
        if !saved { return; }
        self.settings = settings;
        self.reader_ui.draft = settings;
        self.standalone_status = Status::NotFound;
        self.restore_failure = newt_reader_ui::RestoreFailure::None;
        if let Some(memory) = self.memory.external_mut() {
            memory.lend(|memory| {
                let runtime = &mut *memory.runtime;
                runtime.owner.settings_committed(&mut runtime.reader, settings);
            });
        }
        self.memory = ReaderBacking::Closed;
    }

    fn same_books(&mut self, io: &newt_native_io::UiIo, kept: &[u8], copy: &[u8]) -> bool {
        super::same_books(io, kept, copy)
    }

    fn purge_cache(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>) -> bool {
        self.purge_cache_image(home, ui)
    }

    fn settle_press(&mut self, cleanup: bool) { self.reading_refresh.painted(cleanup); }

    fn finish_sleep(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>, logical: &mut Frame,
        events: &mut Events<'_, '_>, now: u32,
    ) -> bool {
        self.finish_sleep_image(home, ui, logical, events, now)
    }

    fn prepare_sleep(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>, logical: &mut Frame,
        events: &mut Events<'_, '_>, now: u32,
    ) -> SleepRecord {
        Runtime::prepare_sleep(self, home, ui, logical, events, now)
    }

    fn cancel_sleep(&mut self, home: &mut Home<'_>, now: u32) { Runtime::cancel_sleep(self, home, now); }

    fn note_input(&mut self, now: u32) { self.note_idle_input(now); }

    fn note_paint(&mut self, screen: Screen, generation: u32, includes_cover: bool, now: u32) {
        self.note_idle_paint(screen, generation, includes_cover, now);
    }

    #[cfg(feature = "dev-usb")]
    fn consumed_input(&mut self, physical: bool, button: u8, action: Action, now: u32,
        before: Screen, after: Screen, reader_was_open: bool,
    ) {
        use crate::retained_runtime::wake::InputSource;
        self.wake.consumed_input(if physical { InputSource::Physical } else { InputSource::Usb },
            button, action, now, before, after, reader_was_open);
    }

    #[cfg(feature = "dev-usb")]
    fn completed_paint(&mut self, screen: Screen, generation: u32) {
        self.wake.completed_paint(screen, generation);
    }
}

impl Runtime {
    /// All actual working-frame call sites share one notification boundary.
    pub(super) fn acknowledge_work_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut Frame, work: newt_reader_ui::SlowWork, trace: crate::reader_action::Trace<'_>,
    ) -> bool {
        let painted = match ui.acknowledge_reader_work(home, work, self.settings, logical, trace) {
            Ok(painted) => painted,
            Err(_) => crate::app_power::stop(),
        };
        if painted {
            self.note_idle_paint(ui.app.screen, ui.displayed.generation, false, home.bus.board.millis());
            #[cfg(feature = "dev-usb")]
            self.wake.completed_paint(ui.app.screen, ui.displayed.generation);
        }
        painted
    }
}
