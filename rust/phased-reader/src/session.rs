//! MIT. `RevisionImportSession` for phased reader source verification.
//! Borrowed collaborators never retain the source passed to one operation.
use super::{Digest, ImportedRevision, live};
use crate::zip_container::{Entry, Status};
use newt_core::member_check::Checked;
use crate::zip_window::source::BookSource;
use newt_core::{admission::WorkEvent, reader_action_profile::ActionStage,
    reader_work_profile::ReaderOpenRoute};
#[cfg(feature = "dev-usb")]
use newt_core::admission::WorkKind;

/// Existing PhasedReader collaboration boundary. `commit` publishes only staged
/// canonical selection scalars, performs no I/O/callback, and cannot fail.
pub trait Reader {
    fn trace(&self, _event: WorkEvent) {}
    fn close(&mut self);
    fn strict_open(&mut self, source: &mut dyn BookSource) -> Status;
    fn spine_count(&self) -> u32;
    fn stage(
        &mut self,
        source: &mut dyn BookSource,
        spine: u32,
        plain: bool,
    ) -> Result<Checked, Status>;
    fn commit(&mut self, spine: u32);
    /// An owning adapter composes selection AND verification inside its real
    /// parse loan. None keeps the original legacy acquire/finish policy intact.
    fn stage_original_verified(
        &mut self,
        _revision: &mut ImportedRevision<'_>,
        _spine: u32,
        _plain: bool,
    ) -> Option<Result<(), Status>> {
        None
    }

    // Only the owning-stage route uses this rollback hook; legacy selection
    // keeps its original policy and never enters it.
    fn discard_owned_stage(&mut self) {}

    // Import uses original revision scratch, but still returns the reader's
    // actual parser lease after verification and before its final guard.
    fn acquire_original(&mut self) -> Result<(), Status> {
        Ok(())
    }
    fn finish_original(&mut self) -> Result<(), Status> {
        Ok(())
    }
}

struct ActionScope<'a, R: Reader + ?Sized> {
    #[cfg(feature = "dev-usb")] reader: &'a R,
    #[cfg(not(feature = "dev-usb"))] unused: core::marker::PhantomData<&'a R>,
}
impl<'a, R: Reader + ?Sized> ActionScope<'a, R> {
    fn new(reader: &'a R, stage: ActionStage) -> Self {
        #[cfg(feature = "dev-usb")]
        reader.trace(WorkEvent::new(WorkKind::ActionBoundary,
            i32::from(stage as u8 | ActionStage::Push as u8)));
        #[cfg(not(feature = "dev-usb"))]
        let _ = (reader, stage);
        Self {
            #[cfg(feature = "dev-usb")] reader,
            #[cfg(not(feature = "dev-usb"))] unused: core::marker::PhantomData,
        }
    }
}
impl<R: Reader + ?Sized> Drop for ActionScope<'_, R> {
    fn drop(&mut self) {
        #[cfg(feature = "dev-usb")]
        self.reader.trace(WorkEvent::new(WorkKind::ActionBoundary, ActionStage::Restore as i32));
    }
}
fn route(reader: &impl Reader, value: ReaderOpenRoute) {
    #[cfg(feature = "dev-usb")]
    reader.trace(WorkEvent::new(WorkKind::OpenRoute, value as i32));
    #[cfg(not(feature = "dev-usb"))]
    let _ = (reader, value);
}
/// Only completed zero-page chapters permit scanning the next spine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Build {
    Ready,
    Empty,
    Failed(Status),
}
/// Cache owns no Reader reference; the owner passes disjoint loans at each call.
pub trait Cache<R: Reader> {
    fn build(
        &mut self,
        source: &mut dyn BookSource,
        digest: &Digest,
        spine: u32,
        reader: &mut R,
    ) -> Build;
    fn stored(&mut self, digest: &Digest, spine: u32, reader: &mut R) -> bool;
}

pub struct Session<'s, 'source, R: Reader, C: Cache<R> + ?Sized> {
    pub revision: &'s mut ImportedRevision<'source>,
    pub reader: &'s mut R,
    pub cache: &'s mut C,
}
impl<'source, R: Reader, C: Cache<R> + ?Sized> Session<'_, 'source, R, C> {
    pub fn import(&mut self, source: &'source mut dyn BookSource) -> Status {
        self.reader.close();
        if !self.revision.begin_import(source) {
            return self.reject(Status::IoError);
        }
        self.finish_prepared_import()
    }
    pub fn finish_prepared_import(&mut self) -> Status {
        self.reader.close();
        if !self.revision.provisional() || !self.revision.good() {
            return Status::Unsupported;
        }
        route(self.reader, ReaderOpenRoute::StrictAdmission);
        let mut status = self.reader.strict_open(self.revision);
        if status == Status::Ok {
            status = Status::NotFound;
            let digest = *self.revision.digest();
            for spine in 0..self.reader.spine_count() {
                match self.cache.build(self.revision, &digest, spine, self.reader) {
                    Build::Empty => continue,
                    Build::Failed(error) => {
                        status = error;
                        break;
                    }
                    Build::Ready => {
                        // Import uses the owner's original scratch, as pinned.
                        status = self.select_original(spine, true);
                        break;
                    }
                }
            }
        }
        if status == Status::Ok {
            let sealed = self.revision.finish_import();
            route(self.reader, if sealed { ReaderOpenRoute::SealSucceeded } else { ReaderOpenRoute::SealFailed });
            if !sealed { status = Status::IoError; }
        }
        if status != Status::Ok {
            return self.reject(status);
        }
        status
    }
    /// Strict EPUB admission, then promotion; a writable page cache is optional.
    pub fn finish_prepared_open(&mut self) -> Status {
        self.finish_prepared_open_with(false)
    }
    /// The owning reader may consume the held source for pinned strict fallback.
    pub fn finish_prepared_open_retaining_source(&mut self) -> Status {
        self.finish_prepared_open_with(true)
    }
    fn finish_prepared_open_with(&mut self, retain_source: bool) -> Status {
        self.reader.close();
        if !self.revision.provisional() || !self.revision.good() {
            return Status::Unsupported;
        }
        route(self.reader, ReaderOpenRoute::StrictAdmission);
        let mut status = self.reader.strict_open(self.revision);
        if status == Status::Ok {
            let sealed = self.revision.finish_import();
            route(self.reader, if sealed { ReaderOpenRoute::SealSucceeded } else { ReaderOpenRoute::SealFailed });
            if !sealed { status = Status::IoError; }
        }
        if status != Status::Ok {
            if retain_source && status == Status::IoError {
                self.reader.close();
                return status;
            }
            return self.reject(status);
        }
        status
    }
    /// A real prepared-open route while display storage is loaned. No owning
    /// Action is stored in Session. Its promotion is gated by actual admission.
    pub fn finish_prepared_open_lent(&mut self, lent: &mut [u8]) -> Status {
        self.reader.close();
        if !self.revision.provisional() || !self.revision.good() {
            return Status::Unsupported;
        }
        let status = {
            let Some(mut action) = self.revision.lend_action_storage(lent) else {
                return Status::Unsupported;
            };
            route(self.reader, ReaderOpenRoute::StrictAdmission);
            let mut status = self.reader.strict_open(&mut action);
            if status == Status::Ok {
                let sealed = action.finish_import();
                route(self.reader, if sealed { ReaderOpenRoute::SealSucceeded } else { ReaderOpenRoute::SealFailed });
                if !sealed { status = Status::IoError; }
            }
            status
        }; // Withdraw retained scratch tags before returning to the owner.
        if status != Status::Ok {
            return self.reject(status);
        }
        if !self.revision.good() {
            return self.reject(Status::Stale);
        }
        Status::Ok
    }
    /// Value parameter snapshots all 256 bits before load/reset (even if the
    /// caller obtained the value from this revision's previous digest).
    pub fn warm_open(
        &mut self,
        source: &'source mut dyn BookSource,
        expected: Digest,
        spine: u32,
        lease: &mut impl ParseLease,
    ) -> Status {
        self.reader.close();
        let loaded = {
            let _action = ActionScope::new(self.reader, ActionStage::ReceiptLoad);
            self.revision.load(source)
        };
        if !loaded || *self.revision.digest() != expected {
            return self.reject(Status::Stale);
        }
        route(self.reader, ReaderOpenRoute::ReceiptLoaded);
        route(self.reader, ReaderOpenRoute::CompactAdmission);
        let stamp = stamp(self.revision);
        let status = self.select(spine, lease);
        if status != Status::Ok && stamp == self::stamp(self.revision) {
            return self.reject(status);
        }
        status
    }
    /// Source was loaded by the owner; use the real owned stage/verify path.
    /// No ParseLease can substitute for the physical SDK lender.
    pub fn open_loaded_owned(&mut self, expected: Digest, spine: u32) -> Status {
        self.reader.close();
        if self.revision.provisional()
            || !self.revision.good()
            || *self.revision.digest() != expected
        {
            return self.reject(Status::Stale);
        }
        let before = stamp(self.revision);
        let status = self.stage_owned(spine);
        if status == Status::Ok {
            self.reader.commit(spine);
        }
        if status != Status::Ok && before == stamp(self.revision) {
            return self.reject(status);
        }
        status
    }
    /// Pinned openRevision's warm route (reader_revision_session.h:188-213)
    /// over a receipt the owner already selected: warmOpenSelected at `spine`,
    /// then, on any miss, a strict reimport of the source this revision still
    /// holds, recovered before a reject could drop it. The reimport must name
    /// `expected` exactly; finish_prepared_open admits and promotes it, and
    /// the owner then normalizes the locator.
    pub fn open_selected_owned(&mut self, expected: Digest, spine: u32) -> Status {
        self.open_selected_owned_with(expected, spine, false)
    }
    pub fn open_selected_owned_retaining_source(&mut self, expected: Digest, spine: u32) -> Status {
        self.open_selected_owned_with(expected, spine, true)
    }
    fn open_selected_owned_with(
        &mut self,
        expected: Digest,
        spine: u32,
        retain_source: bool,
    ) -> Status {
        self.reader.close();
        if self.revision.provisional() {
            return self.reject(Status::Stale);
        }
        let before = stamp(self.revision);
        route(self.reader, ReaderOpenRoute::WarmAttempt);
        let warm = if self.revision.good() && *self.revision.digest() == expected {
            self.stage_owned(spine)
        } else {
            Status::Stale
        };
        route(self.reader, if warm == Status::Ok { ReaderOpenRoute::WarmHit } else { ReaderOpenRoute::WarmMiss });
        if warm == Status::Ok {
            self.reader.commit(spine);
            return warm;
        }
        // bindWarm: a stage that reloaded or reset the revision left a
        // selection, and a source, this attempt no longer owns.
        if before != stamp(self.revision) {
            return warm;
        }
        // allowed() && live(source): FileBookSource's probe also answers its
        // cancel authority.
        if !self.held_source_live() {
            return self.reject(Status::Stale);
        }
        route(self.reader, ReaderOpenRoute::ImportBegin);
        let revision = &mut *self.revision;
        if !revision.core.reimport(revision.scratch) {
            route(self.reader, ReaderOpenRoute::ImportFailed);
            let status = if self.held_source_live() {
                Status::IoError
            } else {
                Status::Stale
            };
            if retain_source && status == Status::IoError {
                self.reader.close();
                return status;
            }
            return self.reject(status);
        }
        route(self.reader, ReaderOpenRoute::ImportVerified);
        if *self.revision.digest() != expected {
            return self.reject(Status::Stale);
        }
        self.finish_prepared_open_with(retain_source)
    }
    fn held_source_live(&mut self) -> bool {
        self.revision
            .core
            .source
            .as_deref_mut()
            .is_some_and(|source| live(source))
    }
    pub fn select(&mut self, spine: u32, lease: &mut impl ParseLease) -> Status {
        let status = self.stage_verified(spine, lease);
        if status == Status::Ok {
            self.reader.commit(spine);
        }
        status
    }
    /// Stage and verify without publishing selection. Cache showStaged owns the
    /// commit, after the page and its source guard have both succeeded.
    pub fn stage_verified(&mut self, spine: u32, lease: &mut impl ParseLease) -> Status {
        if self.revision.provisional() {
            return Status::Unsupported;
        }
        let before = stamp(self.revision);
        let result = self.stage_leased(spine, lease);
        let released = lease.finish();
        if before != stamp(self.revision) || (result.is_ok() && !self.revision.good()) {
            return Status::Stale;
        }
        if let Err(error) = released {
            return error;
        }
        match result {
            Ok(()) => { route(self.reader, ReaderOpenRoute::ChapterVerified); Status::Ok },
            Err(error) => error,
        }
    }
    fn stage_leased(&mut self, spine: u32, lease: &mut impl ParseLease) -> Result<(), Status> {
        let before = stamp(self.revision);
        let tail = lease.acquire()?;
        if before != stamp(self.revision) {
            return Err(Status::Stale);
        }
        let plain = self
            .cache
            .stored(self.revision.digest(), spine, self.reader);
        let borrow = self.revision.core.valid
            && !self.revision.core.provisional
            && self.revision.core.block.is_some()
            && self.revision.scratch.len() == 256;
        if let Some(tail) = tail.filter(|_| borrow) {
            let mut action = self.revision.selection_scratch(tail).ok_or(Status::Stale)?;
            let entry = self.reader.stage(&mut action, spine, plain)?;
            route(self.reader, ReaderOpenRoute::CanonicalSelected);
            {
                let _trace = ActionScope::new(self.reader, ActionStage::ChapterVerify);
                let (at, bytes) = chapter_range(&mut action, &entry)?;
                if !action.verify_range(at, bytes) { return Err(Status::Stale); }
            }
            // Drop restores the original scratch role and invalidates retained
            // tags, including on every early error. Core never held a tail ptr.
        } else {
            let entry = self.reader.stage(self.revision, spine, plain)?;
            route(self.reader, ReaderOpenRoute::CanonicalSelected);
            {
                let _trace = ActionScope::new(self.reader, ActionStage::ChapterVerify);
                let (at, bytes) = chapter_range(self.revision, &entry)?;
                if !self.revision.verify_range(at, bytes) { return Err(Status::Stale); }
            }
        }
        Ok(())
    }
    /// Owner-composed route: no external ParseLease or pre-created parse views.
    pub fn stage_owned(&mut self, spine: u32) -> Status {
        if self.revision.provisional() {
            return Status::Unsupported;
        }
        let before = stamp(self.revision);
        let plain = self
            .cache
            .stored(self.revision.digest(), spine, self.reader);
        let Some(result) = self
            .reader
            .stage_original_verified(self.revision, spine, plain)
        else {
            return Status::Unsupported;
        };
        if before != stamp(self.revision) || (result.is_ok() && !self.revision.good()) {
            self.reader.discard_owned_stage();
            return Status::Stale;
        }
        match result {
            Ok(()) => { route(self.reader, ReaderOpenRoute::ChapterVerified); Status::Ok },
            Err(e) => {
                self.reader.discard_owned_stage();
                e
            }
        }
    }
    fn select_original(&mut self, spine: u32, plain: bool) -> Status {
        let before = stamp(self.revision);
        if let Some(result) = self
            .reader
            .stage_original_verified(self.revision, spine, plain)
        {
            if !self.revision.good() || before != stamp(self.revision) {
                self.reader.discard_owned_stage();
                return Status::Stale;
            }
            if let Err(error) = result {
                self.reader.discard_owned_stage();
                return error;
            }
            route(self.reader, ReaderOpenRoute::ChapterVerified);
            self.reader.commit(spine);
            return Status::Ok;
        }

        if let Err(error) = self.reader.acquire_original() {
            return error;
        }
        let result = if before != stamp(self.revision) {
            Err(Status::Stale)
        } else {
            self.reader
                .stage(self.revision, spine, plain)
                .and_then(|entry| {
                    route(self.reader, ReaderOpenRoute::CanonicalSelected);
                    let _trace = ActionScope::new(self.reader, ActionStage::ChapterVerify);
                    let (at, bytes) = chapter_range(self.revision, &entry)?;
                    if self.revision.verify_range(at, bytes) {
                        Ok(())
                    } else {
                        Err(Status::Stale)
                    }
                })
        };
        let released = self.reader.finish_original();
        if !self.revision.good() || before != stamp(self.revision) {
            return Status::Stale;
        }
        if let Err(error) = released {
            return error;
        }
        if let Err(error) = result {
            return error;
        }
        route(self.reader, ReaderOpenRoute::ChapterVerified);
        self.reader.commit(spine);
        Status::Ok
    }
    fn reject(&mut self, status: Status) -> Status {
        self.reader.close();
        self.revision.reset();
        status
    }
}
pub(crate) fn stamp(revision: &ImportedRevision<'_>) -> (u64, u64) {
    (revision.core.epoch, revision.core.generation)
}
pub(crate) fn chapter_range(
    source: &mut dyn BookSource,
    entry: &Entry,
) -> Result<(u64, u64), Status> {
    let mut header = [0; 30];
    let at = u64::from(entry.local_header_offset);
    if source.read_at(at, &mut header) != 30 || header[..4] != [0x50, 0x4b, 3, 4] {
        return Err(Status::Stale);
    }
    let name = u16::from_le_bytes([header[26], header[27]]);
    let extra = u16::from_le_bytes([header[28], header[29]]);
    Ok((
        at,
        30 + u64::from(name) + u64::from(extra) + u64::from(entry.compressed_size),
    ))
}

#[path = "metadata_reader.rs"]
pub mod metadata_reader;

#[path = "session/lease.rs"]
mod lease;
pub use lease::{BorrowedParseLease, LeaseCallbacks, ParseLease};
