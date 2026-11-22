// SPDX-License-Identifier: MIT
//! The frozen optional revision session's physical owners, without self loans.
use crate::{reader_hash::MbedHasher, reader_verification::ReaderVerification,
    sd_revision::SdRevisionState};
use newt_native_storage::{Native, RouteState, Authority, LoanError,
    chapter_cache::ChapterCacheState};
use newt_reader_ui::ReaderSourceState;
use newt_phased_reader::imported_revision::RevisionPorts;
use newt_sdk_heap::Fixed;
use super::ReaderBacking;

pub(super) struct Session {
    pub(super) route: RouteState,
    pub(super) receipt: SdRevisionState,
    pub(super) pages: ChapterCacheState,
    pub(super) whole: MbedHasher,
    pub(super) block: MbedHasher,
    pub(super) scratch: [u8; 256],
    pub(super) source: Option<ReaderSourceState>,
    pub(super) verification: ReaderVerification<MbedHasher>,
}
impl Session {
    pub(super) fn prepared_change<N: Native>(&mut self,
        selected: &mut super::selected::SelectedSource, source_native: &mut N,
        receipt_native: &mut N, cancel: &dyn Authority, yielded: &dyn Fn(), now: fn() -> u32,
    ) -> Option<[u8; 32]> {
        if !self.verification.control().flags.prepared()
            || self.verification.state() != crate::reader_verification::Verification::Changed { return None; }
        let route = self.route.lend(cancel, now);
        let mut receipt = crate::sd_revision::SdRevisionStorage::lend(&mut self.receipt,
            receipt_native, &route).ok()?;
        let mut whole = self.whole.borrow_with_yield(yielded);
        let mut block = self.block.borrow_with_yield(yielded);
        let digest = selected.with_source(source_native, Some(cancel), |raw|
            with_source(&mut self.source, RevisionPorts { source: Some(raw), store: &mut receipt,
                whole: &mut whole, block: Some(&mut block), scratch: &mut self.scratch },
                |revision| revision.digest())).ok()?.ok()?;
        self.verification.adopt_changed().then_some(digest)
    }

    /// Allocate at frozen new(nothrow) ReaderRevisionSession, before either
    /// hasher constructor. The caller holds the initialized SDK/SD domain.
    // End construction moves before the caller begins import.
    #[inline(never)]
    pub(super) fn try_new<N: Native>(native: &mut N, key: &str,
        cancel: &dyn Authority, now: fn() -> u32,
    ) -> Option<Fixed<Self>> {
        Fixed::try_new_with(|| {
            let route = RouteState::new(crate::reader_book::CACHE_ROOT, key);
            let receipt = SdRevisionState::new(native, &route.lend(cancel, now));
            Self {
                route, receipt, pages: ChapterCacheState::new(),
                // SAFETY: the enclosing image's Home owns the initialized SDK.
                // Actual streaming always uses borrow_with_yield's UI hook;
                // no borrowed callback enters these persistent owners.
                whole: unsafe { MbedHasher::new(None) },
                // SAFETY: same SDK lease, with a distinct exclusive context.
                block: unsafe { MbedHasher::new(None) },
                scratch: [0; 256], source: None,
                verification: ReaderVerification::new(),
            }
        })
    }

    pub(super) fn close<N: Native>(&mut self, native: &mut N,
        cancel: &dyn Authority, now: fn() -> u32,
    ) -> Result<bool, LoanError> {
        self.verification.reset();
        self.source = None;
        let route = self.route.lend(cancel, now);
        // Both real descriptors are closed, even if the first close fails.
        let receipt = self.receipt.close(native, &route);
        let pages = self.pages.close(native, &route);
        match (receipt, pages) {
            (Ok(receipt), Ok(pages)) => Ok(receipt.clean && pages.clean),
            (Err(error), _) | (_, Err(error)) => Err(error),
        }
    }
}

#[path = "reader_image_source_loan.rs"]
mod source_loan;
pub(super) use source_loan::{with_source, with_source_lent};

impl super::Runtime {
    /// A changed source or an interrupted open reopens once no input is pending.
    pub(super) fn reopen_owed(&self) -> bool {
        matches!(self.memory, ReaderBacking::Interrupted) || self.session.as_ref()
            .is_some_and(|s| s.verification.state() == crate::reader_verification::Verification::Changed)
    }

    /// Whether a Back or Power press (not a timeout) stopped this open. That is a
    /// pause, not a result: no issue or verification result is kept, the session
    /// is released as if the open never started, and a reopen is owed.
    pub(super) fn interrupted_open(&mut self, home: &mut crate::app_image::Home<'_>,
        events: &crate::app_image::Events<'_, '_>) -> bool {
        if !events.control.cancelled() || events.control.timed_out() { return false; }
        if self.close_runtime(home).is_err() { crate::app_power::stop(); }
        self.memory = ReaderBacking::Interrupted;
        true
    }

    /// Frozen reader.close then selection.closeSource: optional session first,
    /// then its selected file. No field or policy borrow survives this call.
    pub(super) fn close_runtime(&mut self, home: &mut crate::app_image::Home<'_>)
        -> Result<(), newt_c3_board::Error> {
        use crate::home_startup::CardPort;
        if let Some(memory) = self.memory.external_mut() {
            memory.lend(|memory| {
                let book = &mut *memory.runtime;
                book.owner.close(&mut book.reader);
            });
        }
        self.memory = ReaderBacking::Closed;
        self.standalone_status = newt_core::zip_container::Status::NotFound;
        self.restore_failure = newt_reader_ui::RestoreFailure::None;
        if self.session.is_none() && self.source.is_none() {
            self.selection.close_source(&mut || {});
            return Ok(());
        }
        home.with_storage_io(|home, io| {
            let mut native = home.card.port(io);
            let mut result = Ok(());
            self.selection.close_source(&mut || {
                if let Some(mut session) = self.session.take() {
                    if !matches!(session.close(&mut native, &super::boot::NoCancel,
                        super::millis), Ok(true)) {
                        result = Err(newt_c3_board::Error::Io);
                    }
                }
                if let Some(source) = self.source.as_mut() { source.close(&mut native); }
            });
            result
        })?
    }
}
