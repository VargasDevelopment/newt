// SPDX-License-Identifier: MIT
//! Frozen ReaderRevisionSession's deferred digest and producer retry storage.
//! The optional session embeds this IN PLACE of its one 256-byte ledger,
//! session flags/alignment gap and producer view. It is never a second owner.
use core::mem::{ManuallyDrop, MaybeUninit, align_of, size_of};
use newt_core::zip_window::source::BookSource;
use newt_phased_reader::imported_revision::{Digest, RevisionHasher};
#[path = "reader_verification_control.rs"]
mod control;
use control::Phase;
pub use control::{PrefetchTrace, SessionControl, SessionFlags};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verification { Verified, Pending, Changed, Failed }

struct Deferred<H> { hash: H, bytes: u32, offset: u32 }
union Region<H> {
    pending: ManuallyDrop<Deferred<H>>,
    bytes: MaybeUninit<[u8; 256]>,
}

/// One physical region, shared exclusively by the pending SHA context and
/// initialized retry bytes. A short byte callback cannot retain its loan.
///
/// Relend the SAME source and expected digest to every step. The enclosing
/// selected-source owner enforces that contract, just as it does for revision
/// operation loans; this type retains no source or cancellation pointer.
pub struct ReaderVerification<H: RevisionHasher> {
    region: Region<H>,
    control: SessionControl,
}

impl<H: RevisionHasher> ReaderVerification<H> {
    pub fn new() -> Self {
        const { assert!(size_of::<Deferred<H>>() <= 256); }
        const { assert!(align_of::<Deferred<H>>() <= 16); }
        Self { region: Region { bytes: MaybeUninit::uninit() }, control: SessionControl::new() }
    }

    pub fn control(&mut self) -> &mut SessionControl { &mut self.control }

    pub fn state(&self) -> Verification {
        match self.control.phase {
            Phase::Uninitialized | Phase::Ready => Verification::Verified,
            Phase::Pending => Verification::Pending,
            Phase::Changed | Phase::ChangedRevoked => Verification::Changed,
            Phase::Failed | Phase::FailedRevoked => Verification::Failed,
        }
    }

    /// Called only after the existing hint/receipt admission. The required
    /// factory creates the actual SDK hasher at this lifecycle point. Its init
    /// precedes source.size(), then begin(), as in DeferredDigest's constructor.
    pub fn begin<S: BookSource + ?Sized>(&mut self, source: &S, make_hash: impl FnOnce() -> H) -> bool {
        self.clear();
        self.control.phase = Phase::Uninitialized;
        let mut hash = make_hash();
        let bytes = source.size() as u32;
        if !hash.begin() { return false; }
        self.region.pending = ManuallyDrop::new(Deferred { hash, bytes, offset: 0 });
        self.control.phase = Phase::Pending;
        true
    }

    /// End only a pending context. Existing initialized retry bytes survive a
    /// verified close; a later visit explicitly clears them at the frozen site.
    pub fn clear(&mut self) {
        if self.control.phase == Phase::Pending {
            // Disarm first as well for a host-only destructor unwind: Drop
            // must never attempt to destroy a partially destroyed hash twice.
            self.control.phase = Phase::Uninitialized;
            // SAFETY: Pending is published only after constructing this member.
            // Exclusive self prevents a byte or hash loan during destruction.
            unsafe { ManuallyDrop::drop(&mut self.region.pending); }
        } else if self.control.phase != Phase::Ready {
            self.control.phase = Phase::Uninitialized;
        }
    }

    /// Terminal tags remember successful revocation, without another owner
    /// field or repeated directory work on every idle tick. begin/reset clear it.
    pub fn index_revoked(&self) -> bool {
        matches!(self.control.phase,Phase::ChangedRevoked|Phase::FailedRevoked)
    }
    pub fn mark_index_revoked(&mut self) {
        self.control.phase=match self.control.phase {
            Phase::Changed=>Phase::ChangedRevoked,Phase::Failed=>Phase::FailedRevoked,other=>other,
        };
    }

    pub fn mark_changed(&mut self) { self.clear(); self.control.phase = Phase::Changed; }
    pub fn mark_failed(&mut self) { self.clear(); self.control.phase = Phase::Failed; }
    /// A new source preparation retires the prior verifier/producer epoch.
    pub fn reset(&mut self) { self.clear(); self.control = SessionControl::new(); }
    pub fn producer_ready(&self) -> bool { self.control.phase == Phase::Ready }
    pub fn adopt_changed(&mut self) -> bool {
        if self.state() != Verification::Changed { return false; }
        self.control.phase = Phase::Uninitialized;
        true
    }

    /// LibraryReaderSelection calls this at its first eligible producer visit.
    /// It is unavailable during verification; it neither allocates nor hashes.
    pub fn reset_producer_visit(&mut self) -> bool {
        if self.state() != Verification::Verified { return false; }
        // SAFETY: no pending context or byte loan exists. Write the exact
        // region in place before publishing Ready, without an array temporary.
        unsafe { core::ptr::addr_of_mut!(self.region.bytes).cast::<u8>().write_bytes(0, 256); }
        self.control.producer_view = u16::MAX;
        self.control.phase = Phase::Ready;
        true
    }

    pub fn with_producer<R>(&mut self, work: impl for<'loan> FnOnce(&'loan mut [u8; 256]) -> R)
        -> Option<R> {
        self.with_producer_visit(|bytes, _view| work(bytes))
    }

    pub fn with_producer_visit<R>(&mut self,
        work: impl for<'loan> FnOnce(&'loan mut [u8; 256], &'loan mut u16) -> R,
    ) -> Option<R> {
        if self.control.phase != Phase::Ready { return None; }
        // SAFETY: only reset_producer_visit publishes Ready, after initializing
        // every byte. The callback's result cannot borrow this exclusive loan.
        Some(work(unsafe { self.region.bytes.assume_init_mut() }, &mut self.control.producer_view))
    }

    /// Cancellation after I/O pauses at the SAME offset before any hash credit.
    /// EOF cancellation likewise retains the unfinished context for a retry.
    pub fn step<S: BookSource + ?Sized>(&mut self, source: &mut S, expected: &Digest,
        scratch: &mut [u8], allowed: &mut dyn FnMut() -> bool) -> bool {
        self.step_with_update(source, expected, scratch, allowed, &mut |hash, bytes| hash.update(bytes))
    }

    pub fn step_with_update<S: BookSource + ?Sized>(&mut self, source: &mut S, expected: &Digest,
        scratch: &mut [u8], allowed: &mut dyn FnMut() -> bool,
        update: &mut dyn FnMut(&mut H, &[u8]) -> bool,
    ) -> bool {
        if self.control.phase != Phase::Pending { return self.state() == Verification::Verified; }
        if scratch.is_empty() || !allowed() { return false; }
        // SAFETY: Pending identifies the initialized union member. No byte loan
        // can coexist; this borrow ends before any transition destroys it.
        let pending = unsafe { &mut *self.region.pending };
        if pending.offset < pending.bytes {
            let want = scratch.len().min(32_768).min((pending.bytes - pending.offset) as usize);
            let got = source.read_at(u64::from(pending.offset), &mut scratch[..want]);
            if !allowed() { return false; }
            if got <= 0 || got as usize > want || source.size() != u64::from(pending.bytes)
                || !update(&mut pending.hash, &scratch[..got as usize]) {
                self.clear(); self.control.phase = Phase::Failed;
                return false;
            }
            pending.offset += got as u32;
        }
        if pending.offset != pending.bytes { return false; }
        let live = source.size() != 0 && source.read_at(0, &mut []) == 0;
        if !allowed() { return false; }
        let mut actual = [0; 32];
        let finished = live && pending.hash.finish(&mut actual);
        self.clear(); // Free the context before publishing the terminal state.
        self.control.phase = if !finished { Phase::Failed } else if actual == *expected {
            Phase::Uninitialized
        } else { Phase::Changed };
        self.state() == Verification::Verified
    }

    pub fn finish<S: BookSource + ?Sized>(&mut self, source: &mut S, expected: &Digest,
        scratch: &mut [u8], allowed: &mut dyn FnMut() -> bool) -> bool {
        self.finish_with_update(source, expected, scratch, allowed, &mut |hash, bytes| hash.update(bytes))
    }

    pub fn finish_with_update<S: BookSource + ?Sized>(&mut self, source: &mut S, expected: &Digest,
        scratch: &mut [u8], allowed: &mut dyn FnMut() -> bool,
        update: &mut dyn FnMut(&mut H, &[u8]) -> bool,
    ) -> bool {
        if scratch.is_empty() { return false; }
        while self.control.phase == Phase::Pending && allowed() {
            self.step_with_update(source, expected, scratch, allowed, update);
        }
        self.state() == Verification::Verified
    }
}

impl<H: RevisionHasher> Drop for ReaderVerification<H> {
    fn drop(&mut self) { self.clear(); }
}
