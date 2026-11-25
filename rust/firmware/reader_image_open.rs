// SPDX-License-Identifier: MIT
//! Source preparation uses the selected descriptor and the optional session.
use super::*;
use crate::{app_image::{Application, Events, Home}, home_startup::CardPort,
    reader_book::BookFailure, reader_duty::ReaderDuty,
    sd_revision::SdRevisionStorage};
use newt_core::{app::Screen, reader_work_profile::ReaderWorkProfile,
    selection_authority::flag, zip_container::Status};
use newt_native_storage::Authority;
use newt_phased_reader::imported_revision::ImportedRevision;
use newt_reader_ui::{ReaderSource, RestorePolicy};

struct Cancel<'a, 'task>(&'a ReaderDuty<'a, 'task>);
impl Authority for Cancel<'_, '_> {
    fn cancelled(&self) -> bool { self.0.cancelled() }
    fn action_boundary(&self, stage: u8) { self.0.action_boundary(stage); }
    fn open_route(&self, route: u32) { self.0.open_route(route); }
    fn cache_miss(&self, reason: u8) { self.0.cache_miss(reason); }
}

impl Runtime {
    /// Frozen resumeAfterCard. An active selection checks the card's record
    /// (checkSelection). An inactive one reads it first; only its failed
    /// unavailable case probes absence.
    pub(super) fn resume_card_image(&mut self, home: &mut Home<'_>) -> bool {
        use newt_core::{durable::Load, selected_book::SelectedBookStore,
            reading_stats_store::StatsBackend, selection_authority::CardObservation};
        use newt_native_storage::Native;
        if self.selection.active() {
            let _ = self.with_store_io(home, |this, store, io, _log, _board| {
                let generation = &mut this.cover_generation;
                let mut card = CardObservation {
                    reading_state_unavailable: &mut |store| StatsBackend::reading_state_unavailable(store),
                    card_absent: &mut |_| matches!(io.port().stat("/newt-sd/Newt/Books", false),
                        Err(newt_native_storage::Error::Unavailable)),
                    // Books closed the optional session before its scan.
                    count_denied_if_session: &mut |_| {},
                    home_cover_changed: &mut |_| *generation = generation.wrapping_add(1),
                };
                this.selection.check_selection(store, &mut || false, &mut card);
            });
            *self.selection.flags_mut() &= !(flag::CARD_MISSING | flag::SAMPLE_CHOSEN);
            return true;
        }
        let (loaded, unavailable) = home.with_state_store(|_, store| {
            let loaded = SelectedBookStore::new(store).load();
            (loaded, store.reading_state_unavailable())
        });
        let silent = if !matches!(loaded, Load::Loaded(_)) && unavailable {
            if loaded == Load::Empty { true } else {
                home.bus.board.with_optional_sd_io(|_, io| matches!(
                    io.port().stat("/newt-sd/Newt/Books", false), Err(newt_native_storage::Error::Unavailable)))
            }
        } else { false };
        if silent { return false; }
        if !matches!(loaded, Load::Loaded(_) | Load::Empty) { *self.selection.flags_mut() |= flag::UNAVAILABLE; }
        else { *self.selection.flags_mut() &= !flag::UNAVAILABLE; }
        if let Load::Loaded(identity) = loaded { self.restore_boot_identity(&identity); }
        *self.selection.flags_mut() &= !(flag::CARD_MISSING | flag::SAMPLE_CHOSEN);
        true
    }

    pub(super) fn prepare_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>, logical: &mut newt_core::frame::Frame, events: &Events<'_, '_>,
        filename: &CStr, expected: Option<[u8; 32]>, profile: Option<&ReaderWorkProfile>,
    ) -> Result<[u8; 32], BookFailure> {
        // Frozen handoffCoverToReading: free only this optional backing before
        // source/session allocation; a refused open keeps the cover retry view.
        if self.idle.screen != Screen::EpubReading as u8 { self.idle.resume = None; }
        let filename_str = filename.to_str().map_err(|_| BookFailure::Route)?;
        let mut path = [0; b"/newt-sd/Newt/Books".len() + 2 + newt_core::library_name::FILENAME_BYTES];
        if !newt_library_current::library_book_path(Some(b"/newt-sd/Newt/Books\0"),
            Some(filename.to_bytes_with_nul()), Some(&mut path)) { return Err(BookFailure::Source); }
        let path = CStr::from_bytes_until_nul(&path).ok().and_then(|p| p.to_str().ok())
            .ok_or(BookFailure::Source)?;
        let mut key = [0; 64];
        let key = crate::reader_book::route_key(filename_str,
            &mut crate::reader_hash::PackageHasher::from_live_sdk(), &mut key).ok_or(BookFailure::Route)?;
        let clock = home.bus.board.monotonic_clock();
        let duty = ReaderDuty::new(events.control, &self.cadence, &clock, profile, events.action);
        let observations = profile.map(|profile| super::profile::Observations { profile, clock });
        let file_observer = observations.as_ref().map(|o| o as &dyn selected::FileObserver);
        let hash_observer = observations.as_ref().map(|o| o as &dyn crate::reader_hash::SourceHashObserver);
        let cancel = Cancel(&duty);
        let yielded = || duty.yield_now();
        // The library's source owner is lazy; allocation failure cannot publish
        // an invented source or a successful reader admission.
        if self.source.is_none() { self.source = Fixed::try_new_with(selected::SelectedSource::new); }
        if home.bus.board.ensure_sd_mounted().is_err() {
            if self.source.is_some() {
                *self.selection.flags_mut() |= flag::CARD_MISSING | flag::UNCONFIRMED;
                self.cover_generation = self.cover_generation.wrapping_add(1);
            }
            return Err(BookFailure::Source);
        }
        let source = self.source.as_mut().ok_or(BookFailure::Open(Status::OutOfMemory))?;
        let context = &mut self.session;
        let idle = &mut self.idle;
        let wake = &mut self.wake;
        let settings = self.settings;
        let cover_generation = &mut self.cover_generation;
        let selection = &mut self.selection;
        home.with_reader_io(|mut scope| {
            let mut native = scope.card.port(scope.io);
            if source.bound() {
                let live = source.with_source_observed(&mut native, Some(&cancel), file_observer, |raw|
                    raw.size() != 0 && raw.read_at(0, &mut []) == 0).unwrap_or(false);
                if !live {
                    let mut closed = true;
                    selection.close_source(&mut || {
                        if let Some(session) = context.as_mut() {
                            closed = matches!(session.close(&mut native, &cancel, millis), Ok(true));
                        }
                        source.close(&mut native);
                    });
                    // A remount invalidates held handles: reopen the same name and compare.
                    *selection.flags_mut() |= flag::UNCONFIRMED;
                    *cover_generation = cover_generation.wrapping_add(1);
                    if !closed { return Err(BookFailure::Source); }
                }
            }
            if !source.bound() && !source.open_observed(&mut native, path, Some(&cancel), file_observer) {
                return Err(BookFailure::Source);
            }
            if context.is_none() { *context = session::Session::try_new(&mut native, key, &cancel, millis); }
            let Some(context) = context.as_mut() else {
                super::picture_work::answer_slow(&mut scope, ui, logical, settings, key,
                    events.control, events.action, idle, wake);
                return source.with_source_observed(&mut native, Some(&cancel), file_observer, |raw| {
                    let mut digest = [0; 32];
                    // SAFETY: this Home owns the pinned SDK; raw is the same
                    // selected physical descriptor throughout the bounded hash.
                    let hashed = unsafe { crate::reader_hash::hash_source(raw, &mut digest,
                        &mut || duty.yield_now(), hash_observer) };
                    if !hashed || cancel.cancelled() { return Err(BookFailure::Source); }
                    if expected.is_some_and(|expected| digest != expected) {
                        return Err(BookFailure::Open(Status::Stale));
                    }
                    Ok(digest)
                }).map_err(|_| BookFailure::Source)?;
            };
            let context = &mut **context;
            if !matches!(context.close(&mut native, &cancel, millis), Ok(true)) {
                return Err(BookFailure::Revision);
            }
            let session::Session { route, receipt, whole, block, scratch, source: state,
                verification, .. } = context;
            let route = route.lend(&cancel, millis);
            let mut receipt = SdRevisionStorage::lend(receipt, &mut native, &route)
                .map_err(|_| BookFailure::Route)?;
            let mut source_native = scope.card.port(scope.io);
            let mut hints_native = scope.card.port(scope.io);
            let hints = super::hints::current(&mut hints_native, path);
            let mut whole = whole.borrow_with_yield(&yielded);
            let mut block = block.borrow_with_yield(&yielded);
            source.with_source_observed(&mut source_native, Some(&cancel), file_observer, |source| {
                let mut revision = ImportedRevision::compact(&mut receipt, &mut whole, &mut block, scratch);
                let mut admit = |raw: &mut dyn newt_core::zip_window::source::BookSource,
                    digest: &[u8; 32]| {
                    hints.valid() && hints.bytes == raw.size()
                        && super::hints::matching(&mut hints_native, &route, digest, hints)
                        && verification.begin(raw, || {
                            // SAFETY: this is the actual session's pending
                            // context, constructed only under Home's SDK loan.
                            unsafe { crate::reader_hash::MbedHasher::new(None) }
                        })
                };
                let mut before = |scratch: &mut [u8]| {
                    let Ok(frame) = scratch.try_into() else { crate::app_power::stop(); };
                    super::picture_work::answer_slow(&mut scope, ui, frame, settings, key,
                        events.control, events.action, idle, wake);
                };
                let chosen = revision.select_source_admitted_before_work(source, logical,
                    &|| !cancel.cancelled(), &mut admit, &mut before);
                let Some(chosen) = chosen else {
                    verification.mark_failed();
                    return Err(BookFailure::Revision);
                };
                let (prepared, digest) = chosen;
                if expected.is_some_and(|expected| expected != digest) {
                    *state = Some(ReaderSource::new(revision).into_state());
                    verification.control().flags.set_prepared(prepared);
                    verification.mark_changed();
                    return Err(BookFailure::Open(Status::Stale));
                }
                *state = Some(ReaderSource::new(revision).into_state());
                verification.control().flags.set_prepared(prepared);
                if verification.state() == crate::reader_verification::Verification::Verified {
                    if hints == super::hints::current(&mut hints_native, path) {
                        super::hints::record(&mut hints_native, &route, &digest, hints);
                    }
                }
                Ok(digest)
            }).map_err(|_| BookFailure::Source)?
        }).map_err(|_| BookFailure::Source)?
    }

    pub(super) fn open_boot_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut newt_core::frame::Frame, events: &mut Events<'_, '_>,
    ) {
        if ui.app.screen != Screen::EpubReading { return; }
        self.open_selected_image(home, ui, logical, events, false);
    }

    pub(super) fn open_selected_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut newt_core::frame::Frame,
        events: &mut Events<'_, '_>, session_checkpoint: bool,
    ) -> bool {
        if self.session.as_ref().is_some_and(|s| s.verification.state()
            == crate::reader_verification::Verification::Changed) {
            return self.reopen_changed_image(home, ui, logical, events);
        }
        super::profile::with_profile(|profile|
            self.open_selected_profile(home, ui, logical, events, profile, None, session_checkpoint))
    }

    pub(super) fn open_selected_profile(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut newt_core::frame::Frame, events: &mut Events<'_, '_>, profile: Option<&ReaderWorkProfile>,
        prepared: Option<[u8; 32]>, session_checkpoint: bool,
    ) -> bool {
        let _action = events.action.scope(crate::reader_action::ActionStage::SessionSetup);
        let Events { queue, control, task, action: trace } = events;
        let at = millis();
        control.with_open(at, |control, owned| {
            let mut events = Events { queue, control, action: *trace, task: task.reborrow() };
            let opened = self.open_selected_armed(home, ui, logical, &mut events, profile, prepared, session_checkpoint, at, owned);
            if !self.opened() { self.reflow_anchor = None; }
            let snapshot = events.control.snapshot(millis());
            if let Some(profile) = profile { super::profile::finish(self, profile, snapshot, &mut home.bus.log); }
            opened
        })
    }

    fn open_selected_armed(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut newt_core::frame::Frame,
        events: &mut Events<'_, '_>, profile: Option<&ReaderWorkProfile>,
        prepared: Option<[u8; 32]>, session_checkpoint: bool, at: u32, owned: bool,
    ) -> bool {
        if !session_checkpoint { self.reflow_anchor = None; }
        let recovery_open = self.recovery.is_some();
        // This open answers an interrupted one's owed reopen.
        if matches!(self.memory, ReaderBacking::Interrupted) { self.memory = ReaderBacking::Closed; }
        self.reader_ui.notice = newt_reader_ui::ReaderUiNotice::None;
        self.reading_refresh.reset();
        if self.idle.screen != Screen::EpubReading as u8 { self.idle.resume = None; }
        let was_active = self.selection.active();
        if !self.picture_candidate_image(home, ui, owned, session_checkpoint) && owned {
            let work = if was_active {
                let name = self.selection.identity().filename;
                let Ok(name) = CStr::from_bytes_until_nul(&name) else { return false; };
                if self.receipt_present_image(home, name) { newt_reader_ui::SlowWork::Opening }
                else { newt_reader_ui::SlowWork::FirstOpen }
            } else { newt_reader_ui::SlowWork::Opening };
            self.acknowledge_work_image(home, ui, logical, work, events.control, events.action);
            events.control.restart(millis());
        }
        if !was_active {
            if self.selection.flags() & flag::CARD_MISSING != 0 && !recovery_open {
                self.resume_card_image(home);
            }
            if !self.selection.active() {
                let ready = self.selection.flags() & flag::CARD_MISSING == 0
                    || self.selection.flags() & flag::SAMPLE_CHOSEN != 0 || recovery_open;
                if self.selection.selection_blocked() {
                    self.progress_ready = false;
                    self.saves.set_progress_unavailable(true);
                    self.selection.publish_selection_notice(&mut self.saves);
                    self.publish_notice_image(ui);
                    return true;
                }
                if !ready {
                    self.saves.set_source(self.selection.source_diagnostic());
                    self.selection.publish_selection_notice(&mut self.saves);
                    self.observe_wake_attempt(newt_wake_evidence::Attempt::SourceUnavailable, self.restore_policy_image());
                    self.publish_notice_image(ui);
                    return true;
                }
                #[cfg(feature = "embedded-sample")]
                return self.open_sample_image(home, ui, logical, events, session_checkpoint);
                #[cfg(not(feature = "embedded-sample"))]
                {
                    // Explicit bench cut: no ROM sample or sample backing.
                    self.progress_ready = false;
                    self.home.reading_available = false;
                    self.open_failed(BookFailure::Open(Status::Unsupported));
                    self.publish_notice_image(ui);
                    return true;
                }
            }
            let _ = self.with_store(home, |this, store| this.load_progress(store, events.action));
        }
        let mut attempted_policy = self.restore_policy_image();
        let name = self.selection.identity().filename;
        let Ok(name) = CStr::from_bytes_until_nul(&name) else { return false; };
        // prepareSource closes the reader before selected file/session work.
        // In particular, an old sample bank cannot survive into SD admission.
        let prepared = {
        let _source = events.action.scope(crate::reader_action::ActionStage::SourcePrepare);
        self.memory = ReaderBacking::Closed;
        self.standalone_status = Status::NotFound;
        // Only the candidate/change caller can consume the import it just
        // prepared. A later Read always rechecks the retained source.
        match prepared {
            Some(digest) if digest == *self.selection.digest() => Ok(digest),
            Some(_) => Err(BookFailure::Open(Status::Stale)),
            None => self.prepare_image(home, ui, logical, events, name, Some(*self.selection.digest()), profile),
        }
        };
        if let Err(error) = prepared {
            if let Some(profile) = profile {
                profile.terminal(newt_core::reader_work_profile::ReaderOpenTerminal::SourceUnavailable,
                    None, false, &events.control.elapsed(millis()),
                    &events.control.snapshot(millis()).reason, &events.control.active());
            }
            if !self.interrupted_open(home, events) {
                if matches!(error, BookFailure::Open(Status::Stale))
                    || (error == BookFailure::Source && self.source.as_ref().is_some_and(|s| s.bound())) {
                    self.note_home_cover_changed();
                }
                self.open_failed(error);
            }
            self.observe_wake_attempt(newt_wake_evidence::Attempt::SourceUnavailable, attempted_policy);
            self.publish_notice_image(ui);
            return true;
        }
        *self.selection.flags_mut() &= !flag::CARD_MISSING;
        if self.saves.source() == newt_core::save_notice::SourceIssue::NoCard as u8 {
            self.saves.set_source(newt_core::save_notice::SourceIssue::None);
        }
        if self.selection.flags() & flag::TITLE_DEFERRED == 0
            && self.checkpoint.loaded() == Load::Failed {
            let _ = self.with_store(home, |this, store| {
                this.checkpoint.restore(store, this.selection.digest());
            });
        }
        // The saved page settles after source preparation, before its picture
        // or reader backing can publish (frozen loadReaderProgress(false)).
        if !session_checkpoint && (attempted_policy.reloads_saved_progress() || !self.progress_ready) {
            let (ready, loaded) = self.with_store(home, |this, store| {
                let _load = events.action.scope(crate::reader_action::ActionStage::ProgressLoad);
                let loaded = this.selection.load_progress(store, &mut this.saved);
                this.progress_ready = matches!(loaded, Load::Loaded(()) | Load::Empty);
                this.saves.set_progress_unavailable(!this.progress_ready);
                (this.progress_ready, loaded == Load::Loaded(()))
            }).unwrap_or((false, false));
            if loaded {
                self.reconcile_recovery_progress();
                attempted_policy = self.restore_policy_image();
            }
            if self.recovery.is_none() && !ready {
                self.saves.set_progress_unavailable(true);
                self.publish_notice_image(ui);
                return true;
            }
        }
        self.answer_picture_image(home, ui, logical, events.control, events.action);
        // The source is prepared first. Reader backing is admitted here, not
        // at boot, and every explicit open can retry a previous allocation failure.
        self.memory = ReaderBacking::Closed;
        let admitted = self.admit_image_memory(home, ui);
        self.restore_failure = newt_reader_ui::RestoreFailure::Unavailable;
        if let Err(attempt) = admitted {
            self.open_failed(BookFailure::Open(Status::OutOfMemory));
            self.observe_wake_attempt(attempt, attempted_policy);
            self.publish_notice_image(ui);
            return true;
        }
        let indexed = match self.index_image(home, logical, events, profile) {
            Ok(indexed) => indexed,
            Err(error) => {
                if !self.interrupted_open(home, events) { self.open_failed(error); }
                self.observe_wake_attempt(newt_wake_evidence::Attempt::OpenFailed, attempted_policy);
                self.publish_notice_image(ui);
                return true;
            }
        };
        let result = self.operate_image(home, ui, logical, events, Some(super::drive::Opening {
            accepted_save: !recovery_open || session_checkpoint, at, indexed }), None, true, profile, None);
        self.invalidate_failed_index(home);
        match result {
            Ok(exit) => {
                self.restore_failure = newt_reader_ui::RestoreFailure::None;
                self.observe_operation(&exit);
                self.saves.set_reader(newt_core::save_notice::ReaderIssue::None);
                self.saves.set_source(newt_core::save_notice::SourceIssue::None);
                self.observe_wake_attempt(newt_wake_evidence::Attempt::Succeeded, attempted_policy);
                self.publish_notice_image(ui);
                self.update_home_image(home, events.action);
                true
            }
            Err(error) => {
                if !self.interrupted_open(home, events) {
                    self.open_failed(error);
                    self.memory = ReaderBacking::Closed;
                }
                self.observe_wake_attempt(newt_wake_evidence::Attempt::OpenFailed, attempted_policy);
                self.publish_notice_image(ui);
                true
            }
        }
    }

    /// Frozen ReaderRecovery::attempt's SD admission (#325) and its DEV row.
    /// Its own frame keeps these locals out of the shared open ancestor.
    #[inline(never)]
    fn admit_image_memory(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
    ) -> Result<(), newt_wake_evidence::Attempt> {
        use core::fmt::Write as _;
        use newt_c3_board::reader_memory::{LARGEST_ALLOCATION_BYTES, OWNER_ALIGN_ALLOWANCE_BYTES, REQUIRED_HEAP_BYTES, Refusal};
        use newt_wake_evidence::Attempt;
        let free = match ui.hardware.free_heap_bytes() { Ok(value) => value, Err(_) => crate::app_power::stop() };
        let largest = match ui.hardware.largest_heap_block_bytes() { Ok(value) => value, Err(_) => crate::app_power::stop() };
        // Frozen's formula, not its number: the bound session's own size plus
        // the allocator allowance, the bytes its release would return.
        let credit = self.session.as_ref().map_or(0, |_| size_of::<session::Session>() + OWNER_ALIGN_ALLOWANCE_BYTES);
        let _ = writeln!(home.bus.log, "Reader admission: free={} largest={} required={} block={} credit={}",
            free, largest, REQUIRED_HEAP_BYTES, LARGEST_ALLOCATION_BYTES, credit);
        let memory = ReaderMemory::try_admitted(free as usize, largest as usize, credit).map_err(|refusal| match refusal {
            Refusal::LargestBlock => Attempt::LargestBlockGuard,
            Refusal::FreeHeap => Attempt::FreeHeapGuard,
            Refusal::Allocation => Attempt::OpenFailed,
        })?;
        self.memory = ReaderBacking::External(memory);
        Ok(())
    }

    #[inline(never)]
    pub(super) fn reconcile_recovery_progress(&mut self) {
        let Some(pending) = self.recovery.as_mut() else { return; };
        if pending.reconcile_durable(self.saved, self.saves.checkpoint_saved()) {
            self.saves.set_checkpoint_saved(true);
            self.saves.restore(self.saves.retained() & !4, false, false);
            self.checkpoint.restore_uncertainty(false, self.saves.destination_unconfirmed());
            self.checkpoint.reset_attempted(self.saved.book, self.saved.sequence);
        }
    }

    pub(super) fn restore_policy_image(&self) -> RestorePolicy {
        self.recovery.as_ref().map_or_else(|| {
            if self.selection.active() && self.selection.flags() & flag::SAVED_PRESENT != 0 {
                RestorePolicy::Validated
            } else { RestorePolicy::AllowInitialFallback }
        }, |pending| pending.policy())
    }

    pub(super) fn open_failed(&mut self, failure: BookFailure) {
        use newt_core::save_notice::{SourceIssue, ReaderIssue};
        let status = match failure {
            BookFailure::Open(Status::Ok) => Status::Unsupported,
            BookFailure::Open(status) => status,
            BookFailure::Source => Status::IoError,
            BookFailure::Route | BookFailure::Revision => Status::Unsupported,
        };
        self.standalone_status = status;
        if let Some(memory) = self.memory.external_mut() {
            memory.lend(|memory| {
                let owner = &mut memory.runtime.owner;
                owner.state = newt_reader_ui::ReaderOwnerState::from_open(
                    owner.state.has_progress(), status, owner.state.readable_spines());
            });
        }
        if matches!(failure, BookFailure::Source | BookFailure::Route | BookFailure::Revision) {
            self.saves.set_source(if self.selection.flags() & flag::CARD_MISSING != 0 {
                SourceIssue::NoCard
            } else { SourceIssue::Changed });
        }
        if let Some(book) = self.book() { self.restore_failure = book.owner.restore_failure; }
        let malformed = self.memory.external_mut().is_some_and(|memory| memory.lend(|loan|
            loan.runtime.owner.malformed_utf8(&loan.runtime.reader)));
        let issue = if matches!(failure, BookFailure::Source) { ReaderIssue::SourceUnavailable }
        else if status == Status::OutOfMemory { ReaderIssue::Memory }
        else if self.restore_failure == newt_reader_ui::RestoreFailure::InvalidCheckpoint { ReaderIssue::InvalidCheckpoint }
        else if malformed { ReaderIssue::InvalidText }
        else { ReaderIssue::for_open_failure(status) };
        self.saves.set_reader(issue);
    }

    pub(super) fn update_home_image(&mut self, home: &mut Home<'_>, trace: crate::reader_action::Trace<'_>) {
        let _action = trace.scope(crate::reader_action::ActionStage::MetadataPublish);
        #[cfg(feature = "embedded-sample")]
        if let Some(memory) = self.memory.sample() {
            let view = memory.view();
            let owner = newt_reader_ui::ReaderOwnerState::from_open(true, self.standalone_status, view.spines);
            let page = owner.position_info(view.reader.info(), view.spines).page;
            let section = if owner.opened() { view.reader.current_section() }
                else { newt_reader_navigation::Section::default() };
            self.home.note_position(Some(section.label), page.overall_known,
                i32::from(page.overall_percent), false);
            return;
        }
        let Some(memory) = self.memory.external() else { return; };
        let book = memory.view();
        if !book.owner.state.opened() { return; }
        let title = book.reader.selection().title();
        if self.selection.flags() & flag::TITLE_DEFERRED == 0 {
            self.title.fill(0);
            let n = title.len().min(self.title.len() - 1);
            self.title[..n].copy_from_slice(&title[..n]);
        }
        let position = book.owner.state.position_info(book.reader.info(), book.reader.selection().spines).page;
        self.home.percent = if position.overall_known { i32::from(position.overall_percent) } else { -1 };
        self.home.progress_unavailable = !position.overall_known;
        self.home.reading_available = self.progress_ready;
        self.home.set_chapter(Some(book.reader.current_section().label));
        // Frozen showBookFinish (#319): the open's restore and each Done mark
        // or fold set the row, so Home follows it; only a No fact shows Mark.
        self.home.set_finished(self.reader_ui.finish != newt_reader_ui::ReaderFinishRow::Mark);
        self.publish_title_image(home);
    }
}
