//! Copy to target-probe/src/identity_probe.rs; instantiate from exported probe.
use super::Memory;
use newt_core::durable::{Admission, Commit, Load};
use newt_core::sd_resume::SdResumeRecord;
use newt_core::selected_book::SelectedBookStore;
use newt_core::sleep_intent::{SleepResumeIntent, SleepResumeStore, progress_checkpoint};

pub(super) fn probe(input: u32) -> u32 {
    let mut digest = [0; 32];
    digest[..4].copy_from_slice(&input.wrapping_add(1).to_le_bytes());
    digest[31] = 1;
    let Some(identity) = SdResumeRecord::new(c"Probe.epub", digest) else {
        return 0;
    };
    let mut selected_memory = Memory {
        slots: [[0; 144]; 2],
        present: [false; 2],
        fault: input >> 8,
    };
    let mut selected = SelectedBookStore::new(&mut selected_memory);
    let saved = selected.save(identity) == Commit::Saved;
    let loaded = matches!(selected.load(), Load::Loaded(_));
    let matched = matches!(selected.load_book(identity.book_id()), Load::Loaded(_));
    let protected = selected.save_protected(identity, identity.book_id()) == Commit::Saved;
    let mut quiet = || input & 1 != 0;
    let mut admission = Admission {
        quiet: Some(&mut quiet),
        entered: false,
        deferred: false,
    };
    let admitted = selected.save_admitted(identity, &mut admission) == Commit::Saved;
    let mut sleep_memory = Memory {
        slots: [[0; 32]; 2],
        present: [false; 2],
        fault: input >> 8,
    };
    let progress = newt_core::progress::Progress {
        sequence: input,
        book: identity.book_id(),
        ..Default::default()
    };
    let intent = SleepResumeIntent {
        book: identity.book_id(),
        checkpoint: progress_checkpoint(&progress),
        settings: 0x504 | (input & 11),
    };
    let mut sleep = SleepResumeStore::new(&mut sleep_memory);
    let sleep_saved = sleep.save(intent) == Commit::Saved;
    let sleep_loaded = matches!(sleep.load(), Load::Loaded(_));
    let home = sleep.save(SleepResumeIntent::default()) == Commit::Saved;
    u32::from(saved)
        | u32::from(loaded) << 1
        | u32::from(matched) << 2
        | u32::from(protected) << 3
        | u32::from(admitted) << 4
        | u32::from(sleep_saved) << 5
        | u32::from(sleep_loaded) << 6
        | u32::from(home) << 7
}
