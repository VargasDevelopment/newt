//! Bounded in-memory book catalog and path validation.
//! Scanning, file I/O and content comparison remain caller responsibilities.
use crate::library_name::{FILENAME_BYTES, name_bytes, valid_library_book_name};
use core::cmp::Ordering;

pub const BOOK_CAPACITY: usize = 128;
pub const LIBRARY_DIRECTORY: &[u8] = b"/Newt/Books\0";
pub const FOLDER_DEPTH: u32 = 6;
pub const SCAN_ENTRIES: usize = 4096;
pub const PROOF_BUDGET: u16 = 128;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum LibraryStatus {
    #[default]
    Ok,
    Unavailable,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LibraryOmissions {
    pub full: u16,
    pub names: u16,
    pub folders: u16,
    pub stopped: bool,
}
#[derive(Clone, Copy, Debug)]
pub enum LibraryOmission {
    Name,
    Folder,
    Stopped,
}

/// `bytes` is indexed by insertion slot until finish(), just like the scanner.
/// A false proof (including an I/O failure mapped to false) retains both books
/// and continues probing later equal-size slots. No callback runs for size zero.
pub struct LibraryCopies<F> {
    pub bytes: [u32; BOOK_CAPACITY],
    pub same: Option<F>,
}
impl<F> Default for LibraryCopies<F> {
    fn default() -> Self {
        Self {
            bytes: [0; BOOK_CAPACITY],
            same: None,
        }
    }
}

fn bare(name: &[u8]) -> bool {
    name.first() != Some(&b':')
}
fn c_bytes(bytes: &[u8]) -> Option<&[u8]> {
    Some(&bytes[..bytes.iter().position(|&b| b == 0)?])
}
/// Unbounded C-string view, matching the basename helper even for invalid names.
pub fn library_base_name(name: Option<&[u8]>) -> Option<&[u8]> {
    let name = c_bytes(name?)?;
    Some(base_name(name))
}
fn base_name(name: &[u8]) -> &[u8] {
    if bare(name) {
        name
    } else {
        &name[name.iter().rposition(|&b| b == b':').map_or(0, |i| i + 1)..]
    }
}
fn compare(a: &[u8], b: &[u8]) -> Ordering {
    a.iter()
        .map(u8::to_ascii_lowercase)
        .cmp(b.iter().map(u8::to_ascii_lowercase))
}
fn library_order(a: &[u8], b: &[u8]) -> Ordering {
    compare(base_name(a), base_name(b))
        .then_with(|| compare(a, b))
        .then_with(|| a.cmp(b))
}
fn kept_before(a: &[u8], b: &[u8]) -> bool {
    if bare(a) != bare(b) {
        bare(a)
    } else {
        library_order(a, b).is_lt()
    }
}
/// Catalog/path authorization is deliberately stricter than retained validation.
pub fn listable(name: &[u8]) -> bool {
    if !valid_library_book_name(name) {
        return false;
    }
    let name = name_bytes(name).unwrap();
    if bare(name) {
        return true;
    }
    let mut count = 0;
    let mut newt = false;
    let mut books = false;
    for segment in name[1..].split(|&b| b == b':') {
        if segment.is_empty() || segment[0] == b'.' {
            return false;
        }
        if count == 0 {
            newt = segment.eq_ignore_ascii_case(b"Newt");
        }
        if count == 1 {
            books = segment.eq_ignore_ascii_case(b"Books");
        }
        count += 1;
    }
    !newt || (books && count >= 4)
}

/// snprintf's observable bytes: NUL when capacity > 0, untouched tail, and
/// truncation before any colon-to-slash conversion. Invalid input never writes.
fn print_parts(parts: &[&[u8]], output: &mut [u8]) -> bool {
    let mut written = 0usize;
    for part in parts {
        for &byte in *part {
            if written < output.len().saturating_sub(1) {
                output[written] = byte;
            }
            written += 1;
        }
    }
    if !output.is_empty() {
        output[written.min(output.len() - 1)] = 0;
    }
    written > 0 && written < output.len()
}
pub fn library_book_path(
    books: Option<&[u8]>,
    name: Option<&[u8]>,
    output: Option<&mut [u8]>,
) -> bool {
    let (Some(books), Some(name), Some(output)) = (books, name, output) else {
        return false;
    };
    if !listable(name) {
        return false;
    }
    let Some(books) = c_bytes(books) else {
        return false;
    };
    let name = name_bytes(name).unwrap();
    let (prefix, separator) = if bare(name) {
        (books, b"/".as_slice())
    } else {
        let Some(prefix) = books.strip_suffix(b"/Newt/Books") else {
            return false;
        };
        (prefix, b"".as_slice())
    };
    if !print_parts(&[prefix, separator, name], output) {
        return false;
    }
    if !bare(name) {
        for byte in &mut output[prefix.len()..prefix.len() + name.len()] {
            if *byte == b':' {
                *byte = b'/';
            }
        }
    }
    true
}
pub fn library_name_file(
    directory: Option<&[u8]>,
    name: Option<&[u8]>,
    suffix: Option<&[u8]>,
    output: Option<&mut [u8]>,
) -> bool {
    let (Some(directory), Some(name), Some(suffix), Some(output)) =
        (directory, name, suffix, output)
    else {
        return false;
    };
    if !listable(name) {
        return false;
    }
    let (Some(directory), Some(suffix)) = (c_bytes(directory), c_bytes(suffix)) else {
        return false;
    };
    let name = name_bytes(name).unwrap();
    if bare(name) {
        return print_parts(&[directory, b"/", name, suffix], output);
    }
    let mut hash = 14695981039346656037u64;
    for &byte in name {
        hash = (hash ^ u64::from(byte)).wrapping_mul(1099511628211);
    }
    let mut stem = *b"/.0000000000000000.epub";
    for i in 0..16 {
        stem[2 + i] = b"0123456789abcdef"[((hash >> ((15 - i) * 4)) & 15) as usize];
    }
    print_parts(&[directory, &stem, suffix], output)
}

/// The sole resident name array belongs to the caller. No owned clone API is
/// supplied: firmware-facing probes borrow this state rather than creating it.
pub struct LibraryCatalog {
    books: [[u8; FILENAME_BYTES]; BOOK_CAPACITY],
    count: u8,
    status: LibraryStatus,
    finished: bool,
    copies: u8,
    omissions: LibraryOmissions,
}
impl Default for LibraryCatalog {
    fn default() -> Self {
        Self {
            books: [[0; FILENAME_BYTES]; BOOK_CAPACITY],
            count: 0,
            status: LibraryStatus::Ok,
            finished: false,
            copies: 0,
            omissions: LibraryOmissions::default(),
        }
    }
}
impl LibraryCatalog {
    pub fn reset(&mut self, status: LibraryStatus) {
        self.count = 0;
        self.omissions = LibraryOmissions::default();
        self.copies = 0;
        self.status = status;
        self.finished = false;
    }
    fn reject(&mut self, status: LibraryStatus) -> LibraryStatus {
        self.reset(status);
        status
    }
    pub fn status(&self) -> LibraryStatus {
        self.status
    }
    pub fn size(&self) -> usize {
        if self.status == LibraryStatus::Ok && self.finished {
            usize::from(self.count)
        } else {
            0
        }
    }
    pub fn omissions(&self) -> &LibraryOmissions {
        &self.omissions
    }
    pub fn copies(&self) -> u8 {
        self.copies
    }
    pub fn add_plain(&mut self, name: &[u8], bytes: u32) -> LibraryStatus {
        self.add::<fn(&[u8], &[u8]) -> bool>(name, bytes, None)
    }
    pub fn add<F: FnMut(&[u8], &[u8]) -> bool>(
        &mut self,
        name: &[u8],
        bytes: u32,
        mut copies: Option<&mut LibraryCopies<F>>,
    ) -> LibraryStatus {
        if self.status != LibraryStatus::Ok {
            return self.status;
        }
        if self.finished {
            return self.reject(LibraryStatus::Unavailable);
        }
        if !listable(name) {
            self.omissions.names = self.omissions.names.saturating_add(1);
            return self.status;
        }
        let name = name_bytes(name).unwrap();
        if let Some(proofs) = copies.as_mut() {
            if let Some(same) = proofs.same.as_mut() {
                for i in 0..usize::from(self.count) {
                    if bytes == 0 || proofs.bytes[i] != bytes {
                        continue;
                    }
                    let listed = name_bytes(&self.books[i]).unwrap();
                    if !same(listed, name) {
                        continue;
                    }
                    self.copies = self.copies.saturating_add(1);
                    if kept_before(name, listed) {
                        self.books[i][..name.len()].copy_from_slice(name);
                        self.books[i][name.len()] = 0;
                    }
                    return self.status;
                }
            }
        }
        let mut slot = usize::from(self.count);
        if slot == BOOK_CAPACITY {
            self.omissions.full = self.omissions.full.saturating_add(1);
            slot = 0;
            for i in 1..usize::from(self.count) {
                if kept_before(
                    name_bytes(&self.books[slot]).unwrap(),
                    name_bytes(&self.books[i]).unwrap(),
                ) {
                    slot = i;
                }
            }
            if !kept_before(name, name_bytes(&self.books[slot]).unwrap()) {
                return self.status;
            }
        } else {
            self.count += 1;
        }
        self.books[slot][..name.len()].copy_from_slice(name);
        self.books[slot][name.len()] = 0;
        if let Some(proofs) = copies {
            proofs.bytes[slot] = bytes;
        }
        self.status
    }
    pub fn omit(&mut self, what: LibraryOmission) {
        if self.status != LibraryStatus::Ok || self.finished {
            return;
        }
        match what {
            LibraryOmission::Name => self.omissions.names = self.omissions.names.saturating_add(1),
            LibraryOmission::Folder => {
                self.omissions.folders = self.omissions.folders.saturating_add(1)
            }
            LibraryOmission::Stopped => self.omissions.stopped = true,
        }
    }
    pub fn finish(&mut self) -> LibraryStatus {
        if self.status != LibraryStatus::Ok || self.finished {
            return self.status;
        }
        self.books[..usize::from(self.count)]
            .sort_unstable_by(|a, b| library_order(name_bytes(a).unwrap(), name_bytes(b).unwrap()));
        self.finished = true;
        self.status
    }
    /// NUL-terminated name; storage bytes beyond the NUL stay private.
    pub fn filename(&self, index: usize) -> Option<&[u8]> {
        if index >= self.size() {
            return None;
        }
        let bytes = &self.books[index];
        Some(&bytes[..name_bytes(bytes).unwrap().len() + 1])
    }
    pub fn path(&self, index: usize, output: Option<&mut [u8]>) -> bool {
        library_book_path(Some(LIBRARY_DIRECTORY), self.filename(index), output)
    }
    pub fn notice(&self) -> Option<&'static str> {
        if self.status != LibraryStatus::Ok {
            None
        } else if self.omissions.stopped {
            Some("The card has too many files to list all.")
        } else if self.omissions.full != 0 {
            Some("Only the first 128 books are shown.")
        } else if self.omissions.names != 0 {
            Some("Some books have names Newt can't show.")
        } else if self.omissions.folders != 0 {
            Some("Some folders are too deep or unreadable.")
        } else {
            None
        }
    }
}
