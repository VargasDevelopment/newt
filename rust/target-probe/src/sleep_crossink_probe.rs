//! Retention probes for a separate target harness, using safe Rust ABI borrowed I/O.
//! Replace `newt_core` with `newt_core` after integration.
//! `no_mangle` exports follow the existing target-probe convention; no unsafe blocks.
use newt_core::{crossink_stats::*, reading_stats::*, sleep_stats::*, stats_clock::*};
#[unsafe(no_mangle)]
pub fn newt_rust_sleep_crossink_sleep_probe(
    ledger: &StatsLedger,
    cache: &StatsStreak,
    archive: Option<&StatsMonth>,
    summary: &StatsSummary,
    today: StatsClock,
    unsaved: u32,
    recent: u8,
    week: &mut SleepWeek,
    habit: &mut ReadingHabit,
    built: &mut [u8; STREAK_BYTES],
    facts: &mut [u32; 5],
) {
    *week = sleep_week(ledger, cache, archive, today, unsaved);
    *habit = reading_habit(ledger, archive, today, unsaved);
    *built = encode_streak(&sleep_streak(ledger, summary, recent, today));
    let before = sleep_seconds_before(ledger, archive, today.day);
    *facts = [
        u32::from(before.is_some()),
        before.unwrap_or(0),
        u32::from(sleep_archive_month(ledger, today, false)),
        u32::from(sleep_archive_month(ledger, today, true)),
        u32::from(sleep_today_archived(ledger, today)),
    ];
}
#[unsafe(no_mangle)]
pub fn newt_rust_sleep_crossink_decode_probe(
    input: &[u8],
    name: &[u8],
    history: &mut Option<CrossInkHistory>,
    streak: &mut Option<StatsStreak>,
    global: &mut Option<CrossInkGlobalCopy>,
    source: &mut Option<CrossInkSource>,
    import: &mut Option<ImportRecord>,
    flags: &mut [u8; 4],
) {
    *history = decode_cross_ink_global(input);
    *streak = decode_streak(input);
    *global = decode_cross_ink_global_record(input);
    *source = decode_cross_ink_source(input);
    *import = decode_import(input);
    *flags = [
        match classify_cross_ink_global(input) {
            CrossInkFile::Invalid => 0,
            CrossInkFile::Valid => 1,
            CrossInkFile::Newer => 2,
        },
        u8::from(valid_cross_ink_book(input)),
        u8::from(cross_ink_book_completed(input)),
        u8::from(cross_ink_source_name(name)),
    ];
}
#[unsafe(no_mangle)]
pub fn newt_rust_sleep_crossink_encode_probe(
    global: &CrossInkGlobalCopy,
    source: &CrossInkSource,
    import: &ImportRecord,
    streak: &StatsStreak,
    global_out: &mut Option<[u8; GLOBAL_RECORD_BYTES]>,
    source_out: &mut Option<[u8; SOURCE_RECORD_BYTES]>,
    import_out: &mut [u8; IMPORT_RECORD_BYTES],
    streak_out: &mut [u8; STREAK_BYTES],
    history: &mut CrossInkHistory,
) {
    *global_out = encode_cross_ink_global(global);
    *source_out = encode_cross_ink_source(source);
    *import_out = encode_import(import);
    *streak_out = encode_streak(streak);
    *history = cross_ink_history(global);
}
