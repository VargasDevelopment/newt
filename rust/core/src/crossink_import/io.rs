// SPDX-License-Identifier: MIT
//! Bounded read-only file and short SHA lifecycle from crossink_import.h.
use super::{BOOK_MAX_BYTES, PATH_BYTES};
use crate::reading_stats::StatsDigest;
use core::ffi::CStr;

/// Missing means ENOENT or ENOTDIR only. Other errors must never hide a source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IoError {
    Missing,
    Interrupted,
    Failed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    Directory,
    Unknown,
    Other,
}
pub struct Entry<'dir> {
    pub name: &'dir CStr,
    pub kind: EntryKind,
}
/// Every open is read-only. `next` borrows the native directory's entry name;
/// it must distinguish end-of-directory from errors without copying its buffer.
pub trait Files {
    type File;
    type Directory;
    fn open(&mut self, path: &CStr) -> Result<Self::File, IoError>;
    fn size(&mut self, file: &mut Self::File) -> Result<u64, IoError>;
    fn read(&mut self, file: &mut Self::File, output: &mut [u8]) -> Result<usize, IoError>;
    fn close(&mut self, file: Self::File) -> Result<(), IoError>;
    fn open_directory(&mut self, path: &CStr) -> Result<Self::Directory, IoError>;
    fn next<'dir>(
        &mut self,
        directory: &'dir mut Self::Directory,
    ) -> Result<Option<Entry<'dir>>, IoError>;
    fn close_directory(&mut self, directory: Self::Directory) -> Result<(), IoError>;
    fn is_directory(&mut self, path: &CStr) -> Result<bool, IoError>;
}
pub trait BookHasher {
    fn begin(&mut self) -> bool;
    fn update(&mut self, bytes: &[u8]) -> bool;
    fn finish(&mut self, output: &mut StatsDigest) -> bool;
}
/// Allocate the short-lived native SHA context only when invoked. The fstat
/// attempt precedes this call; begin is conditional on successful admission.
/// Namespace SHA remains the separate one-shot seam.
pub trait HashFactory {
    type Hasher: BookHasher;
    fn with_hasher<R>(&mut self, run: impl FnOnce(&mut Self::Hasher) -> R) -> R;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Found {
    Missing,
    Read,
    Failed,
}
pub(super) struct Path {
    pub(super) bytes: [u8; PATH_BYTES],
    pub(super) len: usize,
}
impl Path {
    // Match snprintf's bounded, NUL-terminated path construction.
    pub(super) fn new(parts: &[&[u8]]) -> Self {
        let mut path = Self {
            bytes: [0; PATH_BYTES],
            len: 0,
        };
        for part in parts {
            path.append(part);
        }
        path
    }
    pub(super) fn append(&mut self, bytes: &[u8]) {
        let count = bytes.len().min(PATH_BYTES - 1 - self.len);
        self.bytes[self.len..self.len + count].copy_from_slice(&bytes[..count]);
        self.len += count;
        self.bytes[self.len] = 0;
    }
    pub(super) fn truncate(&mut self, len: usize) {
        self.len = len;
        self.bytes[len] = 0;
    }
    pub(super) fn c_str(&self) -> Result<&CStr, IoError> {
        CStr::from_bytes_with_nul(&self.bytes[..=self.len]).map_err(|_| IoError::Failed)
    }
}
// A successful read resets the frozen four-attempt interrupted-syscall budget.
fn read_chunk<F: Files>(
    files: &mut F,
    file: &mut F::File,
    output: &mut [u8],
    interrupted: &mut u8,
) -> Result<usize, IoError> {
    match files.read(file, output) {
        Err(IoError::Interrupted) => {
            *interrupted += 1;
            Err(if *interrupted < 4 {
                IoError::Interrupted
            } else {
                IoError::Failed
            })
        }
        result => {
            *interrupted = 0;
            result
        }
    }
}
pub(super) fn read_file<F: Files>(
    files: &mut F,
    path: &CStr,
    bytes: &mut [u8],
    size: &mut usize,
) -> Found {
    let mut file = match files.open(path) {
        Ok(file) => file,
        Err(IoError::Missing) => return Found::Missing,
        Err(_) => return Found::Failed,
    };
    let mut ok = match files.size(&mut file) {
        Ok(observed) => {
            *size = observed as usize;
            true
        }
        Err(_) => {
            *size = 0;
            false
        }
    };
    let want = (*size).min(bytes.len());
    let mut got = 0;
    let mut interrupted = 0;
    while ok && got < want {
        match read_chunk(files, &mut file, &mut bytes[got..want], &mut interrupted) {
            Err(IoError::Interrupted) => continue,
            Ok(n) if n > 0 && n <= want - got => got += n,
            _ => ok = false,
        }
    }
    let _ = files.close(file);
    if ok { Found::Read } else { Found::Failed }
}
#[inline(never)]
pub(super) fn hash_book<F: Files, H: HashFactory>(
    files: &mut F,
    factory: &mut H,
    path: &CStr,
    digest: &mut StatsDigest,
    cancelled: &mut dyn FnMut() -> bool,
    stopped: &mut bool,
) -> Found {
    let mut file = match files.open(path) {
        Ok(file) => file,
        Err(IoError::Missing) => return Found::Missing,
        Err(_) => return Found::Failed,
    };
    let observed = files.size(&mut file);
    let size = observed.unwrap_or(0);
    let admitted = size != 0 && size <= BOOK_MAX_BYTES;
    factory.with_hasher(|hasher| {
        let mut ok = observed.is_ok() && (!admitted || hasher.begin());
        let mut chunk = [0; 512];
        let mut at = 0;
        let mut interrupted = 0;
        while ok && admitted && at < size {
            if cancelled() {
                *stopped = true;
                break;
            }
            let want = (size - at).min(chunk.len() as u64) as usize;
            match read_chunk(files, &mut file, &mut chunk[..want], &mut interrupted) {
                Err(IoError::Interrupted) => continue,
                Ok(n) if n > 0 && n <= want => {
                    ok = hasher.update(&chunk[..n]);
                    if ok {
                        at += n as u64;
                    }
                }
                _ => ok = false,
            }
        }
        let _ = files.close(file);
        if !ok {
            Found::Failed
        } else if *stopped || !admitted {
            Found::Missing
        } else if hasher.finish(digest) {
            Found::Read
        } else {
            Found::Failed
        }
    })
}
// CrossInk hashes the card-relative path, including its leading slash.
pub(super) struct SourceName {
    bytes: [u8; 32],
    pub(super) len: usize,
}
impl SourceName {
    pub(super) fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}
pub(super) fn epub_source_name(path: &[u8]) -> SourceName {
    let mut hash = 14695981039346656037u64;
    for &byte in path {
        hash = (hash ^ u64::from(byte)).wrapping_mul(1099511628211);
    }
    let mut digits = [0; 20];
    let mut at = digits.len();
    loop {
        at -= 1;
        digits[at] = b'0' + (hash % 10) as u8;
        hash /= 10;
        if hash == 0 {
            break;
        }
    }
    let mut name = SourceName {
        bytes: [0; 32],
        len: 5 + digits.len() - at,
    };
    name.bytes[..5].copy_from_slice(b"epub_");
    name.bytes[5..name.len].copy_from_slice(&digits[at..]);
    name
}
pub(super) fn epub_file(name: &[u8]) -> bool {
    name.len() > 5 && name[name.len() - 5..].eq_ignore_ascii_case(b".epub")
}
