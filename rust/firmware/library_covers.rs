// SPDX-License-Identifier: MIT
//! Prepared cover files on SD. No EPUB decode.
use crate::reader_image::cover_idle::ShelfCoverState;
use core::str;
use newt_core::cover::{BareRecord, NoCoverReason, bare_record};
use newt_library_current::{library_book_path, library_name_file, valid_library_book_name};
use newt_library_current_ui::CoverSlotStatus;
use newt_native_storage::{Error, Kind, Native, Open};

const HEADER: [u8; 8] = *b"NLC2\x90\0\xbc\0";
const MAX_BOOK_BYTES: i64 = 64 * 1024 * 1024;
pub const PIXELS: usize = 3384;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CoverFact {
    #[default]
    Unchecked = 0,
    Pending = 1,
    Unavailable = 2,
    Missing = 3,
    Unsupported = 4,
    Incomplete = 5,
    Invalid = 6,
}
impl CoverFact {
    pub const fn from_reason(reason: NoCoverReason) -> Self {
        match reason {
            NoCoverReason::Missing => Self::Missing,
            NoCoverReason::Unsupported => Self::Unsupported,
            NoCoverReason::Incomplete => Self::Incomplete,
            NoCoverReason::Invalid => Self::Invalid,
        }
    }
    pub const fn is_no_cover(self) -> bool {
        matches!(self, Self::Missing | Self::Unsupported | Self::Incomplete | Self::Invalid)
    }
}

#[repr(C)]
#[derive(Default)]
pub struct CoverBookFacts {
    pub record: u32,
    pub size: u32,
    pub mtime: u32,
    pub fact: CoverFact,
}
const _: () = assert!(core::mem::size_of::<CoverBookFacts>() == 16);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Learn {
    Known,
    Unavailable,
    Retryable,
}

/// A short read adapter. The mounted card belongs to the enclosing Home scope;
/// this owner retains only its actual descriptor and streaming CRC scalars.
pub struct PreparedCovers<'a, N: Native> {
    native: &'a mut N,
    remembered: Option<&'a mut CoverBookFacts>,
    shelf: Option<(ShelfCoverState, u32)>,
    fd: i32,
    rows: u32,
    expected: u32,
    crc: u32,
    last_fact: CoverFact,
    any_making: bool,
}
fn path(bytes: &[u8]) -> Option<&str> {
    str::from_utf8(bytes.split(|&b| b == 0).next()?).ok()
}
fn crc(mut value: u32, bytes: &[u8]) -> u32 {
    for &byte in bytes {
        value ^= u32::from(byte);
        for _ in 0..8 { value = (value >> 1) ^ if value & 1 != 0 { 0xedb8_8320 } else { 0 }; }
    }
    value
}
impl<'a, N: Native> PreparedCovers<'a, N> {
    pub fn new(native: &'a mut N, remembered: Option<&'a mut CoverBookFacts>) -> Self {
        Self {
            native, remembered, shelf: None, fd: -1, rows: 0, expected: 0, crc: u32::MAX,
            last_fact: CoverFact::Unavailable, any_making: false,
        }
    }
    pub fn for_library(native: &'a mut N, state: ShelfCoverState, view: u32) -> Self {
        Self {
            native, remembered: None, shelf: Some((state, view)), fd: -1, rows: 0, expected: 0,
            crc: u32::MAX, last_fact: CoverFact::Unavailable, any_making: false,
        }
    }
    pub fn any_making(&self) -> bool { self.any_making }
    fn learn(native: &mut N, filename: &[u8], book: &mut CoverBookFacts, scratch: &mut [u8;192]) -> Learn {
        if !library_book_path(Some(b"/newt-sd/Newt/Books\0"), Some(filename), Some(scratch)) { return Learn::Unavailable; }
        let Some(path) = path(scratch) else { return Learn::Unavailable; };
        let info = match native.stat(path, false) {
            Ok(info) => info,
            Err(Error::Missing) => return Learn::Unavailable,
            Err(_) => return Learn::Retryable,
        };
        if info.kind != Kind::Regular || info.size <= 0 || info.size > MAX_BOOK_BYTES
            || info.seconds < 0 || (info.seconds as u64 >> 1) > u64::from(u32::MAX) { return Learn::Unavailable; }
        book.size = info.size as u32;
        book.mtime = (info.seconds as u64 >> 1) as u32;
        Learn::Known
    }
    fn begin_impl(&mut self, filename: &[u8]) -> bool {
        self.end();
        self.last_fact = CoverFact::Unavailable;
        if !valid_library_book_name(filename) { return false; }
        let mut scratch = [0;192];
        let mut listed = CoverBookFacts::default();
        let remembered = self.remembered.is_some();
        let book = self.remembered.as_deref_mut().unwrap_or(&mut listed);
        if !remembered {
            match Self::learn(self.native, filename, book, &mut scratch) {
                Learn::Known => {}
                Learn::Unavailable => return false,
                Learn::Retryable => { self.last_fact = CoverFact::Pending; return false; }
            }
        }
        if !library_name_file(Some(b"/newt-sd/Newt/Covers\0"), Some(filename), Some(b".nlc\0"), Some(&mut scratch)) { return false; }
        let Some(cover) = path(&scratch) else { return false; };
        self.fd = match self.native.open(cover, Open::SourceReadOnly) {
            Ok(fd) => fd,
            Err(error) => {
                self.last_fact = CoverFact::Pending;
                if error == Error::Missing && remembered { book.fact = CoverFact::Pending; }
                return false;
            }
        };
        let Ok(info) = self.native.fstat(self.fd) else {
            self.last_fact = CoverFact::Pending;
            return self.reject();
        };
        let bare = info.size == 28;
        if info.kind != Kind::Regular || (info.size != (28 + PIXELS) as i64 && !bare) {
            self.last_fact = CoverFact::Pending;
            if remembered { book.fact = CoverFact::Pending; }
            return self.reject();
        }
        if book.size == 0 {
            match Self::learn(self.native, filename, book, &mut scratch) {
                Learn::Known => {}
                Learn::Unavailable => {
                    self.last_fact = CoverFact::Unavailable;
                    if remembered { book.fact = CoverFact::Unavailable; }
                    return self.reject();
                }
                Learn::Retryable => {
                    self.last_fact = CoverFact::Pending;
                    return self.reject();
                }
            }
        }
        let mut header = [0;28];
        let field = |header: &[u8; 28], at: usize| {
            <[u8; 8]>::try_from(&header[at..at + 8]).ok().map(u64::from_le_bytes)
        };
        if self.native.read(self.fd, &mut header) != Ok(header.len()) || header[..8] != HEADER
            || field(&header, 8) != Some(u64::from(book.size))
            || field(&header, 16) != Some(u64::from(book.mtime) << 1) {
            self.last_fact = CoverFact::Pending;
            if remembered { book.fact = CoverFact::Pending; }
            return self.reject();
        }
        self.expected = u32::from_le_bytes([header[24], header[25], header[26], header[27]]);
        if bare {
            let fact = match bare_record(self.expected) {
                BareRecord::Current(reason) => CoverFact::from_reason(reason),
                BareRecord::Retry | BareRecord::Other => CoverFact::Pending,
            };
            self.last_fact = fact;
            if remembered { book.fact = fact; }
            return self.reject();
        }
        self.last_fact = CoverFact::Pending;
        self.crc = u32::MAX;
        self.rows = 0;
        true
    }
    fn slot_status_for(&self, slot: usize) -> CoverSlotStatus {
        match self.last_fact {
            CoverFact::Missing => CoverSlotStatus::Reason(NoCoverReason::Missing),
            CoverFact::Unsupported => CoverSlotStatus::Reason(NoCoverReason::Unsupported),
            CoverFact::Incomplete => CoverSlotStatus::Reason(NoCoverReason::Incomplete),
            CoverFact::Invalid => CoverSlotStatus::Reason(NoCoverReason::Invalid),
            CoverFact::Unavailable | CoverFact::Unchecked => CoverSlotStatus::NoCover,
            CoverFact::Pending => {
                let Some((state, view)) = self.shelf else { return CoverSlotStatus::NoCover; };
                let same_view = state.active && state.view == view;
                let slot_done = same_view
                    && (state.all_stopped || (slot < 4 && (state.settled_slots & (1u8 << slot)) != 0));
                if !slot_done {
                    CoverSlotStatus::Making
                } else if state.paused_retryable {
                    CoverSlotStatus::Deferred
                } else {
                    CoverSlotStatus::NoCover
                }
            }
        }
    }
    fn inspect_slot(&mut self, filename: Option<&[u8]>, slot: usize) -> CoverSlotStatus {
        let Some(name) = filename else { return CoverSlotStatus::NoCover; };
        if self.begin_impl(name) {
            self.end();
            return CoverSlotStatus::NoCover;
        }
        self.slot_status_for(slot)
    }
    fn reject(&mut self) -> bool { self.end(); false }
    pub fn begin(&mut self, filename: &[u8]) -> bool { self.begin_impl(filename) }
    pub fn read_row(&mut self, row: &mut [u8;18]) -> bool {
        if self.fd < 0 || self.rows >= 188 { return false; }
        if self.native.read(self.fd, row) != Ok(row.len()) { return self.reject(); }
        self.crc = crc(self.crc, row);
        self.rows += 1;
        if self.rows == 188 && self.crc ^ u32::MAX != self.expected { return self.reject(); }
        true
    }
    pub fn read_all(&mut self, pixels: &mut [u8;PIXELS]) -> bool {
        if self.fd < 0 || self.rows != 0 || self.native.read(self.fd, pixels) != Ok(pixels.len())
            || crc(self.crc, pixels) ^ u32::MAX != self.expected { return self.reject(); }
        self.rows = 188;
        true
    }
    pub fn end(&mut self) {
        if self.fd >= 0 { let _ = self.native.close(self.fd); }
        self.fd = -1;
        self.rows = 0;
    }
}
impl<N: Native> Drop for PreparedCovers<'_, N> { fn drop(&mut self) { self.end(); } }
impl<N: Native> newt_stage_home::HomeCoverSource for PreparedCovers<'_, N> {
    fn begin(&mut self, filename: &[u8]) -> bool { self.begin(filename) }
    fn read_all(&mut self, pixels: &mut [u8;PIXELS]) -> bool { self.read_all(pixels) }
    fn end(&mut self) { self.end(); }
}
impl<N: Native> newt_library_current_ui::LibraryCoverSource for PreparedCovers<'_, N> {
    fn begin(&mut self, _: &mut dyn newt_library_current::list::Files, filename: Option<&[u8]>) -> bool {
        self.end();
        self.last_fact = CoverFact::Unavailable;
        filename.is_some_and(|name| self.begin(name))
    }
    fn read_row(&mut self, _: &mut dyn newt_library_current::list::Files, row: &mut [u8;18]) -> bool {
        self.read_row(row)
    }
    fn end(&mut self, _: &mut dyn newt_library_current::list::Files) { self.end(); }
    fn slot_status(&self, slot: usize) -> CoverSlotStatus { self.slot_status_for(slot) }
    fn inspect(&mut self, _: &mut dyn newt_library_current::list::Files, filename: Option<&[u8]>, slot: usize) -> CoverSlotStatus {
        self.inspect_slot(filename, slot)
    }
    fn shelf_painted(&mut self, any_making: bool) { self.any_making = any_making; }
}
