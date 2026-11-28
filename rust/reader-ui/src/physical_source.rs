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
pub(crate) use operations::StrictCheck;

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

/// Why a source that a cancel closed did not reload: another cancel, which
/// says nothing about the book, or a book that no longer matches its digest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Interrupted {
    Cancelled,
    Changed,
}

/// How a source that a cancel closed becomes readable again.
#[derive(Clone, Copy)]
pub(crate) enum Recovery {
    /// Reload the sealed receipt under the digest.
    Receipt,
    /// The strict fallback has no sealed receipt: hash the held source again.
    Strict,
}

#[derive(Clone, Copy)]
pub(crate) enum ReaderSourceMode {
    Revision,
    Strict,
    /// A cancel ended the last action: the receipt closed and the reader kept
    /// its page. Like Closed it has no digest and no read path. Only `reload`
    /// leaves it, and the owner runs that before every action and repaint (#199).
    MustReload(Recovery),
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
    /// None while the source has no read path (closed, or closed by a cancel).
    pub fn digest(&self) -> Option<Digest> {
        match self.mode {
            ReaderSourceMode::Revision => Some(*self.revision.digest()),
            ReaderSourceMode::Strict => Some(self.digest),
            ReaderSourceMode::MustReload(_) | ReaderSourceMode::Closed => None,
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
    /// A cancel ended the last action. Close the source and keep the way
    /// back: the next action or repaint reloads under the same digest (#199).
    /// The strict fallback has no receipt to close; its reload resets the
    /// revision as its open did.
    pub(crate) fn close_after_cancel(&mut self) {
        let recovery = match self.mode {
            ReaderSourceMode::Revision => {
                self.digest = *self.revision.digest();
                self.revision.close_receipt();
                Recovery::Receipt
            }
            ReaderSourceMode::Strict => Recovery::Strict,
            ReaderSourceMode::MustReload(recovery) => recovery,
            ReaderSourceMode::Closed => return,
        };
        self.mode = ReaderSourceMode::MustReload(recovery);
    }
    /// Leaves MustReload before an action or repaint. Readable and closed
    /// sources pass through unchanged.
    pub(crate) fn reload(&mut self) -> Result<(), Interrupted> {
        match self.mode {
            ReaderSourceMode::MustReload(recovery) => self.reload_as(recovery, self.digest),
            _ => Ok(()),
        }
    }
    /// The reader's exit after a cancel: make a source that the cancel closed
    /// readable again under the selected digest, so its position can be saved.
    /// A closed source can only have a receipt to reload. True when readable.
    pub fn reload_closed(&mut self, expected: Digest) -> bool {
        match self.mode {
            ReaderSourceMode::Revision | ReaderSourceMode::Strict => true,
            ReaderSourceMode::MustReload(recovery) => self.reload_as(recovery, expected).is_ok(),
            ReaderSourceMode::Closed => self.reload_as(Recovery::Receipt, expected).is_ok(),
        }
    }
    /// The one reload for both kinds of source. Either way the whole check
    /// runs again under `expected`, so a changed book can't reload. A failure
    /// that a new cancel caused leaves the mode as it was; any other closes.
    fn reload_as(&mut self, recovery: Recovery, expected: Digest) -> Result<(), Interrupted> {
        let (reloaded, mode) = match recovery {
            Recovery::Receipt => (self.revision.reload_held_receipt(expected), ReaderSourceMode::Revision),
            Recovery::Strict => (
                self.revision.prepare_strict_source(expected, &|| true)
                    == newt_core::zip_container::Status::Ok,
                ReaderSourceMode::Strict,
            ),
        };
        if reloaded {
            self.digest = expected;
            self.mode = mode;
            return Ok(());
        }
        if self.revision.cancelled() {
            return Err(Interrupted::Cancelled);
        }
        self.mode = ReaderSourceMode::Closed;
        Err(Interrupted::Changed)
    }
    /// One frozen producer receipt scope over this same selected source owner.
    /// The caller records cancellation.result inside work, before settlement.
    pub fn with_producer_read(
        &mut self,
        work: impl FnOnce(&mut Self) -> newt_core::zip_container::Status,
    ) -> (newt_core::zip_container::Status, bool) {
        if matches!(self.mode, ReaderSourceMode::MustReload(_) | ReaderSourceMode::Closed) {
            return (newt_core::zip_container::Status::Unsupported, false);
        }
        self.revision.begin_immutable_read();
        let status = work(self);
        let fresh = self.revision.finish_immutable_read();
        (status, fresh)
    }
    /// Firmware's way to the held bytes outside a page operation, such as a
    /// verification pass: through the reload gate, under the digest the bytes
    /// were checked against (#199). Err says why the gate stayed shut.
    pub fn with_checked_source<R>(&mut self, work: impl FnOnce(&mut dyn BookSource, &Digest) -> R)
        -> Result<R, Interrupted> {
        self.reload()?;
        self.with_held_source(work).ok_or(Interrupted::Changed)
    }
    /// The held raw bytes with the digest they were checked under, while the
    /// source is readable. The source cannot escape its loan.
    pub(crate) fn with_held_source<R>(
        &mut self, work: impl FnOnce(&mut dyn BookSource, &Digest) -> R,
    ) -> Option<R> {
        let digest = self.digest()?;
        self.revision.with_held_source(|raw| work(raw, &digest))
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
            ReaderSourceMode::MustReload(_) | ReaderSourceMode::Closed => 0,
        }
    }
    fn read_at(&mut self, offset: u64, output: &mut [u8]) -> i32 {
        match self.mode {
            ReaderSourceMode::Revision => self.revision.read_at(offset, output),
            ReaderSourceMode::Strict => self
                .revision
                .with_held_source(|s| s.read_at(offset, output))
                .unwrap_or(-1),
            ReaderSourceMode::MustReload(_) | ReaderSourceMode::Closed => -1,
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
            ReaderSourceMode::MustReload(_) | ReaderSourceMode::Closed => false,
        }
    }
}
