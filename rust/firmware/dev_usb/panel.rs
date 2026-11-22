// SPDX-License-Identifier: MIT
//! The frozen 31-entry panel history; bytes are encoded explicitly on the wire.
pub const MODES: &[&str] = &[
    "fast", "full", "window-fast", "gray-base-half", "gray-overlay", "gray-direct",
    "gray-direct-region", "binary-island-fast", "binary-regions-fast", "reset", "gray-base",
];
pub const REASONS: &[&str] = &[
    "working-first-open", "working-first-open-inverse", "working-opening", "working-opening-inverse",
    "working-saving", "working-saving-inverse", "working-applying", "working-applying-inverse",
    "working-loading", "working-loading-inverse", "working-unknown", "leave-overlay-inverse",
    "reader", "ui", "zone-focus", "zone-cleanup", "home-cleanup", "home-inverse-rebase",
    "home-overlay-base", "home", "home-battery", "home-focus", "home-frosted", "home-fallback",
    "home-stage-focus", "home-stage", "sleep-scene", "sleep-fallback", "boot-or-wake", "resume-picture",
];
#[derive(Clone, Copy, Default)]
#[repr(C)]
struct Entry { started: u32, duration: u16, mode: u8, reason_outcome: u8 }
#[derive(Clone, Copy)]
pub enum PanelOutcome { Accepted, Rejected, Fallback, Pending }
#[derive(Clone, Copy)]
pub struct PanelToken { sequence: u32, index: u8 }
#[repr(C)]
#[derive(Default)]
pub struct PanelRing {
    entries: [Entry; 31],
    pub next: u32,
    head: u8,
    pub count: u8,
}
impl PanelRing {
    pub fn begin(&mut self, now: u32, mode: &str, reason: &str) -> PanelToken {
        let id = |name: &str, table: &[&str]| table.iter().position(|&s| s == name).map_or(63, |v| v as u8);
        let token = PanelToken { sequence: self.next, index: self.head };
        self.next = self.next.wrapping_add(1);
        self.entries[usize::from(self.head)] = Entry {
            started: now, duration: 0, mode: id(mode, MODES), reason_outcome: id(reason, REASONS) | 0xc0,
        };
        self.head = (self.head + 1) % 31;
        self.count = (self.count + 1).min(31);
        token
    }
    pub fn finish(&mut self, token: PanelToken, now: u32, outcome: PanelOutcome) {
        if self.next.wrapping_sub(token.sequence) > u32::from(self.count) { return; }
        let entry = &mut self.entries[usize::from(token.index)];
        let elapsed = now.wrapping_sub(entry.started);
        entry.duration = elapsed.min(65_535) as u16;
        if elapsed > 65_535 { entry.mode |= 0x80; }
        entry.reason_outcome = (entry.reason_outcome & 63) | ((outcome as u8) << 6);
    }
    pub(super) fn byte(&self, offset: usize) -> u8 {
        let index = (usize::from(self.head) + 31 - usize::from(self.count) + offset / 8) % 31;
        let entry = self.entries[index];
        match offset % 8 {
            0..=3 => (entry.started >> (8 * (offset % 8))) as u8,
            4..=5 => (entry.duration >> (8 * (offset % 8 - 4))) as u8,
            6 => entry.mode,
            _ => entry.reason_outcome,
        }
    }
}
const _: () = assert!(core::mem::size_of::<Entry>() == 8);
const _: () = assert!(core::mem::size_of::<PanelRing>() == 256);
