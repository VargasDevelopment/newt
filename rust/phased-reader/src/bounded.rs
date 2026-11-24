// SPDX-License-Identifier: MIT
//! Frozen src/reader.cpp's bounded stored-EPUB engine. Metadata and the three
//! banks belong to the sample owner; every parser/source/frame loan is short.
use crate::{PageInfo, zip_container::Status};
use newt_core::{chapter_layout::types::Sink, preferences::ViewSettings};
use newt_phased_sink::{Frame, Target};
use newt_reader_navigation::{BookView, Navigation, Section, NO_TOC};
pub use newt_reader_navigation::metadata::Metadata;

mod sink;
pub mod layout;
pub mod book;
pub mod source;
pub mod initialization;

pub const BOOK_BYTES: usize = 8 * 1024;
pub const LAYOUT_BYTES: usize = 48 * 1024;
pub const PARSE_BYTES: usize = 12 * 1024;
pub const MAX_SOURCE_BYTES: u64 = 32 * 1024;
pub const MAX_ENTRIES: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LaidOut {
    pub pages: u32,
    pub characters: u32,
}

/// The concrete bank owner lends the real ChapterLayout and exact-anchor
/// validator. Both operations consume this chapter before returning; neither
/// may retain the metadata, sink, source or parser capability.
pub trait Chapters {
    fn layout(&mut self, book: &Metadata<'_, '_>, spine: u32,
        settings: ViewSettings, sink: &mut dyn Sink) -> Result<LaidOut, Status>;
    fn validate_anchor(&mut self, book: &Metadata<'_, '_>, spine: u32,
        fragment: &[u8]) -> Status;
    fn with_mapping<R>(&mut self, count: usize,
        operation: impl FnOnce(&mut [u32]) -> R) -> Result<R, Status>;
}

/// Only the original Reader's scalar state and two owned navigation projections.
/// The already-open metadata stays in the sample's book bank, not in this value.
pub struct Reader {
    chapter_characters: [u32; MAX_ENTRIES],
    book_characters: u64,
    settings: ViewSettings,
    info: PageInfo,
    navigation: [Navigation; 2],
    active: usize,
    opened: bool,
}
impl Reader {
    pub fn new(settings: ViewSettings) -> Self {
        Self { chapter_characters: [0; MAX_ENTRIES], book_characters: 0,
            settings, info: PageInfo::default(), navigation: core::array::from_fn(|_| Navigation::default()),
            active: 0, opened: false }
    }
    /// Called before catalog/package opening, including a failed attempt.
    pub fn close(&mut self) {
        self.opened = false;
        self.info = PageInfo::default();
        for navigation in &mut self.navigation { navigation.reset(); }
        self.active = 0;
        self.chapter_characters.fill(0);
        self.book_characters = 0;
    }
    pub fn opened(&self) -> bool { self.opened }
    pub fn info(&self) -> PageInfo { self.info }
    pub fn settings(&self) -> ViewSettings { self.settings }
    pub fn current_section(&self) -> Section<'_> {
        self.navigation[self.active].section(self.info.char_start)
    }
    pub fn section_at(&self, spine: u32, character: u32) -> Section<'_> {
        if self.info.spine == spine { self.navigation[self.active].section(character) }
        else { Section::default() }
    }
    pub fn chapter_menu(&self) -> crate::ChapterMenuView<'_> {
        crate::ChapterMenuView::from_navigation(&self.navigation[self.active ^ 1])
    }
    pub fn progress_inputs(&self) -> newt_reader_navigation::ProgressInputs {
        self.navigation[self.active].progress_inputs()
    }

    /// Complete open after the owner's actual ZIP preflight and SDK package/TOC
    /// parser. Count each spine with the same layout used for page publication.
    pub fn index(&mut self, book: &Metadata<'_, '_>, chapters: &mut impl Chapters) -> Status {
        self.close();
        let count = book.package.spine_count;
        if count == 0 { return Status::NotEpub; }
        if count > MAX_ENTRIES { return Status::Unsupported; }
        let Some(manifest) = book.manifest.get(..book.package.manifest_count) else {
            return Status::Unsupported;
        };
        for item in manifest {
            let media = item.media_type.and_then(|value| book.arena.c_string(value));
            if item.href.is_none() || !matches!(media,
                Some(b"application/xhtml+xml" | b"application/x-dtbncx+xml")) {
                return Status::Unsupported;
            }
        }
        for spine in 0..count {
            match chapters.layout(book, spine as u32, self.settings, &mut sink::Counting) {
                Ok(layout) => {
                    self.chapter_characters[spine] = layout.characters;
                    self.book_characters += u64::from(layout.characters);
                }
                Err(status) => return status,
            }
        }
        self.opened = true;
        Status::Ok
    }

    pub fn show_page(&mut self, book: &Metadata<'_, '_>, chapters: &mut impl Chapters,
        spine: u32, page: u32, frame: Option<&mut Frame>,
    ) -> Status {
        self.layout(book, chapters, spine, page, Target::Page, frame)
    }
    pub fn restore(&mut self, book: &Metadata<'_, '_>, chapters: &mut impl Chapters,
        spine: u32, character: u32, frame: Option<&mut Frame>,
    ) -> Status {
        self.layout(book, chapters, spine, character, Target::Anchor, frame)
    }
    pub fn show_chapter(&mut self, book: &Metadata<'_, '_>, chapters: &mut impl Chapters,
        spine: u32, toc: u16, frame: Option<&mut Frame>,
    ) -> Status {
        if toc == NO_TOC { return self.restore(book, chapters, spine, 0, frame); }
        let status = self.layout(book, chapters, spine, u32::from(toc), Target::TocResolve, None);
        if status != Status::Ok { return status; }
        let navigation = &mut self.navigation[self.active ^ 1];
        let page = navigation.staged_resolved_page();
        let target = navigation.target_by_toc(toc);
        let status = chapters.validate_anchor(book, spine, target.fragment);
        if status != Status::Ok { return status; }
        navigation.stage_required_toc(toc);
        self.layout(book, chapters, spine, page, Target::TocPage, frame)
    }
    /// Allocate mapping only after reuse fails, at the original parse-bank
    /// boundary. No mapping or metadata reference survives the operation.
    pub fn prepare_chapter_menu(&mut self, book: &Metadata<'_, '_>, focus: u32,
        chapters: &mut impl Chapters,
    ) -> bool {
        if !self.opened { return false; }
        let (left, right) = self.navigation.split_at_mut(1);
        let (current, staged) = if self.active == 0 { (&left[0], &mut right[0]) }
            else { (&right[0], &mut left[0]) };
        let section = current.section(self.info.char_start);
        if staged.reuse_menu(section, focus) { return true; }
        staged.invalidate_menu();
        chapters.with_mapping(book.toc_count(), |mapping|
            staged.bind_menu(book, section, focus, mapping)).unwrap_or(false)
    }

    fn layout(&mut self, book: &Metadata<'_, '_>, chapters: &mut impl Chapters,
        spine: u32, target: u32, mode: Target, frame: Option<&mut Frame>,
    ) -> Status {
        let navigation = &mut self.navigation[self.active ^ 1];
        let required = match mode {
            Target::TocResolve => target as u16,
            Target::TocPage => navigation.staged_required_toc(),
            _ => NO_TOC,
        };
        navigation.invalidate_menu();
        let resolving = mode == Target::TocResolve;
        if !self.opened || (!resolving && frame.is_none()) { return Status::NotFound; }
        let Some(item) = book.spine_item(spine as usize) else { return Status::NotFound; };
        if item.href.and_then(|href| book.member(href)).is_none() { return Status::NotFound; }
        navigation.stage_required_toc(required);
        if !navigation.bind(book, spine) { return Status::OutOfMemory; }
        navigation.begin_anchors();
        let mut sink = sink::Selecting::new(target, mode,
            self.settings.orientation.is_landscape(), frame, navigation);
        let result = chapters.layout(book, spine, self.settings, &mut sink);
        let (found, page, character) = (sink.found, sink.selected_page, sink.selected_char);
        drop(sink);
        let layout = match result { Ok(layout) => layout, Err(status) => return status };
        if !found { return Status::NotFound; }
        if resolving {
            let chapter = navigation.target_by_toc(required);
            if !chapter.target_known || !chapter.anchor_known { return Status::NotFound; }
            navigation.stage_resolved_page(page);
            return Status::Ok;
        }
        if mode == Target::TocPage {
            let chapter = navigation.target_by_toc(required);
            if !chapter.target_known || !chapter.anchor_known { return Status::NotFound; }
        }
        let Some(expected) = self.chapter_characters.get(spine as usize) else { return Status::Unsupported; };
        if layout.characters != *expected { return Status::Unsupported; }
        let mut book_start = u64::from(character);
        for &count in &self.chapter_characters[..spine as usize] { book_start += u64::from(count); }
        let known = self.book_characters != 0;
        let percent = if known { (book_start * 100 / self.book_characters) as u8 } else { 0 };
        self.info = PageInfo { spine, page, page_count: layout.pages, char_start: character,
            total_chars: layout.characters, book_char_start: book_start,
            book_total_chars: self.book_characters, overall_known: known,
            overall_percent: percent, overall_estimated: false };
        self.active ^= 1;
        Status::Ok
    }
}
