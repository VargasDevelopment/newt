// SPDX-License-Identifier: MIT
//! DONE's commit: what ends it before the first rename, how a refused rename
//! is reported, and that a marker a power loss left refuses the folder's next
//! FILE. A FAT rename cut short can leave two names, or a name and a hidden
//! short alias, on one file, so recovery deletes nothing.
use super::*;
use fakes::FREED;

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
    assert_eq!((card.file(OLD), card.file(MARKER)), (Some(&b"old"[..]), Some(&b"Newt/Dev/runtime.bin"[..])));
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
        assert_eq!(card.file(MARKER), Some(&b"Newt/Dev/runtime.bin"[..]));
    }
}

#[test]
fn a_rename_a_power_loss_cut_short_is_never_unlinked() {
    // (how the rename was cut, the cut rename, whether an old file was being
    // replaced). A cut leaves both long names, or only the source's hidden short
    // alias; `runtime.bin` fits 8.3, so it has no long name to lose.
    let both = FakeCard::cut_rename as fn(&mut FakeCard, &str, &str);
    let alias = FakeCard::orphan_rename as fn(&mut FakeCard, &str, &str);
    let cases = [(both, TARGET, OLD, true), (both, TEMP, TARGET, true), (both, TEMP, TARGET, false),
                 (alias, TEMP, TARGET, true), (alias, TEMP, TARGET, false)];
    for (cut, from, to, old) in cases {
        let mut card = FakeCard::default();
        card.put_file(TEMP, b"verified new bytes");
        card.put_file(if from == TARGET { TARGET } else { OLD }, b"the old file");
        if !old { card.nodes.remove(OLD); }
        card.put_file(MARKER, b"runtime.bin");
        cut(&mut card, from, to);
        refuses_and_keeps_everything(&mut card, &format!("{to} {old}"));
    }
}

/// The folder's next write refuses, removes nothing and keeps the marker.
fn refuses_and_keeps_everything(card: &mut FakeCard, case: &str) {
    let before = card.nodes.clone();
    let outcome = lease(card, Host::new().put("dev", "other.bin", b"other"));
    assert_eq!(outcome.bodies(), [BANNER, "ERROR recovery-needed step=commit code=0"], "{case}");
    assert_eq!(card.count("remove"), 0, "{case}");
    assert_eq!(card.nodes, before, "nothing is touched: {case}");
    assert!(card.nodes.values().all(|node| *node != Node::File(FREED.to_vec())), "{case}");
}

#[test]
fn a_marker_left_by_any_commit_refuses_whatever_the_names_show() {
    // A missing long name proves nothing, so every presence of the target,
    // backup and temp beside a marker refuses. The target alone (bit 0), what
    // a finished commit whose cleanup was lost looks like, goes first.
    for present in (0..8).map(|n| n ^ 1) {
        let mut card = FakeCard::default();
        card.put_file(MARKER, b"runtime.bin");
        for (bit, path) in [TARGET, OLD, TEMP].into_iter().enumerate() {
            if present & 1 << bit != 0 { card.put_file(path, path.as_bytes()); }
        }
        refuses_and_keeps_everything(&mut card, &format!("present {present:03b}"));
    }
}

#[test]
fn a_backup_without_its_marker_refuses_every_write() {
    let mut card = FakeCard::default();
    card.put_file(OLD, b"the only copy");
    let outcome = lease(&mut card, Host::new().put("dev", "other.bin", b"other"));
    assert_eq!(outcome.bodies(), [BANNER, "ERROR recovery-needed step=backup code=0"]);
    assert_eq!(card.file(OLD), Some(&b"the only copy"[..]), "the backup is never removed");
    assert_eq!((card.file(TARGET), card.file(OTHER)), (None, None));
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
fn active_transfers_have_no_total_size_or_time_cap() {
    for step in [("read", 1), ("sync", 2)] {
        let (card, outcome, new) = replace_with(FakeCard { late_on: Some((step.0, step.1, 600_000)), ..FakeCard::default() });
        assert_eq!(outcome.last(), done(&new), "{step:?}");
        assert_eq!(card.file(TARGET), Some(&new[..]));
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
