// SPDX-License-Identifier: MIT
//! One actual exit checkpoint/fold/destination operation, shared with restart.
use super::*;

impl<N: nvs::Native> MountedCheckpoint<'_, N> {
    pub(crate) fn finish_exit_captured<'io, P: CardPort>(
        &mut self, io: &'io UiIo, card: &mut P,
        mut resources: &mut Resources<'_, '_, '_, '_, '_, P::Port<'io>>,
        reader: &PhasedReader<'_, '_>, revision: &impl newt_reader_ui::SourceOperations,
        mut owner: &mut ExternalOwner, mut ui: &mut ReaderUiState,
        opened_digest: Digest, request: &impl StatsRequest, stats_sha256: Sha256<'_>,
        mut statistics: &mut ReadingStatsRuntime, at: u32,
        accepted_open_owed: &mut bool, sleeping: bool, allow_cancelled: bool,
        mut capture: Option<&mut crate::reader_exit_metadata::ExitMetadata>,
    ) where P::Port<'io>: Directory {
        let settings = resources.settings;
        self.stats(
            io,
            card,
            &mut resources,
            &reader,
            &mut owner,
            &mut ui,
            &opened_digest,
            revision.source_verified() && revision.digest() == Some(opened_digest),
            request,
            stats_sha256,
            &mut statistics,
            at,
            StatsOperation::Lease { idle: false },
        );
        let candidate = *owner.tracker.snapshot(at);
        let mut peers = newt_core::progress_store::PeerLocators::default();
        self.save_with_peers(
            io,
            card,
            resources,
            &opened_digest,
            candidate,
            false,
            *accepted_open_owed,
            (owner.state.opened() || allow_cancelled)
                && revision.source_verified()
                && revision.digest() == Some(opened_digest),
            capture.as_ref().map(|_| &mut peers),
            request,
        );
        if checkpoint_verified(self.checkpoint) {
            if let (Some(capture), Some(saved)) = (capture.as_mut(),
                self.checkpoint.acknowledged_for(&opened_digest)) {
                let section = reader.section_at(saved.locator);
                capture.keep_identity(&opened_digest, saved.locator, Some(section.label),
                    &peers, |at| reader.section_at(at));
                if reader.info().overall_estimated {
                    capture.keep_estimate(reader.progress_estimate());
                }
            }
            *accepted_open_owed = false;
            if self.checkpoint.notice().is_none() {
                resources.recovery = None;
            }
        }
        self.stats(
            io,
            card,
            &mut resources,
            &reader,
            &mut owner,
            &mut ui,
            &opened_digest,
            revision.source_verified() && revision.digest() == Some(opened_digest),
            request,
            stats_sha256,
            &mut statistics,
            at,
            StatsOperation::Fold,
        );
        self.destination(
            io,
            card,
            &mut *resources.pages,
            &opened_digest,
            settings,
            sleeping,
            false,
            revision.source_verified() && revision.digest() == Some(opened_digest),
            request,
        );
    }

    pub(crate) fn observe_exit(&self, intent: Option<ReaderUiIntent>, action_at: u32, digest: Digest,
        owner: &mut ExternalOwner, ui: &ReaderUiState, recovery: Option<PendingRecovery>,
        preferences_unavailable: bool, statistics: &ReadingStatsRuntime,
        accepted_open_owed: bool, now: fn() -> u32,
    ) -> BookExit {
        BookExit {
            intent, action_at, digest,
            idle_quiet_from: None,
            progress: if owner.state.opened() {
                *owner.tracker.snapshot(now())
            } else { recovery.map(|pending| pending.progress).unwrap_or_else(|| *owner.tracker.snapshot(now())) },
            preferences_unavailable,
            statistics: observe(statistics, owner, ui, self.checkpoint, accepted_open_owed),
        }
    }
}
