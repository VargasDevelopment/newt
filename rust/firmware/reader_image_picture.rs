// SPDX-License-Identifier: MIT
//! Frozen resume-picture publication over the existing frame and reader owners.
#![forbid(unsafe_code)]
use super::{Runtime, picture_native::PictureIo, picture_slot::Slot};
use crate::{app_image::{Application, Events, Home, PressState}, app_runtime::display,
    home_startup::CardPort, reader_action::{ActionStage, Trace}, reader_pages::ReaderPages,
    reader_verification::Verification};
use newt_c3_board::{Error as BoardError, Refresh};
use newt_core::{app::Screen, frame::{self, Frame}, resume_picture::{Binding, Footer,
    FooterView, Load, Record, Save, FILE_BYTES}, zip_container::Status};
use newt_native_storage::{RouteState, chapter_cache::ChapterCacheStorage};
use newt_phased_reader::CachePresence;

impl Runtime {
    fn picture_binding_image(&self) -> Option<Binding> {
        let saved = self.recovery.as_ref().map_or(&self.saved, |pending| pending.progress());
        if !self.selection.active() || saved.book != self.selection.book_id() { return None; }
        Some(Binding { book: *self.selection.digest(), anchor: saved.locator,
            settings: self.recovery.as_ref().map_or(self.settings, |pending| pending.settings) })
    }

    #[inline(never)]
    pub(super) fn picture_candidate_image(&mut self, home: &mut Home<'_>,
        ui: &mut Application<'_>, owned: bool, session_checkpoint: bool,
    ) -> bool {
        if ui.press == PressState::Deferred { ui.press = PressState::Accepted; }
        if !owned || !home.bus.board.display_ready() || ui.press != PressState::Accepted
            || session_checkpoint || ui.app.screen != Screen::EpubReading { return false; }
        let Some(binding) = self.picture_binding_image() else { return false; };
        let mut record = Record::default();
        let matches = home.with_storage_io(|_, io| {
            let mut native = PictureIo::new(io);
            Slot::new(&mut native, newt_native_io::cover::rename_replaces_destination())
                .header(&mut record) == Load::Shown && record.binding == binding
        }).unwrap_or(false);
        if !matches || !self.picture_source_hints_match(home) { return false; }
        ui.press = PressState::Deferred;
        true
    }

    // Keep pathname and hint-record scratch out of the header/open stack frame.
    #[inline(never)]
    fn picture_source_hints_match(&self, home: &mut Home<'_>) -> bool {
        let filename = &self.selection.identity().filename;
        let Some(name) = newt_core::library_name::name_bytes(filename)
            .and_then(|name| core::str::from_utf8(name).ok()) else { return false; };
        let mut path = [0; b"/newt-sd/Newt/Books".len() + 2 + newt_core::library_name::FILENAME_BYTES];
        if !newt_library_current::library_book_path(Some(b"/newt-sd/Newt/Books\0"),
            Some(filename), Some(&mut path)) { return false; }
        let Some(end) = path.iter().position(|&byte| byte == 0) else { return false; };
        let Ok(path) = core::str::from_utf8(&path[..end]) else { return false; };
        home.with_storage_io(|home, io| {
            let mut native = home.card.port(io);
            let current = super::hints::current(&mut native, path);
            let mut key = [0; 64];
            let mut hash = crate::reader_hash::PackageHasher::from_live_sdk();
            let Some(key) = crate::reader_book::route_key(name, &mut hash, &mut key) else { return false; };
            let route = RouteState::new(crate::reader_book::CACHE_ROOT, key);
            let route = route.lend(&super::boot::NoCancel, super::millis);
            super::hints::matching(&mut native, &route, self.selection.digest(), current)
        }).unwrap_or(false)
    }

    #[inline(never)]
    pub(super) fn show_picture_image(&mut self, home: &mut Home<'_>,
        ui: &mut Application<'_>, logical: &mut Frame, control: &newt_input_policy::UiControl<'_>, trace: Trace<'_>,
    ) -> Result<bool, BoardError> {
        if ui.press != PressState::Accepted || ui.app.screen != Screen::EpubReading { return Ok(false); }
        let Some(binding) = self.picture_binding_image() else { return Ok(false); };
        let started = display::refresh_start(&mut home.bus.board);
        let mut footer = Footer::default();
        let loaded = home.with_storage_io(|_, io| {
            let mut native = PictureIo::new(io);
            Slot::new(&mut native, newt_native_io::cover::rename_replaces_destination())
                .load(&binding, logical, &mut footer)
        }).unwrap_or(Load::Unreadable);
        if loaded != Load::Shown {
            trace_picture(home, trace, loaded.name(), false, 0);
            return Ok(false);
        }
        // The slot closes before the actual session's Pages presence loan;
        // that owner's descriptor/lease survives for the ordinary open.
        if !self.picture_publishes_before_verification(home, &binding) {
            trace_picture(home, trace, "unverified", false, 0);
            return Ok(false);
        }
        newt_reader_ui::render_reader_chrome(&newt_reader_ui::ReaderFooter {
            spine: footer.spine, page: footer.page, pages: footer.pages,
            overall_known: footer.overall_known, overall_percent: footer.overall_percent,
            section_label: Some(&footer.label),
        }, &ui.info, binding.settings, true, Some(logical));
        ui.home_refresh.invalidate();
        ui.home_cover.release();
        let orientation = binding.settings.orientation;
        let inverse = ui.displayed.valid && ui.displayed.overlay;
        let full = !ui.displayed.valid || ui.displayed.grayscale
            || orientation != ui.displayed.orientation()
            || self.reading_refresh.needs_full(u32::from(binding.settings.refresh_pages));
        frame::to_native_frame(logical,
            home.bus.board.frame()?.try_into().map_err(|_| BoardError::Length)?, orientation);
        {
            let _action = trace.scope(ActionStage::BlockingDisplay);
            if inverse { display::drive_from_inverse(&mut home.bus.board)?; }
            #[cfg(feature = "dev-usb")]
            let panels = Some(&mut *ui.panels);
            #[cfg(not(feature = "dev-usb"))]
            let panels = None;
            display::blocking_binary(&mut home.bus.board, panels,
                if full && !inverse { Refresh::Full } else { Refresh::Fast }, "resume-picture")?;
        }
        ui.displayed.completed(&mut home.bus.board, orientation, false, false, started, control);
        self.picture.shown(ui.displayed.generation, logical);
        ui.press = if full || inverse { PressState::AnsweredCleanup } else { PressState::AnsweredFast };
        self.note_idle_paint(ui.app.screen, ui.displayed.generation, false, home.bus.board.millis());
        #[cfg(feature = "dev-usb")]
        {
            ui.runtime_lows.painted(ui.app.screen);
            self.wake.completed_paint(ui.app.screen, ui.displayed.generation);
            trace_picture(home, trace, "shown", full || inverse, ui.displayed.refresh_ms);
        }
        Ok(true)
    }

    #[inline(never)]
    fn picture_publishes_before_verification(&mut self, home: &mut Home<'_>, binding: &Binding) -> bool {
        let Some(session) = self.session.as_mut() else { return false; };
        let session = &mut **session;
        match session.verification.state() {
            Verification::Verified => return true,
            Verification::Pending => {},
            Verification::Changed | Verification::Failed => return false,
        }
        home.with_storage_io(|home, io| {
            let route = session.route.lend(&super::boot::NoCancel, super::millis);
            let mut native = home.card.port(io);
            let Ok(storage) = ChapterCacheStorage::lend(&mut session.pages, &mut native, &route)
                else { return false; };
            let mut pages = ReaderPages(storage);
            let mut hash = crate::reader_hash::PackageHasher::from_live_sdk();
            newt_phased_reader::cached(binding.settings, &binding.book, binding.anchor.spine,
                &mut hash, &mut |name| pages.present(name, true))
        }).unwrap_or(false)
    }

    #[inline(never)]
    pub(super) fn save_picture_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut Frame, events: &mut Events<'_, '_>,
    ) {
        if !self.saves.checkpoint_saved() || !self.opened() { return; }
        let Some(binding) = self.picture_binding_image() else { return; };
        let mut record = Record { binding, ..Record::default() };
        let Some(book) = self.book() else { return; };
        let page = book.owner.state.position_info(book.reader.info(), book.reader.selection().spines).page;
        let section = book.reader.current_section();
        if !record.footer.assign(FooterView { spine: section.spine, page: page.page,
            pages: page.page_count, overall_known: page.overall_known,
            overall_percent: page.overall_percent, section_label: Some(section.label) }) { return; }
        let started = home.bus.board.millis();
        // A stale picture's binding and footer still match; only its pixels changed.
        let holds = !self.picture.stale() && home.with_storage_io(|_, io| {
            let mut native = PictureIo::new(io);
            Slot::new(&mut native, newt_native_io::cover::rename_replaces_destination())
                .holds(&record.binding, &record.footer)
        }).unwrap_or(false);
        let result = if holds { Save::Same }
        else if self.compose_picture_image(home, ui, logical, events) {
            home.with_storage_io(|_, io| {
                let mut native = PictureIo::new(io);
                Slot::new(&mut native, newt_native_io::cover::rename_replaces_destination())
                    .write(&mut record, logical)
            }).unwrap_or(Save::Failed)
        } else { Save::Failed };
        if result == Save::Written { self.picture.written(); }
        #[cfg(feature = "dev-usb")]
        let _ = crate::app_runtime::trace::row(&home.bus.log, 96, format_args!(
            "Reader picture save: result={} ms={} bytes={}\n", result as u8,
            home.bus.board.millis().wrapping_sub(started),
            if result == Save::Written { FILE_BYTES } else { 0 }));
        #[cfg(not(feature = "dev-usb"))]
        let _ = (started, result, FILE_BYTES);
    }

    // A changed ready frame is only a miss on the cached picture's own page.
    #[cfg(feature = "dev-usb")]
    #[inline(never)]
    pub(super) fn trace_picture_miss_image(&self, home: &mut Home<'_>) {
        let Some(book) = self.book().filter(|book| book.owner.state.opened()) else { return; };
        let page = book.owner.state.position_info(book.reader.info(), book.reader.selection().spines).page;
        let mut record = Record::default();
        let matches = home.with_storage_io(|_, io| {
            let mut native = PictureIo::new(io);
            Slot::new(&mut native, newt_native_io::cover::rename_replaces_destination())
                .header(&mut record) == Load::Shown
                && record.footer.spine == page.spine && record.footer.page == page.page
        }).unwrap_or(false);
        if matches {
            let _ = crate::app_runtime::trace::row(&home.bus.log, 96, format_args!(
                "Reader picture miss: spine={} page={}\n", page.spine, page.page));
        }
    }

    #[inline(never)]
    fn compose_picture_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut Frame, events: &mut Events<'_, '_>,
    ) -> bool {
        let previous = core::mem::take(&mut self.reader_ui);
        let previous_result = self.checkpoint.result();
        let bare = BareReaderUi { runtime: self, previous, previous_result };
        if bare.runtime.operate_image(home, ui, logical, events, None, None, true, None, None).is_err()
            || bare.runtime.status() != Status::Ok { return false; }
        let Ok(native) = home.bus.board.frame() else { return false; };
        if native.len() != logical.len() { return false; }
        for (out, byte) in logical.iter_mut().zip(native) { *out = !*byte; }
        true
    }
}

/// The normal compositor temporarily sees the next open's bare page state.
/// Restore the live menu/notice state on every exit, including host unwind.
struct BareReaderUi<'a> {
    runtime: &'a mut Runtime,
    previous: newt_reader_ui::ReaderUiState,
    previous_result: crate::reader_book::checkpoint::CheckpointResult,
}
impl Drop for BareReaderUi<'_> {
    fn drop(&mut self) {
        self.runtime.reader_ui = self.previous;
        self.runtime.checkpoint.observe(self.previous_result);
    }
}

#[inline(never)]
fn trace_picture(home: &mut Home<'_>, trace: Trace<'_>, result: &str, cleanup: bool, refresh_ms: u32) {
    #[cfg(feature = "dev-usb")]
    if let Some(elapsed) = trace.since(home.bus.board.micros()) {
        let _ = crate::app_runtime::trace::row(&home.bus.log, 96, format_args!(
            "Reader picture: result={} us={} refresh_ms={} cleanup={}\n",
            result, elapsed, refresh_ms, u8::from(cleanup)));
    }
    #[cfg(not(feature = "dev-usb"))]
    let _ = (home, trace, result, cleanup, refresh_ms);
}
