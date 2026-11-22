// SPDX-License-Identifier: MIT
//! Runtime-borrowed retention probes; no hardware/store evidence.
use newt_core::{book_days::*, reading_stats::*, stats_graph_data::*};
#[unsafe(no_mangle)]
pub fn nrust19_fold_probe(
    days: &mut BookDays,
    before: &StatsCounters,
    fold: &StatsFold,
    target: &BookDayBlock,
    use_target: bool,
    encoded: &mut [u8; BOOK_DAYS_BYTES],
    digest: &StatsDigest,
    generation: u32,
    needed: &mut u16,
    periods: &mut StatsPeriodsGraph,
) -> bool {
    *needed = book_fold_block(days, before, fold);
    let changed = fold_book_days(
        days,
        before,
        fold,
        if use_target { Some(target) } else { None },
    );
    *encoded = encode_book_days(digest, days, generation);
    *periods = book_periods_graph(days);
    changed
}
#[unsafe(no_mangle)]
pub fn nrust19_decode_probe(
    head: &[u8],
    archive: &[u8],
    digest: &StatsDigest,
    block: u16,
    days: &mut BookDays,
    out: &mut BookDayBlock,
) -> u8 {
    u8::from(decode_book_days(head, digest, days))
        | (u8::from(decode_book_archive(archive, digest, block, out)) << 1)
}
#[unsafe(no_mangle)]
pub fn nrust19_book_graph_probe(
    days: &BookDays,
    archive: &BookDayBlock,
    status: BookBlockRead,
    last: u16,
    count: u32,
    calls: &mut u32,
    last_request: &mut u16,
    output: &mut StatsDaysGraph,
    window: &mut u16,
    today: u16,
    back: u32,
) {
    *output = book_days_graph(
        days,
        |b, out| {
            *calls = calls.wrapping_add(1);
            *last_request = b;
            *out = *archive;
            status
        },
        last,
        count,
    );
    *window = book_window_end(today, last, days.since_day, count, back);
}
#[unsafe(no_mangle)]
pub fn nrust19_library_probe(
    ledger: &StatsLedger,
    archive: &StatsMonth,
    status: GraphMonthRead,
    history: &CrossInkHistory,
    weekdays: &[u32; 7],
    imported: &[u32; 4],
    last: u16,
    count: u32,
    total: u32,
    days_calls: &mut u32,
    periods_calls: &mut u32,
    output: &mut StatsGraphs,
    oldest: &mut u16,
) {
    output.days = library_days_graph(
        ledger,
        |_, out| {
            *days_calls = days_calls.wrapping_add(1);
            *out = *archive;
            status
        },
        history,
        weekdays,
        last,
        count,
    );
    output.periods = library_periods_graph(
        ledger,
        |_, out| {
            *periods_calls = periods_calls.wrapping_add(1);
            *out = *archive;
            status
        },
        imported,
        total,
    );
    *oldest = library_oldest_day(ledger, history);
}
#[unsafe(no_mangle)]
pub fn nrust19_crossink_probe(
    bytes: &[u8],
    sum: &mut BookImported,
    periods: &mut [u32; 4],
    weekdays: &mut [u32; 7],
) -> u8 {
    u8::from(cross_ink_global_periods(bytes, periods))
        | (u8::from(cross_ink_global_weekdays(bytes, weekdays)) << 1)
        | (u8::from(cross_ink_book_periods(bytes, periods)) << 2)
        | (u8::from(cross_ink_book_totals(bytes, sum)) << 3)
}
#[unsafe(no_mangle)]
pub fn nrust19_helpers_probe(
    periods: &[u32; 4],
    total: u64,
    newest: u16,
    oldest: u16,
    count: u32,
    back: u32,
    unknown: &mut u32,
    weekday: &mut usize,
) -> u16 {
    *unknown = stats_unknown_seconds(periods, total);
    *weekday = stats_weekday(newest);
    stats_window_end(newest, oldest, count, back)
}
#[unsafe(no_mangle)]
pub fn nrust19_archive_encode_probe(
    digest: &StatsDigest,
    archive: &BookDayBlock,
    generation: u32,
    output: &mut [u8; BOOK_ARCHIVE_BYTES],
) {
    *output = encode_book_archive(digest, archive, generation);
}
