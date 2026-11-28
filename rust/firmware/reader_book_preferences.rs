// SPDX-License-Identifier: MIT
//! Compatibility settings flow for the whole-book borrowed component entry.
//! reader_image handles this intent after releasing its owned backing loan.
use super::*;

pub(super) struct SettingsChange<'op, 'r, 'cp, 'io, 'm, 'p, 'scope, 'sdk, 'route, 'bank,
    N: nvs::Native, P: CardPort, S: newt_reader_ui::SourceOperations> {
    pub(super) io: &'io UiIo,
    pub(super) card: &'op mut P,
    pub(super) request: &'op BookRequest<'op>,
    pub(super) reader: &'op mut PhasedReader<'m, 'p>,
    pub(super) owner: &'op mut ExternalOwner,
    pub(super) ui: &'op mut ReaderUiState,
    pub(super) revision: &'op mut S,
    pub(super) resources: &'op mut Resources<'r, 'scope, 'sdk, 'route, 'bank, P::Port<'io>>,
    pub(super) preferences: &'op mut ReaderPreferences,
    pub(super) checkpoint: &'op mut MountedCheckpoint<'cp, N>,
    pub(super) statistics: &'op mut ReadingStatsRuntime,
    pub(super) stats_sha256: Sha256<'op>,
    pub(super) accepted_open_owed: &'op mut bool,
    pub(super) opened_digest: Digest,
    pub(super) book_identity: u64,
    pub(super) at: u32,
}

impl<'op, 'r, 'cp, 'io, 'm, 'p, 'scope, 'sdk, 'route, 'bank,
    N: nvs::Native, P: CardPort, S: newt_reader_ui::SourceOperations>
    SettingsChange<'op, 'r, 'cp, 'io, 'm, 'p, 'scope, 'sdk, 'route, 'bank, N, P, S>
where P::Port<'io>: Directory {
    pub(super) fn apply(self) -> Result<(), Status> {
        let Self { io, card, request, mut reader, mut owner, mut ui, revision,
            mut resources, preferences, checkpoint, mut statistics, stats_sha256,
            accepted_open_owed, opened_digest, book_identity, at } = self;
        let info = request.info;
        let settings = resources.settings;
        // Frozen main.cpp397-425: recover before touching NVS;
        // failed recovery keeps the draft and existing preferences.
        if !owner.state.opened()
            && (resources.recovery.is_some()
                || checkpoint.checkpoint.notice().is_some())
        {
            let retry = resources
                .recovery
                .map(|pending| pending.progress)
                .unwrap_or_else(|| *owner.tracker.snapshot(at));
            let reopened = resources.reopen(
                &mut reader,
                &mut *revision,
                &mut owner,
                &opened_digest,
                book_identity,
                retry,
                settings,
                RestorePolicy::Validated,
                at,
                request.cancel,
            );
            if reopened {
                checkpoint.publish_open(
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
                    false,
                    revision.with_producer_source(|source| source.is_some()),
                );
                if checkpoint.checkpoint.notice().is_some() {
                    checkpoint.stats(
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
                    checkpoint.save(
                        io,
                        card,
                        resources,
                        &opened_digest,
                        *owner.tracker.snapshot(at),
                        true,
                        *accepted_open_owed,
                        owner.state.opened() && revision.source_verified() && revision.digest() == Some(opened_digest),
                        request,
                    );
                }
            }
            if !reopened || !checkpoint_verified(checkpoint.checkpoint) {
                ui.notice = ReaderUiNotice::SaveFailed;
                return checkpoint.paint(
                    io,
                    card,
                    &mut resources,
                    &statistics,
                    &opened_digest,
                    request,
                    &mut reader,
                    &mut *revision,
                    &mut owner,
                    &mut ui,
                    info,
                    at,
                );

            }
            resources.recovery = None;
            owner.pause(at);
        }
        let proposed = ui.draft;
        if !ui.preferences_writable || !preferences.save(&mut *checkpoint.nvs, proposed)
        {
            ui.notice = ReaderUiNotice::SaveFailed;
            return checkpoint.paint(
                io,
                card,
                &mut resources,
                &statistics,
                &opened_digest,
                request,
                &mut reader,
                &mut *revision,
                &mut owner,
                &mut ui,
                info,
                at,
            );

        }
        let session_checkpoint = owner.state.has_progress();
        let candidate = *owner.tracker.snapshot(at);
        let saved = if session_checkpoint {
            checkpoint.stats(
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
            checkpoint.save(
                io,
                card,
                resources,
                &opened_digest,
                candidate,
                false,
                *accepted_open_owed,
                owner.state.opened() && revision.source_verified() && revision.digest() == Some(opened_digest),
                request,
            );
            if checkpoint_verified(checkpoint.checkpoint) {
                checkpoint
                    .checkpoint
                    .acknowledged_for(&opened_digest)
                    .unwrap_or(candidate)
            } else {
                candidate
            }
        } else {
            checkpoint
                .checkpoint
                .acknowledged_for(&opened_digest)
                .unwrap_or(candidate)
        };
        let reopened = resources.reopen(
            &mut reader,
            &mut *revision,
            &mut owner,
            &opened_digest,
            book_identity,
            saved,
            proposed,
            if session_checkpoint {
                RestorePolicy::Validated
            } else {
                RestorePolicy::AllowInitialFallback
            },
            at,
            request.cancel,
        );
        ui.panel = newt_reader_ui::ReaderPanel::Page;
        ui.selected = 0;
        ui.notice = ReaderUiNotice::None;
        if reopened {
            *accepted_open_owed = checkpoint.publish_open(
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
                true,
                revision.with_producer_source(|source| source.is_some()),
            ).0;
        }
        // #285: parse/reflow borrowed the sole logical frame.
        // Rebuild the whole page before any refresh or overlay.
        checkpoint.paint(
            io,
            card,
            &mut resources,
            &statistics,
            &opened_digest,
            request,
            &mut reader,
            &mut *revision,
            &mut owner,
            &mut ui,
            info,
            at,
        )
    }
}
