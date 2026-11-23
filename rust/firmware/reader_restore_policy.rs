// SPDX-License-Identifier: MIT
//! Select a reader restore policy from the admitted boot or sleep record.
#![forbid(unsafe_code)]
use crate::retained_runtime::record::Recovery;
use newt_reader_ui::RestorePolicy;

pub fn boot_restore_policy(durable: bool, anchor: bool, retained: Option<Recovery>) -> RestorePolicy {
    match (durable, anchor, retained) {
        (true, true, _) | (false, _, Some(Recovery::PendingAnchor)) => RestorePolicy::Validated,
        (true, false, _) => RestorePolicy::ColdCheckpoint,
        (false, _, Some(Recovery::Ordinary | Recovery::PendingExact) | None) => RestorePolicy::ExactPage,
    }
}

pub fn pending_recovery(policy: RestorePolicy) -> Recovery {
    match policy {
        RestorePolicy::Validated | RestorePolicy::ColdCheckpoint => Recovery::PendingAnchor,
        RestorePolicy::AllowInitialFallback | RestorePolicy::ExactPage => Recovery::PendingExact,
    }
}
