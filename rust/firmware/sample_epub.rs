// SPDX-License-Identifier: MIT
//! CC0 embedded sample EPUB fixture and MIT saved-position projection.
//! The three-page Reading scene is separate.
use newt_core::progress::Progress;
use newt_reader_navigation::{Identity, resolve_identity};

#[derive(Debug)]
pub struct OpenFailure {
    pub status: newt_core::zip_container::Status,
    pub restore: newt_reader_ui::RestoreFailure,
}

pub const BYTES: &[u8; 6931] = include_bytes!("sample.epub");
pub const IDENTITY: u64 = 0x5e4c9375286ce1cb;
pub const CHARACTERS: [u32; 2] = [2261, 2215];
const CHAPTERS: [Identity<'static>; 2] = [
    Identity { label: b"A Light for the Walk Home", projection_spine: 1, char_start: 0,
        title_key: 1, depth: 0, inherited: false, target_known: true, anchor_known: true },
    Identity { label: b"Morning at the Pond", projection_spine: 0, char_start: 0,
        title_key: 0, depth: 0, inherited: false, target_known: true, anchor_known: true },
];

pub fn valid_progress(saved: &Progress) -> bool {
    saved.book == IDENTITY && CHARACTERS.get(saved.locator.spine as usize)
        .is_some_and(|&count| saved.locator.character < count)
}

/// The returned title borrows Home's actual buffer. Invalid/absent checkpoints
/// do not parse a book, allocate a reader or fabricate a known percentage.
pub fn overview<'a>(saved: &Progress, title: &'a mut [u8]) -> Option<(&'a [u8], u8)> {
    if !valid_progress(saved) { return None; }
    let mut total = 0;
    let mut position = u64::from(saved.locator.character);
    for (index, &count) in CHARACTERS.iter().enumerate() {
        total += u64::from(count);
        if index < saved.locator.spine as usize { position += u64::from(count); }
    }
    if total == 0 || !resolve_identity(&CHAPTERS, saved.locator.spine,
        saved.locator.character, title) { return None; }
    let end = title.iter().position(|&byte| byte == 0)?;
    Some((&title[..end], (position * 100 / total) as u8))
}

/// The actual sample open boundary: strict validation precedes the one storage
/// allocation; every failed load/restore drops it before returning to the app.
pub fn open<B: newt_core::xml_session::ParserBackend + ?Sized>(saved: Progress,
    settings: newt_core::preferences::ViewSettings, policy: newt_reader_ui::RestorePolicy,
    now: u32, frame: &mut newt_core::frame::Frame, backend: &mut B,
    breaker: &mut dyn newt_core::chapter_layout::types::LineBreak,
) -> Result<newt_c3_board::sample_memory::SampleMemory, OpenFailure> {
    use newt_core::zip_container::Status;
    use newt_reader_ui::{ReaderOwnerState, RestoreFailure, SampleOperation};
    if !policy.allows_initial_fallback() && !valid_progress(&saved) {
        return Err(OpenFailure { status: Status::NotFound, restore: RestoreFailure::InvalidCheckpoint });
    }
    let Some(mut memory) = newt_c3_board::sample_memory::SampleMemory::try_new(saved, settings, IDENTITY) else {
        return Err(OpenFailure { status: Status::OutOfMemory, restore: RestoreFailure::Unavailable });
    };
    let status = memory.load(BYTES, backend, breaker);
    if status != Status::Ok {
        return Err(OpenFailure { status, restore: RestoreFailure::Unavailable });
    }
    let mut owner = ReaderOwnerState::default();
    let restored = memory.with(|loan| {
        let mut chapters = newt_phased_reader::phase_storage::bounded_chapters::Work::new(
            BYTES, loan.layout, loan.parse, backend, breaker);
        SampleOperation { reader: loan.reader, metadata: loan.metadata,
            tracker: loan.tracker, owner: &mut owner }
            .restore_initial(&mut chapters, policy, now, frame)
    });
    match restored {
        Ok(RestoreFailure::None) => Ok(memory),
        Ok(restore) => Err(OpenFailure { status: owner.status(), restore }),
        Err(status) => Err(OpenFailure { status, restore: RestoreFailure::Unavailable }),
    }
}

/// Frozen firmware/home_progress.h uses the real newt_reader NVS store through
/// the same router. A failed read retains saved; only confirmed absence resets
/// it. Another book's valid record cannot authorize the sample or its progress.
pub fn load_home_progress<B: newt_core::durable::Backend>(
    store: &mut newt_core::progress_store::ProgressStore<'_, B>,
    saved: &mut Progress, home: &mut newt_device_ui::HomeData<'_>,
) -> bool {
    use newt_core::durable::Load;
    let loaded = store.load();
    match loaded {
        Load::Loaded(value) => *saved = value,
        Load::Empty => *saved = Progress::default(),
        Load::Failed | Load::Ambiguous => {}
    }
    let percent = if matches!(loaded, Load::Loaded(_)) {
        overview(saved, &mut home.chapter_title).map(|(_, percent)| percent)
    } else { None };
    let known = percent.is_some();
    home.percent = percent.map_or(-1, i32::from);
    if !known {
        home.clear_chapter();
    }
    home.set_estimated(false);
    home.progress_unavailable = loaded != Load::Empty && !known;
    home.reading_available = loaded == Load::Empty || known;
    home.reading_available
}
