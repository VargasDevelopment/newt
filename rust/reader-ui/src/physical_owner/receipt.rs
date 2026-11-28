// SPDX-License-Identifier: MIT
//! Checked receipt action exit and source authority withdrawal.
use super::*;

pub(super) fn finish_reader_action(reader: &mut PhasedReader<'_, '_>, source: &mut ReaderSource<'_>,
    request: PageRequest, work: PageWork) -> PageWork {
    // Pending preserves the actual deferred verification/source for a retry.
    // A failed admission still closes before the checked receiptAction exit.
    if request == PageRequest::Open
        && !matches!(work.result, Completion::Published | Completion::VerificationPending) {
        reader.close();
        source.revision.close_receipt();
        source.mode = ReaderSourceMode::Closed;
    }
    // A cancel (Back or the time budget) refuses the final freshness read and
    // every read after it, so it says nothing about the book. A failed action
    // published nothing, so the reader keeps its page. The source closes, and
    // the next action or repaint reloads it under the same digest (#199). A
    // page published from unchecked reads still closes the reader below.
    if request != PageRequest::Open && matches!(work.result, Completion::Failed(_))
        && reader.opened() && source.revision.cancelled() {
        let _ = finish_receipt_checks(&mut source.revision, request);
        source.close_after_cancel();
        return PageWork::CANCELLED;
    }
    let work = finish_receipt_action(reader, &mut source.revision, request, work);
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
    let fragment = matches!(request, PageRequest::Chapter(_, toc) if toc != u16::MAX);
    let fresh = finish_receipt_checks(revision, request);
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
