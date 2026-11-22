// SPDX-License-Identifier: MIT
//! Frozen library_source_verification.h and library_reader_save.h live metadata.
//! Both session and strict ownerless identity use the existing Pages owner.
use core::cell::Cell;
use newt_core::{progress::{Locator, Progress}, progress_store::PeerLocators};
use newt_phased_reader::{PhasedReader, imported_revision::Digest};
use crate::reader_metadata::{self, IdentityStorage};
use newt_reader_navigation::{Estimate, Section};

/// Metadata may borrow the live reader only after parser ownership returns.
pub(crate) trait MetadataReader {
    fn section_at(&self, at: Locator) -> Section<'_>;
    fn progress_estimate(&self) -> Estimate;
    fn overall_estimated(&self) -> bool;
}
impl MetadataReader for PhasedReader<'_, '_> {
    fn section_at(&self, at: Locator) -> Section<'_> { self.section_at(at) }
    fn progress_estimate(&self) -> Estimate { self.progress_estimate() }
    fn overall_estimated(&self) -> bool { self.info().overall_estimated }
}

/// Caller binds the saved book/source and defers pinned pages before loading
/// durable peers. True means an observed stop owes a retry, not a failed write.
// Keep cold metadata locals out of the persistent reader loop's frame.
#[inline(never)]
pub(crate) fn publish<S: IdentityStorage>(
    pages: &mut S, reader: &dyn MetadataReader,
    digest: &Digest, saved: &Progress, peers: &PeerLocators,
    source_verified: bool, title_deferred: bool, session_present: bool,
    delayed: bool, producing: bool, stop: &dyn Fn() -> bool,
) -> bool {
    let eligible = !title_deferred && source_verified;
    if eligible && !producing {
        let _ = reader_metadata::save_checkpoint(
            pages, digest, saved.locator, reader.section_at(saved.locator), peers,
            |at| reader.section_at(at), stop,
        );
    }
    if stop() { return true; }

    // The strict ownerless route publishes identity only; estimates require
    // the session, as in saveCheckpointEstimate/saveEstimate.
    if !eligible || !session_present || !reader.overall_estimated() || producing { return false; }
    let estimate = reader.progress_estimate();
    // A chapter turn cannot replace durable weights for an unacknowledged spine.
    if delayed && (estimate.spine != saved.locator.spine
        || saved.locator.character > estimate.chapter_chars) { return false; }

    let interrupted = Cell::new(false);
    let _ = reader_metadata::estimate::save(pages, digest, estimate, &|| {
        let stopped = interrupted.get() || stop();
        interrupted.set(stopped);
        stopped
    });
    pages.close_checkpoint_storage();
    // An already-equal record never calls the pre-publication stop callback.
    interrupted.get()
}
