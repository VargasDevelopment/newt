// SPDX-License-Identifier: MIT
//! Candidate recovery and bounded read/hash failures on synthetic bytes.
use super::*;

#[test]
fn candidate_order_uses_full_size_and_stops_at_newer_global() {
    let mut card = Card::default();
    card.file(
        b"/card/.crosspoint/global_stats.bin",
        &vec![1; ci::GLOBAL_BYTES + 1],
    );
    card.file(b"/card/.crosspoint/global_stats.bin.bak", &global(2));
    let mut copy = ci::CrossInkGlobalCopy::default();
    assert_eq!(read_global(&mut card, c"/card", &mut copy), Found::Read);
    assert_eq!(
        (copy.status, copy.candidate, copy.size),
        (ci::CrossInkStatus::Newer, ci::CrossInkCandidate::Primary, 0)
    );
    assert_eq!(card.opens, 1);
    card.file(b"/card/.crosspoint/global_stats.bin", &[0]);
    assert_eq!(read_global(&mut card, c"/card", &mut copy), Found::Read);
    assert_eq!(
        (copy.status, copy.candidate, copy.size),
        (
            ci::CrossInkStatus::Imported,
            ci::CrossInkCandidate::Backup,
            17
        )
    );
}

#[test]
fn source_recovery_requires_complete_v5_temp_and_never_skips_read_errors() {
    let mut card = Card::default();
    card.source(b"a_1", b"stats_v5.bin", &[0]);
    card.source(b"a_1", b"stats_v5.bin.tmp", &book(4));
    card.source(b"a_1", b"stats_v5.bin.bak", &book(3));
    card.source(b"a_1", b"stats_v4.bin", &book(4));
    let mut copy = ci::CrossInkCopy::default();
    assert_eq!(
        read_source(&mut card, c"/card", b"a_1", &mut copy),
        Found::Read
    );
    assert_eq!(
        (copy.candidate, copy.size),
        (ci::CrossInkCandidate::Backup, 16)
    );
    card.source(b"a_1", b"stats_v5.bin.tmp", &book(5));
    assert_eq!(
        read_source(&mut card, c"/card", b"a_1", &mut copy),
        Found::Read
    );
    assert_eq!(copy.candidate, ci::CrossInkCandidate::Temp);
    card.failed = Some(b"/card/.crosspoint/a_1/stats_v5.bin".to_vec());
    assert_eq!(
        read_source(&mut card, c"/card", b"a_1", &mut copy),
        Found::Failed
    );
    assert_eq!(card.opens, card.closes);
}

#[test]
fn prefix_reads_retry_four_times_reset_on_success_and_ignore_close_results() {
    let mut card = Card {
        close_fail: true,
        read_limit: Some(1),
        ..Card::default()
    };
    card.file(b"/card/file", &[1, 2]);
    card.read_faults.extend([Err(IoError::Interrupted); 3]);
    let mut bytes = [0; 2];
    let mut size = 0;
    assert_eq!(
        read_file(&mut card, c"/card/file", &mut bytes, &mut size),
        Found::Read
    );
    assert_eq!(bytes, [1, 2]);
    assert_eq!(size, 2);
    assert_eq!(card.reads, 5);
    card.read_faults.extend([Err(IoError::Interrupted); 4]);
    assert_eq!(
        read_file(&mut card, c"/card/file", &mut bytes, &mut size),
        Found::Failed
    );
    card.read_faults.push_back(Ok(0));
    assert_eq!(
        read_file(&mut card, c"/card/file", &mut bytes, &mut size),
        Found::Failed
    );
    assert_eq!(card.opens, card.closes);
}

#[test]
fn hash_context_follows_fstat_even_when_unadmitted_and_cancellation_rechecks_retries() {
    for (size, stat_fail, expected) in [
        (0, false, Found::Missing),
        (BOOK_MAX_BYTES + 1, false, Found::Missing),
        (1, true, Found::Failed),
    ] {
        let mut card = Card {
            stat_fail,
            ..Card::default()
        };
        card.file(b"/card/book", &[3]);
        card.files.get_mut(b"/card/book".as_slice()).unwrap().1 = size;
        let mut factory = Factory::default();
        let mut stopped = false;
        assert_eq!(
            hash_book(
                &mut card,
                &mut factory,
                c"/card/book",
                &mut [0; 32],
                &mut || false,
                &mut stopped
            ),
            expected
        );
        assert_eq!(
            (
                factory.contexts,
                factory.hasher.begins,
                factory.hasher.finishes
            ),
            (1, 0, 0)
        );
        assert_eq!(card.opens, card.closes);
    }
    let mut card = Card::default();
    card.file(b"/card/book", &[3]);
    card.read_faults.push_back(Err(IoError::Interrupted));
    let mut factory = Factory::default();
    let mut stopped = false;
    let mut checks = 0;
    assert_eq!(
        hash_book(
            &mut card,
            &mut factory,
            c"/card/book",
            &mut [0; 32],
            &mut || {
                checks += 1;
                checks == 2
            },
            &mut stopped
        ),
        Found::Missing
    );
    assert!(stopped);
    assert_eq!((card.reads, factory.hasher.finishes), (1, 0));
    assert_eq!(card.opens, card.closes);
}
