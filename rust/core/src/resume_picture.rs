//! Frozen resume-picture header and panel mark from firmware/resume_picture.h.
use crate::frame::Frame;
use crate::preferences::{Orientation, TextSize, ViewSettings};
use crate::progress::Locator;
use crate::wire::{crc32, get32, put32};

pub const LABEL_BYTES: usize = 256;
pub const HEADER_BYTES: usize = 512;
pub const FILE_BYTES: usize = HEADER_BYTES + crate::frame::FRAME_BYTES;

/// What the picture shows. It holds no firmware identity, so an update that
/// draws the same page keeps the picture; one that draws it differently is
/// caught by the panel mark and repainted once (#105).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Binding {
    pub book: [u8; 32],
    pub anchor: Locator,
    pub settings: ViewSettings,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Footer {
    pub spine: u32,
    pub page: u32,
    pub pages: u32,
    pub overall_known: bool,
    pub overall_percent: u8,
    pub label: [u8; LABEL_BYTES],
}

impl Default for Footer {
    fn default() -> Self {
        Self {
            spine: 0,
            page: 0,
            pages: 0,
            overall_known: false,
            overall_percent: 0,
            label: [0; LABEL_BYTES],
        }
    }
}

/// Borrowed C-string bytes: None is empty, and the first NUL ends the label.
#[derive(Clone, Copy, Debug)]
pub struct FooterView<'a> {
    pub spine: u32,
    pub page: u32,
    pub pages: u32,
    pub overall_known: bool,
    pub overall_percent: u8,
    pub section_label: Option<&'a [u8]>,
}

fn text_length(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len())
}

impl Footer {
    /// Keep the whole label or leave every previous footer field unchanged.
    pub fn assign(&mut self, footer: FooterView<'_>) -> bool {
        let text = footer.section_label.unwrap_or(&[]);
        let length = text_length(text);
        if length >= LABEL_BYTES {
            return false;
        }
        self.spine = footer.spine;
        self.page = footer.page;
        self.pages = footer.pages;
        self.overall_known = footer.overall_known;
        self.overall_percent = footer.overall_percent;
        self.label.fill(0);
        self.label[..length].copy_from_slice(&text[..length]);
        true
    }

    pub fn view(&self) -> FooterView<'_> {
        FooterView {
            spine: self.spine,
            page: self.page,
            pages: self.pages,
            overall_known: self.overall_known,
            overall_percent: self.overall_percent,
            section_label: Some(&self.label),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Record {
    pub binding: Binding,
    pub footer: Footer,
    pub frame_crc: u32,
}

impl Record {
    pub const MAGIC: u32 = 0x5052_574e;
    /// Version 1 bound the firmware digest at bytes 8..40; those bytes are
    /// now reserved zero, so a version 1 picture is rejected once.
    pub const VERSION: u32 = 2;

    /// The footer normally comes from assign or decode and has a terminal NUL.
    pub fn encode(&self, bytes: &mut [u8; HEADER_BYTES]) {
        bytes.fill(0);
        put32(bytes, 0, Self::MAGIC);
        put32(bytes, 4, Self::VERSION);
        bytes[40..72].copy_from_slice(&self.binding.book);
        put32(bytes, 72, self.binding.anchor.spine);
        put32(bytes, 76, self.binding.anchor.character);
        bytes[80] = self.binding.settings.orientation as u8;
        bytes[81] = u8::from(self.binding.settings.slim_progress_bar);
        bytes[82] = self.binding.settings.refresh_pages;
        // 0 for the default size, so default pictures keep their old bytes.
        bytes[83] = self.binding.settings.text_size.byte();
        put32(bytes, 84, self.footer.spine);
        put32(bytes, 88, self.footer.page);
        put32(bytes, 92, self.footer.pages);
        bytes[96] = u8::from(self.footer.overall_known);
        bytes[97] = self.footer.overall_percent;
        let length = text_length(&self.footer.label);
        bytes[98..100].copy_from_slice(&(length as u16).to_le_bytes());
        bytes[100..100 + length].copy_from_slice(&self.footer.label[..length]);
        put32(bytes, 356, self.frame_crc);
        let checksum = Self::crc(&bytes[..508]);
        put32(bytes, 508, checksum);
    }

    /// Damaged, foreign, or newer headers leave the complete record unchanged.
    pub fn decode(&mut self, bytes: &[u8; HEADER_BYTES]) -> bool {
        let length = usize::from(u16::from_le_bytes([bytes[98], bytes[99]]));
        let (Some(orientation), Some(text_size)) = (Orientation::from_byte(bytes[80]), TextSize::from_byte(bytes[83])) else {
            return false;
        };
        if get32(bytes, 0) != Self::MAGIC
            || get32(bytes, 4) != Self::VERSION
            || get32(bytes, 508) != Self::crc(&bytes[..508])
            || bytes[8..40].iter().any(|&byte| byte != 0)
            || bytes[81] > 1
            || bytes[96] > 1
            || length >= LABEL_BYTES
            || bytes[100..100 + length].contains(&0)
        {
            return false;
        }
        for (at, &byte) in bytes.iter().enumerate().take(508).skip(100 + length) {
            if !(356..360).contains(&at) && byte != 0 {
                return false;
            }
        }
        // Validate first, then fill the caller's record without a second record.
        self.binding.book.copy_from_slice(&bytes[40..72]);
        self.binding.anchor.spine = get32(bytes, 72);
        self.binding.anchor.character = get32(bytes, 76);
        self.binding.settings.orientation = orientation;
        self.binding.settings.slim_progress_bar = bytes[81] != 0;
        self.binding.settings.refresh_pages = bytes[82];
        self.binding.settings.text_size = text_size;
        self.footer.spine = get32(bytes, 84);
        self.footer.page = get32(bytes, 88);
        self.footer.pages = get32(bytes, 92);
        self.footer.overall_known = bytes[96] != 0;
        self.footer.overall_percent = bytes[97];
        self.footer.label.fill(0);
        self.footer.label[..length].copy_from_slice(&bytes[100..100 + length]);
        self.frame_crc = get32(bytes, 356);
        true
    }

    pub fn crc(bytes: &[u8]) -> u32 {
        crc32(bytes)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Load {
    Shown,
    Missing,
    Unreadable,
    Corrupt,
    Mismatch,
}

impl Load {
    pub fn name(self) -> &'static str {
        match self {
            Self::Shown => "shown",
            Self::Missing => "missing",
            Self::Unreadable => "unreadable",
            Self::Corrupt => "corrupt",
            Self::Mismatch => "mismatch",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Save {
    Written,
    Same,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Check {
    None,
    Covered,
    Differs,
}

/// Ordinary RAM only; a cold boot has no mark. The first check consumes it.
/// A picture that differed from the real page stays stale until rewritten:
/// its binding and footer still match, so only this mark forces the save.
/// It stays 8 bytes (RAM is at its ceiling): while a check is pending
/// (`generation` != 0), `crc` is the shown frame's; otherwise it is STALE or 0.
#[derive(Debug, Default)]
pub struct Mark {
    generation: u32,
    crc: u32,
}

const STALE: u32 = 1;

impl Mark {
    pub const fn new() -> Self {
        Self {
            generation: 0,
            crc: 0,
        }
    }

    pub fn stale(&self) -> bool {
        self.generation == 0 && self.crc == STALE
    }

    pub fn written(&mut self) {
        if self.stale() {
            self.crc = 0;
        }
    }

    /// A stale picture is not armed: it would differ again, and its check
    /// returning None repaints the page just the same while it stays stale.
    pub fn shown(&mut self, generation: u32, frame: &Frame) {
        if self.stale() {
            return;
        }
        self.generation = generation;
        self.crc = if generation == 0 { 0 } else { Record::crc(frame) };
    }

    pub fn check(&mut self, generation: u32, frame: &Frame) -> Check {
        if self.generation == 0 {
            return Check::None;
        }
        let held = generation == self.generation;
        self.generation = 0;
        let check = if !held {
            Check::None
        } else if Record::crc(frame) == self.crc {
            Check::Covered
        } else {
            Check::Differs
        };
        self.crc = if check == Check::Differs { STALE } else { 0 };
        check
    }
}
