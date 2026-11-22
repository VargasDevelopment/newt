// SPDX-License-Identifier: MIT
//! Reader-only CrossInk sum from already-frozen Newt statistics records.
use super::*;
use newt_core::{book_stats_import::{cross_ink_import_frozen, import_book_cross_ink},
    save_notice::ReadingSaveNotice, reading_stats_service::StatsResult};

impl<N: nvs::Native> MountedCheckpoint<'_, N> {
    // Import storage locals do not belong to the continuous reader driver.
    #[inline(never)]
    pub(super) fn import_book<'io, P: CardPort>(&mut self, io: &'io UiIo, card: &mut P,
        resources: &mut Resources<'_, '_, '_, '_, '_, P::Port<'io>>,
        reader: &PhasedReader<'_, '_>, owner: &mut ExternalOwner, ui: &ReaderUiState,
        digest: &Digest, verified: bool, request: &BookRequest<'_>, saves: &mut ReadingSaveNotice,
        sha256: Sha256<'_>,
    ) -> (bool, bool) where P::Port<'io>: Directory {
        let marks = owner.tracker.stats_marks();
        if marks.import_checked || !marks.lease_checked || request.cancelled() { return (false, false); }
        let Some(book) = panel_statistics::stats_book(self.checkpoint, digest, verified, owner, reader)
            else { return (false, false); };
        let mut access = resources.pages.0.access();
        let mut medium = MountedStateMedium::new(io, Some(&mut access));
        let mut native = card.port(io);
        let mut store = ReadingStateRouter::new(NvsBlobBackend::new(&mut *self.nvs),
            &mut *self.facts, &mut medium, &mut native);
        if !saves.import_complete() {
            if saves.import_settled() || !cross_ink_import_frozen(&mut store) {
                owner.tracker.stats_marks_mut().import_checked = true;
                return (false, false);
            }
            saves.set_import_complete(true);
        }
        let mut wrote = false;
        let result = import_book_cross_ink(&mut store, &book.digest,
            &mut || request.cancelled(), sha256, &mut wrote);
        if result != StatsResult::Cancelled { owner.tracker.stats_marks_mut().import_checked = true; }
        (true, wrote && ui.panel == newt_reader_ui::ReaderPanel::Statistics)
    }
}
