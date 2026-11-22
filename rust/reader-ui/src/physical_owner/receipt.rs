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
    let work = finish_receipt_action(reader, &mut source.revision, request, work);
    if !reader.opened() && work.result != Completion::VerificationPending {
        source.mode = ReaderSourceMode::Closed;
    }
    work
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
    let fresh = if fragment {
        revision.finish_deferred_path_checks()
    } else {
        revision.finish_immutable_read()
    };
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
