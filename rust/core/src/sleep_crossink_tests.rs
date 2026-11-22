use crate::{
    crossink_stats::*,
    reading_stats::*,
    sleep_stats::*,
    stats_clock::*,
    wire::{fnv1a, put32},
};
fn reseal(b: &mut [u8]) {
    let end = b.len() - 4;
    let sum = fnv1a(&b[..end]);
    put32(b, end, sum);
}
fn clock(month: u8, day: u8, hour: u8) -> StatsClock {
    stats_clock(crate_clock(month, day, hour))
}
fn crate_clock(month: u8, day: u8, hour: u8) -> crate::time_zone::ClockTime {
    crate::time_zone::ClockTime {
        year: 2026,
        month,
        day,
        hour,
        minute: 0,
    }
}
fn ledger(month: u16, oldest: u16, days: [u16; 31], buckets: [u32; 4], total: u32) -> StatsLedger {
    // Construct only through the ledger's validating public codec; no private-field adapter.
    let mut b = encode_ledger(&seed_ledger(&StatsSeed::default()), 0);
    b[12..14].copy_from_slice(&oldest.to_le_bytes());
    b[14..16].copy_from_slice(&month.to_le_bytes());
    b[30..32].copy_from_slice(&month.to_le_bytes());
    put32(&mut b, 16, total);
    for (i, value) in buckets.into_iter().enumerate() {
        put32(&mut b, 96 + i * 4, value);
    }
    for (i, value) in days.into_iter().enumerate() {
        b[112 + i * 2..114 + i * 2].copy_from_slice(&value.to_le_bytes());
    }
    reseal(&mut b);
    let mut state = StatsState::default();
    assert!(decode_ledger(&b, &mut state));
    *state.seeded().unwrap()
}
#[test]
fn streak_exact_layout_generation_padding_and_consistency() {
    let cache = StatsStreak {
        day: 9497,
        run: 3,
        days: 0x0e,
        before: 0x01020304,
    };
    let mut b = encode_streak(&cache);
    assert_eq!(
        &b[..24],
        &[
            78, 87, 84, 75, 1, 0, 0, 0, 0, 0, 0, 0, 25, 37, 3, 0, 4, 3, 2, 1, 14, 0, 0, 0
        ]
    );
    assert_eq!(decode_streak(&b), Some(cache));
    for n in 0..28 {
        assert_eq!(decode_streak(&b[..n]), None);
    }
    put32(&mut b, 8, 71);
    reseal(&mut b);
    assert_eq!(decode_streak(&b), Some(cache));
    for at in [0, 4, 20, 21, 22, 23] {
        let mut bad = b;
        bad[at] ^= if at == 20 { 2 } else { 1 };
        reseal(&mut bad);
        assert_eq!(decode_streak(&bad), None, "offset {at}");
    }
    let mut bad = b;
    bad[27] ^= 1;
    assert_eq!(decode_streak(&bad), None);
    assert_eq!(
        decode_streak(&encode_streak(&StatsStreak::default())),
        Some(StatsStreak::default())
    );
    for cache in [
        StatsStreak {
            day: 0,
            before: 1,
            ..StatsStreak::default()
        },
        StatsStreak {
            day: 9496,
            ..StatsStreak::default()
        },
        StatsStreak {
            day: 36525,
            ..StatsStreak::default()
        },
    ] {
        assert!(decode_streak(&encode_streak(&cache)).is_none());
    }
}
#[test]
fn cache_proof_age_unsaved_wrap_and_clock() {
    let today = clock(10, 10, 8);
    let mut days = [0; 31];
    days[..10].fill(60);
    let l = ledger(today.month, today.month, days, [0; 4], 600);
    let cache = StatsStreak {
        day: today.day,
        run: 9,
        days: 255,
        before: 540,
    };
    assert_eq!(
        sleep_week(&l, &cache, None, today, 0),
        SleepWeek {
            days: 127,
            streak: 10
        }
    );
    assert_eq!(
        sleep_week(&l, &cache, None, stats_clock_for_day(today.day + 1, 8), 59),
        SleepWeek {
            days: 126,
            streak: 0
        }
    );
    assert_eq!(
        sleep_week(&l, &cache, None, stats_clock_for_day(today.day + 1, 8), 60),
        SleepWeek {
            days: 127,
            streak: 11
        }
    );
    assert_eq!(
        sleep_week(
            &l,
            &StatsStreak {
                before: 539,
                ..cache
            },
            None,
            today,
            0
        )
        .streak,
        0
    );
    assert_eq!(
        sleep_week(
            &l,
            &StatsStreak {
                day: today.day + 1,
                ..cache
            },
            None,
            today,
            0
        )
        .streak,
        0
    );
    assert_eq!(
        sleep_week(&l, &cache, None, StatsClock::default(), 600),
        SleepWeek::default()
    );
    assert_eq!(
        sleep_week(&l, &StatsStreak::default(), None, today, u32::MAX).days,
        126
    ); // C++ unsigned wrap
    let empty = seed_ledger(&StatsSeed::default());
    assert_eq!(
        sleep_week(
            &empty,
            &cache,
            None,
            stats_clock_for_day(today.day + 6, 8),
            60
        )
        .streak,
        0
    ); // proof mismatch
    let imported = StatsStreak { before: 0, ..cache };
    assert_eq!(
        sleep_week(&empty, &imported, None, today, 0),
        SleepWeek {
            days: 127,
            streak: 10
        }
    );
    assert_eq!(
        sleep_week(
            &empty,
            &imported,
            None,
            stats_clock_for_day(today.day + 7, 8),
            60
        ),
        SleepWeek { days: 1, streak: 0 }
    );
    let summary = StatsSummary {
        dated: true,
        current_streak: 10,
        ..StatsSummary::default()
    };
    assert_eq!(sleep_streak(&l, &summary, 255, today), cache);
    assert_eq!(
        sleep_streak(&l, &StatsSummary::default(), 255, today),
        StatsStreak::default()
    );
}
#[test]
fn month_provability_and_habit_boundaries() {
    let today = clock(11, 4, 23);
    let mut days = [0; 31];
    days[..4].fill(60);
    let l = ledger(today.month, today.month - 1, days, [0, 1800, 0, 0], 1140);
    let mut archive = empty_stats_month(today.month - 1);
    archive.day_seconds[26..31].fill(180);
    archive.bucket_seconds[3] = 1800;
    assert_eq!(sleep_archive_month(&l, today, false), today.month - 1);
    assert_eq!(sleep_archive_month(&l, clock(11, 7, 8), false), 0);
    assert_eq!(
        sleep_archive_month(&l, clock(11, 7, 8), true),
        today.month - 1
    );
    assert_eq!(
        sleep_week(&l, &StatsStreak::default(), Some(&archive), today, 0).days,
        127
    );
    assert_eq!(
        sleep_week(&l, &StatsStreak::default(), None, today, 0).days,
        15
    );
    let oct = clock(10, 31, 8);
    assert_eq!(sleep_seconds_before(&l, None, oct.day), None);
    assert_eq!(sleep_seconds_before(&l, Some(&archive), oct.day), Some(720));
    assert_eq!(
        sleep_seconds_before(&l, Some(&archive), clock(9, 30, 8).day),
        Some(0)
    );
    assert!(sleep_today_archived(&l, oct));
    assert_eq!(
        reading_habit(&l, Some(&archive), oct, 9000),
        ReadingHabit::None
    );
    assert_eq!(reading_habit(&l, None, today, 9000), ReadingHabit::None);
    assert_eq!(
        reading_habit(&l, Some(&archive), today, 0),
        ReadingHabit::Afternoon
    );
    assert_eq!(
        reading_habit(&l, Some(&archive), today, 1),
        ReadingHabit::Night
    );
    let empty = seed_ledger(&StatsSeed::default());
    assert_eq!(reading_habit(&empty, None, today, 1799), ReadingHabit::None);
    assert_eq!(
        reading_habit(&empty, None, today, 1800),
        ReadingHabit::Night
    );
    let saturated = ledger(
        today.month,
        today.month,
        [0; 31],
        [u32::MAX, u32::MAX, 0, 0],
        0,
    );
    assert_eq!(
        reading_habit(&saturated, None, clock(11, 4, 13), u32::MAX),
        ReadingHabit::Morning
    );
    let january = stats_clock_for_day(clock(12, 31, 8).day + 1, 8);
    let year = ledger(january.month, january.month - 1, [0; 31], [0; 4], 0);
    let mut december = empty_stats_month(january.month - 1);
    december.bucket_seconds[0] = 1800;
    assert_eq!(
        reading_habit(&year, Some(&december), january, 0),
        ReadingHabit::Morning
    );
}
#[test]
fn crossink_layouts_dates_and_history() {
    for (v, n) in [(1, 13), (2, 17), (3, 159)] {
        let mut b = [0; 159];
        b[0] = v;
        put32(&mut b, 1, 17);
        put32(&mut b, 5, 3600);
        put32(&mut b, 9, 91);
        if n >= 17 {
            put32(&mut b, 13, 3);
        }
        if n == 159 {
            put32(&mut b, 61, 36524);
            b[65] = 5;
            b[156] = 2;
            b[157] = 33;
        }
        let h = decode_cross_ink_global(&b[..n]).unwrap();
        assert_eq!((h.sessions, h.seconds, h.pages), (17, 3600, 91));
        assert_eq!(h.finished, if n >= 17 { 3 } else { 0 });
        if n == 159 {
            assert!(h.read(36524));
            assert!(!h.read(36523));
            assert!(h.read(36522));
            assert!(h.read(35795));
            assert!(!h.read(35794));
            assert_eq!(h.longest_streak, 33);
        }
        for len in 0..n {
            assert_eq!(classify_cross_ink_global(&b[..len]), CrossInkFile::Invalid);
        }
    }
    assert_eq!(classify_cross_ink_global(&[4]), CrossInkFile::Newer);
    assert_eq!(classify_cross_ink_global(&[0; 160]), CrossInkFile::Newer);
    let mut b = [0; 159];
    b[0] = 3;
    b[156] = 4;
    assert_eq!(classify_cross_ink_global(&b), CrossInkFile::Invalid);
    b[156] = 0;
    put32(&mut b, 61, 36525);
    assert_eq!(classify_cross_ink_global(&b), CrossInkFile::Invalid);
    for (v, n) in [(1, 11), (2, 12), (3, 16), (4, 69), (5, 73)] {
        let mut b = [0; 73];
        b[0] = v;
        assert!(valid_cross_ink_book(&b[..n]));
        assert!(!cross_ink_book_completed(&b[..n]));
        if n >= 12 {
            b[11] = 1;
            assert!(cross_ink_book_completed(&b[..n]));
            b[11] = 2;
            assert!(!valid_cross_ink_book(&b[..n]));
        }
    }
    let mut b = [0; 73];
    b[0] = 5;
    b[17..21].copy_from_slice(&[0xe8, 7, 2, 29]);
    assert!(valid_cross_ink_book(&b));
    b[17] = 0xe9;
    assert!(!valid_cross_ink_book(&b));
    b[17..21].fill(0);
    b[16] = 4;
    assert!(!valid_cross_ink_book(&b));
    b[16] = 0;
    b[24] = 1;
    assert!(!valid_cross_ink_book(&b));
}
#[test]
fn frozen_records_status_candidates_padding_binding() {
    let mut c = CrossInkGlobalCopy::default();
    c.status = CrossInkStatus::Imported;
    c.candidate = CrossInkCandidate::Backup;
    c.size = 13;
    c.bytes[0] = 1;
    let b = encode_cross_ink_global(&c).unwrap();
    assert_eq!(
        &b[..16],
        &[78, 87, 67, 71, 1, 0, 0, 0, 0, 0, 0, 0, 1, 3, 13, 0]
    );
    assert_eq!(decode_cross_ink_global_record(&b), Some(c));
    for at in [15, 29, 175] {
        let mut bad = b;
        bad[at] = 1;
        reseal(&mut bad);
        assert!(decode_cross_ink_global_record(&bad).is_none());
    }
    for candidate in [
        CrossInkCandidate::None,
        CrossInkCandidate::Temp,
        CrossInkCandidate::Previous,
        CrossInkCandidate::Legacy,
    ] {
        c.candidate = candidate;
        assert!(decode_cross_ink_global_record(&encode_cross_ink_global(&c).unwrap()).is_none());
    }
    c = CrossInkGlobalCopy::default();
    assert!(decode_cross_ink_global_record(&encode_cross_ink_global(&c).unwrap()).is_some());
    c.status = CrossInkStatus::Newer;
    c.candidate = CrossInkCandidate::Primary;
    assert_eq!(cross_ink_history(&c).seconds, 0);
    assert!(decode_cross_ink_global_record(&encode_cross_ink_global(&c).unwrap()).is_some());
    c.size = 1;
    assert!(decode_cross_ink_global_record(&encode_cross_ink_global(&c).unwrap()).is_none());
    c.size = 160;
    assert!(encode_cross_ink_global(&c).is_none());
    let mut source = CrossInkSource::default();
    source.name[..7].copy_from_slice(b"epub_77");
    source.copy.status = CrossInkStatus::Imported;
    source.copy.candidate = CrossInkCandidate::Temp;
    source.copy.size = 73;
    source.copy.bytes[0] = 5;
    source.binding = CrossInkBinding::Unverified;
    source.digest[0] = 0xab;
    let b = encode_cross_ink_source(&source).unwrap();
    assert_eq!(decode_cross_ink_source(&b), Some(source));
    for at in [43, 47, 121, 122, 123] {
        let mut bad = b;
        bad[at] = 9;
        reseal(&mut bad);
        assert!(decode_cross_ink_source(&bad).is_none());
    }
    let mut bad = b;
    bad[124] = 0;
    reseal(&mut bad);
    assert!(decode_cross_ink_source(&bad).is_none());
    source.copy.size = 69;
    source.copy.bytes[0] = 4;
    assert!(decode_cross_ink_source(&encode_cross_ink_source(&source).unwrap()).is_none());
    source.copy.candidate = CrossInkCandidate::Previous;
    source.binding = CrossInkBinding::Manifest;
    assert!(decode_cross_ink_source(&encode_cross_ink_source(&source).unwrap()).is_some());
    for name in [
        b"epub_1".as_slice(),
        b"txt_007",
        b"xtc_18446744073709551615",
    ] {
        assert!(cross_ink_source_name(name));
    }
    for name in [
        b"_1".as_slice(),
        b"epub_",
        b"Epub_1",
        b"epub_2x",
        b"abcdefghijklmnopqrstuvwxyz_12345",
        b"a_1\0",
    ] {
        assert!(!cross_ink_source_name(name));
    }
    let record = ImportRecord {
        sources: 7,
        manifest: [0x42; 32],
    };
    let b = encode_import(&record);
    assert_eq!(decode_import(&b), Some(record));
    for n in 0..56 {
        assert!(decode_import(&b[..n]).is_none());
    }
    for at in [0, 4, 48, 49, 50, 51] {
        let mut bad = b;
        bad[at] ^= 1;
        reseal(&mut bad);
        assert!(decode_import(&bad).is_none());
    }
}
#[test]
fn cache_six_day_window_and_proof_failures() {
    let today = clock(10, 16, 8);
    let mut days = [0; 31];
    days[..16].fill(60);
    let l = ledger(today.month, today.month, days, [0; 4], 960);
    let cache = StatsStreak {
        day: today.day - 6,
        run: 9,
        days: 255,
        before: 540,
    };
    assert_eq!(
        sleep_week(&l, &cache, None, today, 0),
        SleepWeek {
            days: 127,
            streak: 16
        }
    );
    assert_eq!(
        sleep_week(&l, &cache, None, clock(10, 17, 8), 60),
        SleepWeek {
            days: 127,
            streak: 0
        }
    );
    let broken = ledger(today.month, today.month, days, [0; 4], 1);
    assert_eq!(
        sleep_seconds_before(&broken, None, clock(10, 1, 8).day),
        None
    );
    let gap = ledger(today.month, today.month - 2, days, [0; 4], 960);
    let archive = empty_stats_month(today.month - 1);
    assert_eq!(
        sleep_seconds_before(&gap, Some(&archive), clock(8, 31, 8).day),
        None
    );
}
