// SPDX-License-Identifier: MIT
//! Opt-in boot observations. No clock, storage or display work lives here.
#![forbid(unsafe_code)]
use core::cell::Cell;

pub const WORDS: usize = 24;
pub const SCHEMA: u32 = 1;
const FROZEN: u32 = 1;
const PAGE: u32 = 2;

#[repr(usize)]
#[derive(Clone, Copy)]
pub enum Stage {
    Entry, Serial, Probe, DisplayReady, Sd, Application, HomeRestore, ReaderRestore,
    InputReady, PictureCandidate, SourceReady, PageReady, PictureReady,
    DisplayStart, DisplayReturn, PageComplete,
}

/// One UI-task owner. Snapshot reads never re-arm or alter the observations.
#[repr(C)]
pub struct Timeline {
    stamps: [Cell<u32>; 16],
    metadata: [Cell<u32>; 8],
}
impl Timeline {
    pub const fn new() -> Self {
        Self { stamps: [const { Cell::new(0) }; 16], metadata: [const { Cell::new(0) }; 8] }
    }
    pub fn begin(&self, now: u32, cpu_mhz: u32) {
        for word in self.stamps.iter().chain(self.metadata.iter()) { word.set(0); }
        self.metadata[0].set(SCHEMA);
        self.metadata[5].set(cpu_mhz);
        self.stamp(Stage::Entry, now);
    }
    fn frozen(&self) -> bool { self.metadata[2].get() & FROZEN != 0 }
    pub fn stamp(&self, stage: Stage, now: u32) {
        let index = stage as usize;
        let bit = 1 << index;
        if self.frozen() || self.metadata[1].get() & bit != 0 { return; }
        self.stamps[index].set(now);
        self.metadata[1].set(self.metadata[1].get() | bit);
    }
    pub fn boot(&self, reset: u32, cause: u32, panel: u32) {
        if self.frozen() { return; }
        self.metadata[3].set(reset);
        self.metadata[4].set(cause);
        self.metadata[7].set(panel & 255);
    }
    pub fn restored(&self, requested: u8, restored: bool) {
        if self.frozen() { return; }
        self.metadata[6].set((self.metadata[6].get() & 255)
            | u32::from(requested) << 8 | u32::from(restored) << 16);
    }
    /// Only the real page paths arm this; Home, Opening and error paints don't.
    pub fn page(&self, kind: u8) {
        if self.frozen() { return; }
        self.metadata[6].set((self.metadata[6].get() & !255) | u32::from(kind));
        self.metadata[2].set(self.metadata[2].get() | PAGE);
    }
    pub fn display_start(&self, now: u32, mode: u32) {
        if self.frozen() || self.metadata[2].get() & PAGE == 0 { return; }
        self.stamp(Stage::DisplayStart, now);
        self.metadata[7].set((self.metadata[7].get() & 255) | mode << 8);
        let count = ((self.metadata[2].get() >> 8) & 255).saturating_add(1).min(255);
        self.metadata[2].set((self.metadata[2].get() & 255) | count << 8);
    }
    pub fn display_return(&self, now: u32) {
        if self.frozen() || self.metadata[2].get() & PAGE == 0 { return; }
        // An inverse cleanup can precede the target wave: keep its final return.
        self.stamps[Stage::DisplayReturn as usize].set(now);
        self.metadata[1].set(self.metadata[1].get() | 1 << Stage::DisplayReturn as usize);
    }
    pub fn complete(&self, now: u32) {
        if self.frozen() || self.metadata[2].get() & PAGE == 0 { return; }
        self.stamp(Stage::PageComplete, now);
        self.metadata[2].set((self.metadata[2].get() & !PAGE) | FROZEN);
    }
    pub fn snapshot(&self) -> Option<[u32; WORDS]> {
        if !self.frozen() { return None; }
        let mut words = [0; WORDS];
        for (word, value) in words.iter_mut().zip(self.stamps.iter().chain(self.metadata.iter())) {
            *word = value.get();
        }
        Some(words)
    }
}
impl Default for Timeline { fn default() -> Self { Self::new() } }
const _: () = assert!(core::mem::size_of::<Timeline>() == 96);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_a_real_page_freezes_and_later_work_cannot_replace_it() {
        let t = Timeline::new(); t.begin(0, 160);
        t.stamp(Stage::Serial, 10); t.stamp(Stage::Serial, 20);
        t.display_start(30, 0); t.display_return(40); t.complete(50);
        assert_eq!(t.snapshot(), None);
        t.boot(1, 0, 2); t.restored(4, true); t.page(1);
        t.display_start(100, 2); t.display_return(120);
        t.display_start(130, 0); t.display_return(160); t.complete(170);
        let saved = t.snapshot().unwrap();
        assert_eq!((saved[0], saved[1], saved[13], saved[14], saved[15]), (0, 10, 100, 160, 170));
        assert_eq!(saved[17] & 1, 1); // Zero is a valid entry timestamp.
        assert_eq!(saved[18] >> 8, 2);
        assert_eq!((saved[19], saved[20], saved[21], saved[22], saved[23]), (1, 0, 160, 1 | 4 << 8 | 1 << 16, 2));
        t.stamp(Stage::Sd, 200); t.boot(3, 7, 1); t.restored(0, false);
        t.page(2); t.display_start(210, 2); t.display_return(220); t.complete(230);
        assert_eq!(t.snapshot(), Some(saved));
        assert_eq!(t.snapshot(), Some(saved));
    }
    #[test]
    fn a_new_boot_discards_the_previous_awake_only_trace() {
        let t = Timeline::new(); t.begin(u32::MAX - 10, 160);
        t.page(2); t.display_start(u32::MAX - 5, 0); t.display_return(2); t.complete(3);
        assert_eq!(t.snapshot().unwrap()[15].wrapping_sub(t.snapshot().unwrap()[0]), 14);
        t.begin(5, 80); assert_eq!(t.snapshot(), None);
        t.page(2); t.complete(10);
        let saved = t.snapshot().unwrap();
        assert_eq!((saved[0], saved[15], saved[21]), (5, 10, 80));
        assert_eq!(saved[17], 1 | 1 << 15);
    }
}
