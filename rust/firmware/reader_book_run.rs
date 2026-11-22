// SPDX-License-Identifier: MIT
//! Short image loans, plus the fixture-only whole-book loop.
use super::*;

#[derive(Clone, Copy)]
pub(crate) enum Route {
    Short,
    #[cfg(feature = "checkpoint-fixture")]
    WholeLoop,
}

pub(crate) fn drive<'io, 'm, 'p, 'scope, 'sdk, 'route, P: CardPort, F, N: nvs::Native>(
    io: &'io UiIo, card: &mut P, request: &BookRequest<'_>,
    reader: &mut PhasedReader<'m, 'p>, owner: &mut ExternalOwner, ui: &mut ReaderUiState,
    revision: &mut impl newt_reader_ui::SourceOperations,
    mut resources: &mut Resources<'_, 'scope, 'sdk, 'route, '_, P::Port<'io>>,
    preferences: &mut ReaderPreferences, checkpoint: &mut MountedCheckpoint<'_, N>,
    stats_sha256: Sha256<'_>, statistics: &mut ReadingStatsRuntime,
    accepted_open: &mut bool, recovery: &mut Option<PendingRecovery>,
    book_identity: u64, saved: Progress, digest: Digest,
    producer: &ProducerCallbacks<'_>, producer_ledger: Option<&mut [u8; 256]>,
    initialize: bool, paint_first: bool, route: Route, accepted_save: bool,
    visit_parts: Option<VisitParts<'_>>,
    _metadata: Option<&mut Option<newt_sdk_heap::Fixed<crate::reader_exit_metadata::ExitMetadata>>>,
    title_deferred: bool, mut saves: Option<&mut newt_core::save_notice::ReadingSaveNotice>,
    mut next: F, pause: &mut ProducerPauseFn<'_, 'io, N, P>,
) -> Result<BookExit, BookFailure>
where
    P::Port<'io>: Directory + newt_native_storage::page_entries::PageEntries,
    F: FnMut(Between<'_, 'io, 'm, 'p, 'scope, 'sdk, '_, P>, &mut N, &mut StateFacts)
        -> Option<BookStep>,
{
    #[cfg(feature = "checkpoint-fixture")]
    let mut metadata = _metadata;
    checkpoint.checkpoint.begin_operation();
    let (mut reader, mut owner, mut ui) = (reader, owner, ui);
    if initialize {
        reader.set_closed_orientation(resources.settings.orientation == Orientation::LandscapeCcw);
    }
    ui.preferences_writable = preferences.writable();
    resources.recovery = *recovery;
    let mut producer_visit = producer_ledger.map(|ledger| visit::Visit::new(ledger, visit_parts));
    let mut idle_quiet_from = None;
    let now = request.now;
    let policy = request.policy;
    let opened_at = now();
    let mut action_at = opened_at;
    let opened = if initialize {
        let _action = StatsActionScope::new(request, crate::reader_action::ActionStage::OrdinaryFallback);
        owner.open_physical(&mut reader, &mut *revision, book_identity, saved, policy,
            opened_at, &mut resources.owner())
    } else { owner.state.opened() || owner.state.has_progress() };
    let result = if opened {
        // The selected identity outlives revocable receipt authority.
        let opened_digest = digest;
        let info = request.info;
        let mut statistics = &mut *statistics;
        let mut accepted_open_owed = *accepted_open;
        if initialize {
            let (owed, saved_at) = checkpoint.publish_open(
            io,
            card,
            &mut resources,
            &reader,
            &mut owner,
            &mut ui,
            &opened_digest,
            revision.source_verified() && revision.digest() == opened_digest,
            request,
            stats_sha256,
            &mut statistics,
            opened_at,
            accepted_save,
            revision.with_producer_source(|source| source.is_some()),
        );
            accepted_open_owed = owed;
            if let Some(at) = saved_at { action_at = at; }
        }
        let paint = match route {
            Route::Short => paint_first,
            #[cfg(feature = "checkpoint-fixture")]
            Route::WholeLoop => initialize || paint_first,
        };
        let first = if paint { Some(checkpoint.paint(
            io, card, &mut resources, &statistics, &opened_digest, request,
            &mut reader, &mut *revision, &mut owner, &mut ui, info, now(),
        )) } else { None };
        let mut composed = first;
        let intent = loop {
            let turn = Between {
                panel: &mut *resources.panel,
                reader: &reader,
                owner: &owner,
                ui: &ui,
                pages: &mut *resources.pages,
                io,
                card: &mut *card,
                composed: composed.take(),
                statistics: observe(
                    &statistics,
                    &owner,
                    &ui,
                    checkpoint.checkpoint,
                    accepted_open_owed,
                ),
            };
            let Some(step) = next(turn, &mut *checkpoint.nvs, &mut *checkpoint.facts) else {
                break None;
            };
            let at = now();
            action_at = at;
            let settings = resources.settings;
            let action = match step {
                BookStep::Action(action) => action,
                BookStep::Sleep(plane) => {
                    composed = Some(match request.sleep {
                        Some(scene) => resources.paint_sleep(
                            &mut reader, &mut *revision, &mut owner, scene, info, plane, at,
                        ),
                        None => Err(Status::Unsupported),
                    });
                    continue;
                }
                BookStep::Idle { .. } | BookStep::Save | BookStep::ReflowCheckpoint => {
                    let idle = matches!(step, BookStep::Idle { .. });
                    let requested = matches!(step, BookStep::Idle { checkpoint: true, .. });
                    let entered = matches!(step, BookStep::Idle { admitted: true, .. });
                    let ready = owner.state.opened() && (entered || !request.cancel.cancelled());
                    let save = !idle || (requested && ready);
                    if save {
                        let mut peers = newt_core::progress_store::PeerLocators::default();
                        let publish_metadata = !idle || accepted_open_owed;
                        let mut quiet = || !request.cancel.cancelled();
                        let mut admission = newt_core::durable::Admission {
                            quiet: Some(&mut quiet), entered, deferred: false,
                        };
                        if idle && !admission.ready() { continue; }
                        let admitted = checkpoint.stats(
                            io,
                            card,
                            &mut resources,
                            &reader,
                            &mut owner,
                            &mut ui,
                            &opened_digest,
                            revision.source_verified() && revision.digest() == opened_digest,
                            request,
                            stats_sha256,
                            &mut statistics,
                            at,
                            StatsOperation::Lease { idle },
                        );
                        if !admitted {
                            continue;
                        }
                        if idle && !entered && request.cancel.cancelled() {
                            continue;
                        }
                        action_at = now();
                        let candidate = *owner.tracker.snapshot(action_at);
                        let commit_reading = !matches!(step, BookStep::ReflowCheckpoint);
                        checkpoint.save_admitted_core(
                            io,
                            card,
                            resources,
                            &opened_digest,
                            candidate,
                            commit_reading && !publish_metadata,
                            accepted_open_owed,
                            owner.state.opened() && revision.source_verified() && revision.digest() == opened_digest,
                            publish_metadata.then_some(&mut peers),
                            request,
                            idle.then_some(&mut admission),
                        );
                        if admission.deferred { continue; }
                        if publish_metadata && checkpoint.checkpoint.result().progress == checkpoint::Outcome::Verified {
                            if let Some(saved) = checkpoint.checkpoint.acknowledged_for(&opened_digest) {
                                super::metadata::publish(resources.pages, reader, &opened_digest,
                                    &saved, &peers, revision.source_verified(), title_deferred,
                                    revision.with_producer_source(|source| source.is_some()),
                                    false, false, &|| false);
                            }
                            if commit_reading {
                                checkpoint.destination(io, card, resources.pages, &opened_digest,
                                    resources.settings, true, revision.source_verified(), request);
                            }
                        }
                        PendingRecovery::complete_checkpoint(&mut resources.recovery, checkpoint.checkpoint,
                            candidate, resources.settings, &opened_digest);
                        if checkpoint_verified(checkpoint.checkpoint) {
                            accepted_open_owed = false;
                            if checkpoint.checkpoint.notice().is_none() {
                                resources.recovery = None;
                            }
                        }
                        // Frozen idle checkpoints rearm quiet before any later duty.
                        if idle {
                            idle_quiet_from = Some(now());
                            continue;
                        }
                    }
                    if idle {
                        if checkpoint.stats(
                            io,
                            card,
                            &mut resources,
                            &reader,
                            &mut owner,
                            &mut ui,
                            &opened_digest,
                            revision.source_verified() && revision.digest() == opened_digest,
                            request,
                            stats_sha256,
                            &mut statistics,
                            now(),
                            StatsOperation::Idle,
                        ) {
                            // An attempted seed/fold consumes this quiet turn too.
                            idle_quiet_from = Some(now());
                            continue;
                        }
                        if let Some(saves) = saves.as_deref_mut() {
                            let (attempted, repaint) = checkpoint.import_book(io, card, resources,
                                &reader, &mut owner, &ui, &opened_digest,
                                revision.source_verified() && revision.digest() == opened_digest,
                                request, saves, stats_sha256);
                            if attempted {
                                idle_quiet_from = Some(now());
                                if repaint { break Some(ReaderUiIntent::Repaint); }
                                continue;
                            }
                        }
                        let (parsed, repaint, quiet_from) = if let Some(visit) = producer_visit.as_mut() {
                            let Some(result) = revision.with_producer_source(|source| source.map(|source|
                                visit.with(|visit| resources.producer_idle(io, card, checkpoint, pause, visit,
                                    &mut reader, source, &mut owner, &mut ui, &mut statistics,
                                    &mut accepted_open_owed, &opened_digest, request, producer))))
                                .flatten() else { return Err(BookFailure::Revision); };
                            result
                        } else { (false, false, None) };
                        if quiet_from.is_some() { idle_quiet_from = quiet_from; }
                        if parsed {
                            let restored = resources.owner().restore_retained_native(
                                &reader, &owner, &ui, settings, info,
                                owner.tracker.snapshot(now()), None,
                            );
                            if repaint || restored.is_err() { composed = Some(restored); }
                        }
                    }
                    continue;
                }
            };
            let handled = owner.handle_ui_physical(
                &mut reader,
                &mut *revision,
                &mut ui,
                settings,
                action,
                at,
                &mut resources.owner(),
            );
            let done = owner.complete_ui_physical(
                &mut reader,
                &mut *revision,
                &mut ui,
                handled,
                at,
                &mut resources.owner(),
            );
            match done.work {
                ReaderUiWork::Intent(ReaderUiIntent::None) => {}
                ReaderUiWork::Intent(ReaderUiIntent::Repaint) => {
                    // The image loop composes after this short action returns.
                    match route {
                        Route::Short => break Some(ReaderUiIntent::Repaint),
                        #[cfg(feature = "checkpoint-fixture")]
                        Route::WholeLoop => {
                            let painted = checkpoint.paint(
                                io, card, &mut resources, &statistics, &opened_digest, request,
                                &mut reader, &mut *revision, &mut owner, &mut ui, info, at,
                            );
                            composed = Some(painted);
                        }
                    }
                }
                ReaderUiWork::Intent(intent @ (ReaderUiIntent::Home | ReaderUiIntent::Sleep)) => {
                    match route {
                        Route::Short => break Some(intent),
                        #[cfg(feature = "checkpoint-fixture")]
                        Route::WholeLoop => {
                            let capture = if intent == ReaderUiIntent::Home {
                                metadata.as_deref_mut().and_then(|slot| {
                                    if slot.is_none() {
                                        *slot = newt_sdk_heap::Fixed::try_new_with(crate::reader_exit_metadata::ExitMetadata::new);
                                    }
                                    slot.as_deref_mut()
                                })
                            } else { None };
                            checkpoint.finish_exit_captured(io, card, &mut resources, &reader, &*revision,
                                &mut owner, &mut ui, opened_digest, request, stats_sha256,
                                &mut statistics, at, &mut accepted_open_owed,
                                intent == ReaderUiIntent::Sleep, capture);
                            break Some(intent);
                        }
                    }
                }
                ReaderUiWork::Intent(ReaderUiIntent::MarkFinished) => {
                    checkpoint.stats(
                        io,
                        card,
                        &mut resources,
                        &reader,
                        &mut owner,
                        &mut ui,
                        &opened_digest,
                        revision.source_verified() && revision.digest() == opened_digest,
                        request,
                        stats_sha256,
                        &mut statistics,
                        at,
                        StatsOperation::Mark {
                            opening: accepted_open_owed,
                        },
                    );
                    match route {
                        Route::Short => break Some(ReaderUiIntent::Repaint),
                        #[cfg(feature = "checkpoint-fixture")]
                        Route::WholeLoop => {
                            // Parsing may have lent the logical frame as scratch.
                            // Reconstruct the complete raw page before its overlay.
                            composed = Some(checkpoint.paint(
                                io, card, &mut resources, &statistics, &opened_digest, request,
                                &mut reader, &mut *revision, &mut owner, &mut ui, info, at,
                            ));
                        }
                    }
                }
                ReaderUiWork::Intent(ReaderUiIntent::SaveSettings) => {
                    // Firmware's app layer must end this loan before
                    // setSettings releases the backing and opens fresh storage.
                    match route {
                        Route::Short => break Some(ReaderUiIntent::SaveSettings),
                        #[cfg(feature = "checkpoint-fixture")]
                        Route::WholeLoop => {
                            composed = Some(super::preferences::SettingsChange {
                                io, card, request, reader: &mut reader, owner: &mut owner,
                                ui: &mut ui, revision, resources: &mut resources, preferences,
                                checkpoint, statistics: &mut statistics, stats_sha256,
                                accepted_open_owed: &mut accepted_open_owed,
                                opened_digest, book_identity, at,
                            }.apply());
                        }
                    }
                }
                ReaderUiWork::Intent(intent) => break Some(intent),
                // complete_ui settles every turn and jump into an intent.
                ReaderUiWork::Turn(_) | ReaderUiWork::Jump(_) => {}
            }
        };
        *accepted_open = accepted_open_owed;
        let mut exit = checkpoint.observe_exit(intent, action_at, opened_digest, &mut owner, &ui,
            resources.recovery, preferences.unavailable(), &statistics,
            accepted_open_owed, now);
        exit.idle_quiet_from = idle_quiet_from;
        Ok(exit)
    } else {
        Err(BookFailure::Open(owner.state.status()))
    };
    *recovery = resources.recovery;
    #[cfg(feature = "checkpoint-fixture")]
    if matches!(route, Route::WholeLoop) { owner.close(&mut reader); }
    result
}
