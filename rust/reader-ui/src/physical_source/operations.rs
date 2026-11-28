// SPDX-License-Identifier: MIT
//! Operation-local source authority; no callback enters a persistent owner.
use super::{sealed, Digest, Interrupted, PhysicalSource, PhysicalSourceRef, ReaderSource};
use crate::ExternalOwner;
use newt_core::selection_authority::SelectionAuthority;
use newt_core::zip_window::source::BookSource;
use newt_phased_reader::PhasedReader;

/// The caller hashes the SAME raw source with its actual SDK/Duty context.
/// True requires a completed full digest equal to expected; cancellation and
/// failed/changed sources must return false, never authorize a later open.
pub type SourceAuthorizer<'op> = dyn FnMut(
    &mut dyn BookSource, &Digest, &mut [u8], &dyn Fn() -> bool,
) -> bool + 'op;

/// Closed source operations shared by the existing reader drive and Resources.
/// A matching receipt digest alone is not full-source verification.
pub trait SourceOperations: PhysicalSource {
    /// None while the source has no read path.
    fn digest(&self) -> Option<Digest>;
    fn prepare_reopen(&mut self, expected: Digest, scratch: &mut [u8],
        allowed: &dyn Fn() -> bool) -> Option<bool>;
    fn source_verified(&self) -> bool;
    /// The full check is still running and has not failed (#238). Only
    /// read-only views may use this; it never authorizes a write or an open.
    fn source_verification_pending(&self) -> bool { false }
    fn with_producer_source<R>(&mut self,
        work: impl FnOnce(Option<&mut ReaderSource<'_>>) -> R) -> R;
}

impl SourceOperations for ReaderSource<'_> {
    fn digest(&self) -> Option<Digest> { ReaderSource::digest(self) }
    fn prepare_reopen(&mut self, expected: Digest, scratch: &mut [u8],
        allowed: &dyn Fn() -> bool) -> Option<bool> {
        ReaderSource::prepare_reopen(self, expected, scratch, allowed)
    }
    fn source_verified(&self) -> bool { true }
    fn with_producer_source<R>(&mut self,
        work: impl FnOnce(Option<&mut ReaderSource<'_>>) -> R) -> R {
        work(Some(self))
    }
}

/// A short raw-source view. It carries neither a receipt nor an import owner.
/// Bind the required authorizer before entering physical reader operations.
pub struct StrictSource<'op> {
    pub(in crate::physical_source) source: &'op mut dyn BookSource,
    pub(in crate::physical_source) digest: &'op Digest,
    pub(in crate::physical_source) check: StrictCheck,
    pub(in crate::physical_source) open_authorized: bool,
}

/// What this strict view knows about its bytes. Each loan is short, so a
/// cancel's owed re-hash rides on the owner into the next loan (#199).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::physical_source) enum StrictCheck {
    /// Never hashed in this view: only an open may authorize it.
    Unchecked,
    Verified,
    /// A cancel stopped an action or repaint. Nothing reads until the owner
    /// hashes the source again under the same digest.
    MustReload,
}
impl<'op> StrictSource<'op> {
    pub fn new(source: &'op mut dyn BookSource, digest: &'op Digest) -> Self {
        Self { source, digest, check: StrictCheck::Unchecked, open_authorized: false }
    }
    /// This descriptor was fully hashed against `expected` earlier in this open,
    /// and any intervening reopen was fully re-hashed. Firmware establishes this
    /// in prepare_image (reader_image_open.rs) and reader_index_reopen.rs.
    /// A matching index/receipt digest alone does not meet this precondition.
    pub fn from_hashed_descriptor(source: &'op mut dyn BookSource, expected: &'op Digest) -> Self {
        Self { source, digest: expected, check: StrictCheck::Verified, open_authorized: true }
    }
    /// Relend only the actual selected FD which opened this reader. Selection
    /// owns full D; the tracker's book binds its existing successful owner.
    /// The source probe checks the reader's admitted byte count and live FD,
    /// not a fresh digest. This grants no first-open/reopen bypass.
    pub fn for_live_reader(source: &'op mut dyn BookSource,
        selection: &'op SelectionAuthority, owner: &ExternalOwner,
        reader: &mut PhasedReader<'_, '_>) -> Option<Self> {
        Self::for_live_inner(source, selection, owner, reader, true)
    }
    /// Relend the selected FD on exit after a cancelled or timed-out live
    /// reader action cleared `owner.state.opened()` while keeping `has_progress()`.
    pub fn for_cancelled_exit(source: &'op mut dyn BookSource,
        selection: &'op SelectionAuthority, owner: &ExternalOwner,
        reader: &mut PhasedReader<'_, '_>) -> Option<Self> {
        Self::for_live_inner(source, selection, owner, reader, false)
    }
    fn for_live_inner(source: &'op mut dyn BookSource,
        selection: &'op SelectionAuthority, owner: &ExternalOwner,
        reader: &mut PhasedReader<'_, '_>, require_opened: bool) -> Option<Self> {
        let ready = if require_opened {
            owner.state.opened() && reader.opened()
        } else {
            owner.state.has_progress()
        };
        if !selection.active() || !ready
            || owner.tracker.snapshot(0).book != selection.book_id()
            || selection.identity().digest != *selection.digest()
            || (reader.opened() && !reader.source_available(source)) {
            return None;
        }
        let check = if owner.strict_must_reload { StrictCheck::MustReload } else { StrictCheck::Verified };
        Some(Self { source, digest: selection.digest(), check, open_authorized: false })
    }
    pub fn with_reopen_authorizer(self, authorize: &'op mut SourceAuthorizer<'op>,
        allowed: &'op dyn Fn() -> bool) -> StrictOperations<'op> {
        StrictOperations { source: self, authorize, allowed }
    }
}
impl BookSource for StrictSource<'_> {
    fn size(&self) -> u64 { self.source.size() }
    fn read_at(&mut self, offset: u64, output: &mut [u8]) -> i32 {
        self.source.read_at(offset, output)
    }
}

/// All fields are borrowed for one operation; there is no optional authorizer.
pub struct StrictOperations<'op> {
    pub(crate) source: StrictSource<'op>,
    pub(crate) authorize: &'op mut SourceAuthorizer<'op>,
    pub(crate) allowed: &'op dyn Fn() -> bool,
}
impl StrictOperations<'_> {
    /// The digest this raw view is bound to.
    pub(crate) fn expected(&self) -> Digest { *self.source.digest }
    pub(crate) fn take_open_authorization(&mut self) -> Option<bool> {
        core::mem::take(&mut self.source.open_authorized).then(|| (self.allowed)())
    }
    pub(crate) fn authorize_open(&mut self, scratch: &mut [u8]) -> bool {
        let verified = (self.authorize)(self.source.source, self.source.digest, scratch, self.allowed);
        self.source.check = if verified { StrictCheck::Verified } else { StrictCheck::Unchecked };
        verified
    }
    /// A cancel stopped this view's action or repaint: nothing reads it until
    /// `reload` hashes it again.
    pub(crate) fn close_after_cancel(&mut self) {
        self.source.check = StrictCheck::MustReload;
    }
    /// The strict side of the owner's one reload gate: hash again what a
    /// cancel closed. Readable and unchecked views pass through unchanged.
    pub(crate) fn reload(&mut self, scratch: &mut [u8]) -> Result<(), Interrupted> {
        if self.source.check != StrictCheck::MustReload || self.authorize_open(scratch) {
            return Ok(());
        }
        // authorize_open left it Unchecked: a non-Open action then gives Stale.
        if (self.allowed)() { Err(Interrupted::Changed) } else {
            self.source.check = StrictCheck::MustReload;
            Err(Interrupted::Cancelled)
        }
    }
    /// The reader's exit after a cancel, as `ReaderSource::reload_closed`.
    /// True when readable.
    pub fn reload_closed(&mut self, scratch: &mut [u8]) -> bool {
        self.reload(scratch).is_ok() && self.source.check == StrictCheck::Verified
    }
    /// True while a cancel's re-hash is owed. The owner keeps it across loans.
    pub(crate) fn must_reload(&self) -> bool { self.source.check == StrictCheck::MustReload }
}
impl SourceOperations for StrictOperations<'_> {
    fn digest(&self) -> Option<Digest> { Some(*self.source.digest) }
    fn prepare_reopen(&mut self, expected: Digest, scratch: &mut [u8],
        allowed: &dyn Fn() -> bool) -> Option<bool> {
        self.source.check = StrictCheck::Unchecked;
        self.source.open_authorized = false;
        if expected != *self.source.digest
            || !(self.authorize)(self.source.source, &expected, scratch, allowed) {
            return None;
        }
        self.source.check = StrictCheck::Verified;
        self.source.open_authorized = true;
        Some(false)
    }
    fn source_verified(&self) -> bool { self.source.check == StrictCheck::Verified }
    fn with_producer_source<R>(&mut self,
        work: impl FnOnce(Option<&mut ReaderSource<'_>>) -> R) -> R { work(None) }
}
impl BookSource for StrictOperations<'_> {
    fn size(&self) -> u64 { self.source.size() }
    fn read_at(&mut self, offset: u64, output: &mut [u8]) -> i32 {
        self.source.read_at(offset, output)
    }
}
impl sealed::Sealed for StrictOperations<'_> {}
impl PhysicalSource for StrictOperations<'_> {
    fn with_physical_source<R>(&mut self,
        operation: impl for<'loan, 'source, 'hooks>
            FnOnce(PhysicalSourceRef<'loan, 'source, 'hooks>) -> R) -> R {
        operation(PhysicalSourceRef::Strict(self))
    }
}

/// Only the selected chapter's cached pages may be shown before finish succeeds.
/// The callback captures disjoint verification/Duty state, never this revision.
pub struct PendingReaderSource<'op, 'source> {
    pub(crate) source: &'op mut ReaderSource<'source>,
    pub(crate) selected_spine: u32,
    pub(crate) finish: &'op mut SourceAuthorizer<'op>,
    pub(crate) allowed: &'op dyn Fn() -> bool,
    pub(crate) verified: bool,
    // No finish attempt has ended unverified in this loan. A cancelled
    // attempt counts as ended, so a later panel stays conservative.
    pub(crate) pending: bool,
}
impl<'source> ReaderSource<'source> {
    pub fn with_verification_gate<'op>(&'op mut self, selected_spine: u32,
        finish: &'op mut SourceAuthorizer<'op>, allowed: &'op dyn Fn() -> bool,
    ) -> PendingReaderSource<'op, 'source> {
        PendingReaderSource { source: self, selected_spine, finish, allowed, verified: false, pending: true }
    }
}
impl PendingReaderSource<'_, '_> {
    /// The pass finished this same digest; preserve the pending operation route.
    pub fn start_indexed(&mut self, indexed: &newt_phased_reader::index_build::Indexed) {
        if indexed.verified_now() && Some(*indexed.digest()) == self.source.digest() {
            self.verified = true;
            self.pending = false;
        }
    }

    pub(crate) fn finish_verification(&mut self, scratch: &mut [u8],
        allowed: &dyn Fn() -> bool) -> bool {
        if !self.verified {
            let finish = &mut *self.finish;
            self.verified = self.source.with_held_source(|raw, digest|
                finish(raw, digest, scratch, allowed)).unwrap_or(false);
            self.pending = false;
        }
        self.verified
    }
}
impl SourceOperations for PendingReaderSource<'_, '_> {
    fn digest(&self) -> Option<Digest> { self.source.digest() }
    fn prepare_reopen(&mut self, expected: Digest, scratch: &mut [u8],
        allowed: &dyn Fn() -> bool) -> Option<bool> {
        if !self.finish_verification(scratch, allowed) { return None; }
        let selected = self.source.prepare_reopen(expected, scratch, allowed);
        if selected.is_none() { self.verified = false; }
        selected
    }
    fn source_verified(&self) -> bool { self.verified }
    fn source_verification_pending(&self) -> bool { !self.verified && self.pending }
    fn with_producer_source<R>(&mut self,
        work: impl FnOnce(Option<&mut ReaderSource<'_>>) -> R) -> R {
        if self.verified { work(Some(&mut *self.source)) } else { work(None) }
    }
}
impl BookSource for PendingReaderSource<'_, '_> {
    fn size(&self) -> u64 { self.source.size() }
    fn read_at(&mut self, offset: u64, output: &mut [u8]) -> i32 {
        self.source.read_at(offset, output)
    }
}
impl sealed::Sealed for PendingReaderSource<'_, '_> {}
impl PhysicalSource for PendingReaderSource<'_, '_> {
    fn with_physical_source<R>(&mut self,
        operation: impl for<'loan, 'source, 'hooks>
            FnOnce(PhysicalSourceRef<'loan, 'source, 'hooks>) -> R) -> R {
        operation(PhysicalSourceRef::Pending(self))
    }
}
