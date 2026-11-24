// SPDX-License-Identifier: MIT
//! Tracker/restore/UI policy regressions. Chapter callbacks are scripted here;
//! actual parser, metadata-bank and whole-frame parity use the C++ differential.
extern crate std;
use super::*;
use crate::{ReaderPanel, ReaderUiIntent, ReaderUiState, ReaderUiWork};
use newt_core::{chapter_layout::types::{Page, Sink}, package_parsers::{ManifestItem, PackageResult},
    preferences::ViewSettings, progress::{Locator, Progress}, zip_container::{Entry, hash_path},
    zip_window::arena::Arena};
use newt_phased_reader::bounded::LaidOut;
use std::vec;

#[derive(Default)]
struct Script { calls: usize, fail: Option<Status>, deny_mapping: bool }
impl Chapters for Script {
    fn layout(&mut self, _book: &Metadata<'_, '_>, _spine: u32,
        _settings: ViewSettings, sink: &mut dyn Sink,
    ) -> Result<LaidOut, Status> {
        self.calls += 1;
        if let Some(status) = self.fail.take() { return Err(status); }
        for index in 0..4 {
            assert!(sink.page(Page { bytes: &[], runs: &[], images: &[], links: &[],
                index, char_start: index * 10 }));
        }
        Ok(LaidOut { pages: 4, characters: 40 })
    }
    fn validate_anchor(&mut self, _book: &Metadata<'_, '_>, _spine: u32, _fragment: &[u8]) -> Status {
        Status::Ok
    }
    fn with_mapping<R>(&mut self, count: usize,
        work: impl FnOnce(&mut [u32]) -> R,
    ) -> Result<R, Status> {
        if self.deny_mapping { return Err(Status::OutOfMemory); }
        Ok(work(&mut vec![0; count]))
    }
}

fn with_sample(saved: Locator, work: impl for<'op, 'book, 'storage>
    FnOnce(SampleOperation<'op, 'book, 'storage>, &mut Script, &mut Frame),
) {
    let mut bank = [0; 256];
    let mut arena = Arena::new(&mut bank);
    let media = arena.copy_nul_ref(Some(b"application/xhtml+xml")).unwrap();
    let hrefs = [arena.copy_nul_ref(Some(b"one.xhtml")).unwrap(),
        arena.copy_nul_ref(Some(b"two.xhtml")).unwrap()];
    let manifest = hrefs.map(|href| ManifestItem { href: Some(href), media_type: Some(media),
        ..ManifestItem::default() });
    let entries = core::array::from_fn::<_, 2, _>(|index| Entry {
        name: Some(hrefs[index]), name_hash: hash_path(if index == 0 { b"one.xhtml" } else { b"two.xhtml" }),
        ..Entry::default() });
    let package = PackageResult { manifest_count: 2, spine_count: 2, ..PackageResult::default() };
    let metadata = Metadata { arena: &arena, package: &package,
        manifest: &manifest, spine: &[0, 1], entries: &entries, toc: &[] };
    let mut reader = Reader::new(ViewSettings::default());
    let mut script = Script::default();
    assert_eq!(reader.index(&metadata, &mut script), Status::Ok);
    let mut tracker = ReadingTracker::new(Progress { book: 1, locator: saved, ..Progress::default() });
    let mut owner = ReaderOwnerState::default();
    let mut frame = vec![0x5a; newt_core::frame::FRAME_BYTES].into_boxed_slice();
    work(SampleOperation { reader: &mut reader, metadata: &metadata,
        tracker: &mut tracker, owner: &mut owner }, &mut script,
        frame.as_mut().try_into().unwrap());
}

#[test]
fn exact_page_rejects_interior_anchor_while_validated_reflows_it() {
    for (policy, expected) in [(RestorePolicy::ExactPage, RestoreFailure::InvalidCheckpoint),
        (RestorePolicy::Validated, RestoreFailure::None),
        (RestorePolicy::ColdCheckpoint, RestoreFailure::None)] {
        with_sample(Locator { spine: 0, character: 15 }, |mut sample, chapters, frame| {
            assert_eq!(sample.restore_initial(chapters, policy, 100, frame), expected);
            assert_eq!(sample.owner.opened(), expected == RestoreFailure::None);
            assert_eq!(sample.tracker.reading(), expected == RestoreFailure::None);
            if expected == RestoreFailure::None {
                assert_eq!(sample.tracker.snapshot(0).locator.character, 10);
            }
        });
    }
}

#[test]
fn only_initial_fallback_may_replace_a_missing_coordinate() {
    for (policy, fallback) in [(RestorePolicy::Validated, false),
        (RestorePolicy::ColdCheckpoint, false), (RestorePolicy::AllowInitialFallback, true)] {
        with_sample(Locator { spine: 99, character: 0 }, |mut sample, chapters, frame| {
            let restored = sample.restore_initial(chapters, policy, 0, frame);
            if fallback {
                assert_eq!(restored, RestoreFailure::None);
                assert_eq!(sample.tracker.snapshot(0).locator, Locator::default());
            } else {
                assert_eq!(restored, RestoreFailure::Unavailable);
                assert!(!sample.owner.has_progress());
            }
        });
    }
}

#[test]
fn turn_boundaries_do_no_work_and_cross_spine_moves_keep_tracker_rules() {
    with_sample(Locator::default(), |mut sample, chapters, frame| {
        assert_eq!(sample.restore_initial(chapters, RestorePolicy::Validated, 0, frame), RestoreFailure::None);
        let calls = chapters.calls;
        assert!(!sample.turn(chapters, Action::Previous, 2500, frame));
        assert_eq!(chapters.calls, calls);
        assert_eq!(sample.tracker.snapshot(0).active_milliseconds, 0);
        assert!(sample.jump_to_spine(chapters, 1, 2500, frame));
        sample.resume(3000);
        assert!(sample.turn(chapters, Action::Previous, 6000, frame));
        assert_eq!(sample.tracker.snapshot(0).locator, Locator { spine: 0, character: 30 });
        assert_eq!(sample.tracker.snapshot(0).forward_pages, 0);
        assert!(sample.turn(chapters, Action::Next, 9000, frame));
        assert_eq!(sample.tracker.snapshot(0).locator, Locator { spine: 1, character: 0 });
        assert_eq!(sample.tracker.snapshot(0).forward_pages, 1);
    });
}

#[test]
fn failed_paint_cannot_resurrect_a_paused_reader() {
    with_sample(Locator::default(), |mut sample, chapters, frame| {
        assert_eq!(sample.restore_initial(chapters, RestorePolicy::Validated, 100, frame), RestoreFailure::None);
        chapters.fail = Some(Status::IoError);
        assert!(!sample.paint(chapters, 1600, frame));
        assert_eq!(sample.owner.status(), Status::IoError);
        assert!(sample.owner.has_progress());
        assert!(!sample.tracker.reading());
        let calls = chapters.calls;
        assert!(!sample.paint(chapters, 2000, frame));
        sample.resume(2000);
        assert_eq!(chapters.calls, calls);
        assert!(!sample.tracker.reading());
        assert_eq!(sample.tracker.snapshot(0).active_milliseconds, 1500);
    });
}

#[test]
fn chapter_relocation_is_paused_and_never_earns_turn_credit() {
    with_sample(Locator::default(), |mut sample, chapters, frame| {
        assert_eq!(sample.restore_initial(chapters, RestorePolicy::Validated, 0, frame), RestoreFailure::None);
        assert_eq!(sample.jump_to_row(chapters, 1, 2500, frame), ChapterJumpOutcome::Moved);
        assert_eq!(sample.tracker.snapshot(0).locator, Locator { spine: 1, character: 0 });
        assert_eq!(sample.tracker.snapshot(0).forward_pages, 0);
        assert!(!sample.tracker.reading());
    });
}

#[test]
fn optional_menu_refusal_uses_shared_ui_without_invalidating_the_page() {
    with_sample(Locator::default(), |mut sample, chapters, frame| {
        assert_eq!(sample.restore_initial(chapters, RestorePolicy::Validated, 0, frame), RestoreFailure::None);
        let mut ui = ReaderUiState::default();
        assert_eq!(sample.handle_ui(chapters, &mut ui, Action::Activate, 2000).work,
            ReaderUiWork::Intent(ReaderUiIntent::Repaint));
        assert_eq!(ui.panel, ReaderPanel::Menu);
        assert!(!sample.tracker.reading());
        ui.selected = 1;
        chapters.deny_mapping = true;
        let result = sample.handle_ui(chapters, &mut ui, Action::Activate, 2500);
        assert_eq!(ui.panel, ReaderPanel::Chapters);
        assert!(!result.menu.unwrap().valid);
        assert!(sample.owner.opened());
        assert_eq!(sample.owner.status(), Status::Ok);
    });
}
