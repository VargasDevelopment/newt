// SPDX-License-Identifier: MIT
//! Select a reader restore policy from the admitted boot or sleep record.
#![forbid(unsafe_code)]
use crate::{home_startup::boot_resume::Target,
    retained_runtime::record::{Recovery, SleepRecord}};
use newt_core::preferences::ViewSettings;
use newt_reader_ui::RestorePolicy;

pub enum DurableCheckpoint {
    PageStart,
    Anchor,
}

impl DurableCheckpoint {
    /// A target saved under other settings is an anchor (`Target::load_committed`).
    pub fn of(target: &Target) -> Self {
        if target.anchor { Self::Anchor } else { Self::PageStart }
    }
}

/// `durable` is the power-on resume target that reopened the book, else
/// `retained` is the admitted sleep record. `settings` are the ones the book
/// reopens with. A sleep page start only holds under the settings it was saved
/// under; when the durable ones differ (a lost save acknowledgement, missing
/// preferences), the book reopens at the page holding its locator (#107).
/// ColdCheckpoint keeps the sleep position without reloading progress, and a
/// match stays ExactPage, so the common wake does no extra I/O.
pub fn boot_restore_policy(durable: Option<DurableCheckpoint>, retained: Option<&SleepRecord>,
    settings: ViewSettings,
) -> RestorePolicy {
    match (durable, retained.map(|record| (record.recovery_kind(), record.reader_view()))) {
        (Some(DurableCheckpoint::Anchor), _) | (None, Some((Some(Recovery::PendingAnchor), _))) => RestorePolicy::Validated,
        (Some(DurableCheckpoint::PageStart), _) => RestorePolicy::ColdCheckpoint,
        (None, Some((_, saved_under))) if saved_under != Some(settings) => RestorePolicy::ColdCheckpoint,
        (None, Some((Some(Recovery::Ordinary | Recovery::PendingExact) | None, _)) | None) => RestorePolicy::ExactPage,
    }
}

pub fn pending_recovery(policy: RestorePolicy) -> Recovery {
    match policy {
        RestorePolicy::Validated | RestorePolicy::ColdCheckpoint => Recovery::PendingAnchor,
        RestorePolicy::AllowInitialFallback | RestorePolicy::ExactPage => Recovery::PendingExact,
    }
}
