// SPDX-License-Identifier: MIT
//! Finish the index before the reader operation lends its larger frame.
use super::*;
use crate::{app_image::{Application, Events, Home}, home_startup::CardPort,
    reader_book::BookFailure, reader_duty::ReaderDuty};
use newt_core::{admission::{ScopedWork, WorkEvent}, reader_work_profile::ReaderWorkProfile,
    zip_container::Status};
use newt_native_storage::{Authority, Native, page_entries::PageEntries};
use newt_phased_reader::{index_build::{self, Indexed, Verification}, imported_revision::RevisionHasher,
    phase_storage::{BuildLoan, Storage}};
use crate::{reader_hash::MbedHasher, reader_pages::ReaderPages,
    reader_verification::{ReaderVerification, Verification as SourceVerification}};
use super::sources::{Operation, Source};

struct Cancel<'loan, 'task>(&'loan ReaderDuty<'loan, 'task>);
impl Authority for Cancel<'_, '_> {
    fn cancelled(&self) -> bool { self.0.cancelled() }
}

impl Runtime {
    #[inline(never)]
    pub(super) fn index_image(&mut self, home: &mut Home<'_>, _ui: &mut Application<'_>,
        events: &Events<'_, '_>, profile: Option<&ReaderWorkProfile>,
    ) -> Result<Indexed, BookFailure> {
        let digest = self.selection.digest();
        let name = &self.selection.identity().filename;
        let filename = newt_core::library_name::name_bytes(name)
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
            super::sources::with_operation(context.as_deref_mut(), source, filename,
                &mut source_native, &mut receipt_native, &mut pages_native,
                &cancel, file_observer, &yielded, None, IndexPass {
                    storage: &mut memory.storage, lender: scope.bus.panel,
                    digest, hook: Some(&hook), cancel: &cancel, yielded: &yielded,
                })
        }).map_err(|_| BookFailure::Source).and_then(|result| result));
        #[cfg(feature = "dev-usb")]
        index_profile.trace(&mut home.bus.log);
        result
    }
}

struct IndexPass<'loan, 'bank, L> {
    storage: &'loan mut Storage<'bank>,
    lender: &'loan mut L,
    digest: &'loan [u8; 32],
    hook: newt_core::admission::Yield<'loan>,
    cancel: &'loan dyn Authority,
    yielded: &'loan dyn Fn(),
}
impl<N: Native + PageEntries, L: BuildLoan> Operation<N, Indexed> for IndexPass<'_, '_, L> {
    #[inline(always)]
    fn run(self, pages: &mut ReaderPages<'_, N>, source: Source<'_, '_>) -> Result<Indexed, BookFailure> {
        let Self { storage, lender, digest, hook, cancel, yielded } = self;
        let mut parser = newt_target_expat::Expat::default();
        match source {
            Source::Strict(raw) => index_build::open(raw, digest, storage, lender,
                &mut parser, pages, hook, Verification::Verified),
            Source::Session(revision, verification) => {
                let mode = verification.state();
                let mut finish = |raw: &mut dyn newt_core::zip_window::source::BookSource, scratch: &mut [u8]| {
                    finish_pending(verification, raw, digest, scratch, cancel, yielded)
                };
                let verification = match mode {
                    SourceVerification::Pending => Verification::Pending(&mut finish),
                    SourceVerification::Verified => Verification::Verified,
                    _ => return Err(BookFailure::Open(Status::Stale)),
                };
                revision.with_held_source(|raw| index_build::open(raw, digest,
                    storage, lender, &mut parser, pages, hook, verification))
                    .unwrap_or(Err(Status::Stale))
            }
        }.map_err(BookFailure::Open)
    }
}

// Hashing finishes before attempt(). Its SDK/update closure frames must not
// enlarge the later build's ancestor frame.
#[inline(never)]
fn finish_pending(verification: &mut ReaderVerification<MbedHasher>,
    raw: &mut dyn newt_core::zip_window::source::BookSource, digest: &[u8; 32], scratch: &mut [u8],
    cancel: &dyn Authority, yielded: &dyn Fn(),
) -> Result<(), Status> {
    if verification.finish_with_update(raw, digest, scratch, &mut || !cancel.cancelled(),
        &mut |hash, bytes| hash.borrow_with_yield(yielded).update(bytes)) {
        Ok(())
    } else if verification.state() == SourceVerification::Failed {
        Err(Status::IoError)
    } else { Err(Status::Stale) }
}
