// SPDX-License-Identifier: MIT
//! Frozen PendingExactResume geometry and completeCheckpoint policy.
use newt_core::{preferences::ViewSettings, progress::Progress};
use newt_reader_ui::RestorePolicy;
use super::checkpoint::{CheckpointResult, Outcome, ReaderCheckpoint};

/// The failed attempt owns its geometry; durable acknowledgement stays on the
/// checkpoint/save-notice owners. This adds no backing or source lifetime.
#[derive(Clone, Copy)]
pub(crate) struct PendingRecovery {
    pub(crate) progress: Progress,
    pub(crate) attempted_sequence: u32,
    pub(crate) settings: ViewSettings,
    pub(crate) policy: RestorePolicy,
}
impl PendingRecovery {
    /// Frozen armCheckpoint after a confirmed preference: failed progress
    /// keeps the live locator, while both paths use the proposed geometry.
    pub(crate) fn for_reflow(candidate: Progress, acknowledged: Progress,
        settings: ViewSettings, saved: bool, attempted_sequence: u32,
    ) -> Self {
        let (progress, attempted_sequence) = if saved {
            (acknowledged, acknowledged.sequence)
        } else {
            (candidate, attempted_sequence)
        };
        Self { progress, attempted_sequence, settings, policy: RestorePolicy::Validated }
    }
    pub(crate) fn progress(&self) -> &Progress { &self.progress }
    pub(crate) fn attempted_sequence(&self) -> u32 { self.attempted_sequence }
    pub(crate) fn settings(&self) -> ViewSettings { self.settings }
    pub(crate) fn policy(&self) -> RestorePolicy { self.policy }

    /// Replace a stale pending position with a newer loaded durable checkpoint.
    /// Downgrades `ExactPage` because durable progress may belong to another layout.
    pub(crate) fn reconcile_durable(&mut self, durable: Progress, checkpoint_saved: bool) -> bool {
        if !durable.supersedes(&self.progress, checkpoint_saved, self.attempted_sequence) {
            return false;
        }
        self.progress = durable;
        self.attempted_sequence = durable.sequence;
        if self.policy == RestorePolicy::ExactPage {
            self.policy = RestorePolicy::ColdCheckpoint;
        }
        true
    }

    /// Frozen blockedCheckpoint: Changed revokes the old page; an accepted
    /// open still awaiting verification is deferred, not a failed save.
    pub(crate) fn blocked_checkpoint(pending: &mut Option<Self>, checkpoint: &mut ReaderCheckpoint,
        candidate: Option<Progress>, settings: ViewSettings, digest: &[u8; 32],
        source_changed: bool, accepted_pending: bool,
    ) -> Option<CheckpointResult> {
        let candidate = candidate?;
        if source_changed || accepted_pending { return None; }
        let result = checkpoint.refuse();
        Self::complete_checkpoint(pending, checkpoint, candidate, settings, digest);
        Some(result)
    }

    pub(crate) fn complete_checkpoint(pending: &mut Option<Self>,
        checkpoint: &ReaderCheckpoint, candidate: Progress,
        settings: ViewSettings, digest: &[u8; 32],
    ) {
        let result = checkpoint.result();
        if result.progress == Outcome::Unconfirmed || checkpoint.notice().is_some() {
            let (progress, attempted_sequence) = if result.progress == Outcome::Verified {
                let saved = checkpoint.acknowledged_for(digest).unwrap_or(candidate);
                (saved, saved.sequence)
            } else {
                (candidate, checkpoint.attempted_sequence_for(&candidate))
            };
            *pending = Some(Self {
                progress,
                attempted_sequence,
                settings,
                policy: RestorePolicy::ExactPage,
            });
        } else {
            *pending = None;
        }
    }
}
