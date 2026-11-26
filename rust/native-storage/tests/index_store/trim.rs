// SPDX-License-Identifier: MIT
//! Open reservation and resumable idle trim over the same crashable card.
use super::*;
use core::cell::Cell;

fn reserve(card: &mut Card) {
    with_store(card, |cache| {
        let mut work = IndexWorkState::new();
        let mut store = IndexBuildStore::lend(cache, &mut work, &BOOK.digest).unwrap();
        store.census(&|| false);
    });
}
fn populated(ties: bool) -> Card {
    let mut card = Card::new();
    for i in 0..32u8 {
        card.seed(&leaf(&[i; 32]), 100 + usize::from(i) * 17,
            if ties { 0 } else { i64::from(i % 7) });
    }
    card
}
fn trim(card: &mut Card, cancel: &dyn Authority) -> MissingProbe {
    let pinned = leaf(&BOOK.digest);
    with_store(card, |s| s.index_trim(cancel, now, None, pinned.as_bytes())).outcome
}
fn opened(ties: bool) -> Card {
    let mut card = populated(ties);
    reserve(&mut card);
    with_store(&mut card, |s| write(s, BOOK)).unwrap();
    assert_eq!(card.indexes().len(), 31);
    card
}
fn expected(ties: bool) -> Vec<String> {
    let mut card = populated(ties);
    with_store(&mut card, |s| s.index_census(&Never, None, None, None));
    with_store(&mut card, |s| write(s, BOOK)).unwrap();
    card.indexes()
}

// For current indexes within the byte quota: at most 2 deletes at 32 files,
// none at 30 or fewer. Temporaries and legacy long names always go, and byte
// pressure can take more (see the census tests).
#[test]
fn open_frees_only_the_two_slots_a_spilled_build_needs() {
    for count in [32, 0, 3, 24, 29, 30, 31, 33, 40] {
        let mut card = Card::new();
        for i in 0..count { card.seed(&leaf(&[i; 32]), 100, i64::from(i)); }
        reserve(&mut card);
        assert_eq!(count as usize - card.indexes().len(), usize::from(count.saturating_sub(30)),
            "open reservation at {count} files");
        assert!(card.indexes().len() <= 30);
    }
}

#[test]
fn idle_trim_kept_set_equals_inline_census_for_mixed_sizes_times_and_ties() {
    for ties in [false, true] {
        let mut card = opened(ties);
        assert_eq!(trim(&mut card, &Never), MissingProbe::Ready);
        assert!(card.indexes().len() <= 24);
        assert_eq!(card.indexes(), expected(ties));
        assert_eq!(with_store(&mut card, |s| read(s, &BOOK, complete)), Some(true));
    }
}

#[test]
fn idle_trim_pins_only_the_open_index_and_ages_out_its_siblings() {
    use newt_core::book_index::{FileKind, FileName};
    for ties in [false, true] {
        let mut card = populated(ties);
        let name = leaf(&BOOK.digest);
        // The open `.IDX` is set aside even when oldest and over the whole quota.
        card.seed(&name, 17 * 1024 * 1024, -1);
        // A rebuild's `.BAK` keeps the old index's time, and an orphan `.WRK`
        // shares the 8.3 base. Both count and go like any other file.
        card.seed(&backup(&name), 100, -1);
        card.seed(FileName::new(&BOOK.digest, FileKind::Work).as_str(), 100, 40);
        assert_eq!(trim(&mut card, &Never), MissingProbe::Ready);
        assert_eq!(card.indexes(), expected(ties));
    }
}

#[test]
fn failed_idle_delete_is_denied_and_a_later_trim_finishes() {
    let mut card = opened(false);
    card.model.faults.push(Fault::new(Op::Unlink, 1, 1, Effect::Errno("EIO"), 0));
    assert_eq!(trim(&mut card, &Never), MissingProbe::Denied);
    assert_eq!(card.indexes().len(), 31);
    assert_eq!(with_store(&mut card, |s| read(s, &BOOK, complete)), Some(true));
    assert_eq!(trim(&mut card, &Never), MissingProbe::Ready);
    assert_eq!(card.indexes(), expected(false));
}

struct After { polls: Cell<usize>, stop: usize }
impl Authority for After {
    fn cancelled(&self) -> bool {
        let polls = self.polls.get(); self.polls.set(polls + 1); polls >= self.stop
    }
}
#[test]
fn cancellation_at_every_trim_poll_preserves_the_open_index_and_resumes() {
    let probe = After { polls: Cell::new(0), stop: usize::MAX };
    assert_eq!(trim(&mut opened(true), &probe), MissingProbe::Ready);
    let expected = expected(true);
    let mut interrupted_after_delete = false;
    for stop in 0..probe.polls.get() {
        let mut card = opened(true);
        assert_eq!(trim(&mut card, &After { polls: Cell::new(0), stop }), MissingProbe::Stopped);
        assert!(card.indexes().len() <= 31);
        interrupted_after_delete |= card.indexes().len() < 31;
        assert_eq!(with_store(&mut card, |s| read(s, &BOOK, complete)), Some(true));
        assert_eq!(trim(&mut card, &Never), MissingProbe::Ready);
        assert_eq!(card.indexes(), expected);
    }
    assert!(interrupted_after_delete);
}
