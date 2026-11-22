// SPDX-License-Identifier: MIT
//! Idempotent one-time reading statistics import helper.
use crate::crossink_stats as ci;
use crate::reading_stats::*;
use crate::reading_stats_store::{StatsLoad, StatsStore, load, valid_named_source};
use crate::stats_service_backend::NamespaceBackend;

/// Native SHA provider is independent of the store. Its return value and partial
/// output follow cross_ink_source_namespace; never replace it with a test hash.
pub type Sha256<'a> = &'a dyn Fn(&[u8], &mut StatsDigest) -> bool;

#[inline(never)]
pub fn count_once<B: NamespaceBackend>(
    stats: &mut StatsStore<'_, B>,
    ledger: &StatsLedger,
    fold: &mut StatsFold,
    target: Option<&StatsMonth>,
    next: &mut StatsLedger,
    sha256: Sha256<'_>,
) -> bool {
    if next.books_finished() <= ledger.books_finished() {
        return true;
    }
    let mut unreadable = false;
    let mut history = CrossInkHistory::default();
    if !stats.load_cross_ink(&mut history, Some(&mut unreadable)) {
        return !unreadable;
    }
    let mut completed = false;
    let mut failed = false;
    let listed = stats.store().for_each_namespace("ci", &mut |store, name| {
        let loaded = load::<_, { ci::SOURCE_RECORD_BYTES }>(
            store,
            name,
            ci::SOURCE_MAGIC,
            |bytes| valid_named_source(bytes, name, sha256),
            |bytes| {
                let source = ci::decode_cross_ink_source(bytes).expect("validated source");
                completed |= source.binding != ci::CrossInkBinding::None
                    && source.digest == *ledger.lease_digest()
                    && source.copy.status == ci::CrossInkStatus::Imported
                    && ci::cross_ink_book_completed(
                        &source.copy.bytes[..usize::from(source.copy.size)],
                    );
            },
        );
        if loaded == StatsLoad::Unavailable {
            failed = true;
            return false;
        }
        true
    });
    if !listed || failed {
        return false;
    }
    if completed {
        fold.cross_ink_finished = true;
        *next = fold_ledger(ledger, fold, target);
    }
    true
}
