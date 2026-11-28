// SPDX-License-Identifier: MIT
//! Owned book authority: an actual imported revision or pinned strict-source
//! fallback. No raw source is presented as a valid imported receipt.
use newt_core::zip_window::source::BookSource;
use newt_phased_reader::imported_revision::{Digest, ImportedRevision};

mod sealed {
    pub trait Sealed {}
}
mod operations;
pub use operations::{PendingReaderSource, SourceAuthorizer, SourceOperations, StrictOperations,
    StrictSource};

/// Only actual revision owners and short, explicitly bound source views enter
/// PhysicalOwner's private page route.
pub enum PhysicalSourceRef<'loan, 'source, 'hooks> {
    Revision(&'loan mut ImportedRevision<'source>),
    Reader(&'loan mut ReaderSource<'source>),
    Strict(&'loan mut StrictOperations<'hooks>),
    Pending(&'loan mut PendingReaderSource<'hooks, 'source>),
}

pub trait PhysicalSource: BookSource + sealed::Sealed {
    fn with_physical_source<R>(
        &mut self,
        operation: impl for<'loan, 'source, 'hooks> FnOnce(PhysicalSourceRef<'loan, 'source, 'hooks>) -> R,
    ) -> R;
}

impl sealed::Sealed for ImportedRevision<'_> {}
impl PhysicalSource for ImportedRevision<'_> {
    fn with_physical_source<R>(
        &mut self,
        operation: impl for<'loan, 'source, 'hooks> FnOnce(PhysicalSourceRef<'loan, 'source, 'hooks>) -> R,
    ) -> R {
        operation(PhysicalSourceRef::Revision(self))
    }
}

#[derive(Clone, Copy)]
pub(crate) enum ReaderSourceMode {
    Revision,
    Strict,
    Closed,
}

/// One real source/store owner lives through checked exit in both modes.
/// The exact source identity survives optional receipt reset separately.
pub struct ReaderSource<'a> {
    pub(crate) revision: ImportedRevision<'a>,
    pub(crate) mode: ReaderSourceMode,
    pub(crate) digest: Digest,
}
impl<'a> ReaderSource<'a> {
    pub fn lend_action_storage(&mut self, storage: &'a mut [u8])
        -> Option<newt_phased_reader::imported_revision::OriginalScratch<'a>> {
        self.revision.replace_action_storage(storage)
    }
    pub fn withdraw_action_storage(
        &mut self, original: newt_phased_reader::imported_revision::OriginalScratch<'a>,
    ) {
        self.revision.restore_action_storage(original);
    }
    pub fn new(mut revision: ImportedRevision<'a>) -> Self {
        revision.retain_source_for_reopen(true);
        let digest = *revision.digest();
        Self {
            revision,
            mode: ReaderSourceMode::Revision,
            digest,
        }
    }
    pub fn digest(&self) -> Digest {
        match self.mode {
            ReaderSourceMode::Revision => *self.revision.digest(),
            ReaderSourceMode::Strict => self.digest,
            ReaderSourceMode::Closed => [0; 32],
        }
    }
    /// A settings/recovery open repeats the original complete-source
    /// selection before it can leave Closed or replace receipt authority.
    pub fn prepare_reopen(
        &mut self,
        expected: Digest,
        scratch: &mut [u8],
        allowed: &dyn Fn() -> bool,
    ) -> Option<bool> {
        let (imported, digest) = self.revision.select_held_source(scratch, allowed)?;
        if digest != expected {
            self.reset();
            return None;
        }
        self.digest = expected;
        self.mode = ReaderSourceMode::Revision;
        Some(imported)
    }
    /// Reopen an already-verified receipt after a cancelled deferred-path check
    /// closed the receipt handle. Fails closed unless the stored receipt still
    /// matches `expected` and passes validation.
    pub fn reload_closed_receipt(&mut self, expected: Digest) -> bool {
        if !matches!(self.mode, ReaderSourceMode::Closed)
            || !self.revision.reload_held_receipt(expected)
        {
            return false;
        }
        self.digest = expected;
        self.mode = ReaderSourceMode::Revision;
        true
    }
    /// One frozen producer receipt scope over this same selected source owner.
    /// The caller records cancellation.result inside work, before settlement.
    pub fn with_producer_read(
        &mut self,
        work: impl FnOnce(&mut Self) -> newt_core::zip_container::Status,
    ) -> (newt_core::zip_container::Status, bool) {
        if matches!(self.mode, ReaderSourceMode::Closed) {
            return (newt_core::zip_container::Status::Unsupported, false);
        }
        self.revision.begin_immutable_read();
        let status = work(self);
        let fresh = self.revision.finish_immutable_read();
        (status, fresh)
    }
    /// The index pass verifies and reads the held physical bytes independently
    /// of the optional receipt's block checks. The source cannot escape its loan.
    pub fn with_held_source<R>(&mut self, work: impl FnOnce(&mut dyn BookSource) -> R) -> Option<R> {
        self.revision.with_held_source(work)
    }
    pub fn reset(&mut self) {
        self.revision.retain_source_for_reopen(false);
        self.revision.reset();
        self.mode = ReaderSourceMode::Closed;
    }
}
impl BookSource for ReaderSource<'_> {
    fn size(&self) -> u64 {
        match self.mode {
            ReaderSourceMode::Revision => self.revision.size(),
            ReaderSourceMode::Strict => self.revision.held_source_bytes(),
            ReaderSourceMode::Closed => 0,
        }
    }
    fn read_at(&mut self, offset: u64, output: &mut [u8]) -> i32 {
        match self.mode {
            ReaderSourceMode::Revision => self.revision.read_at(offset, output),
            ReaderSourceMode::Strict => self
                .revision
                .with_held_source(|s| s.read_at(offset, output))
                .unwrap_or(-1),
            ReaderSourceMode::Closed => -1,
        }
    }
}
impl sealed::Sealed for ReaderSource<'_> {}
impl PhysicalSource for ReaderSource<'_> {
    fn with_physical_source<R>(
        &mut self,
        operation: impl for<'loan, 'source, 'hooks> FnOnce(PhysicalSourceRef<'loan, 'source, 'hooks>) -> R,
    ) -> R {
        operation(PhysicalSourceRef::Reader(self))
    }
}

// Frozen ReaderRevisionSession::producerReceiptLive: the actual owner selects
// its existing revision/strict mode after complete digest authorization.
impl newt_core::producer_policy::ProducerReceipt for ReaderSource<'_> {
    fn producer_receipt_live(&mut self) -> bool {
        match self.mode {
            ReaderSourceMode::Revision => newt_core::producer_policy::ProducerReceipt::producer_receipt_live(&mut self.revision),
            ReaderSourceMode::Strict => self.revision.strict_producer_receipt_live(),
            ReaderSourceMode::Closed => false,
        }
    }
}
