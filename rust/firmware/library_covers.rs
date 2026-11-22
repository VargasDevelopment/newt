// SPDX-License-Identifier: MIT
//! Prepared cover files on SD. No EPUB decode.
use core::str;
use newt_library_current::{library_book_path, library_name_file, valid_library_book_name};
use newt_native_storage::{Error, Kind, Native, Open};

const HEADER: [u8; 8] = *b"NLC2\x90\0\xbc\0";
pub const PIXELS: usize = 3384;
#[repr(C)]
#[derive(Default)]
pub struct CoverBookFacts {
    pub record: u32,
    pub size: u32,
    pub mtime: u32,
    pub no_cover: bool,
}

/// A short read adapter. The mounted card belongs to the enclosing Home scope;
/// this owner retains only its actual descriptor and streaming CRC scalars.
pub struct PreparedCovers<'a, N: Native> {
    native: &'a mut N,
    remembered: Option<&'a mut CoverBookFacts>,
    fd: i32,
    rows: u32,
    expected: u32,
    crc: u32,
}
fn path(bytes: &[u8]) -> Option<&str> {
    str::from_utf8(bytes.split(|&b| b == 0).next()?).ok()
}
fn little(bytes: &[u8]) -> u64 {
    bytes.iter().enumerate().fold(0, |v, (i, b)| v | (u64::from(*b) << (8 * i)))
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
        Self { native, remembered, fd: -1, rows: 0, expected: 0, crc: u32::MAX }
    }
    fn learn(native: &mut N, filename: &[u8], book: &mut CoverBookFacts, scratch: &mut [u8;192]) -> bool {
        if !library_book_path(Some(b"/newt-sd/Newt/Books\0"), Some(filename), Some(scratch)) { return false; }
        let Some(path) = path(scratch) else { return false; };
        let Ok(info) = native.stat(path, false) else { return false; };
        if info.kind != Kind::Regular || info.size <= 0 || info.size > i64::from(u32::MAX)
            || info.seconds < 0 || (info.seconds as u64 >> 1) > u64::from(u32::MAX) { return false; }
        book.size = info.size as u32;
        book.mtime = (info.seconds as u64 >> 1) as u32;
        true
    }
    fn begin_impl(&mut self, filename: &[u8]) -> bool {
        self.end();
        if !valid_library_book_name(filename) { return false; }
        let mut scratch = [0;192];
        let mut listed = CoverBookFacts::default();
        let remembered = self.remembered.is_some();
        let book = self.remembered.as_deref_mut().unwrap_or(&mut listed);
        if !remembered && !Self::learn(self.native, filename, book, &mut scratch) { return false; }
        if !library_name_file(Some(b"/newt-sd/Newt/Covers\0"), Some(filename), Some(b".nlc\0"), Some(&mut scratch)) { return false; }
        let Some(cover) = path(&scratch) else { return false; };
        self.fd = match self.native.open(cover, Open::SourceReadOnly) {
            Ok(fd) => fd,
            Err(error) => {
                if error == Error::Missing && remembered { book.no_cover = true; }
                return false;
            }
        };
        let Ok(info) = self.native.fstat(self.fd) else { return self.reject(); };
        let bare = info.size == 28;
        if info.kind != Kind::Regular || (info.size != (28 + PIXELS) as i64 && !(remembered && bare)) {
            return self.reject();
        }
        if book.size == 0 && !Self::learn(self.native, filename, book, &mut scratch) { return self.reject(); }
        let mut header = [0;28];
        if self.native.read(self.fd, &mut header) != Ok(header.len()) || header[..8] != HEADER
            || little(&header[8..16]) != u64::from(book.size)
            || little(&header[16..24]) != (u64::from(book.mtime) << 1) { return self.reject(); }
        self.expected = little(&header[24..28]) as u32;
        if bare {
            if newt_core::cover::no_art(self.expected) { book.no_cover = true; }
            return self.reject();
        }
        self.crc = u32::MAX;
        self.rows = 0;
        true
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
        filename.is_some_and(|name| self.begin(name))
    }
    fn read_row(&mut self, _: &mut dyn newt_library_current::list::Files, row: &mut [u8;18]) -> bool {
        self.read_row(row)
    }
    fn end(&mut self, _: &mut dyn newt_library_current::list::Files) { self.end(); }
}
