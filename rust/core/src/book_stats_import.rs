// SPDX-License-Identifier: MIT
//! Reader-only CrossInk per-book statistics import helpers.
#![forbid(unsafe_code)]
use crate::book_days::{BOOK_IMPORT_CHECKED, BookDays, BookImported, cross_ink_book_totals};
use crate::book_days_store::{load_book_days, save_book_days, set_aside_book_days};
use crate::crossink_stats as ci;
use crate::reading_stats::StatsDigest;
use crate::reading_stats_once::Sha256;
use crate::reading_stats_service::{StatsResult, committed};
use crate::reading_stats_store::{StatsBackend, StatsLoad, StatsStore, load, valid_named_source};
use crate::stats_service_backend::NamespaceBackend;

/// Frozen sources bound to this book are summed once into its head. `wrote`
/// becomes true only after admission, including an attempted corrupt-head repair.
#[inline(never)]
pub fn import_book_cross_ink<B: NamespaceBackend>(
    store: &mut B,
    digest: &StatsDigest,
    cancelled: &mut dyn FnMut() -> bool,
    sha256: Sha256<'_>,
    wrote: &mut bool,
) -> StatsResult {
    let mut stats = StatsStore::new(store);
    let mut days = BookDays::default();
    let loaded = load_book_days(&mut stats, digest, &mut days);
    if loaded == StatsLoad::Unavailable {
        return StatsResult::Unavailable;
    }
    if loaded == StatsLoad::Unsupported
        || (loaded == StatsLoad::Loaded && days.flags & BOOK_IMPORT_CHECKED != 0)
    {
        return StatsResult::Done;
    }
    let mut sum = BookImported::default();
    let mut failed = false;
    let mut stopped = false;
    let listed = stats.store().for_each_namespace("ci", &mut |store, name| {
        if cancelled() {
            stopped = true;
            return false;
        }
        let loaded = load::<_, { ci::SOURCE_RECORD_BYTES }>(
            store,
            name,
            ci::SOURCE_MAGIC,
            |bytes| valid_named_source(bytes, name, sha256),
            |bytes| {
                let Some(source) = ci::decode_cross_ink_source(bytes) else {
                    failed = true;
                    return;
                };
                if source.binding != ci::CrossInkBinding::None
                    && source.digest == *digest
                    && source.copy.status == ci::CrossInkStatus::Imported
                {
                    cross_ink_book_totals(&source.copy.bytes[..usize::from(source.copy.size)], &mut sum);
                }
            },
        );
        if loaded == StatsLoad::Unavailable {
            failed = true;
            return false;
        }
        true
    });
    if stopped {
        return StatsResult::Cancelled;
    }
    if !listed || failed {
        return StatsResult::Unavailable;
    }
    let Some(effects) = StatsStore::<B>::begin_effects(Some(cancelled)) else {
        return StatsResult::Cancelled;
    };
    *wrote = true;
    if loaded == StatsLoad::Corrupt {
        if !set_aside_book_days(&effects, &mut stats, digest) {
            return StatsResult::Unavailable;
        }
        days = BookDays::default();
    }
    days.imported = sum;
    days.flags |= BOOK_IMPORT_CHECKED;
    committed(save_book_days(&effects, &mut stats, digest, &days))
}

/// An earlier completed import lets a resumed reader sum frozen sources before Home.
pub fn cross_ink_import_frozen<B: StatsBackend>(store: &mut B) -> bool {
    let mut stats = StatsStore::new(store);
    load::<_, { ci::IMPORT_RECORD_BYTES }>(
        stats.store(),
        "newt_import",
        ci::IMPORT_MAGIC,
        |bytes| ci::decode_import(bytes).is_some(),
        |_| {},
    ) == StatsLoad::Loaded
}

#[cfg(test)]
#[path = "book_stats_import_tests.rs"]
mod tests;
