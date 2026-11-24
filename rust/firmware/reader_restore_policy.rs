// SPDX-License-Identifier: MIT
//! Select a reader restore policy from the admitted boot or sleep record.
#![forbid(unsafe_code)]
use crate::retained_runtime::record::Recovery;
use newt_reader_ui::RestorePolicy;

pub enum DurableCheckpoint {
    PageStart,
    Anchor,
}

pub fn boot_restore_policy(durable: Option<DurableCheckpoint>, retained: Option<Recovery>) -> RestorePolicy {
    match (durable, retained) {
        (Some(DurableCheckpoint::Anchor), _) | (None, Some(Recovery::PendingAnchor)) => RestorePolicy::Validated,
        (Some(DurableCheckpoint::PageStart), _) => RestorePolicy::ColdCheckpoint,
        (None, Some(Recovery::Ordinary | Recovery::PendingExact) | None) => RestorePolicy::ExactPage,
    }
}

pub fn pending_recovery(policy: RestorePolicy) -> Recovery {
    match policy {
        RestorePolicy::Validated | RestorePolicy::ColdCheckpoint => Recovery::PendingAnchor,
        RestorePolicy::AllowInitialFallback | RestorePolicy::ExactPage => Recovery::PendingExact,
    }
}
