// SPDX-License-Identifier: MIT
//! Opt-in boot observations. No clock, storage or display work lives here.
#![forbid(unsafe_code)]
use core::sync::atomic::{AtomicU32, Ordering};

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

/// UI observations use safe atomic storage, without a mutable global lease.
/// Snapshot reads never re-arm or alter the observations.
#[repr(C)]
pub struct Timeline {
    stamps: [AtomicU32; 16],
    metadata: [AtomicU32; 8],
}
impl Timeline {
    pub const fn new() -> Self {
        Self { stamps: [const { AtomicU32::new(0) }; 16], metadata: [const { AtomicU32::new(0) }; 8] }
    }
    pub fn begin(&self, now: u32, cpu_mhz: u32) {
        // A new boot supplies fresh zeroed storage; no caller can re-arm it.
        if self.metadata[0].load(Ordering::Acquire) != 0 { return; }
        self.metadata[5].store(cpu_mhz, Ordering::Relaxed);
        self.stamp(Stage::Entry, now);
        self.metadata[0].store(SCHEMA, Ordering::Release);
    }
    fn frozen(&self) -> bool { self.metadata[2].load(Ordering::Acquire) & FROZEN != 0 }
    pub fn stamp(&self, stage: Stage, now: u32) {
        let index = stage as usize;
        let bit = 1 << index;
        if self.frozen() || self.metadata[1].load(Ordering::Relaxed) & bit != 0 { return; }
        self.stamps[index].store(now, Ordering::Relaxed);
        self.metadata[1].store(self.metadata[1].load(Ordering::Relaxed) | bit, Ordering::Relaxed);
    }
    pub fn boot(&self, reset: u32, cause: u32, panel: u32) {
        if self.frozen() { return; }
        self.metadata[3].store(reset, Ordering::Relaxed);
        self.metadata[4].store(cause, Ordering::Relaxed);
        self.metadata[7].store(panel & 255, Ordering::Relaxed);
    }
    pub fn restored(&self, requested: u8, restored: bool) {
        if self.frozen() { return; }
        self.metadata[6].store((self.metadata[6].load(Ordering::Relaxed) & 255)
            | u32::from(requested) << 8 | u32::from(restored) << 16, Ordering::Relaxed);
    }
    /// Only the real page paths arm this; Home, Opening and error paints don't.
    pub fn page(&self, kind: u8) {
        if self.frozen() { return; }
        self.metadata[6].store((self.metadata[6].load(Ordering::Relaxed) & !255) | u32::from(kind), Ordering::Relaxed);
        self.metadata[2].store(self.metadata[2].load(Ordering::Relaxed) | PAGE, Ordering::Relaxed);
    }
    pub fn display_start(&self, now: u32, mode: u32) {
        if self.frozen() || self.metadata[2].load(Ordering::Relaxed) & PAGE == 0 { return; }
        self.stamp(Stage::DisplayStart, now);
        self.metadata[7].store((self.metadata[7].load(Ordering::Relaxed) & 255) | mode << 8, Ordering::Relaxed);
        let count = ((self.metadata[2].load(Ordering::Relaxed) >> 8) & 255).saturating_add(1).min(255);
        self.metadata[2].store((self.metadata[2].load(Ordering::Relaxed) & 255) | count << 8, Ordering::Relaxed);
    }
    pub fn display_return(&self, now: u32) {
        if self.frozen() || self.metadata[2].load(Ordering::Relaxed) & PAGE == 0 { return; }
        // Record the native refresh return; inverse RAM cleanup is outside this call.
        self.stamps[Stage::DisplayReturn as usize].store(now, Ordering::Relaxed);
        self.metadata[1].store(self.metadata[1].load(Ordering::Relaxed) | 1 << Stage::DisplayReturn as usize, Ordering::Relaxed);
    }
    pub fn complete(&self, now: u32) {
        if self.frozen() || self.metadata[2].load(Ordering::Relaxed) & PAGE == 0 { return; }
        self.stamp(Stage::PageComplete, now);
        self.metadata[2].store((self.metadata[2].load(Ordering::Relaxed) & !PAGE) | FROZEN, Ordering::Release);
    }
    pub fn snapshot(&self) -> Option<[u32; WORDS]> {
        if !self.frozen() { return None; }
        let mut words = [0; WORDS];
        for (word, value) in words.iter_mut().zip(self.stamps.iter().chain(self.metadata.iter())) {
            *word = value.load(Ordering::Relaxed);
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
        t.begin(999, 80);
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
        let cold = Timeline::new();
        cold.begin(5, 80); assert_eq!(cold.snapshot(), None);
        cold.page(2); cold.complete(10);
        let saved = cold.snapshot().unwrap();
        assert_eq!((saved[0], saved[15], saved[21]), (5, 10, 80));
        assert_eq!(saved[17], 1 | 1 << 15);
    }
}
