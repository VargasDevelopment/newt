// SPDX-License-Identifier: MIT
//! Finish the index before the reader operation lends its larger frame.
use super::*;
use crate::{app_image::{Application, Events, Home}, home_startup::CardPort,
    reader_book::BookFailure, reader_duty::ReaderDuty};
use newt_core::{admission::{ScopedWork, WorkEvent}, reader_work_profile::ReaderWorkProfile,
    zip_container::Status};
use newt_native_storage::Authority;
use newt_phased_reader::{index_build::{self, Indexed, Verification}, imported_revision::RevisionHasher};

struct Cancel<'loan, 'task>(&'loan ReaderDuty<'loan, 'task>);
impl Authority for Cancel<'_, '_> {
    fn cancelled(&self) -> bool { self.0.cancelled() }
}

impl Runtime {
    #[inline(never)]
    pub(super) fn index_image(&mut self, home: &mut Home<'_>, _ui: &mut Application<'_>,
        events: &Events<'_, '_>, profile: Option<&ReaderWorkProfile>,
    ) -> Result<Indexed, BookFailure> {
        let digest = *self.selection.digest();
        let name = self.selection.identity().filename;
        let filename = newt_core::library_name::name_bytes(&name)
            .and_then(|name| core::str::from_utf8(name).ok()).ok_or(BookFailure::Source)?;
        let clock = home.bus.board.monotonic_clock();
        let duty = ReaderDuty::new(events.control, &self.cadence, &clock, profile, events.action);
        let cancel = Cancel(&duty);
        let yielded = || duty.yield_now();
        #[cfg(feature = "dev-usb")]
        let index_profile = newt_core::reader_work_profile::IndexProfile::default();
        let observed = |event: WorkEvent| {
            duty.event(&event);
            #[cfg(feature = "dev-usb")]
            index_profile.event(&event, clock.micros());
        };
        let hook = ScopedWork::new(&yielded, Some(&observed));
        let observations = profile.map(|profile| super::profile::Observations { profile, clock });
        let file_observer = observations.as_ref().map(|o| o as &dyn selected::FileObserver);
        let (Some(memory), Some(source)) = (self.memory.external_mut(), self.source.as_mut()) else {
            return Err(BookFailure::Open(Status::OutOfMemory));
        };
        let context = &mut self.session;
        let result = memory.lend(|memory| home.with_reader_io(|scope| {
            let mut source_native = scope.card.port(scope.io);
            let mut receipt_native = scope.card.port(scope.io);
            let mut pages_native = scope.card.port(scope.io);
            super::sources::with_sources(context.as_deref_mut(), source, filename,
                &mut source_native, &mut receipt_native, &mut pages_native,
                &cancel, file_observer, &yielded, None, |pages, source| {
                    let mut parser = newt_target_expat::Expat::default();
                    match source {
                        super::sources::Source::Strict(raw) => index_build::open(raw, &digest,
                            &mut memory.storage, scope.bus.panel, &mut parser, pages, Some(&hook), Verification::Verified),
                        super::sources::Source::Session(revision, verification) => {
                            let mode = verification.state();
                            let mut finish = |raw: &mut dyn newt_core::zip_window::source::BookSource, scratch: &mut [u8]| {
                                if verification.finish_with_update(raw, &digest, scratch, &mut || !cancel.cancelled(),
                                    &mut |hash, bytes| hash.borrow_with_yield(&yielded).update(bytes)) {
                                    Ok(())
                                } else if verification.state() == crate::reader_verification::Verification::Failed {
                                    Err(Status::IoError)
                                } else { Err(Status::Stale) }
                            };
                            let verification = match mode {
                                crate::reader_verification::Verification::Pending => Verification::Pending(&mut finish),
                                crate::reader_verification::Verification::Verified => Verification::Verified,
                                _ => return Err(BookFailure::Open(Status::Stale)),
                            };
                            revision.with_held_source(|raw| index_build::open(raw, &digest,
                                &mut memory.storage, scope.bus.panel, &mut parser, pages, Some(&hook), verification))
                                .unwrap_or(Err(Status::Stale))
                        }
                    }.map_err(BookFailure::Open)
                })
        }).map_err(|_| BookFailure::Source).and_then(|result| result));
        #[cfg(feature = "dev-usb")]
        index_profile.trace(&mut home.bus.log);
        result
    }
}
