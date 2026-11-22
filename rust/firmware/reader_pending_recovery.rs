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
    pub(crate) settings: ViewSettings,
    pub(crate) policy: RestorePolicy,
}
impl PendingRecovery {
    /// Frozen armCheckpoint after a confirmed preference: failed progress
    /// keeps the live locator, while both paths use the proposed geometry.
    pub(crate) fn for_reflow(candidate: Progress, acknowledged: Progress,
        settings: ViewSettings, saved: bool,
    ) -> Self {
        Self { progress: if saved { acknowledged } else { candidate },
            settings, policy: RestorePolicy::Validated }
    }
    pub(crate) fn progress(&self) -> &Progress { &self.progress }
    pub(crate) fn settings(&self) -> ViewSettings { self.settings }
    pub(crate) fn policy(&self) -> RestorePolicy { self.policy }

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
            let progress = if result.progress == Outcome::Verified {
                checkpoint.acknowledged_for(digest).unwrap_or(candidate)
            } else { candidate };
            *pending = Some(Self { progress, settings, policy: RestorePolicy::ExactPage });
        } else {
            *pending = None;
        }
    }
}
