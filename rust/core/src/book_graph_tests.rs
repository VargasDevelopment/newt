// SPDX-License-Identifier: MIT
extern crate std;
use self::std::vec::Vec;
use crate::{book_days::*, reading_stats::*, stats_clock::*, stats_graph_data::*};
fn fold(ms: u64, pages: u32, sessions: u32, day: u16, hour: u8) -> StatsFold {
    StatsFold {
        counters: StatsCounters {
            milliseconds: ms,
            pages,
            sessions,
        },
        clock: stats_clock_for_day(day, hour),
        ..StatsFold::default()
    }
}
#[test]
fn credit_subseconds_resets_lost_save_and_missing_archive() {
    let mut h = BookDays::default();
    let zero = StatsCounters::default();
    assert!(!fold_book_days(
        &mut h,
        &zero,
        &fold(999, 0, 0, 9497, 5),
        None
    ));
    assert!(fold_book_days(
        &mut h,
        &zero,
        &fold(1999, 0, 0, 9497, 5),
        None
    ));
    assert_eq!(
        (h.watermark, h.since_day, h.period_seconds),
        (1000, 9497, [1, 0, 0, 0])
    );
    assert!(!fold_book_days(
        &mut h,
        &zero,
        &fold(1999, 0, 0, 9497, 5),
        None
    ));
    // NWTS already credited 5 seconds while this book record lost its write.
    let before = StatsCounters {
        milliseconds: 5000,
        ..zero
    };
    fold_book_days(&mut h, &before, &fold(6500, 0, 0, 9497, 12), None);
    assert_eq!((h.watermark, h.period_seconds), (6000, [1, 1, 0, 0]));
    // Below this record's watermark uses the ledger base (progress reset).
    fold_book_days(&mut h, &zero, &fold(2500, 0, 0, 9497, 17), None);
    assert_eq!((h.watermark, h.period_seconds), (2000, [1, 1, 2, 0]));
    for before in [
        StatsCounters {
            milliseconds: 3000,
            ..zero
        },
        StatsCounters { pages: 1, ..zero },
        StatsCounters {
            sessions: 1,
            ..zero
        },
    ] {
        let old = h.period_seconds;
        fold_book_days(&mut h, &before, &fold(1000, 0, 0, 9497, 21), None);
        assert_eq!(h.period_seconds, old);
        assert_eq!(h.watermark, 1000);
    }
    let f = fold(9000, 0, 0, 9600, 21);
    assert_eq!(book_fold_block(&h, &zero, &f), 154);
    fold_book_days(&mut h, &zero, &f, None);
    assert_eq!(h.watermark, 9000);
    assert_eq!(h.current.block, 153);
    let target = BookDayBlock {
        block: 154,
        ..BookDayBlock::default()
    };
    fold_book_days(&mut h, &zero, &fold(10000, 0, 0, 9600, 21), Some(&target));
    assert_eq!(h.current.seconds[52], 1);
    assert_eq!(h.period_seconds[3], 1);
    let old = h.period_seconds;
    fold_book_days(&mut h, &zero, &fold(11000, 0, 0, 9496, 5), None);
    assert_eq!(h.period_seconds, old);
}
#[test]
fn codecs_all_bytes_reserved_binding_and_untouched_outputs() {
    let digest = [7; 32];
    let h = BookDays::default();
    let original = BookDays { watermark: 99, ..h };
    let b = encode_book_days(&digest, &h, 0x12345678);
    assert_eq!(&b[..12], &[78, 87, 84, 68, 1, 0, 0, 0, 120, 86, 52, 18]);
    let mut out = original;
    assert!(decode_book_days(&b, &digest, &mut out));
    assert_eq!(out, h);
    for i in 0..BOOK_DAYS_BYTES {
        let mut corrupt = b;
        corrupt[i] ^= 1;
        let mut out = original;
        assert!(!decode_book_days(&corrupt, &digest, &mut out));
        assert_eq!(out, original);
    }
    let mut out = original;
    assert!(!decode_book_days(&b[..231], &digest, &mut out));
    assert!(!decode_book_days(&b, &[0; 32], &mut out));
    assert!(!decode_book_days(&b, &[8; 32], &mut out));
    for bad in [
        BookDays { flags: 2, ..h },
        BookDays {
            current: BookDayBlock {
                block: 153,
                ..BookDayBlock::default()
            },
            ..h
        },
        BookDays {
            period_seconds: [1, 0, 0, 0],
            ..h
        },
        BookDays { since_day: 1, ..h },
    ] {
        assert!(!decode_book_days(
            &encode_book_days(&digest, &bad, 0),
            &digest,
            &mut out
        ));
        assert_eq!(out, original);
    }
    let a = BookDayBlock {
        block: 153,
        seconds: [u16::MAX; 62],
    };
    let b = encode_book_archive(&digest, &a, 9);
    let mut out = BookDayBlock::default();
    assert!(decode_book_archive(&b, &digest, 153, &mut out));
    assert_eq!(out, a);
    for i in 0..BOOK_ARCHIVE_BYTES {
        let mut corrupt = b;
        corrupt[i] ^= 1;
        let mut out = a;
        assert!(!decode_book_archive(&corrupt, &digest, 153, &mut out));
        assert_eq!(out, a);
    }
    assert!(!decode_book_archive(&b, &digest, 154, &mut out));
}
#[test]
fn callbacks_absent_unreadable_wrong_block_and_bounded_window() {
    let h = BookDays {
        since_day: 9497,
        current: BookDayBlock {
            block: 155,
            ..BookDayBlock::default()
        },
        ..BookDays::default()
    };
    for status in [
        BookBlockRead::Loaded,
        BookBlockRead::Absent,
        BookBlockRead::Unreadable,
    ] {
        let mut calls = Vec::new();
        let g = book_days_graph(
            &h,
            |b, out| {
                calls.push(b);
                out.seconds.fill(17);
                status
            },
            9550,
            42,
        );
        assert_eq!(calls, [153, 154]);
        assert_eq!(g.count, 42);
        assert_eq!(
            g.kinds[0],
            if status == BookBlockRead::Unreadable {
                StatsDayKind::Unrecorded
            } else {
                StatsDayKind::Timed
            }
        );
        assert_eq!(
            g.seconds[0],
            if status == BookBlockRead::Loaded {
                17
            } else {
                0
            }
        );
    }
    let mut calls = 0;
    let g = book_days_graph(
        &h,
        |_, out| {
            calls += 1;
            out.block = 0;
            out.seconds.fill(9);
            BookBlockRead::Loaded
        },
        9550,
        1000,
    );
    assert_eq!(calls, 42);
    assert_eq!(g.seconds, [9; 42]);
    for last in [0, 9496, 36525, 65535] {
        assert_eq!(
            book_days_graph(&h, |_, _| panic!("invalid clock must not load"), last, 42),
            StatsDaysGraph::default()
        );
    }
    assert_eq!(
        book_days_graph(&h, |_, _| panic!("zero window"), 9550, 0).count,
        0
    );
}
#[test]
fn thresholds_windows_period_overflow_and_saturation() {
    assert_eq!(stats_unknown_seconds(&[1, 2, 3, 4], 69), 0);
    assert_eq!(stats_unknown_seconds(&[1, 2, 3, 4], 70), 60);
    assert_eq!(stats_unknown_seconds(&[0; 4], u64::MAX), u32::MAX);
    assert_eq!(stats_window_end(10000, 9900, 42, 2), 9916);
    assert_eq!(stats_window_end(10000, 9900, 42, 3), 10000);
    assert_eq!(stats_window_end(10000, 0, 0, u32::MAX), 10000);
    assert_eq!(book_window_end(10000, 9958, 9900, 42, 0), 9958);
    assert_eq!(book_window_end(10000, 9959, 9900, 42, 0), 10000);
    assert_eq!(book_window_end(0, 0, 0, 42, 1), 0);
    let mut h = BookDays {
        since_day: 9497,
        current: BookDayBlock {
            block: 153,
            seconds: [65535; 62],
        },
        period_seconds: [u32::MAX; 4],
        imported: BookImported {
            periods: [1; 4],
            ..BookImported::default()
        },
        ..BookDays::default()
    };
    assert_eq!(book_periods_graph(&h).seconds, [u32::MAX; 4]);
    fold_book_days(
        &mut h,
        &StatsCounters::default(),
        &fold(u64::MAX, 0, 0, 9497, 5),
        None,
    );
    assert_eq!(h.current.seconds[11], 65535);
    assert_eq!(h.period_seconds, [u32::MAX; 4]);
}
#[test]
fn library_validated_ledger_wrong_month_wrap_and_callback_order() {
    // Construct only a wire fixture; the public codec must approve private ledger state.
    let mut b = [0u8; LEDGER_BYTES];
    b[..4].copy_from_slice(&LEDGER_MAGIC.to_le_bytes());
    b[4] = 1;
    b[12..14].copy_from_slice(&24312u16.to_le_bytes());
    b[14..16].copy_from_slice(&24313u16.to_le_bytes());
    b[30..32].copy_from_slice(&24313u16.to_le_bytes());
    b[88..90].copy_from_slice(&u16::MAX.to_le_bytes());
    b[96..100].copy_from_slice(&10u32.to_le_bytes());
    b[112..114].copy_from_slice(&13u16.to_le_bytes());
    let checksum = crate::wire::fnv1a(&b[..176]);
    b[176..].copy_from_slice(&checksum.to_le_bytes());
    let mut state = StatsState::Unseeded;
    assert!(decode_ledger(&b, &mut state));
    let ledger = state.seeded().expect("validated ledger");
    let mut archive = empty_stats_month(1);
    archive.day_seconds[30] = 9;
    archive.bucket_seconds = [u32::MAX, 1, 2, 3];
    let mut calls = Vec::new();
    let g = library_days_graph(
        ledger,
        |m, out| {
            calls.push(m);
            *out = archive;
            GraphMonthRead::Loaded
        },
        &CrossInkHistory::default(),
        &[0; 7],
        9528,
        2,
    );
    assert_eq!(calls, [24312]);
    assert_eq!(g.kinds[..2], [StatsDayKind::Timed; 2]);
    assert_eq!(g.seconds[..2], [9, 13]);
    let mut calls = Vec::new();
    let p = library_periods_graph(
        ledger,
        |m, out| {
            calls.push(m);
            *out = archive;
            GraphMonthRead::Loaded
        },
        &[0; 4],
        100,
    );
    assert_eq!(calls, [24312]);
    assert_eq!(p.seconds, [9, 1, 2, 3]);
    assert_eq!(p.unknown_seconds, 85);
    for status in [GraphMonthRead::Absent, GraphMonthRead::Unreadable] {
        let mut calls = Vec::new();
        let g = library_days_graph(
            ledger,
            |m, out| {
                calls.push(m);
                *out = archive;
                status
            },
            &CrossInkHistory::default(),
            &[0; 7],
            9528,
            2,
        );
        assert_eq!(calls, [24312]);
        assert_eq!(g.seconds[..2], [0, 13]);
    }
}

#[test]
fn matching_zero_digest_never_publishes_decoded_outputs() {
    let zero = [0; 32];
    let original = BookDays {
        watermark: 99,
        ..BookDays::default()
    };
    let mut output = original;
    let record = encode_book_days(&zero, &BookDays::default(), 0);
    assert!(!decode_book_days(&record, &zero, &mut output));
    assert_eq!(output, original);
    let mut prior = BookDayBlock {
        block: 154,
        ..BookDayBlock::default()
    };
    prior.seconds[0] = 11;
    let mut archive_output = prior;
    let encoded = encode_book_archive(
        &zero,
        &BookDayBlock {
            block: 153,
            ..BookDayBlock::default()
        },
        0,
    );
    assert!(!decode_book_archive(
        &encoded,
        &zero,
        153,
        &mut archive_output
    ));
    assert_eq!(archive_output, prior);
}
#[test]
fn large_credit_clamps_seconds_but_keeps_full_rounded_watermark() {
    for (milliseconds, rounded) in [
        (4_294_967_296_999u64, 4_294_967_296_000u64),
        (u64::MAX, 18_446_744_073_709_551_000u64),
    ] {
        let mut days = BookDays::default();
        assert!(fold_book_days(
            &mut days,
            &StatsCounters::default(),
            &fold(milliseconds, 0, 0, 9497, 5),
            None
        ));
        assert_eq!(days.watermark, rounded);
        assert_eq!(days.period_seconds, [u32::MAX, 0, 0, 0]);
        assert_eq!(days.current.seconds[11], u16::MAX);
    }
}
