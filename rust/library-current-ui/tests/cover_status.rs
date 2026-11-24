//! Focused UI tests for Books cover-making marks, no-cover reasons, notice precedence, and paint_focus.
use newt_core::frame::Frame;
use newt_library_current::{Added, Catalog, LibraryOmission, LibraryStatus, list::Files};
use newt_library_current_ui::{
    COVER_HEIGHT, COVER_ROW_BYTES, COVER_WIDTH, CoverSlotStatus, LibraryCoverSource,
    LibraryUiState, NoCoverReason, SystemBar, paint,
};

struct NoopFiles;
impl Files for NoopFiles {
    fn open(&mut self, _: &[u8], _: bool) -> i32 { -1 }
    fn read(&mut self, _: i32, _: u32, _: &mut [u8]) -> bool { false }
    fn write(&mut self, _: i32, _: u32, _: &[u8]) -> bool { false }
    fn size(&mut self, _: i32) -> i64 { -1 }
    fn close(&mut self, _: i32) -> bool { false }
    fn make_folder(&mut self, _: &[u8]) -> bool { false }
    fn remove(&mut self, _: &[u8]) -> bool { false }
}

struct StubCovers {
    art: [bool; 4],
    statuses: [CoverSlotStatus; 4],
    last_slot: usize,
    painted_making: Option<bool>,
    inspect_calls: usize,
}
impl StubCovers {
    fn new(statuses: [CoverSlotStatus; 4]) -> Self {
        Self { art: [false; 4], statuses, last_slot: 0, painted_making: None, inspect_calls: 0 }
    }
    fn slot_for(&self, filename: Option<&[u8]>) -> usize {
        let Some(name) = filename else { return 0 };
        match name.first().copied() {
            Some(b'A') => 0,
            Some(b'B') => 1,
            Some(b'C') => 2,
            Some(b'D') => 3,
            _ => 0,
        }
    }
}
impl LibraryCoverSource for StubCovers {
    fn begin(&mut self, _: &mut dyn Files, filename: Option<&[u8]>) -> bool {
        self.last_slot = self.slot_for(filename);
        self.art[self.last_slot]
    }
    fn read_row(&mut self, _: &mut dyn Files, row: &mut [u8; COVER_ROW_BYTES]) -> bool {
        row.fill(0x55);
        true
    }
    fn end(&mut self, _: &mut dyn Files) {}
    fn slot_status(&self, slot: usize) -> CoverSlotStatus {
        self.statuses[slot.min(3)]
    }
    fn inspect(&mut self, _: &mut dyn Files, filename: Option<&[u8]>, slot: usize) -> CoverSlotStatus {
        self.inspect_calls += 1;
        let idx = self.slot_for(filename).min(3);
        if self.art[idx] { CoverSlotStatus::NoCover } else { self.statuses[slot.min(3)] }
    }
    fn shelf_painted(&mut self, any_making: bool) {
        self.painted_making = Some(any_making);
    }
}

fn catalog_of(names: &[&[u8]], with_notice: bool) -> Catalog {
    let mut catalog = Catalog::default();
    for &name in names {
        assert_eq!(catalog.add(name, 1024, None, false), Added::Listed);
    }
    if with_notice {
        catalog.omit(LibraryOmission::Folder);
    }
    assert_eq!(catalog.finish(), LibraryStatus::Ok);
    catalog
}

fn crop(frame: &Frame, x0: usize, y0: usize, w: usize, h: usize) -> Vec<bool> {
    let mut out = Vec::with_capacity(w * h);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            let at = y * 528 + x;
            out.push(frame[at / 8] & (0x80 >> (at % 8)) != 0);
        }
    }
    out
}

fn note_strip(frame: &Frame) -> Vec<bool> {
    crop(frame, 50, 675, 428, 30)
}

fn card_box(frame: &Frame, slot: usize) -> Vec<bool> {
    let x = 74 + (slot % 2) * 236;
    let y = 138 + (slot / 2) * 232;
    crop(frame, x, y, COVER_WIDTH as usize, COVER_HEIGHT as usize)
}

#[test]
fn making_cover_draws_mark_and_reports_pending() {
    let names: &[&[u8]] = &[b"A Book.epub\0", b"B Book.epub\0"];
    let mut files = NoopFiles;
    let bar = SystemBar::default();

    let mut making_frame = [0u8; newt_core::frame::FRAME_BYTES];
    let mut making_cat = catalog_of(names, false);
    let mut making_state = LibraryUiState::default();
    let mut making_covers = StubCovers::new([CoverSlotStatus::Making; 4]);
    assert!(paint(Some(&mut making_cat), &mut files, &mut making_state, Some(&mut making_frame), &bar, Some(&mut making_covers)));
    assert_eq!(making_covers.painted_making, Some(true));
    assert!(!note_strip(&making_frame).into_iter().any(|b| b), "Making cover keeps note strip blank");

    let mut plain_frame = [0u8; newt_core::frame::FRAME_BYTES];
    let mut plain_cat = catalog_of(names, false);
    let mut plain_state = LibraryUiState::default();
    let mut plain_covers = StubCovers::new([CoverSlotStatus::NoCover; 4]);
    assert!(paint(Some(&mut plain_cat), &mut files, &mut plain_state, Some(&mut plain_frame), &bar, Some(&mut plain_covers)));
    assert_eq!(plain_covers.painted_making, Some(false));
    assert!(!note_strip(&plain_frame).into_iter().any(|b| b), "Non-retryable NoCover keeps note strip blank");

    assert_ne!(card_box(&making_frame, 0), card_box(&plain_frame, 0), "Making cover card must differ from No cover");
}

#[test]
fn reasons_and_deferred_draw_distinct_notes_and_keep_no_cover_card() {
    let long_name: &[u8] = b"A very long title about the quiet orchard and the letters we carried home.epub\0";
    let names: &[&[u8]] = &[long_name];
    let mut files = NoopFiles;
    let bar = SystemBar::default();

    let mut plain_frame = [0u8; newt_core::frame::FRAME_BYTES];
    let mut cat = catalog_of(names, false);
    let mut state = LibraryUiState::default();
    let mut plain = StubCovers::new([CoverSlotStatus::NoCover; 4]);
    paint(Some(&mut cat), &mut files, &mut state, Some(&mut plain_frame), &bar, Some(&mut plain));
    let expected_card = card_box(&plain_frame, 0);

    let statuses = [
        CoverSlotStatus::Reason(NoCoverReason::Missing),
        CoverSlotStatus::Reason(NoCoverReason::Unsupported),
        CoverSlotStatus::Reason(NoCoverReason::Incomplete),
        CoverSlotStatus::Reason(NoCoverReason::Invalid),
        CoverSlotStatus::Deferred,
    ];
    let mut notes = Vec::new();
    for status in statuses {
        let mut frame = [0u8; newt_core::frame::FRAME_BYTES];
        let mut cat = catalog_of(names, false);
        let mut state = LibraryUiState::default();
        let mut covers = StubCovers::new([status; 4]);
        paint(Some(&mut cat), &mut files, &mut state, Some(&mut frame), &bar, Some(&mut covers));
        assert_eq!(covers.painted_making, Some(false));
        assert_eq!(card_box(&frame, 0), expected_card, "{status:?} must show No cover on card");
        let strip = note_strip(&frame);
        assert!(strip.iter().any(|&b| b), "{status:?} must draw its bottom note");
        // Verify a 3 px white gap exists between contracted title (bottom 673) and note top (~679).
        assert!(!crop(&frame, 50, 673, 428, 3).into_iter().any(|b| b), "{status:?} title must not collide with note");
        notes.push(strip);
    }
    for i in 0..notes.len() {
        for j in (i + 1)..notes.len() {
            assert_ne!(notes[i], notes[j], "statuses {:?} and {:?} must have distinct note text", statuses[i], statuses[j]);
        }
    }
}

#[test]
fn system_notice_overrides_cover_reason_and_focus_updates_note() {
    let names: &[&[u8]] = &[b"A Art.epub\0", b"B Missing.epub\0", b"C Later.epub\0", b"D Blank.epub\0"];
    let mut files = NoopFiles;
    let bar = SystemBar::default();

    let mut cat = catalog_of(names, false);
    let mut state = LibraryUiState::default();
    let mut frame = [0u8; newt_core::frame::FRAME_BYTES];
    let mut covers = StubCovers::new([
        CoverSlotStatus::NoCover,
        CoverSlotStatus::Reason(NoCoverReason::Missing),
        CoverSlotStatus::Deferred,
        CoverSlotStatus::NoCover,
    ]);
    covers.art[0] = true;

    // Initial full paint on slot 0 (art): blank note.
    assert!(paint(Some(&mut cat), &mut files, &mut state, Some(&mut frame), &bar, Some(&mut covers)));
    assert!(!note_strip(&frame).into_iter().any(|b| b));

    // Focus moves via fast paint_focus path across slots 1, 2, 3.
    let mut seen = Vec::new();
    for next in [1usize, 2, 3] {
        state.selected = next;
        state.focus_pending = true;
        assert!(!paint(Some(&mut cat), &mut files, &mut state, Some(&mut frame), &bar, Some(&mut covers)));
        seen.push(note_strip(&frame));
    }
    assert!(seen[0].iter().any(|&b| b), "slot 1 Missing must draw note on paint_focus");
    assert!(seen[1].iter().any(|&b| b), "slot 2 Deferred must draw note on paint_focus");
    assert_ne!(seen[0], seen[1], "Missing and Deferred notes must differ");
    assert!(!seen[2].iter().any(|&b| b), "slot 3 non-retryable NoCover must clear note on paint_focus");
    assert_eq!(covers.inspect_calls, 3);

    // Catalog::notice() overrides cover reasons on both full paint and paint_focus, skipping inspect.
    let mut notice_cat = catalog_of(names, true);
    let mut notice_state = LibraryUiState::default();
    let mut notice_frame = [0u8; newt_core::frame::FRAME_BYTES];
    assert!(paint(Some(&mut notice_cat), &mut files, &mut notice_state, Some(&mut notice_frame), &bar, Some(&mut covers)));
    let before = covers.inspect_calls;
    notice_state.selected = 1;
    notice_state.focus_pending = true;
    assert!(!paint(Some(&mut notice_cat), &mut files, &mut notice_state, Some(&mut notice_frame), &bar, Some(&mut covers)));
    assert_eq!(covers.inspect_calls, before, "Catalog::notice must bypass cover inspect on paint_focus");
    assert_ne!(note_strip(&notice_frame), seen[0], "Catalog::notice must override Missing cover note");
    assert!(note_strip(&notice_frame).iter().any(|&b| b));

    // state.message also takes precedence over cover reasons.
    let mut msg_frame = [0u8; newt_core::frame::FRAME_BYTES];
    let mut msg_state = LibraryUiState { selected: 1, message: Some(b"System warning"), ..LibraryUiState::default() };
    paint(Some(&mut cat), &mut files, &mut msg_state, Some(&mut msg_frame), &bar, Some(&mut covers));
    assert_ne!(note_strip(&msg_frame), seen[0], "system message must override Missing cover note");
    assert!(note_strip(&msg_frame).iter().any(|&b| b));
}
