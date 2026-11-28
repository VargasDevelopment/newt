// SPDX-License-Identifier: MIT
//! Checked receipt action exit and source authority withdrawal.
use super::*;
use newt_phased_reader::PageInfo;

/// The page the reader showed before an action. A cancelled action restores
/// it, so the old position stays authoritative until the final check passes.
#[derive(Clone, Copy)]
pub(super) struct Shown {
    info: PageInfo,
    had_page: bool,
}
impl Shown {
    pub(super) fn of(reader: &PhasedReader<'_, '_>) -> Self {
        Self { info: reader.info(), had_page: reader.has_page() }
    }
}

pub(super) fn finish_reader_action(reader: &mut PhasedReader<'_, '_>, source: &mut ReaderSource<'_>,
    request: PageRequest, shown: Shown, work: PageWork) -> PageWork {
    // Pending preserves the actual deferred verification/source for a retry.
    // A failed admission still closes before the checked receiptAction exit.
    if request == PageRequest::Open
        && !matches!(work.result, Completion::Published | Completion::VerificationPending) {
        reader.close();
        source.revision.close_receipt();
        source.mode = ReaderSourceMode::Closed;
    }
    // The final check runs before the cancel question. A cancel that lands
    // during the check refuses it and stays set, so it is still seen here.
    let fresh = finish_receipt_checks(&mut source.revision, request);
    let cancelled = work.result == Completion::Cancelled || source.revision.cancelled();
    if request != PageRequest::Open && reader.opened() && cancelled {
        // A cancel (Back or the time budget) refuses the final freshness read
        // and every read after it, so it says nothing about the book. Only a
        // page that passed that read replaces the shown one: an unchecked
        // publication is discarded, never kept and never read as Stale. The
        // source closes and the next action or repaint reloads it (#199).
        match work.result {
            Completion::Published if fresh => return work,
            Completion::Published => reader.discard_chapter_publication(shown.info, shown.had_page),
            // Deferred work read nothing the cancel could refuse.
            Completion::VerificationPending => return work,
            Completion::Captured | Completion::Failed(_) | Completion::Cancelled => {}
        }
        source.close_after_cancel();
        return PageWork::CANCELLED;
    }
    let work = reject_unless_fresh(reader, &mut source.revision, request, work, fresh);
    if !reader.opened() && work.result != Completion::VerificationPending {
        source.mode = ReaderSourceMode::Closed;
    }
    work
}

/// Ends the checks begin_receipt_action started; true when the receipt stayed fresh.
fn finish_receipt_checks(revision: &mut ImportedRevision<'_>, request: PageRequest) -> bool {
    if matches!(request, PageRequest::Chapter(_, toc) if toc != u16::MAX) {
        revision.finish_deferred_path_checks()
    } else {
        revision.finish_immutable_read()
    }
}

pub(super) fn begin_receipt_action(revision: &mut ImportedRevision<'_>, request: PageRequest) {
    if matches!(request, PageRequest::Chapter(_, toc) if toc != u16::MAX) {
        revision.begin_deferred_path_checks();
    } else {
        revision.begin_immutable_read();
    }
}
pub(super) fn finish_receipt_action(
    reader: &mut PhasedReader<'_, '_>,
    revision: &mut ImportedRevision<'_>,
    request: PageRequest,
    work: PageWork,
) -> PageWork {
    let fresh = finish_receipt_checks(revision, request);
    reject_unless_fresh(reader, revision, request, work, fresh)
}

/// A refused final check without a cancel means the book changed: close with Stale.
fn reject_unless_fresh(reader: &mut PhasedReader<'_, '_>, revision: &mut ImportedRevision<'_>,
    request: PageRequest, work: PageWork, fresh: bool) -> PageWork {
    let fragment = matches!(request, PageRequest::Chapter(_, toc) if toc != u16::MAX);
    if !fresh && (fragment || work.result == Completion::Published) {
        reject_changed_receipt(reader, revision);
        return PageWork::failed(Status::Stale);
    }
    work
}

/// Matches the pinned rare failed-exit path, before owner acknowledgment.
pub(super) fn reject_changed_receipt(reader: &mut PhasedReader<'_, '_>, revision: &mut ImportedRevision<'_>) {
    reader.close(); // withdraws selections, page plan and menu projections
    revision.close_receipt();
}
