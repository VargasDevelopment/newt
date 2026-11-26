// SPDX-License-Identifier: MIT
//! Statistics records are lent only between physical repaint and composition.
#[path = "reader_statistics_data.rs"]
mod data;
use super::{
    BookRequest, CardPort, DeviceInfo, Digest, MountedCheckpoint, MountedStateMedium,
    NvsBlobBackend, ReaderCheckpoint, ReaderUiState, ReadingStateRouter, ReadingStatsRuntime,
    Resources, Status, UiIo, nvs, read_panel_stats_session_clock,
};
use data::Session;
use newt_core::{
    reading_stats::{PERMILLE_UNKNOWN, stats_counters},
    reading_stats_service::StatsBook,
    tracker::ReadingEnd,
};
use newt_phased_reader::PhasedReader;
use newt_reader_ui::ExternalOwner;

/// Statistics see the selected complete digest and the checkpoint owner's
/// acknowledgement. The live tracker contributes end/position, never counters.
pub(crate) fn stats_book(
    checkpoint: &ReaderCheckpoint,
    digest: &Digest,
    source_verified: bool,
    owner: &ExternalOwner,
    reader: &PhasedReader<'_, '_>,
) -> Option<StatsBook> {
    if !source_verified {
        return None;
    }
    stats_book_at(checkpoint, digest, owner, position_permille(owner, reader))
}

pub(super) fn position_permille(owner: &ExternalOwner, reader: &PhasedReader<'_, '_>) -> u16 {
    let page = owner.state.position(reader).page;
    if !owner.state.opened() {
        PERMILLE_UNKNOWN
    } else if page.overall_known && !page.overall_estimated && page.book_total_chars != 0 {
        (page.book_char_start.wrapping_mul(1000) / page.book_total_chars).min(1000) as u16
    } else if page.overall_known {
        u16::from(page.overall_percent.min(100)) * 10
    } else {
        PERMILLE_UNKNOWN
    }
}

pub(crate) fn stats_book_at(checkpoint: &ReaderCheckpoint, digest: &Digest,
    owner: &ExternalOwner, permille: u16,
) -> Option<StatsBook> {
    let acknowledged = checkpoint.acknowledged_for(digest)?;
    if acknowledged.book != u64::from_le_bytes(core::array::from_fn(|i| digest[i])) { return None; }
    Some(StatsBook {
        digest: *digest,
        counters: stats_counters(&acknowledged),
        end_reached: owner.tracker.stats_marks().end == ReadingEnd::Reached,
        permille,
    })
}

impl<N: nvs::Native> MountedCheckpoint<'_, N> {
    /// #285: finish the full-page repaint before State reads or overlays.
    /// The card view is a bounded read-only loan, never retained by the owner.
    #[inline(never)]
    pub(super) fn paint<'io, P: CardPort>(
        &mut self,
        io: &'io UiIo,
        card: &mut P,
        resources: &mut Resources<'_, '_, '_, '_, '_, P::Port<'io>>,
        runtime: &ReadingStatsRuntime,
        digest: &Digest,
        request: &BookRequest<'_>,
        reader: &mut PhasedReader<'_, '_>,
        revision: &mut impl newt_reader_ui::SourceOperations,
        owner: &mut ExternalOwner,
        ui: &mut ReaderUiState,
        info: &DeviceInfo<'_>,
        now: u32,
    ) -> Result<(), Status> {
        let settings = resources.settings;
        let painted = resources.owner().paint_ui(reader, revision, owner, ui, now);
        let progress = *owner.tracker.snapshot(now);
        let view = if ui.panel == newt_reader_ui::ReaderPanel::Statistics && painted.ready() {
            // As frozen panelStatsBook (#238): the paint writes nothing, so a
            // check still running on the same digest may read the records.
            let authorized = (revision.source_verified() || revision.source_verification_pending())
                && revision.digest() == *digest;
            let session = Session {
                book: stats_book(self.checkpoint, digest, authorized, owner, reader),
                progress: &progress,
                marks: owner.tracker.stats_marks(),
                interval: owner.tracker.open_interval(now),
                fold_owed: runtime.fold_owed,
                now,
                zone: request.zone,
            };
            let mut access = resources.pages.0.access();
            let mut medium = MountedStateMedium::new(io, Some(&mut access));
            let mut native = card.port(io);
            let mut store = ReadingStateRouter::new(
                NvsBlobBackend::new(&mut *self.nvs),
                &mut *self.facts,
                &mut medium,
                &mut native,
            );
            Some(data::read(
                &mut store,
                digest,
                authorized,
                newt_core::app::stats_graph_position(ui.selected as u8),
                newt_reader_ui::book_panel_days(settings),
                &session,
                || {
                    let mut utc = 0;
                    let clock = read_panel_stats_session_clock(
                        resources.panel,
                        request.zone,
                        Some(&mut utc),
                    );
                    (clock, utc)
                },
            ))
        } else {
            None
        };
        resources.owner().compose_native(
            reader,
            owner,
            ui,
            settings,
            info,
            &progress,
            painted,
            view.as_ref(),
        )
    }
}
