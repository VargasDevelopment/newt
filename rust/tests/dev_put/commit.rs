// SPDX-License-Identifier: MIT
//! DONE's commit: what ends it before the first rename, how a refused rename
//! is reported, and what the folder's next FILE may resolve after a power
//! loss. A FAT rename cut short leaves two names on one file, so recovery
//! never deletes while two of the target, backup and temp are present.
use super::*;
use fakes::FREED;
use newt_core::dev_put::TOTAL_MS;

#[test]
fn a_failed_rename_puts_the_old_file_back() {
    let mut card = FakeCard { fail_rename_onto: Some((TARGET, 1)), ..FakeCard::default() };
    card.put_file(TARGET, b"old");
    let outcome = lease(&mut card, Host::new().put("dev", "runtime.bin", &pattern(4_000)));
    assert_eq!(outcome.last(), format!("ERROR card step=rename code={FAILED}"));
    unchanged(&card, Some(b"old"));
    resolved(&card);
}

#[test]
fn a_failed_restore_keeps_the_backup_and_says_so() {
    let new = pattern(4_000);
    let mut card = FakeCard { fail_rename_onto: Some((TARGET, 2)), ..FakeCard::default() };
    card.put_file(TARGET, b"old");
    let mut host = Host::new();
    host.put("dev", "runtime.bin", &new);
    let last_id = host.id;
    let outcome = lease(&mut card, &mut host);
    assert_eq!(outcome.last(), format!("ERROR recovery-needed step=rename code={FAILED}"));
    assert_eq!(outcome.end, End::Kept { last_id });
    assert_eq!((card.file(TARGET), card.file(TEMP)), (None, Some(&new[..])));
    assert_eq!((card.file(OLD), card.file(MARKER)), (Some(&b"old"[..]), Some(&b"runtime.bin"[..])));
    // Backup and temp together: the next write deletes neither.
    let outcome = lease(&mut card, Host::new().put("dev", "other.bin", b"other"));
    assert_eq!(outcome.bodies(), [BANNER, "ERROR recovery-needed step=commit code=0"]);
    assert_eq!((card.file(OLD), card.file(TEMP)), (Some(&b"old"[..]), Some(&new[..])));
}

#[test]
fn a_rename_that_fails_after_moving_keeps_the_marker() {
    let new = pattern(4_000);
    // (old file, the rename that moves then fails, its step, target, backup, temp after)
    for (old, onto, step, target, backup, temp) in [
        (true, OLD, "replace", None, Some(&b"old"[..]), Some(&new[..])),
        (true, TARGET, "rename", Some(&new[..]), Some(&b"old"[..]), None),
        (false, TARGET, "rename", Some(&new[..]), None, None)] {
        let mut card = FakeCard { fail_after_rename_onto: Some(onto), ..FakeCard::default() };
        if old { card.put_file(TARGET, b"old"); }
        let outcome = lease(&mut card, Host::new().put("dev", "runtime.bin", &new));
        assert_eq!(outcome.last(), format!("ERROR recovery-needed step={step} code={FAILED}"), "{onto} {old}");
        assert_eq!((card.file(TARGET), card.file(OLD), card.file(TEMP)), (target, backup, temp), "{onto} {old}");
        assert_eq!(card.file(MARKER), Some(&b"runtime.bin"[..]));
    }
}

#[test]
fn a_rename_a_power_loss_cut_short_is_never_unlinked() {
    // (the cut rename, whether an old file was being replaced)
    for (from, to, old) in [(TARGET, OLD, true), (TEMP, TARGET, true), (TEMP, TARGET, false)] {
        let mut card = FakeCard::default();
        card.put_file(TEMP, b"verified new bytes");
        card.put_file(if from == TARGET { TARGET } else { OLD }, b"the old file");
        if !old { card.nodes.remove(OLD); }
        card.put_file(MARKER, b"runtime.bin");
        card.cut_rename(from, to);
        let before = card.nodes.clone();
        let outcome = lease(&mut card, Host::new().put("dev", "other.bin", b"other"));
        assert_eq!(outcome.bodies(), [BANNER, "ERROR recovery-needed step=commit code=0"], "{to} {old}");
        assert_eq!(card.count("remove"), 0, "{to} {old}");
        assert_eq!(card.nodes, before, "nothing is touched: {to} {old}");
        assert!(card.nodes.values().all(|node| *node != Node::File(FREED.to_vec())));
    }
}

#[test]
fn a_commit_cut_short_with_one_name_left_is_resolved() {
    // (the one name present beside the marker, what the target holds after)
    for (present, target) in [(Some(OLD), Some(&b"kept"[..])), (Some(TARGET), Some(&b"kept"[..])), (Some(TEMP), None),
        (None, None)] {
        let mut card = FakeCard::default();
        card.put_file(MARKER, b"runtime.bin");
        if let Some(path) = present { card.put_file(path, b"kept"); }
        let outcome = lease(&mut card, Host::new().put("dev", "other.bin", b"other"));
        assert_eq!(outcome.last(), done(b"other"), "{present:?}");
        assert_eq!((card.file(TARGET), card.file(OTHER)), (target, Some(&b"other"[..])), "{present:?}");
        unchanged(&card, target);
        resolved(&card);
    }
}

#[test]
fn a_backup_it_cannot_resolve_refuses_every_write() {
    for (marker, reply) in [(None, "ERROR recovery-needed step=backup code=0"),
        (Some(&b"../Books"[..]), "ERROR recovery-needed step=marker code=0"),
        (Some(b""), "ERROR recovery-needed step=marker code=0")] {
        let mut card = FakeCard::default();
        card.put_file(OLD, b"the only copy");
        if let Some(marker) = marker { card.put_file(MARKER, marker); }
        let outcome = lease(&mut card, Host::new().put("dev", "other.bin", b"other"));
        assert_eq!(outcome.bodies(), [BANNER, reply], "{marker:?}");
        assert_eq!(card.file(OLD), Some(&b"the only copy"[..]), "the backup is never removed");
        assert_eq!((card.file(TARGET), card.file(OTHER)), (None, None));
    }
}

/// A replacement whose DONE meets `card` (a press or a late clock) when a step runs.
fn replace_with(mut card: FakeCard) -> (FakeCard, Outcome, Vec<u8>) {
    let new = pattern(4_000);
    card.put_file(TARGET, b"old");
    let outcome = lease(&mut card, Host::new().put("dev", "runtime.bin", &new));
    (card, outcome, new)
}

fn pressed_during_done(step: &'static str, nth: u32) -> (FakeCard, Outcome, Vec<u8>) {
    replace_with(FakeCard { press_on: Some((step, nth)), ..FakeCard::default() })
}

#[test]
fn a_press_while_done_reads_back_stops_it_there() {
    let (card, outcome, _) = pressed_during_done("read", 1);
    assert_eq!(outcome.end, End::Cancelled);
    assert_eq!(outcome.last(), "ERROR abandoned reason=physical-input");
    assert_eq!(card.count("read"), 1, "the read-back stops at the next chunk, not at its end");
    unchanged(&card, Some(b"old"));
    resolved(&card);
}

#[test]
fn a_press_just_before_the_first_rename_keeps_the_old_file() {
    // The second sync is the marker's, the last card step before the renames.
    let (card, outcome, _) = pressed_during_done("sync", 2);
    assert_eq!(outcome.end, End::Cancelled);
    assert_eq!(outcome.last(), "ERROR abandoned reason=physical-input");
    assert_eq!(card.count("rename"), 0);
    unchanged(&card, Some(b"old"));
    resolved(&card);
}

#[test]
fn a_press_after_the_first_rename_never_stops_it_halfway() {
    let (card, outcome, new) = pressed_during_done("rename", 1);
    assert_eq!(outcome.last(), done(&new));
    assert_eq!(card.file(TARGET), Some(&new[..]));
    resolved(&card);
    assert!(outcome.wire.pressed.get(), "the press is left for the screen");
}

#[test]
fn the_deadline_stops_done_before_its_first_rename() {
    for step in [("read", 1), ("sync", 2)] {
        let (card, outcome, _) = replace_with(FakeCard { late_on: Some((step.0, step.1, TOTAL_MS)), ..FakeCard::default() });
        assert_eq!(outcome.end, End::Cancelled, "{step:?}");
        assert_eq!(outcome.last(), "ERROR abandoned reason=total-time", "{step:?}");
        assert_eq!(card.count("rename"), 0, "{step:?}");
        unchanged(&card, Some(b"old"));
        resolved(&card);
    }
}

#[test]
fn a_hello_during_done_is_read_after_its_answer() {
    let new = pattern(4_000);
    let mut card = FakeCard::default();
    card.put_file(TARGET, b"old");
    let mut host = Host::new();
    host.put("dev", "runtime.bin", &new);
    let last_id = host.id;
    let outcome = lease(&mut card, host.then(Event::Line("N1 HELLO\n".into())));
    assert_eq!(outcome.end, End::Kept { last_id }, "DONE is the commit point, not a place to listen");
    assert_eq!(outcome.last(), done(&new));
    assert_eq!(card.file(TARGET), Some(&new[..]));
    resolved(&card);
    assert_eq!(outcome.wire.unread(), "N1 HELLO\n", "the bridge reads the HELLO next");
}
