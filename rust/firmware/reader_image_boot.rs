// SPDX-License-Identifier: MIT
//! Boot arbitration over the actual durable router, before input starts.
use super::*;
use core::fmt::Write;
use crate::{app_image::{Application, Home}, home_startup::CardPort,
    nvs::NvsBlobBackend, reader_restore_policy::{DurableCheckpoint, boot_restore_policy},
    reading_state_medium::MountedStateMedium,
    retained_runtime::{record::{self, BootSnapshot, SleepRecord, Status}, wake::{Attempt, UNAVAILABLE}}};
use newt_c3_board::Error;
use newt_core::{app::Screen, durable::Load, selected_book::SelectedBookStore,
    selection_authority::{flag, CardObservation}};
use newt_native_io::OsNative;
use newt_native_storage::{Authority, Native, reading_state::ReadingStateRouter};

type Store<'s, 'io> = ReadingStateRouter<'s, NvsBlobBackend<'s, crate::sdk::Sdk552>,
    OsNative<'io>, MountedStateMedium<'s, 's, 's, 'io, OsNative<'io>>>;

pub(super) struct NoCancel;
impl Authority for NoCancel { fn cancelled(&self) -> bool { false } }

impl Runtime {
    pub(super) fn with_store<R>(&mut self, home: &mut Home<'_>,
        work: impl for<'s, 'io> FnOnce(&mut Self, &mut Store<'s, 'io>) -> R,
    ) -> Result<R, Error> {
        self.with_store_io(home, |this, store, _io, _log, _board| work(this, store))
    }

    pub(super) fn with_store_io<R>(&mut self, home: &mut Home<'_>,
        work: impl for<'s, 'io> FnOnce(&mut Self, &mut Store<'s, 'io>, &'io newt_native_io::UiIo,
            &crate::serial_console::SerialConsole, &newt_c3_board::Board<'_>) -> R,
    ) -> Result<R, Error> {
        home.with_storage_io(|home, io| {
            let mut native = home.card.port(io);
            if let Some(session) = self.session.as_mut() {
                let session = &mut **session;
                let route = session.route.lend(&NoCancel, millis);
                let mut pages = newt_native_storage::chapter_cache::ChapterCacheStorage::lend(
                    &mut session.pages, &mut native, &route).map_err(|_| Error::Io)?;
                pages.access().release_for_state();
            }
            let mut medium = MountedStateMedium::<OsNative<'_>>::new(io, None);
            let mut store = ReadingStateRouter::new(NvsBlobBackend::new(&mut home.nvs),
                &mut home.facts, &mut medium, &mut native);
            Ok(work(self, &mut store, io, &home.bus.log, &home.bus.board))
        })?
    }

    // Boot-only restore records must leave the stack before the UI loop starts.
    #[inline(never)]
    pub(super) fn boot_restore(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        boot: &BootSnapshot, trace: crate::reader_action::Trace<'_>,
    ) -> bool {
        let power_on = boot.reset == newt_c3_board::Boot::POWER_ON_RESET;
        self.settings = home.preferences.settings();
        self.reader_ui.draft = self.settings;
        self.reader_ui.preferences_writable = home.preferences.writable();
        let mut target = home.load_boot_resume(power_on, &mut self.saves);
        let mut durable = false;
        // The progress as loaded, before a durable checkpoint replaces it,
        // and whether a saved position exists (frozen savedPresent_).
        let mut home_records = None;
        let retained = home.retained_sd_identity().ok()
            .filter(|identity| record::retained_selection(boot, identity));
        #[cfg(feature = "embedded-sample")]
        let retained_sample = record::classify_boot(boot.deep_wake, boot.software_reset, &boot.record)
                != record::Boot::OtherReset
            && record::join_words(boot.record.book_low, boot.record.book_high) == crate::sample_epub::IDENTITY
            && record::validate(&boot.record, crate::sample_epub::IDENTITY, false) == Status::Valid;
        if let Some(identity) = retained.as_ref() {
            self.restore_boot_identity(identity);
            *self.selection.flags_mut() |= flag::UNCONFIRMED;
        }
        if retained.is_some() {
            let restored = self.with_store_io(home, |this, store, io, _log, _board| {
                let generation = &mut this.cover_generation;
                let mut card = CardObservation {
                    reading_state_unavailable: &mut |store| newt_core::reading_stats_store::StatsBackend::reading_state_unavailable(store),
                    card_absent: &mut |_| matches!(io.port().stat("/newt-sd/Newt/Books", false),
                        Err(newt_native_storage::Error::Unavailable)),
                    // Boot precedes construction of the optional session.
                    count_denied_if_session: &mut |_| {},
                    home_cover_changed: &mut |_| *generation = generation.wrapping_add(1),
                };
                if !this.selection.check_selection(store, &mut || false, &mut card) {
                    *this.selection.flags_mut() |= flag::CARD_MISSING;
                }
                this.load_progress(store, trace)
            });
            match restored {
                Ok(loaded) => home_records = Some((self.saved, loaded == Load::Loaded(()))),
                Err(_) => *self.selection.flags_mut() |= flag::CARD_MISSING,
            }
        } else {
            // The router can load the sample's NVS record with no mounted SD.
            // A cold missing card is Empty plus its actual unavailable fact,
            // rather than a synthetic failed mounted-I/O capability.
            home.with_state_store(|_, store| {
                let selected = if power_on && target.committed_book() != 0 {
                    SelectedBookStore::new(store).load_book(target.committed_book())
                } else { SelectedBookStore::new(store).load() };
                if selected == Load::Empty && newt_core::reading_stats_store::StatsBackend::reading_state_unavailable(store) {
                    *self.selection.flags_mut() |= flag::CARD_MISSING;
                }
                #[cfg(feature = "embedded-sample")]
                if retained_sample {
                    *self.selection.flags_mut() |= flag::SAMPLE_CHOSEN;
                    return;
                }
                match selected {
                    Load::Loaded(identity) => {
                        self.restore_boot_identity(&identity);
                        let loaded = self.load_progress(store, trace);
                        home_records = Some((self.saved, loaded == Load::Loaded(())));
                        let ready = matches!(loaded, Load::Loaded(()) | Load::Empty);
                        let selection = &mut self.selection;
                        // Frozen sourceVerified() is true before an optional
                        // session exists; the later actual open still checks bytes.
                        let verified = selection.selection_verified(&mut || self.session.is_none());
                        durable = target.load_committed(store, power_on, selection.active(),
                            &mut self.saved, ready, verified,
                            |store, saved, at| selection.load_checkpoint(store, saved, at)).is_some();
                    }
                    Load::Empty => {}
                    _ => *self.selection.flags_mut() |= flag::UNAVAILABLE,
                }
            });
        }
        if let Some((saved, present)) = home_records { self.load_home_records(home, &saved, present); }
        #[cfg(feature = "embedded-sample")]
        if !self.selection.active() && !self.selection.selection_blocked() { self.load_sample_home(home); }
        if self.selection.selection_blocked() {
            self.home.progress_unavailable = true;
            self.home.percent = -1;
            self.home.clear_chapter();
            self.saves.set_progress_unavailable(true);
        }
        let expected = if self.selection.active() { self.selection.book_id() } else { self.unselected_book_id() };
        let mut consumed = SleepRecord::default();
        let report = home.restore_retained_boot(boot, &mut consumed, &mut ui.app,
            Some(&mut self.saved), expected, self.selection.active());
        if durable && !report.restored {
            ui.app.screen = Screen::EpubReading;
            ui.app.resume_screen = Screen::EpubReading;
        }
        ui.info.wake = Some(if durable && !report.restored {
            b"power-on / saved reading resumed".as_slice()
        } else { report.wake.as_bytes() });
        self.saves.restore(if report.restored { boot.record.recovery } else { 0 },
            report.restored && ui.app.screen != Screen::EpubReading,
            report.restored && self.saved.book != 0 && !report.checkpoint_saved);
        self.checkpoint.restore_uncertainty(self.saves.page_unconfirmed(), self.saves.destination_unconfirmed());
        // The target carries the durable preferences; a page saved under other
        // settings reopens at its anchor (#107), so Home and reader agree.
        if durable {
            self.settings = target.settings;
            self.reader_ui.draft = self.settings;
        }
        let retained_recovery = boot.record.recovery_kind();
        if (report.restored || durable) && (ui.app.screen == Screen::EpubReading
            || retained_recovery != Some(record::Recovery::Ordinary)) {
            self.saves.set_checkpoint_saved(durable || report.checkpoint_saved);
            let checkpoint = durable.then(|| if target.anchor { DurableCheckpoint::Anchor }
                else { DurableCheckpoint::PageStart });
            self.recovery = Some(crate::reader_book::PendingRecovery {
                progress: self.saved,
                settings: self.settings,
                policy: boot_restore_policy(checkpoint, retained_recovery),
            });
        }
        self.wake.available = true;
        self.wake.reset = report.reset;
        self.wake.cause = report.cause;
        self.wake.eligible = record::classify_boot(boot.deep_wake, boot.software_reset,
            &boot.record) != record::Boot::OtherReset;
        self.wake.record = report.status as u8;
        self.wake.restored = report.restored || durable;
        self.wake.durable_resume = durable;
        self.wake.restored_screen = ui.app.screen as u8;
        if durable || report.status == Status::Valid {
            self.wake.requested = if durable { Screen::EpubReading as u8 } else { boot.record.resume_screen as u8 };
            self.wake.rejected_entry = if durable { UNAVAILABLE } else { boot.record.rejected_sleep as u8 };
            self.wake.checkpoint_saved = durable || report.checkpoint_saved;
            self.wake.book = if durable { self.saved.book } else { record::join_words(boot.record.book_low, boot.record.book_high) };
            self.wake.spine = if durable { self.saved.locator.spine } else { boot.record.spine };
            self.wake.character = if durable { self.saved.locator.character } else { boot.record.character };
            if self.wake.book != 0 {
                self.wake.saved_orientation = boot.record.wake_orientation(durable.then_some(target.settings))
                    .map_or(UNAVAILABLE, |orientation| orientation as u8);
            }
        }
        // Frozen reportNvsStorage: the guard's issue and the restored save
        // notice reach Device at boot, which may land on Home with no reader event.
        let issue = home.nvs.storage_issue();
        self.saves.set_nvs_raw(issue);
        self.publish_notice_image(ui);
        let _ = match newt_core::save_notice::NvsIssue::text_raw(issue).and_then(|text| text.to_str().ok()) {
            Some(text) => writeln!(home.bus.log, "Newt preserves shared NVS: {text}"),
            None => writeln!(home.bus.log, "Newt shared NVS ready / preservation guard active"),
        };
        let reopen = (report.restored || durable) && ui.app.screen == Screen::EpubReading;
        if reopen { self.wake.attempt = Attempt::Pending; }
        // W1 consumes BOTH native magics once after this method returns.
        reopen
    }

    /// Frozen loadHomeProjection (#317): Home's title, chapter and percent
    /// come from this book's derived card records, never from the book, so
    /// Home is complete before any open finishes. A missing or corrupt record
    /// leaves the fallback that load_progress set.
    #[inline(never)]
    pub(super) fn load_home_records(&mut self, home: &mut Home<'_>, saved: &Progress, present: bool) {
        if !self.selection.active() || self.selection.selection_blocked()
            || self.selection.flags() & flag::TITLE_DEFERRED != 0 { return; }
        let name = self.selection.identity().filename;
        let Some(filename) = newt_core::library_name::name_bytes(&name)
            .and_then(|name| core::str::from_utf8(name).ok()) else { return; };
        let digest = *self.selection.digest();
        let Self { title, home: data, .. } = self;
        let _ = home.with_storage_io(|_, io| super::checkpoint::with_ownerless_pages(io, filename,
            |pages| load_sd_home(pages, &digest, saved, present, title, data)));
    }
}

/// Frozen loadSdHomeProgress without its cancel hook: boot runs before input.
/// Exact counts when complete, else the chapter weights saved at exit.
fn load_sd_home<S: crate::reader_metadata::IdentityStorage>(pages: &mut S, digest: &[u8; 32],
    saved: &Progress, present: bool, title: &mut [u8; 256], home: &mut HomeData<'_>,
) {
    use newt_phased_reader::semantic_metadata::{PositionProjection, ReaderSemanticMetadata};
    let at = saved.locator;
    let mut label = [0; newt_reader_navigation::TITLE_BYTES];
    let identified = crate::reader_metadata::load_identity(pages, digest, at, &mut label)
        && home.set_chapter(Some(&label));
    if !identified && present { home.set_generic_chapter(at.spine); }
    let mut position = PositionProjection::default();
    let counted = ReaderSemanticMetadata::load_position(pages, digest, 0, at.spine, title, &mut position);
    pages.close_checkpoint_storage();
    if counted && present && position.total != 0 && at.character <= position.chapter_chars {
        let percent = 100 * (position.prefix + u64::from(at.character)) / position.total;
        home.set_percent(percent as i32, false);
        return;
    }
    if present {
        let estimate = crate::reader_metadata::estimate::load_for(pages, digest, at.spine, at.character);
        pages.close_checkpoint_storage();
        if let Some(percent) = estimate.and_then(|estimate| estimate.percent(at.character)) {
            home.set_percent(i32::from(percent), true);
        }
    }
}
