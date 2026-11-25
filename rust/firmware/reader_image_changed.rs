// SPDX-License-Identifier: MIT
//! Revoke a changed book's old resume window before importing the same file.
use super::*;
use crate::{app_image::{Application, Events, Home},
    reader_duty::ReaderDuty, reader_verification::Verification};
use newt_core::{durable::Load, selection_authority::flag};
use newt_native_storage::{Authority, ReaderSourceHints, ReaderSourceHintsState};
use newt_phased_reader::imported_revision::Digest;
use newt_reader_ui::ReaderUiNotice;

struct Cancel<'a, 'task>(&'a ReaderDuty<'a, 'task>);
impl Authority for Cancel<'_, '_> {
    fn cancelled(&self) -> bool { self.0.cancelled() }
    fn action_boundary(&self, stage: u8) { self.0.action_boundary(stage); }
    fn open_route(&self, route: u32) { self.0.open_route(route); }
    fn cache_miss(&self, reason: u8) { self.0.cache_miss(reason); }
}

// End discard/adopt construction moves before reopening the selected profile.
#[inline(never)]
fn discard_or_adopt_changed(home: &mut Home<'_>,
    session: &mut Option<Fixed<session::Session>>,
    source: &mut Option<Fixed<selected::SelectedSource>>, cancel: &dyn Authority,
    yielded: &dyn Fn(), adopted: &mut Option<Digest>,
) -> bool {
    home.with_storage_io(|_, io| {
        let Some(session) = session.as_mut() else { return false; };
        let mut native = io.port();
        if let Some(source) = source.as_mut() {
            let mut source_native = io.port();
            *adopted = session.prepared_change(source, &mut source_native, &mut native,
                cancel, yielded, millis);
            if adopted.is_some() { return true; }
        }
        let closed = session.close(&mut native, &boot::NoCancel, millis);
        if !matches!(closed, Ok(true)) { return false; }
        let route = session.route.lend(cancel, millis);
        let mut hints = ReaderSourceHintsState::new(&mut native, &route);
        if let Ok(mut record) = ReaderSourceHints::lend(&mut hints, &mut native, &route) { record.remove(); }
        let _ = hints.close(&mut native, &route);
        route.remove_record(&mut native, "Imported", "receipt", true)
    }).unwrap_or(false)
}

impl Runtime {
    pub(super) fn reopen_changed_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut newt_core::frame::Frame,
        events: &mut Events<'_, '_>,
    ) -> bool {
        if !self.session.as_ref().is_some_and(|s| s.verification.state() == Verification::Changed) {
            return false;
        }
        let Events { queue, control, task, action: trace } = events;
        control.with_open(millis(), |control, _owned| {
            let mut events = Events { queue, control, action: *trace, task: task.reborrow() };
            self.reimport_changed(home, ui, logical, &mut events)
        })
    }

    fn reimport_changed(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut newt_core::frame::Frame,
        events: &mut Events<'_, '_>,
    ) -> bool {
        let destination = ui.app.screen;
        if ui.app.screen != newt_core::app::Screen::Sleeping && ui.press == crate::app_image::PressState::Idle {
            ui.press = crate::app_image::PressState::Accepted;
        }
        self.acknowledge_work_image(home, ui, logical, newt_reader_ui::SlowWork::FirstOpen, events.control, events.action);
        let may_commit = self.selection.flags() & flag::COMMIT_OWED != 0
            || self.selection.flags() & (flag::SAVE_FAILED | flag::UNAVAILABLE | flag::UNCONFIRMED) == 0;
        let previous_digest = *self.selection.digest();
        let previous_flags = self.selection.flags();
        let previous_saved = self.saved;
        let previous_ready = self.progress_ready;
        let previous_saves = self.saves;
        // Changed bytes revoke the old RAM window even if reimport fails. The
        // old durable checkpoint remains protected by the existing store.
        self.recovery = None;
        self.reflow_anchor = None;
        self.saved = Progress::default();
        self.saves.set_checkpoint_saved(false);
        if let Some(memory) = self.memory.external_mut() {
            memory.lend(|memory| {
                let book = &mut *memory.runtime;
                book.owner.close(&mut book.reader);
            });
        }
        self.memory = ReaderBacking::Closed;
        let clock = home.bus.board.monotonic_clock();
        let duty = ReaderDuty::new(events.control, &self.cadence, &clock, None, events.action);
        let cancel = Cancel(&duty);
        let mut adopted = None;
        let removed = discard_or_adopt_changed(home, &mut self.session, &mut self.source,
            &cancel, &|| duty.yield_now(), &mut adopted);
        // Session close returned its real descriptor results above. The source
        // FD is retained; prepare_image lends it without an open/bind repeat.
        *self.selection.flags_mut() &= !(flag::PAGE_CACHE_MASK | flag::PAGE_CACHE_VISIT);
        let name = self.selection.identity().filename;
        let filename = CStr::from_bytes_until_nul(&name).ok();
        super::profile::with_profile(|profile| {
            let digest = if adopted.is_some() { adopted } else if removed {
                filename.and_then(|name| self.prepare_image(home, ui, logical, events, name, None, profile).ok())
            } else { None };
            let imported = digest.is_some_and(|digest| {
                let Some(name) = filename else { return false; };
                // Keep the previous title until the new open settles so an
                // interrupted reimport can roll back without copying it on the stack.
                self.selection.publish_candidate(name, digest, &mut || {}, &mut || {})
            });
            if !imported {
                if self.interrupted_open(home, events) {
                    self.restore_interrupted_reimport(ui, destination, filename, previous_digest,
                        previous_flags, previous_saved, previous_ready, previous_saves, false);
                    return true;
                }
                if let Some(session) = self.session.as_mut() { session.verification.mark_failed(); }
            }
            if imported {
                *self.selection.flags_mut() &= !(flag::SAVED_PRESENT | flag::ACCEPTED_OPEN_OWED | flag::COMMIT_OWED);
                *self.selection.flags_mut() |= flag::UNCONFIRMED | flag::CHANGED_OPEN
                    | if may_commit { flag::COMMIT_OWED } else { 0 };
                let loaded = self.with_store(home, |this, store| {
                    let _action = events.action.scope(crate::reader_action::ActionStage::ProgressLoad);
                    let loaded = this.selection.load_progress(store, &mut this.saved);
                    this.progress_ready = matches!(loaded, Load::Loaded(()) | Load::Empty);
                    this.saves.set_progress_unavailable(!this.progress_ready);
                    this.progress_ready
                }).unwrap_or(false);
                if loaded { self.open_selected_profile(home, ui, logical, events, profile, digest, false); }
                if matches!(self.memory, ReaderBacking::Interrupted) || self.interrupted_open(home, events) {
                    self.restore_interrupted_reimport(ui, destination, filename, previous_digest,
                        previous_flags, previous_saved, previous_ready, previous_saves, true);
                    return true;
                }
                if self.book().is_some_and(|book| book.owner.state.opened()) {
                    self.commit_verified_image(home, events.action);
                } else {
                    self.title.fill(0);
                    self.home.reading_available = self.progress_ready;
                    self.home.progress_unavailable = true;
                    self.home.percent = -1;
                    self.home.clear_chapter();
                    let finished = self.with_store(home, |this, store| {
                        let mut facts = newt_core::reading_stats::BookFacts::default();
                        this.selection.active()
                            && newt_core::reading_stats_service::stored_book_facts(store, this.selection.digest(), &mut facts)
                                == newt_core::reading_stats_service::StatsResult::Done
                            && newt_core::reading_stats::book_finish(&facts)
                                != newt_core::reading_stats::BookFinish::No
                    }).unwrap_or(false);
                    self.home.set_finished(finished);
                }
            }
            let opened = self.book().is_some_and(|book| book.owner.state.opened());
            self.reader_ui.notice = ReaderUiNotice::BookChanged;
            ui.library.message = Some(if opened { b"Book changed. Reopened." } else { b"Book changed. Reopen required." });
            ui.app.screen = destination;
            self.note_home_cover_changed();
            true
        })
    }

    fn restore_interrupted_reimport(&mut self, ui: &mut Application<'_>,
        destination: newt_core::app::Screen, filename: Option<&CStr>,
        previous_digest: Digest, previous_flags: u16, previous_saved: Progress,
        previous_ready: bool, previous_saves: ReadingSaveNotice, restore_selection: bool,
    ) {
        if restore_selection {
            if let Some(name) = filename {
                let _ = self.selection.publish_candidate(name, previous_digest, &mut || {}, &mut || {});
            }
            *self.selection.flags_mut() = previous_flags & !(flag::PAGE_CACHE_MASK | flag::PAGE_CACHE_VISIT);
        }
        self.saved = previous_saved;
        self.progress_ready = previous_ready;
        self.saves = previous_saves;
        self.saves.set_reader(newt_core::save_notice::ReaderIssue::None);
        self.reader_ui.notice = ReaderUiNotice::None;
        self.publish_notice_image(ui);
        ui.app.screen = destination;
    }
}
