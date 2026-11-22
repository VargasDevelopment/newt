// SPDX-License-Identifier: MIT
//! Short physical reader operations over the one selected source/session.
use super::*;
use crate::{app_image::{Application, Events, Home}, home_startup::CardPort,
    reader_book::{self, BookExit, BookFailure, BookOpen, BookRequest, BookStep,
        ProducerCallbacks},
    reader_duty::ReaderDuty,
    reader_pages::ReaderPages, sd_revision::SdRevisionStorage};
use core::cell::Cell;
use newt_core::{admission::WorkEvent, app::Screen, frame::Frame, reader_work_profile::ReaderWorkProfile,
    selection_authority::flag, zip_container::Status};
use newt_native_storage::{Authority, chapter_cache::{ChapterCacheStorage, ChapterCacheState}, RouteState};
use newt_phased_reader::imported_revision::{RevisionPorts, RevisionHasher};
use newt_reader_ui::{DeviceInfo, PhysicalOpen, StrictSource};

struct Cancel<'a, 'task>(&'a ReaderDuty<'a, 'task>, Option<&'a dyn Fn() -> bool>);
impl Authority for Cancel<'_, '_> {
    fn cancelled(&self) -> bool { self.0.cancelled() || self.1.is_some_and(|pending| pending()) }
    fn action_boundary(&self, stage: u8) { self.0.action_boundary(stage); }
    fn open_route(&self, route: u32) { self.0.open_route(route); }
    fn cache_miss(&self, reason: u8) { self.0.cache_miss(reason); }
}

impl Runtime {
    pub(super) fn operate_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>,
        logical: &mut Frame, events: &mut Events<'_, '_>, opening: Option<super::drive::Opening>, step: Option<BookStep>,
        paint_first: bool, profile: Option<&ReaderWorkProfile>,
        sleep_extras: Option<&newt_device_ui::SleepExtras>,
    ) -> Result<BookExit, BookFailure> {
        let initialize = opening.is_some();
        let opened_at = opening.as_ref().map(|opening| opening.at);
        if matches!(step, Some(BookStep::Save | BookStep::ReflowCheckpoint)) {
            self.selection.retire_checkpoint_metadata();
        }
        self.checkpoint.begin_operation();
        let save_requested = matches!(step, Some(BookStep::Save | BookStep::ReflowCheckpoint | BookStep::Idle { checkpoint: true, .. }));
        let clock = home.bus.board.monotonic_clock();
        let observations = profile.map(|profile| super::profile::Observations { profile, clock });
        let file_observer = observations.as_ref().map(|o| o as &dyn selected::FileObserver);
        let hash_observer = observations.as_ref().map(|o| o as &dyn crate::reader_hash::SourceHashObserver);
        let info = snapshot_info(&ui.info);
        let app = ui.app;
        let mut clock_text = [0; 9];
        let marker = home.clock.begin_paint();
        let shown = home.clock.shown(marker).and_then(|clock| {
            let bytes = clock.to_bytes_with_nul();
            if bytes.len() > clock_text.len() { return None; }
            clock_text[..bytes.len()].copy_from_slice(bytes);
            CStr::from_bytes_with_nul(&clock_text[..bytes.len()]).ok()
        });
        let mut sleep_home = newt_device_ui::HomeData {
            clock: shown, book_title: Some(self.title.as_slice()), chapter_title: self.home.chapter_title,
            percent: self.home.percent, reading_available: self.home.reading_available,
            progress_unavailable: self.home.progress_unavailable, fixture_cover: self.home.fixture_cover,
            flags: self.home.flags, device_description: self.home.device_description,
        };
        sleep_home.set_clock_format(home.clock.format());
        let sleep_style = newt_device_ui::HomeStyle::default();
        let scene = sleep_extras.map(|extras| reader_book::sleep::Scene {
            state: &app, home: &sleep_home, style: &sleep_style, extras });
        let view_valid = self.book().is_some_and(|book| book.owner.state.opened()
            && book.ui.panel == newt_reader_ui::ReaderPanel::Page)
            && app.screen == Screen::EpubReading && self.selection.active()
            && self.source.as_ref().is_some_and(|source| source.bound())
            && self.session.as_ref().is_some_and(|session|
                session.verification.state() == crate::reader_verification::Verification::Verified)
            && ui.displayed.valid
            && self.selection.selection_verified(&mut || true);
        let card_missing = self.selection.flags() & flag::CARD_MISSING != 0;
        let mut accepted = self.selection.flags() & flag::ACCEPTED_OPEN_OWED != 0;
        let accepted_save = opening.is_some_and(|opening| opening.accepted_save)
            && self.selection.flags() & flag::TITLE_DEFERRED == 0;
        let digest = *self.selection.digest();
        let book_identity = self.selection.book_id();
        let name = self.selection.identity().filename;
        let filename = newt_core::library_name::name_bytes(&name)
            .and_then(|name| core::str::from_utf8(name).ok()).ok_or(BookFailure::Source)?;
        let mut path = [0; b"/newt-sd/Newt/Books".len() + 2 + newt_core::library_name::FILENAME_BYTES];
        if !newt_library_current::library_book_path(Some(b"/newt-sd/Newt/Books\0"), Some(&name), Some(&mut path)) {
            return Err(BookFailure::Source);
        }
        let path = core::ffi::CStr::from_bytes_until_nul(&path).ok()
            .and_then(|path| path.to_str().ok()).ok_or(BookFailure::Source)?;
        let saved = self.recovery.as_ref().map_or(self.saved, |pending| *pending.progress());
        let policy = self.restore_policy_image();
        let settings = self.recovery.as_ref().map_or(self.settings, |pending| pending.settings);
        let power = Cell::new((info.charging_known && info.charging)
            || (info.battery_known && info.battery_percent >= 20));
        let battery = Cell::new(ui.last_battery_read);
        let progress = &self.last_progress;
        let mut producer_metadata_owed = false;
        let was_accepted = accepted;
        let title_deferred = self.selection.flags() & flag::TITLE_DEFERRED != 0;
        let Self { memory, reader_ui, source, session: context, checkpoint, statistics, recovery,
            cadence, exit_metadata, selection, idle, saves, settings: current_settings, .. } = self;
        let (Some(memory), Some(source)) = (memory.external_mut(), source.as_mut()) else {
                return Err(BookFailure::Open(Status::OutOfMemory));
            };
        let control = &*events.control;
        let queue = &*events.queue;
        let duty = ReaderDuty::new(control, cadence, &clock, profile, events.action);
        let cancel = Cancel(&duty, None);
        let yielded = || duty.yield_now();
        let observed = |event: WorkEvent| duty.event(&event);
        let result = memory.lend(|memory| home.with_reader_io(|scope| {
            #[cfg(feature = "dev-usb")]
            let background_ready = ui.bridge.background_work_ready();
            let idle_pending = || queue.pending() || {
                #[cfg(feature = "dev-usb")]
                { scope.bus.log.available() > 0 || !background_ready }
                #[cfg(not(feature = "dev-usb"))]
                { false }
            };
            let idle_cancel = Cancel(&duty,
                matches!(step, Some(BookStep::Idle { .. })).then_some(&idle_pending as &dyn Fn() -> bool));
            let producer = super::producer::Poll { duty: &duty, clock, queue,
                console: &*scope.bus.log, view_valid, card_missing,
                power: &power, progress, battery: &battery, began: Cell::new(clock.millis()) };
            let entry = |began| producer.entry(began);
            let stopped = |began| producer.stop(began);
            let pause_request = |began, pauses| producer.pause_request(began, pauses);
            let producer_yield = || producer.yield_now();
            #[cfg(all(feature = "rust-app", feature = "dev-usb"))]
            let producer_trace = |event| producer.trace(event);
            let callbacks = ProducerCallbacks { entry: &entry, stop: &stopped,
                pause_request: &pause_request, yield_now: Some(&producer_yield),
                wait: Some(wait_one),
                #[cfg(all(feature = "rust-app", feature = "dev-usb"))]
                trace: Some(&producer_trace) };
            let mut pause = |duty: crate::reader_book::ProducerPause<'_, '_, '_, '_,
                crate::sdk::Sdk552, crate::home_startup::FirmwareCard>, cancelled: &dyn Fn() -> bool| {
                if cancelled() { return (newt_core::producer_policy::Stop::Interrupted, false); }
                let mut repaint = false;
                if clock.millis().wrapping_sub(battery.get()) >= 60000 {
                    let before = (ui.info.battery_known, ui.info.battery_percent, ui.info.storage());
                    if ui.read_info_with_clock(&clock, events.action).is_err() {
                        return (newt_core::producer_policy::Stop::Terminal, repaint);
                    }
                    battery.set(ui.last_battery_read);
                    power.set((ui.info.charging_known && ui.info.charging)
                        || (ui.info.battery_known && ui.info.battery_percent >= 20));
                    repaint |= before != (ui.info.battery_known, ui.info.battery_percent, ui.info.storage());
                }
                if clock.millis().wrapping_sub(progress.get()) >= 30000 {
                    let (reason, changed) = super::idle_save::checkpoint(duty, idle, &clock,
                        &mut ui.hardware, scope.clock.zone(), progress, &mut producer_metadata_owed,
                        cancelled);
                    repaint |= changed;
                    if reason != newt_core::producer_policy::Stop::None { return (reason, repaint); }
                }
                (producer.after_duty(false, false), repaint)
            };
            let mut source_native = scope.card.port(scope.io);
            let mut receipt_native = scope.card.port(scope.io);
            let mut pages_native = scope.card.port(scope.io);
            let request = BookRequest { path, filename, open: BookOpen::Receipt(digest),
                identity: book_identity, saved, policy, info: &info, cancel: &idle_cancel,
                now: millis, sleep: scene.as_ref(), zone: scope.clock.zone() };
            let run_turn = |prepared| super::drive::Turn { initialize, paint_first, accepted_save,
                step, settings, open: if initialize && prepared { PhysicalOpen::Prepared }
                    else { PhysicalOpen::Selected { digest, spine: saved.locator.spine } } };
            let Some(context) = context.as_mut() else {
                let mut key = [0; 64];
                let key = reader_book::route_key(filename, &mut crate::reader_hash::PackageHasher::from_live_sdk(),
                    &mut key).ok_or(BookFailure::Route)?;
                let route_state = RouteState::new(reader_book::CACHE_ROOT, key);
                let route = route_state.lend(&cancel, millis);
                let mut state = ChapterCacheState::new();
                let result = {
                    let mut pages = ReaderPages(ChapterCacheStorage::lend(&mut state, &mut pages_native, &route)
                        .map_err(|_| BookFailure::Route)?);
                    source.with_source_observed(&mut source_native, Some(&cancel), file_observer, |raw| {
                        let mut authorize = |raw: &mut dyn newt_core::zip_window::source::BookSource,
                            expected: &[u8; 32], _scratch: &mut [u8], allowed: &dyn Fn() -> bool| {
                            if !allowed() { return false; }
                            let mut actual = [0; 32];
                            // SAFETY: Home retains the actual pinned SDK and
                            // raw is this same selected descriptor's short loan.
                            (unsafe { crate::reader_hash::hash_source(raw, &mut actual, &mut || yielded(), hash_observer) })
                                && allowed() && actual == *expected
                        };
                        let strict = if initialize { StrictSource::new(raw, &digest) }
                            else { StrictSource::for_live_reader(raw, selection, &memory.runtime.owner,
                                &mut memory.runtime.reader).ok_or(BookFailure::Source)? };
                        let allowed = || !cancel.cancelled();
                        let mut strict = strict.with_reopen_authorizer(&mut authorize, &allowed);
                        super::drive::RunLoan { memory, ui: reader_ui, checkpoint, statistics, recovery,
                            accepted: &mut accepted, metadata: exit_metadata, flags: None, settings: current_settings }
                            .run(super::drive::Ports { io: scope.io, panel: &mut *scope.bus.panel,
                                card: &mut *scope.card, preferences: &mut *scope.preferences,
                                nvs: &mut *scope.nvs, facts: &mut *scope.facts }, &mut pages, &mut strict, &request, &callbacks, &mut pause,
                                None, run_turn(false), &yielded, &observed, saves, title_deferred)
                    }).map_err(|_| BookFailure::Source)?
                };
                let closed = state.close(&mut pages_native, &route);
                return if !closed.is_ok_and(|outcome| outcome.clean) { Err(BookFailure::Source) } else { result };
            };
            let context = &mut **context;
            let session::Session { route, receipt, pages, whole, block, scratch,
                source: state, verification } = context;
            let prepared = verification.control().flags.prepared();
            if initialize { verification.control().flags.set_prepared(false); }
            let route = route.lend(&cancel, millis);
            let mut receipt = SdRevisionStorage::lend(receipt, &mut receipt_native, &route)
                .map_err(|_| BookFailure::Route)?;
            let mut pages = ReaderPages(ChapterCacheStorage::lend(pages, &mut pages_native, &route)
                .map_err(|_| BookFailure::Route)?);
            let mut whole = whole.borrow_with_yield(&yielded);
            let mut block = block.borrow_with_yield(&yielded);
            // Background/save work may leave the displayed frame unchanged.
            // Only foreground composition owns the full repaint contract.
            let action_storage = if initialize || paint_first
                || matches!(step, Some(BookStep::Action(_) | BookStep::Sleep(_))) {
                Some(&mut logical[..])
            } else { None };
            source.with_source_observed(&mut source_native, Some(&cancel), file_observer, |source| {
                session::with_source_lent(state, RevisionPorts { source: Some(source), store: &mut receipt,
                    whole: &mut whole, block: Some(&mut block), scratch }, action_storage, |revision| {
                    if verification.state() == crate::reader_verification::Verification::Pending {
                        let mut finish = |raw: &mut dyn newt_core::zip_window::source::BookSource,
                            expected: &[u8; 32], scratch: &mut [u8], allowed: &dyn Fn() -> bool| {
                            verification.finish_with_update(raw, expected, scratch, &mut || allowed(),
                                &mut |hash, bytes| hash.borrow_with_yield(&yielded).update(bytes))
                        };
                        let allowed = || !cancel.cancelled();
                        let mut pending = revision.with_verification_gate(saved.locator.spine, &mut finish, &allowed);
                        super::drive::RunLoan { memory, ui: reader_ui, checkpoint, statistics, recovery,
                            accepted: &mut accepted, metadata: exit_metadata, flags: None, settings: current_settings }
                            .run(super::drive::Ports { io: scope.io, panel: &mut *scope.bus.panel,
                                card: &mut *scope.card, preferences: &mut *scope.preferences,
                                nvs: &mut *scope.nvs, facts: &mut *scope.facts }, &mut pages, &mut pending, &request, &callbacks, &mut pause,
                                None, run_turn(false), &yielded, &observed, saves, title_deferred)
                    } else {
                        if verification.state() != crate::reader_verification::Verification::Verified {
                            return Err(BookFailure::Open(Status::Stale));
                        }
                        if !verification.producer_ready() { verification.reset_producer_visit(); }
                        verification.with_producer_visit(|ledger, view|
                            super::drive::RunLoan { memory, ui: reader_ui, checkpoint, statistics, recovery,
                                accepted: &mut accepted, metadata: exit_metadata, flags: Some(selection.flags_mut()), settings: current_settings }
                                .run(super::drive::Ports { io: scope.io, panel: &mut *scope.bus.panel,
                                card: &mut *scope.card, preferences: &mut *scope.preferences,
                                nvs: &mut *scope.nvs, facts: &mut *scope.facts }, &mut pages, revision, &request, &callbacks, &mut pause,
                                    Some((ledger, view)), run_turn(prepared), &yielded, &observed, saves, title_deferred))
                            .ok_or(BookFailure::Revision)?
                    }
                }).map_err(|_| BookFailure::Revision)?
            }).map_err(|_| BookFailure::Source)?
        }).map_err(|_| BookFailure::Source).and_then(|result| result));
        if initialize && accepted_save && result.is_ok() { self.selection.retire_checkpoint_metadata(); }
        if producer_metadata_owed || (matches!(step, Some(BookStep::Idle { checkpoint: true, .. }))
            && !was_accepted && self.checkpoint.result().progress == reader_book::checkpoint::Outcome::Verified) {
            self.selection.defer_checkpoint_metadata();
        }
        if accepted { *self.selection.flags_mut() |= flag::ACCEPTED_OPEN_OWED; }
        else { *self.selection.flags_mut() &= !flag::ACCEPTED_OPEN_OWED; }
        if let Some(at) = opened_at.filter(|_| result.is_ok()) {
            // An accepted checkpoint replaces the initial open anchor; pending
            // verification and recovery opens retain the initial timestamp.
            self.last_progress.set(if self.checkpoint.result().progress
                != reader_book::checkpoint::Outcome::Unchanged {
                result.as_ref().map_or(at, |exit| exit.action_at)
            } else { at });
        } else if save_requested && result.is_err() {
            let now = clock.millis();
            self.refuse_checkpoint_image(now, accepted);
            self.last_progress.set(now);
        } else if save_requested && self.checkpoint.result().progress != reader_book::checkpoint::Outcome::Unchanged {
            self.last_progress.set(result.as_ref().map_or_else(|_| clock.millis(), |exit| exit.action_at));
        }
        if result.is_ok() && self.checkpoint.result().progress == reader_book::checkpoint::Outcome::Verified {
            if let Some(saved) = self.checkpoint.acknowledged_for(&digest) { self.saved = saved; }
        }
        result
    }
}

fn wait_one() { crate::app_power::delay_ms(1); }

pub(super) fn snapshot_info(info: &DeviceInfo<'static>) -> DeviceInfo<'static> {
    let mut copy = DeviceInfo::default();
    copy.battery_known = info.battery_known; copy.battery_percent = info.battery_percent;
    copy.millivolts_known = info.millivolts_known; copy.millivolts = info.millivolts;
    copy.charging_known = info.charging_known; copy.charging = info.charging;
    copy.firmware_switch = info.firmware_switch; copy.other_firmware = info.other_firmware;
    copy.hat = info.hat; copy.free_heap = info.free_heap; copy.panel = info.panel; copy.wake = info.wake;
    #[cfg(feature = "dev-usb")]
    { copy.wake_evidence = info.wake_evidence; }
    copy.with_storage_notice(info.storage(), info.page_save_unconfirmed())
}
