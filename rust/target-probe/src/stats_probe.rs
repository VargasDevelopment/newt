//! Runtime-input target retention probes; no SD I/O, firmware integration or X3 behavior.
//! Add as `mod stats_probe;` in the existing target probe crate.
use newt_core::{reading_stats::*, stats_clock::*};

#[unsafe(no_mangle)]
pub fn newt_rust_stats_seed_probe(
    seed: &StatsSeed,
    output: &mut [u8; LEDGER_BYTES],
    generation: u32,
) -> StatsLedger {
    let ledger = seed_ledger(seed);
    *output = encode_ledger(&ledger, generation);
    ledger
}

#[unsafe(no_mangle)]
pub fn newt_rust_stats_transition_probe(
    ledger: &mut StatsLedger,
    fold: &StatsFold,
    target: &StatsMonth,
    next: &StatsLease,
    mode: u32,
) -> bool {
    let needed = fold_needs_month(ledger, fold);
    let archive = if mode & 4 == 0 { Some(target) } else { None };
    *ledger = match mode % 3 {
        0 => fold_ledger(ledger, fold, archive),
        1 => handover_ledger(ledger, Some(fold), archive, next),
        _ => handover_ledger(ledger, None, archive, next),
    };
    needed
}

#[unsafe(no_mangle)]
pub fn newt_rust_stats_decode_probe(
    ledger: &[u8],
    month: &[u8],
    book: &[u8],
    digest: &StatsDigest,
    state: &mut StatsState,
    decoded_month: &mut Option<StatsMonth>,
    decoded_book: &mut Option<BookFacts>,
) -> u32 {
    let class = match classify_stats_record(ledger, LEDGER_MAGIC) {
        StatsDecode::Valid => 0,
        StatsDecode::Corrupt => 1,
        StatsDecode::Unsupported => 2,
    };
    let valid = decode_ledger(ledger, state);
    *decoded_month = decode_month(month);
    *decoded_book = decode_book_facts(book, digest);
    class | (u32::from(valid) << 8)
}

#[unsafe(no_mangle)]
pub fn newt_rust_stats_encode_probe(
    ledger: &StatsLedger,
    month: &StatsMonth,
    facts: &BookFacts,
    digest: &StatsDigest,
    generation: u32,
    ledger_out: &mut [u8; LEDGER_BYTES],
    month_out: &mut [u8; MONTH_BYTES],
    book_out: &mut [u8; BOOK_BYTES],
) -> u32 {
    *ledger_out = encode_ledger(ledger, generation);
    *month_out = encode_month(month, generation);
    *book_out = encode_book_facts(digest, facts, generation);
    newt_core::wire::fnv1a(book_out)
}

#[unsafe(no_mangle)]
pub fn newt_rust_stats_summary_probe(
    ledger: &StatsLedger,
    day: u16,
    hour: u8,
    archive: &StatsMonth,
    cross: &CrossInkHistory,
    recent: &mut u8,
    calls: &mut u32,
    flags: u32,
) -> StatsSummary {
    summarize_stats(
        ledger,
        stats_clock_for_day(day, hour),
        |month| {
            *calls = calls.wrapping_add(1);
            match flags % 3 {
                0 if month == archive.month => MonthLoad::Loaded(*archive),
                1 => MonthLoad::Unavailable,
                _ => MonthLoad::Missing,
            }
        },
        if flags & 4 == 0 { Some(cross) } else { None },
        Some(recent),
    )
}

#[unsafe(no_mangle)]
pub fn newt_rust_stats_facts_probe(
    facts: &BookFacts,
    progress: &newt_core::progress::Progress,
    day: u16,
    hour: u8,
) -> u64 {
    let clock = stats_clock_for_day(day, hour);
    let counters = stats_counters(progress);
    counters.milliseconds
        ^ u64::from(counters.pages)
        ^ u64::from(counters.sessions)
        ^ u64::from(finished(facts))
        ^ (u64::from(counts_finished(facts)) << 1)
        ^ (u64::from(match book_finish(facts) {
            BookFinish::No => 0u8,
            BookFinish::Yes => 1,
            BookFinish::Unknown => 2,
        }) << 2)
        ^ (u64::from(halloween(clock)) << 4)
        ^ (u64::from(christmas(clock)) << 5)
        ^ u64::from(unknown_book_facts().end_day)
}
