//! Primitive retained identity from firmware/sd_resume_record.h (MIT).
//! Native Rust layout is never persisted; selected-book wire encoding is separate.
use crate::library_name::{FILENAME_BYTES, name_bytes, valid_library_book_name};

pub const MAGIC: u32 = 0x5344_4e31;
pub const RETAINED_WORDS: usize = 35;
const _: () = assert!(FILENAME_BYTES == 96 && RETAINED_WORDS * 4 == 140);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SdResumeRecord {
    pub magic: u32,
    pub version: u32,
    pub digest: [u8; 32],
    pub filename: [u8; FILENAME_BYTES],
    pub checksum: u32,
}

impl SdResumeRecord {
    /// Read primitive C3 RTC words without interpreting validity or Rust layout.
    /// Preserve corrupt/cold evidence; valid() remains the existing authority.
    pub fn from_words(words: &[u32; RETAINED_WORDS]) -> Self {
        let mut record = Self {
            magic: words[0],
            version: words[1],
            digest: [0; 32],
            filename: [0; FILENAME_BYTES],
            checksum: words[34],
        };
        for (out, word) in record.digest.chunks_exact_mut(4).zip(&words[2..10]) {
            out.copy_from_slice(&word.to_le_bytes());
        }
        for (out, word) in record.filename.chunks_exact_mut(4).zip(&words[10..34]) {
            out.copy_from_slice(&word.to_le_bytes());
        }
        record
    }

    /// Write the same primitive fields; no validation, checksum repair or clear.
    pub fn write_words(&self, words: &mut [u32; RETAINED_WORDS]) {
        words[0] = self.magic;
        words[1] = self.version;
        for (out, bytes) in words[2..10].iter_mut().zip(self.digest.chunks_exact(4)) {
            *out = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        }
        for (out, bytes) in words[10..34].iter_mut().zip(self.filename.chunks_exact(4)) {
            *out = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        }
        words[34] = self.checksum;
    }

    /// `name` is a bounded NUL-terminated C string, matching the reference API.
    pub fn new(name: &core::ffi::CStr, digest: [u8; 32]) -> Option<Self> {
        let name = name.to_bytes_with_nul();
        if !valid_library_book_name(name) || !digest.iter().any(|&b| b != 0) {
            return None;
        }
        let name = name_bytes(name)?;
        let mut record = Self {
            magic: MAGIC,
            version: 1,
            digest,
            filename: [0; FILENAME_BYTES],
            checksum: 0,
        };
        record.filename[..name.len()].copy_from_slice(name);
        record.checksum = record.calculate_checksum();
        Some(record)
    }

    pub fn calculate_checksum(&self) -> u32 {
        // Wire format hashes version as ONE integer xor/multiply, not four
        // little-endian bytes. Magic and checksum are deliberately excluded.
        let mut hash = (2_166_136_261u32 ^ self.version).wrapping_mul(16_777_619);
        for &byte in self.digest.iter().chain(self.filename.iter()) {
            hash = (hash ^ u32::from(byte)).wrapping_mul(16_777_619);
        }
        hash
    }

    pub fn valid(&self) -> bool {
        self.magic == MAGIC
            && self.version == 1
            && self.checksum == self.calculate_checksum()
            && valid_library_book_name(&self.filename)
            && self.digest.iter().any(|&b| b != 0)
    }

    pub fn book_id(&self) -> u64 {
        u64::from_le_bytes(self.digest[..8].try_into().expect("fixed SHA prefix"))
    }
}

#[cfg(test)]
#[path = "sd_resume_tests.rs"]
mod tests;
