// SPDX-License-Identifier: MIT
//! Selected-book durable authority (MIT): holds identity, digest and state flags.
//! Source/session/title/RTC/boot operations remain explicit caller loans.
#![forbid(unsafe_code)]
#[path = "selection_authority/card.rs"]
mod card;
#[path = "selection_authority/commit.rs"]
mod commit;
#[path = "selection_authority/progress.rs"]
mod progress;
pub use card::CardObservation;
use crate::{save_notice::{ReadingSaveNotice, SelectionIssue, SourceIssue}, sd_resume::SdResumeRecord};

/// Explicit bitmask flags; never relies on native struct or bitfield layout.
pub mod flag {
    pub const ACTIVE: u16 = 1;
    pub const SAVED_PRESENT: u16 = 1 << 1;
    pub const UNAVAILABLE: u16 = 1 << 2;
    pub const SAVE_FAILED: u16 = 1 << 3;
    pub const OVERVIEW_ATTEMPTED: u16 = 1 << 4;
    pub const TITLE_DEFERRED: u16 = 1 << 5;
    pub const CARD_MISSING: u16 = 1 << 6;
    pub const SAMPLE_CHOSEN: u16 = 1 << 7;
    pub const UNCONFIRMED: u16 = 1 << 8;
    pub const PAGE_CACHE_MASK: u16 = 3 << 9;
    pub const PAGE_CACHE_VISIT: u16 = 1 << 11;
    pub const COMMIT_OWED: u16 = 1 << 12;
    pub const ACCEPTED_OPEN_OWED: u16 = 1 << 13;
    pub const CHANGED_OPEN: u16 = 1 << 14;
    pub const DERIVED_OWED: u16 = 1 << 15;
}

pub struct SelectionAuthority {
    identity: SdResumeRecord,
    digest: [u8; 32],
    flags: u16,
}

impl Default for SelectionAuthority {
    fn default() -> Self { Self::new() }
}
impl SelectionAuthority {
    pub const fn new() -> Self {
        Self { identity: SdResumeRecord { magic: 0, version: 0, digest: [0; 32],
            filename: [0; crate::library_name::FILENAME_BYTES], checksum: 0 }, digest: [0; 32], flags: 0 }
    }
    pub fn identity(&self) -> &SdResumeRecord { &self.identity }
    pub fn digest(&self) -> &[u8; 32] { &self.digest }
    pub fn flags(&self) -> u16 { self.flags }
    /// The remaining physical methods must mutate THIS word, using flag's
    /// mapping. Never maintain a parallel controller or shadow word in Root.
    pub fn flags_mut(&mut self) -> &mut u16 { &mut self.flags }
    pub fn active(&self) -> bool { self.has(flag::ACTIVE) }
    pub fn saved_progress_present(&self) -> bool { self.has(flag::SAVED_PRESENT) }
    pub fn selection_blocked(&self) -> bool { !self.active() && self.has(flag::UNAVAILABLE) }
    pub fn storage_diagnostic(&self) -> SelectionIssue {
        if self.has(flag::SAVE_FAILED) { SelectionIssue::Unverified }
        else if self.selection_blocked() { SelectionIssue::Unavailable }
        else { SelectionIssue::None }
    }
    pub fn source_diagnostic(&self) -> SourceIssue {
        if self.has(flag::CARD_MISSING) { SourceIssue::NoCard } else { SourceIssue::Changed }
    }
    pub fn publish_selection_notice(&self, notice: &mut ReadingSaveNotice) {
        notice.set_selection(self.storage_diagnostic());
    }
    /// Caller supplies the real session query, including its absent-owner
    /// rule. No session/hash object is owned or assumed present by Core.
    pub fn selection_verified(&self, source_verified: &mut dyn FnMut() -> bool) -> bool {
        source_verified() && !self.has(flag::SAVE_FAILED | flag::UNAVAILABLE | flag::UNCONFIRMED)
    }
    pub fn book_id(&self) -> u64 {
        u64::from_le_bytes([self.digest[0], self.digest[1], self.digest[2], self.digest[3],
            self.digest[4], self.digest[5], self.digest[6], self.digest[7]])
    }
    /// Physical callback resets the optional session, closes library/source
    /// and nulls source_ in that order. Flags are cleared first, as closeSource.
    pub fn close_source(&mut self, close: &mut dyn FnMut()) {
        self.flags &= !(flag::PAGE_CACHE_MASK | flag::PAGE_CACHE_VISIT);
        close();
    }
    /// select's prefix AFTER closeSource, BEFORE library.open/import/hash.
    pub fn begin_selection(&mut self) {
        self.flags &= !(flag::COMMIT_OWED | flag::ACCEPTED_OPEN_OWED | flag::CHANGED_OPEN | flag::DERIVED_OWED);
    }
    /// select's infallible publication tail. Root has actually prepared/hashed
    /// the candidate already; publish_source only assigns its real source loan.
    /// Root releases the actual catalog after true, closes source after false.
    pub fn publish_candidate(&mut self, filename: &core::ffi::CStr, digest: [u8; 32],
        publish_source: &mut dyn FnMut(), clear_title: &mut dyn FnMut()) -> bool {
        let Some(identity) = SdResumeRecord::new(filename, digest) else { return false; };
        self.identity = identity;
        self.digest = digest;
        publish_source();
        self.set(flag::ACTIVE, true);
        self.set(flag::OVERVIEW_ATTEMPTED, false);
        if !self.has(flag::TITLE_DEFERRED) { clear_title(); }
        true
    }
    pub fn restore_identity(&mut self, identity: &SdResumeRecord,
        close: &mut dyn FnMut(), clear_title: &mut dyn FnMut()) -> bool {
        if !identity.valid() { return false; }
        self.digest = identity.digest; // readSdResumeRecord publishes before closeSource.
        self.close_source(close);
        self.identity = *identity;
        self.flags &= !(flag::COMMIT_OWED | flag::ACCEPTED_OPEN_OWED | flag::CHANGED_OPEN | flag::DERIVED_OWED);
        if !self.has(flag::TITLE_DEFERRED) { clear_title(); }
        self.set(flag::ACTIVE, true);
        self.set(flag::OVERVIEW_ATTEMPTED, false);
        true
    }
    /// Frozen reset only; never a boot/reset-reason or source-selection policy.
    /// Card/sample/titleDeferred survive, as in library_reader.h::reset.
    pub fn reset(&mut self, close: &mut dyn FnMut(), clear_title: &mut dyn FnMut()) {
        self.close_source(close);
        self.flags &= !(flag::ACTIVE | flag::SAVED_PRESENT | flag::UNAVAILABLE | flag::SAVE_FAILED
            | flag::UNCONFIRMED | flag::COMMIT_OWED | flag::ACCEPTED_OPEN_OWED
            | flag::CHANGED_OPEN | flag::DERIVED_OWED);
        self.identity.magic = 0;
        self.identity.version = 0;
        self.identity.digest.fill(0);
        self.identity.filename.fill(0);
        self.identity.checksum = 0;
        self.digest.fill(0);
        if !self.has(flag::TITLE_DEFERRED) { clear_title(); }
        self.set(flag::OVERVIEW_ATTEMPTED, false);
    }
    pub fn retain(&self, record: &mut SdResumeRecord) -> bool {
        if !self.active() { record.magic = 0; return false; }
        *record = self.identity;
        true
    }
    pub fn defer_accepted_open(&mut self) { if self.active() { self.set(flag::ACCEPTED_OPEN_OWED, true); } }
    pub fn checkpoint_verified(&mut self) { self.set(flag::ACCEPTED_OPEN_OWED, false); }
    pub fn take_changed_open(&mut self) -> bool {
        let changed = self.has(flag::CHANGED_OPEN);
        self.set(flag::CHANGED_OPEN, false);
        changed
    }
    pub fn defer_checkpoint_metadata(&mut self) { if self.active() { self.set(flag::DERIVED_OWED, true); } }
    pub fn retire_checkpoint_metadata(&mut self) { self.set(flag::DERIVED_OWED, false); }
    fn has(&self, bits: u16) -> bool { self.flags & bits != 0 }
    fn set(&mut self, bits: u16, value: bool) {
        self.flags = (self.flags & !bits) | if value { bits } else { 0 };
    }
}
