// SPDX-License-Identifier: MIT
//! The one frozen cover_idle.h global, shared by Home, Library and Reading.
use newt_core::app::Screen;
use super::cover_resume::CoverResumeOwner;

pub const SETTLE_MS: u32 = 2000;
pub const FAILURE_BUDGET: u8 = 4;
pub const INTERRUPTION_LIMIT: u8 = 7;
const SHELF_BITS: u8 = 0x0f;
const REPAINT_PENDING: u8 = 0x80;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum CoverResult {
    #[default]
    Idle, Ready, Produced, Cancelled, Declined, Missing, Unsupported, Incomplete, Invalid, IoError, Busy,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Step { #[default] Empty, Read, Located, Decided }

/// Exactly the interrupted cover job's existing heap record. No producer
/// creates one merely for entering Home, Library or the reader.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CoverResume {
    pub book: u32,
    pub source_crc: u32,
    pub local: u32,
    pub compressed: u32,
    pub uncompressed: u32,
    pub crc32: u32,
    pub data: u32,
    pub step: Step,
    pub method: u8,
    pub verdict: CoverResult,
}

#[repr(C)]
pub struct CoverIdleState {
    pub resume: Option<CoverResumeOwner>,
    pub view: u32,
    pub quiet_from: u32,
    pub settled: u8,
    pub failures: u8,
    pub screen: u8,
    interruption_book: u8,
}
impl Default for CoverIdleState { fn default() -> Self { Self::new() } }
impl CoverIdleState {
    pub const fn new() -> Self {
        Self { resume: None, view: 0, quiet_from: 0, settled: 0, failures: 0,
            screen: 0xff, interruption_book: 0 }
    }
    pub fn note_input(&mut self, now: u32) { self.quiet_from = now; }
    pub fn interruptions(&self) -> u8 { self.interruption_book & SHELF_BITS }
    pub fn book(&self) -> u8 { self.interruption_book >> 4 }
    pub fn set_interruption(&mut self, book: u8, count: u8) {
        self.interruption_book = (book << 4) | (count & SHELF_BITS);
    }
    pub fn restart(&mut self) { self.resume = None; self.interruption_book = 0; }
    pub fn waiting(&self, now: u32) -> u8 {
        if now.wrapping_sub(self.quiet_from) < quiet_ms(self.interruptions()) { self.book() } else { 0 }
    }
    pub fn repaint_pending(&self) -> bool { self.settled & REPAINT_PENDING != 0 }
    pub fn defer_repaint(&mut self) { self.settled |= REPAINT_PENDING; }
    pub fn consume_repaint(&mut self) { self.settled &= !REPAINT_PENDING; }
    pub fn painted(&mut self, screen: Screen, includes_cover: bool, now: u32) {
        if screen == Screen::EpubReading { self.note_input(now); }
        if self.repaint_pending() && (self.screen != screen as u8 || includes_cover) {
            self.settled &= !REPAINT_PENDING;
        }
    }
    pub fn reading_ready(&mut self, screen: Screen, generation: u32, now: u32) -> bool {
        if screen != Screen::EpubReading || self.resume.is_some() { return false; }
        if self.screen != Screen::EpubReading as u8 {
            *self = Self::new();
            self.screen = Screen::EpubReading as u8;
            self.note_input(now);
        }
        if self.view != generation { self.view = generation; self.note_input(now); }
        now.wrapping_sub(self.quiet_from) >= SETTLE_MS
    }
}
pub const fn quiet_ms(interruptions: u8) -> u32 {
    if interruptions <= 2 { 0 } else {
        let doubles = interruptions - 3;
        15000 << if doubles < 3 { doubles } else { 3 }
    }
}
const _: () = assert!(core::mem::size_of::<CoverResume>() == 32);
#[cfg(target_pointer_width = "32")]
const _: () = assert!(core::mem::size_of::<CoverIdleState>() == 16);

impl super::Runtime {
    pub(crate) fn note_idle_input(&mut self, now: u32) { self.idle.note_input(now); }
    pub(crate) fn note_idle_paint(&mut self, screen: Screen, generation: u32, includes_cover: bool, now: u32) {
        self.idle.painted(screen, includes_cover, now);
        // ReadingCacheIdle observes generation at its next service call.
        let _ = generation;
    }
}
