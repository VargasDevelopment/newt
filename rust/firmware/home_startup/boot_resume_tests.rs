// SPDX-License-Identifier: MIT
//! Synthetic authority/loan checks. These do not claim SD/native or app boot.
use super::*;
use newt_core::durable::{BlobRead, Codec, DurableWrite, StorageOpen};
use newt_core::sleep_intent::{BYTES, SleepResumeCodec};

struct Store {
    slots: [Option<[u8; BYTES]>; 2],
    open: StorageOpen,
    begins: u32,
    ends: u32,
}
impl Store {
    fn new() -> Self {
        Self { slots: [None; 2], open: StorageOpen::Opened, begins: 0, ends: 0 }
    }
    fn seed(&mut self, slot: usize, sequence: u32, value: SleepResumeIntent) {
        let mut bytes = [0; BYTES];
        assert!(SleepResumeCodec::encode(value, sequence, &mut bytes));
        self.slots[slot] = Some(bytes);
    }
}
impl Backend for Store {
    fn begin(&mut self, namespace: &str, readonly: bool) -> StorageOpen {
        assert_eq!(namespace, "newt_resume");
        assert!(readonly);
        self.begins += 1;
        self.open
    }
    fn end(&mut self) { self.ends += 1; }
    fn read_blob(&mut self, key: &str, output: &mut [u8]) -> BlobRead {
        let slot = match key { "slot0" => 0, "slot1" => 1, _ => panic!("unexpected key") };
        match self.slots[slot] {
            Some(bytes) => { output[..BYTES].copy_from_slice(&bytes); BlobRead::Present(BYTES) }
            None => BlobRead::Missing,
        }
    }
    fn put_bytes(&mut self, _: DurableWrite, _: &str, _: &[u8]) -> usize {
        panic!("boot reads never write")
    }
    fn readback_durable(&self) -> bool { true }
}

fn progress(character: u32) -> Progress {
    Progress {
        sequence: 7,
        book: 0x1122_3344_5566_7788,
        locator: newt_core::progress::Locator { spine: 3, character },
        active_milliseconds: 12_345,
        forward_pages: 11,
        qualified_sessions: 2,
        ..Progress::default()
    }
}
fn intent(saved: &Progress) -> SleepResumeIntent {
    SleepResumeIntent { book: saved.book, checkpoint: progress_checkpoint(saved), settings: 0x0a0f }
}
fn settings() -> ViewSettings {
    ViewSettings { orientation: Orientation::Portrait, slim_progress_bar: false, refresh_pages: 1 }
}

#[test]
fn one_authority_read_and_required_peer_resolution() {
    for newest in [progress(11), progress(99)] {
        let acknowledged = progress(11);
        let authority = intent(&acknowledged);
        let mut store = Store::new();
        store.seed(0, 4, authority);
        let mut target = Target::load(&mut store, true, settings(), &mut ReadingSaveNotice::default());
        assert_eq!(target.committed_book(), acknowledged.book);
        // A subsequent readable generation cannot replace the setup snapshot.
        store.seed(0, 5, SleepResumeIntent::default());
        let mut saved = newest;
        let mut calls = 0;
        let restored = target.load_committed(&mut store, true, true, &mut saved, true, true,
            |store, out, crc| {
                assert_eq!((store.begins, store.ends), (1, 1));
                assert_eq!(crc, authority.checkpoint);
                calls += 1;
                *out = acknowledged;
                Load::Loaded(())
            }).expect("acknowledged peer");
        assert_eq!((calls, store.begins, store.ends), (1, 1, 1));
        assert_eq!(saved, acknowledged);
        assert_eq!(restored.settings, ViewSettings {
            orientation: Orientation::LandscapeCcw, slim_progress_bar: true, refresh_pages: 10,
        });
        assert!(restored.anchor && restored.checked);
        assert_eq!(restored.issue, ResumeIssue::None);
    }
}

#[test]
fn warm_boot_does_no_authority_io_or_notice_reset() {
    let mut store = Store::new();
    let mut notice = ReadingSaveNotice::default();
    notice.set_resume(ResumeIssue::Unavailable);
    let before = notice;
    let mut target = Target::load(&mut store, false, settings(), &mut notice);
    let mut saved = progress(11);
    assert_eq!(target.committed_book(), 0);
    assert!(target.load_committed(&mut store, false, true, &mut saved, true, true,
        |_, _, _| panic!("warm checkpoint read")).is_none());
    assert_eq!((store.begins, store.ends), (0, 0));
    assert_eq!(notice, before);
    assert!(!target.checked);
}

#[test]
fn failed_torn_and_ambiguous_authority_never_resume_an_older_peer() {
    for kind in 0..6 {
        let mut store = Store::new();
        let original = progress(11);
        match kind {
            0 => store.open = StorageOpen::Missing,
            1 => store.open = StorageOpen::Failed,
            2 => {}
            3 => {
                store.seed(0, 1, intent(&original));
                store.seed(1, 2, SleepResumeIntent::default());
                store.slots[1].as_mut().unwrap()[28] ^= 1;
            }
            4 => {
                store.seed(0, 1, intent(&original));
                store.seed(1, 1, SleepResumeIntent::default());
            }
            5 => {
                store.seed(0, 1, intent(&original));
                store.slots[0].as_mut().unwrap()[4] = 2;
            }
            _ => unreachable!(),
        }
        let mut notice = ReadingSaveNotice::default();
        let mut target = Target::load(&mut store, true, settings(), &mut notice);
        let unavailable = !matches!(kind, 0 | 2);
        let expected = if unavailable { ResumeIssue::Unavailable } else { ResumeIssue::None };
        assert_eq!(notice.message(), expected.text());
        assert_eq!(target.committed_book(), 0);
        let mut saved = original;
        assert!(target.load_committed(&mut store, true, true, &mut saved, true, true,
            |_, _, _| panic!("unavailable authority checkpoint read")).is_none());
        assert_eq!(saved, original);
        assert_eq!(target.issue, expected);
        assert!(target.checked);
        assert_eq!(store.begins, 1);
        assert_eq!(store.ends, u32::from(kind >= 2));
    }
}

#[test]
fn partial_or_wrong_checkpoint_cannot_replace_the_caller() {
    for kind in 0..5 {
        let mut store = Store::new();
        let original = progress(99);
        store.seed(0, 1, intent(&progress(11)));
        let mut target = Target::load(&mut store, true, settings(), &mut ReadingSaveNotice::default());
        let mut saved = original;
        assert!(target.load_committed(&mut store, true, true, &mut saved, true, true,
            |_, out, _| {
                *out = progress(11);
                match kind {
                    0 => Load::Empty,
                    1 => Load::Failed,
                    2 => Load::Ambiguous,
                    3 => { out.book ^= 1; Load::Loaded(()) }
                    _ => { out.locator.character += 1; Load::Loaded(()) }
                }
            }).is_none());
        assert_eq!(saved, original);
        assert_eq!(target.settings, settings());
        assert!(!target.anchor);
    }
}

#[test]
fn admission_guards_keep_the_snapshot_and_fallback_read_is_bounded() {
    for (active, ready, verified) in [(false, true, true), (true, false, true), (true, true, false)] {
        let mut store = Store::new();
        let mut saved = progress(11);
        store.seed(0, 1, intent(&saved));
        let mut target = Target::load(&mut store, true, settings(), &mut ReadingSaveNotice::default());
        assert!(target.load_committed(&mut store, true, active, &mut saved, ready, verified,
            |_, _, _| panic!("unadmitted checkpoint read")).is_none());
        assert_eq!(target.checked, active);
        assert_eq!(store.begins, 1);
    }
    let mut store = Store::new();
    let mut saved = progress(11);
    store.seed(0, 1, intent(&saved));
    let mut target = Target::load(&mut store, false, settings(), &mut ReadingSaveNotice::default());
    assert!(target.load_committed(&mut store, true, true, &mut saved, true, true,
        |_, _, _| Load::Loaded(())).is_some());
    assert_eq!((store.begins, store.ends), (1, 1));
}
