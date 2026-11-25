// SPDX-License-Identifier: MIT
//! Reading save notice value and message policy. MIT. Shared Core owner; the firmware NVS subset re-exports this exact value policy.
#![forbid(unsafe_code)]
use core::ffi::CStr;
pub use crate::update_card::slot_installer::selection::SaveOutcome;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum NvsIssue {
    None = 0,
    Version = 1,
    Recovery = 2,
    Unavailable = 3,
}

impl NvsIssue {
    pub fn text(self) -> Option<&'static CStr> {
        match self {
            Self::None => None,
            Self::Version => Some(c"Storage version unsupported. Existing data kept."),
            Self::Recovery => Some(c"Storage needs recovery. Existing data kept."),
            Self::Unavailable => Some(c"Storage unavailable. Existing data kept."),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum SelectionIssue { None, Unverified, Unavailable }
impl SelectionIssue {
    pub fn text(self) -> Option<&'static CStr> { Self::text_raw(self as u8) }
    pub fn text_raw(raw: u8) -> Option<&'static CStr> {
        match raw {
            1 => Some(c"Book selection not verified. Reading position is kept."),
            2 => Some(c"Saved book selection unavailable. Existing data kept."),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum SourceIssue { None, NoCard, Changed }
impl SourceIssue {
    pub fn text(self) -> Option<&'static CStr> { Self::text_raw(self as u8) }
    pub fn text_raw(raw: u8) -> Option<&'static CStr> {
        match raw {
            1 => Some(c"No SD card. Insert it, then press Read."),
            2 => Some(c"Saved SD book unavailable or changed. Position kept."),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ReaderIssue { None, Session, Memory, InvalidCheckpoint, InvalidText, Io, NotEpub, Encrypted, Truncated, Unsupported, Parse, Unavailable, SourceUnavailable, MetadataTooLarge, PathTooLong }
impl ReaderIssue {
    /// Fixed book-capacity refusals must not suggest a runtime heap failure.
    pub fn for_open_failure(status: crate::zip_container::Status) -> Self {
        use crate::zip_container::Status;
        match status {
            Status::OutOfMemory => Self::Memory,
            Status::IoError | Status::Stale => Self::Io,
            Status::NotZip | Status::NotEpub => Self::NotEpub,
            Status::Encrypted => Self::Encrypted,
            Status::Truncated => Self::Truncated,
            // A damaged required member (container, OPF) keeps the refusal
            // whole-book admission gave it before members were checked on open.
            Status::Unsupported | Status::Damaged => Self::Unsupported,
            Status::MetadataTooLarge => Self::MetadataTooLarge,
            Status::PathTooLong => Self::PathTooLong,
            Status::ParseError => Self::Parse,
            Status::NotFound | Status::Ok => Self::Unavailable,
        }
    }
    pub fn text(self) -> Option<&'static CStr> { Self::text_raw(self as u8) }
    pub fn text_raw(raw: u8) -> Option<&'static CStr> {
        match raw {
            1 => Some(c"Reader session unavailable. Position kept."),
            2 => Some(c"Reader memory is unavailable. Position kept."),
            3 => Some(c"Saved position could not be restored. It is kept."),
            4 => Some(c"This book contains invalid text."),
            5 => Some(c"SD book could not be read. Position kept."),
            6 => Some(c"This file is not a readable EPUB."),
            7 => Some(c"This EPUB is encrypted."),
            8 => Some(c"This EPUB is incomplete."),
            9 => Some(c"This EPUB format or size is not supported."),
            10 => Some(c"This EPUB could not be parsed."),
            11 => Some(c"Book content or saved position unavailable. It is kept."),
            12 => Some(c"SD book unavailable or changed. Position kept."),
            13 => Some(c"Book metadata is too large. Position kept."),
            14 => Some(c"An EPUB path is too long. Position kept."),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ResumeIssue { None, Unavailable }
impl ResumeIssue {
    pub fn text(self) -> Option<&'static CStr> { Self::text_raw(self as u8) }
    pub fn text_raw(raw: u8) -> Option<&'static CStr> {
        match raw {
            1 => Some(c"Saved wake destination unavailable. Existing data kept."),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum StatsCondition { Unknown, Seeded, Unseeded, Unsupported, Unavailable }

impl NvsIssue {
    pub fn text_raw(raw: u8) -> Option<&'static CStr> {
        match raw { 1 => Self::Version.text(), 2 => Self::Recovery.text(), 3 => Self::Unavailable.text(), _ => None }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadingCommitResult { pub progress: SaveOutcome, pub destination: SaveOutcome }
impl Default for ReadingCommitResult {
    fn default() -> Self { Self { progress: SaveOutcome::Unchanged, destination: SaveOutcome::Unchanged } }
}
pub const READING_SAVE_UNCONFIRMED: &CStr = c"Save unconfirmed. Restart may use old or new page.";
pub const PAGE_SAVE_HOME_CONFIRMED: &CStr = c"Page save unconfirmed. Restart returns Home.";
pub const HOME_SAVE_UNCONFIRMED: &CStr = c"Home save unconfirmed. Restart may reopen book.";
pub const PAGE_HOME_SAVE_UNCONFIRMED: &CStr = c"Page and Home save unconfirmed. Restart may reopen book.";
pub const LAST_SAVE_UNCONFIRMED: &CStr = c"Last save unconfirmed";
pub const PAGE_SAVE_ERROR: &CStr = c"PG SAVE ERR";

/// Copied projection fields, borrowed static text; no DeviceInfo/UI dependency.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceNotice { pub storage: Option<&'static CStr>, pub page_save_unconfirmed: bool }
impl DeviceNotice {
    pub fn footer(self) -> Option<&'static CStr> {
        if self.page_save_unconfirmed { Some(PAGE_SAVE_ERROR) }
        else if self.storage.is_some() { Some(c"STORAGE") } else { None }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HomeNotice { pub device: DeviceNotice, pub flags: u8 }

pub const HOME_SAVE_WARNING: u8 = 16;
const SAVED: u32 = 1;
const PAGE: u32 = 2;
const DESTINATION: u32 = 4;
const HOME: u32 = 8;
const IMPORT_SETTLED: u32 = 1 << 6;
const STREAK_SETTLED: u32 = 1 << 7;
const IMPORT_COMPLETE: u32 = 1 << 8;
const FOLD_OWED: u32 = 1 << 23;

/// The exact frozen owner: three align-one bytes, never a retained u32.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReadingSaveNotice { state: [u8; 3] }
const _: () = assert!(core::mem::size_of::<ReadingSaveNotice>() == 3);
const _: () = assert!(core::mem::align_of::<ReadingSaveNotice>() == 1);
impl ReadingSaveNotice {
    pub const fn from_bytes(state: [u8; 3]) -> Self { Self { state } }
    pub const fn to_bytes(self) -> [u8; 3] { self.state }
    /// Snapshot import mirrors bits(value): only low24 bits are stored.
    pub fn from_bits(bits: u32) -> Self { let mut value = Self::default(); value.write_bits(bits); value }
    pub const fn bits(self) -> u32 { self.state[0] as u32 | ((self.state[1] as u32) << 8) | ((self.state[2] as u32) << 16) }
    fn write_bits(&mut self, bits: u32) { for i in 0..3 { self.state[i] = (bits >> (8 * i)) as u8; } }
    fn set(&mut self, flag: u32, value: bool) { self.write_bits((self.bits() & !flag) | if value { flag } else { 0 }); }
    fn field(self, shift: u32, mask: u32) -> u8 { ((self.bits() >> shift) & mask) as u8 }
    fn set_field(&mut self, shift: u32, mask: u32, value: u8) { self.write_bits((self.bits() & !(mask << shift)) | ((u32::from(value) & mask) << shift)); }
    pub fn checkpoint_saved(self) -> bool { self.bits() & SAVED != 0 }
    pub fn set_checkpoint_saved(&mut self, saved: bool) { self.set(SAVED, saved); }
    pub fn page_unconfirmed(self) -> bool { self.bits() & PAGE != 0 }
    pub fn destination_unconfirmed(self) -> bool { self.bits() & DESTINATION != 0 }
    pub fn unconfirmed(self) -> bool { self.bits() & (PAGE | DESTINATION) != 0 }
    pub fn home_hint(self) -> Option<&'static CStr> { if self.unconfirmed() { Some(LAST_SAVE_UNCONFIRMED) } else { None } }
    pub fn checkpoint(&mut self, result: ReadingCommitResult) -> bool {
        self.checkpoint_to(true, result)
    }
    /// Back and sleep publish the actual destination independently of page acknowledgement.
    pub fn checkpoint_to(&mut self, reading: bool, result: ReadingCommitResult) -> bool {
        if result.progress != SaveOutcome::Unchanged {
            self.set(PAGE, result.progress == SaveOutcome::Unconfirmed);
            self.set_checkpoint_saved(result.progress == SaveOutcome::Verified);
            if result.progress == SaveOutcome::Verified { self.set_progress_unavailable(false); }
        }
        self.destination(reading, result.destination);
        result.progress == SaveOutcome::Verified && result.destination != SaveOutcome::Unconfirmed
    }
    pub fn destination(&mut self, reading: bool, result: SaveOutcome) {
        if result == SaveOutcome::Unchanged { return; }
        self.set(DESTINATION, result == SaveOutcome::Unconfirmed); self.set(HOME, !reading);
        if result == SaveOutcome::Verified { self.set_resume(ResumeIssue::None); }
    }
    pub fn set_nvs(&mut self, issue: NvsIssue) { self.set_nvs_raw(issue as u8); }
    pub fn set_nvs_raw(&mut self, raw: u8) { self.set_field(4, 3, raw); }
    pub fn set_selection(&mut self, issue: SelectionIssue) { self.set_selection_raw(issue as u8); }
    pub fn set_selection_raw(&mut self, raw: u8) { self.set_field(9, 3, raw); }
    /// Raw getters preserve unnamed frozen enum values without unsafe casts.
    pub fn source(self) -> u8 { self.field(11, 3) }
    pub fn set_source(&mut self, issue: SourceIssue) { self.set_source_raw(issue as u8); }
    pub fn set_source_raw(&mut self, raw: u8) { self.set_field(11, 3, raw); }
    pub fn reader(self) -> u8 { self.field(13, 15) }
    pub fn set_reader(&mut self, issue: ReaderIssue) { self.set_reader_raw(issue as u8); }
    pub fn set_reader_raw(&mut self, raw: u8) { self.set_field(13, 15, raw); }
    pub fn set_resume(&mut self, issue: ResumeIssue) { self.set_resume_raw(issue as u8); }
    pub fn set_resume_raw(&mut self, raw: u8) { self.set_field(17, 1, raw); }
    pub fn set_progress_unavailable(&mut self, unavailable: bool) { self.set_field(18, 1, u8::from(unavailable)); }
    pub fn set_reader_preference_unavailable(&mut self, unavailable: bool) { self.set_field(19, 1, u8::from(unavailable)); }
    pub fn stats(self) -> u8 { self.field(20, 7) }
    pub fn set_stats(&mut self, condition: StatsCondition) { self.set_stats_raw(condition as u8); }
    pub fn set_stats_raw(&mut self, raw: u8) { self.set_field(20, 7, raw); }
    pub fn import_settled(self) -> bool { self.bits() & IMPORT_SETTLED != 0 }
    pub fn set_import_settled(&mut self, value: bool) { self.set(IMPORT_SETTLED, value); }
    pub fn import_complete(self) -> bool { self.bits() & IMPORT_COMPLETE != 0 }
    pub fn set_import_complete(&mut self, value: bool) { self.set(IMPORT_COMPLETE, value); }
    pub fn streak_settled(self) -> bool { self.bits() & STREAK_SETTLED != 0 }
    pub fn set_streak_settled(&mut self, value: bool) { self.set(STREAK_SETTLED, value); }
    pub fn fold_owed(self) -> bool { self.bits() & FOLD_OWED != 0 }
    pub fn set_fold_owed(&mut self, value: bool) { self.set(FOLD_OWED, value); }
    pub fn exit_saved(self) -> SaveOutcome { if self.bits() & (PAGE | DESTINATION | FOLD_OWED) != 0 { SaveOutcome::Unconfirmed } else { SaveOutcome::Verified } }
    pub fn message(self) -> Option<&'static CStr> {
        if self.page_unconfirmed() {
            if self.bits() & HOME != 0 { return Some(if self.destination_unconfirmed() { PAGE_HOME_SAVE_UNCONFIRMED } else { PAGE_SAVE_HOME_CONFIRMED }); }
            return Some(READING_SAVE_UNCONFIRMED);
        }
        if self.destination_unconfirmed() { return Some(if self.bits() & HOME != 0 { HOME_SAVE_UNCONFIRMED } else { READING_SAVE_UNCONFIRMED }); }
        if let Some(value) = NvsIssue::text_raw(self.field(4, 3)) { return Some(value); }
        if let Some(value) = SourceIssue::text_raw(self.field(11, 3)) { return Some(value); }
        if let Some(value) = SelectionIssue::text_raw(self.field(9, 3)) { return Some(value); }
        if let Some(value) = ReaderIssue::text_raw(self.field(13, 15)) { return Some(value); }
        if let Some(value) = ResumeIssue::text_raw(self.field(17, 1)) { return Some(value); }
        if self.field(18, 1) != 0 { return Some(c"Saved reading progress unavailable. Existing data kept."); }
        if self.field(19, 1) != 0 { return Some(c"Reader preferences unavailable. Existing selection kept."); }
        None
    }
    pub fn publish(self) -> DeviceNotice { DeviceNotice { storage: self.message(), page_save_unconfirmed: self.page_unconfirmed() } }
    pub fn project(self, flags: u8) -> HomeNotice { HomeNotice { device: self.publish(), flags: (flags & !HOME_SAVE_WARNING) | if self.unconfirmed() { HOME_SAVE_WARNING } else { 0 } } }
    pub fn retained(self) -> u32 { (self.bits() & (PAGE | DESTINATION)) << 1 }
    pub fn clear_recovery(&mut self) { self.write_bits(self.bits() & !15u32); }
    pub fn restore(&mut self, recovery: u32, home: bool, live_unacknowledged: bool) {
        self.write_bits((self.bits() & !14u32) | ((recovery >> 1) & (PAGE | DESTINATION)) | if home { HOME } else { 0 });
        if live_unacknowledged { self.set(PAGE, true); }
    }
}
