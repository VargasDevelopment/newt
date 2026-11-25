// SPDX-License-Identifier: MIT
//! Same admitted checkpoint policy while the continuous producer is paused.
use crate::{home_startup::{CardPort, FirmwareCard}, reader_book::{self, PendingRecovery,
    ProducerPause, ReaderCheckpoint}, reading_state_medium::MountedStateMedium, sdk::Sdk552};
use core::cell::Cell;
use newt_c3_board::{MonotonicClock, hardware::HardwareReader};
use newt_core::{durable::Admission, producer_policy::Stop, time_zone::TimeZone};
use newt_native_storage::reading_state::ReadingStateRouter;

// Keep State/lease temporaries inside the paused duty, outside parser callers.
#[inline(never)]
pub(super) fn checkpoint(
    duty: ProducerPause<'_, '_, '_, '_, Sdk552, FirmwareCard>,
    idle: &mut super::cover_idle::CoverIdleState, clock: &MonotonicClock,
    hardware: &mut HardwareReader<'_>, zone: TimeZone,
    last_progress: &Cell<u32>, metadata_owed: &mut bool,
    cancelled: &dyn Fn() -> bool,
) -> (Stop, bool) {
    let before = duty.checkpoint.notice();
    let result = (|| {
        let mut quiet = || !cancelled();
        let mut admission = Admission { quiet: Some(&mut quiet), entered: false, deferred: false };
        if !admission.ready() { return Stop::Interrupted; }
        let book = reader_book::panel_statistics::stats_book_at(duty.checkpoint, duty.digest,
            duty.owner, duty.permille);
        let mut medium = MountedStateMedium::new(duty.io, Some(&mut *duty.pages));
        let mut native = duty.card.port(duty.io);
        let mut store = ReadingStateRouter::new(crate::nvs::NvsBlobBackend::new(&mut *duty.nvs),
            &mut *duty.facts, &mut medium, &mut native);
        let prepared = ReaderCheckpoint::selection_matches(&mut store, duty.digest);
        if !admission.ready() { return Stop::Interrupted; }
        if !prepared {
            let candidate = *duty.owner.tracker.snapshot(clock.millis());
            duty.checkpoint.refuse();
            PendingRecovery::complete_checkpoint(duty.recovery, duty.checkpoint,
                candidate, duty.settings, duty.digest);
            return Stop::Terminal;
        }
        let mut cancelled = || cancelled();
        if !duty.statistics.lease(&mut store, book.as_ref(), duty.owner.tracker.stats_marks_mut(),
            duty.ui, clock.millis(), zone, || {
                let mut utc = 0;
                let clock = crate::stats_clock::read_hardware_stats_session_clock(hardware, zone, Some(&mut utc));
                (clock, utc)
            }, Some(&mut cancelled), &crate::reader_hash::package_hash) || !admission.ready() {
            return Stop::Interrupted;
        }
        let at = clock.millis();
        let candidate = *duty.owner.tracker.snapshot(at);
        let accepted = *duty.accepted_open;
        let mut peers = newt_core::progress_store::PeerLocators::default();
        let result = duty.checkpoint.save_admitted(&mut store, duty.digest, candidate,
            duty.settings, false, accepted, accepted.then_some(&mut peers), Some(&mut admission));
        drop(store);
        drop(medium);
        if admission.deferred { return Stop::Interrupted; }
        if result.progress == reader_book::checkpoint::Outcome::Verified {
            // The in-parser producer still owns Pages and declines optional
            // metadata. The detached final tail may lend the reader normally.
            if accepted && !duty.pages.producing() {
                if let Some(reader) = duty.metadata {
                    reader_book::metadata::publish(duty.pages, reader, duty.digest, &candidate,
                        &peers, true, false, true, false, false, &|| false);
                }
            }
            let mut medium = MountedStateMedium::new(duty.io, Some(&mut *duty.pages));
            let mut native = duty.card.port(duty.io);
            let mut store = ReadingStateRouter::new(crate::nvs::NvsBlobBackend::new(&mut *duty.nvs),
                &mut *duty.facts, &mut medium, &mut native);
            duty.checkpoint.publish_destination(&mut store, duty.digest, duty.settings, true, false, true);
        }
        let result = duty.checkpoint.result();
        PendingRecovery::complete_checkpoint(duty.recovery, duty.checkpoint, candidate, duty.settings, duty.digest);
        idle.note_input(clock.millis());
        if result.progress == reader_book::checkpoint::Outcome::Verified && !accepted { *metadata_owed = true; }
        if result.progress == reader_book::checkpoint::Outcome::Verified
            && result.destination != reader_book::checkpoint::Outcome::Unconfirmed {
            *duty.accepted_open = false;
            last_progress.set(at);
            Stop::None
        } else { Stop::Terminal }
    })();
    (result, before != duty.checkpoint.notice())
}
