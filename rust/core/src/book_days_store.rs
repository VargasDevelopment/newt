// SPDX-License-Identifier: MIT
//! Durable store for per-book day records and archive blocks.
#![forbid(unsafe_code)]
use crate::book_days::*;
use crate::reading_stats::{StatsCounters, StatsDigest, StatsFold};
use crate::reading_stats_store::{
    Namespace, StatsBackend, StatsEffects, StatsLoad, StatsSave, StatsStore, load, save,
    stats_book_namespace,
};
pub fn book_days_namespace(digest: &StatsDigest) -> Namespace {
    let mut name = *stats_book_namespace(digest).c_bytes();
    name[1] = b'd';
    // Namespace constructor remains private; derive ASCII via the shared helper.
    crate::reading_stats_store::namespace_from_book_bytes(name)
}
pub fn book_archive_namespace(digest: &StatsDigest, block: u16) -> Namespace {
    let mut name = *stats_book_namespace(digest).c_bytes();
    name[1] = b'e';
    let hex = b"0123456789abcdef";
    for i in 0..3 {
        name[12 + i] = hex[((block >> (8 - 4 * i)) & 15) as usize];
    }
    crate::reading_stats_store::namespace_from_book_bytes(name)
}
pub fn valid_book_days(bytes: &[u8], digest: &StatsDigest) -> bool {
    decode_book_days(bytes, digest, &mut BookDays::default())
}
pub fn valid_book_archive(bytes: &[u8], digest: &StatsDigest, block: u16) -> bool {
    decode_book_archive(bytes, digest, block, &mut BookDayBlock::default())
}
pub fn load_book_days<B: StatsBackend>(
    stats: &mut StatsStore<'_, B>,
    digest: &StatsDigest,
    out: &mut BookDays,
) -> StatsLoad {
    load::<_, BOOK_DAYS_BYTES>(
        stats.store(),
        book_days_namespace(digest).as_str(),
        BOOK_DAYS_MAGIC,
        |b| valid_book_days(b, digest),
        |b| {
            let _ = decode_book_days(b, digest, out);
        },
    )
}
pub fn save_book_days<B: StatsBackend>(
    effects: &StatsEffects,
    stats: &mut StatsStore<'_, B>,
    digest: &StatsDigest,
    days: &BookDays,
) -> StatsSave {
    save(
        effects,
        stats.store(),
        book_days_namespace(digest).as_str(),
        BOOK_DAYS_MAGIC,
        |b| valid_book_days(b, digest),
        encode_book_days(digest, days, 0),
    )
}
pub fn load_day_block<B: StatsBackend>(
    stats: &mut StatsStore<'_, B>,
    digest: &StatsDigest,
    block: u16,
    out: &mut BookDayBlock,
) -> StatsLoad {
    load::<_, BOOK_ARCHIVE_BYTES>(
        stats.store(),
        book_archive_namespace(digest, block).as_str(),
        BOOK_ARCHIVE_MAGIC,
        |b| valid_book_archive(b, digest, block),
        |b| {
            let _ = decode_book_archive(b, digest, block, out);
        },
    )
}
pub fn save_day_block<B: StatsBackend>(
    effects: &StatsEffects,
    stats: &mut StatsStore<'_, B>,
    digest: &StatsDigest,
    block: &BookDayBlock,
) -> StatsSave {
    save(
        effects,
        stats.store(),
        book_archive_namespace(digest, block.block).as_str(),
        BOOK_ARCHIVE_MAGIC,
        |b| valid_book_archive(b, digest, block.block),
        encode_book_archive(digest, block, 0),
    )
}
pub fn set_aside_book_days<B: StatsBackend>(
    _effects: &StatsEffects,
    stats: &mut StatsStore<'_, B>,
    digest: &StatsDigest,
) -> bool {
    stats
        .store()
        .set_aside(book_days_namespace(digest).as_str())
}
pub fn set_aside_day_block<B: StatsBackend>(
    _effects: &StatsEffects,
    stats: &mut StatsStore<'_, B>,
    digest: &StatsDigest,
    block: u16,
) -> bool {
    stats
        .store()
        .set_aside(book_archive_namespace(digest, block).as_str())
}
/// Call only AFTER NWTS committed this fold over ledger_before. Failures
/// undercount; this function never retries an unconfirmed archive/head commit.
#[inline(never)]
pub fn fold_stored_book_days<B: StatsBackend>(
    stats: &mut StatsStore<'_, B>,
    effects: &StatsEffects,
    digest: &StatsDigest,
    ledger_before: &StatsCounters,
    fold: &StatsFold,
) {
    let mut days = BookDays::default();
    match load_book_days(stats, digest, &mut days) {
        StatsLoad::Loaded | StatsLoad::Absent => {}
        StatsLoad::Corrupt => {
            if !set_aside_book_days(effects, stats, digest) {
                return;
            }
            days = BookDays::default();
        }
        StatsLoad::Unsupported | StatsLoad::Unavailable => return,
    }
    let mut target = BookDayBlock::default();
    let block = book_fold_block(&days, ledger_before, fold);
    if block != 0 {
        let mut archived = save_day_block(effects, stats, digest, &days.current);
        if archived == StatsSave::Corrupt
            && set_aside_day_block(effects, stats, digest, days.current.block)
        {
            archived = save_day_block(effects, stats, digest, &days.current);
        }
        if archived != StatsSave::Saved {
            return;
        }
        let loaded = load_day_block(stats, digest, block, &mut target);
        if loaded == StatsLoad::Unavailable || loaded == StatsLoad::Unsupported {
            return;
        }
        if loaded == StatsLoad::Corrupt && !set_aside_day_block(effects, stats, digest, block) {
            return;
        }
        if loaded != StatsLoad::Loaded {
            target = BookDayBlock {
                block,
                ..BookDayBlock::default()
            };
        }
    }
    if fold_book_days(
        &mut days,
        ledger_before,
        fold,
        if block != 0 { Some(&target) } else { None },
    ) {
        let _ = save_book_days(effects, stats, digest, &days);
    }
}
