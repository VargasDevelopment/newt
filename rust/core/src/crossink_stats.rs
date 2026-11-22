// SPDX-License-Identifier: MIT
//! CrossInk reading-statistics wire formats.
use crate::reading_stats::{CrossInkHistory, StatsDecode, StatsDigest, classify_stats_record};
use crate::wire::{fnv1a, get32, put32};
pub const GLOBAL_BYTES: usize = 159;
pub const BOOK_BYTES: usize = 73;
pub const GLOBAL_MAGIC: u32 = 0x4743574e;
pub const SOURCE_MAGIC: u32 = 0x4243574e;
pub const IMPORT_MAGIC: u32 = 0x4943574e;
pub const GLOBAL_RECORD_BYTES: usize = 180;
pub const SOURCE_RECORD_BYTES: usize = 160;
pub const IMPORT_RECORD_BYTES: usize = 56;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CrossInkFile {
    Invalid,
    Valid,
    Newer,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum CrossInkCandidate {
    #[default]
    None,
    Primary,
    Temp,
    Backup,
    Previous,
    Legacy,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum CrossInkStatus {
    Imported = 1,
    #[default]
    Unrecoverable,
    Newer,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum CrossInkBinding {
    #[default]
    None,
    Unverified,
    Manifest,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CrossInkCopy<const N: usize> {
    pub status: CrossInkStatus,
    pub candidate: CrossInkCandidate,
    pub size: u8,
    pub bytes: [u8; N],
}
impl<const N: usize> Default for CrossInkCopy<N> {
    fn default() -> Self {
        Self {
            status: CrossInkStatus::Unrecoverable,
            candidate: CrossInkCandidate::None,
            size: 0,
            bytes: [0; N],
        }
    }
}
pub type CrossInkGlobalCopy = CrossInkCopy<GLOBAL_BYTES>;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CrossInkSource {
    pub name: [u8; 32],
    pub copy: CrossInkCopy<BOOK_BYTES>,
    pub binding: CrossInkBinding,
    pub digest: StatsDigest,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ImportRecord {
    pub sources: u32,
    pub manifest: StatsDigest,
}
pub fn classify_cross_ink_global(b: &[u8]) -> CrossInkFile {
    if b.is_empty() {
        return CrossInkFile::Invalid;
    }
    if b.len() > GLOBAL_BYTES || b[0] > 3 {
        return CrossInkFile::Newer;
    }
    if !matches!((b[0], b.len()), (1, 13) | (2, 17) | (3, 159)) {
        return CrossInkFile::Invalid;
    }
    if b.len() == GLOBAL_BYTES && (get32(b, 61) > 36524 || b[156] >> 2 != 0) {
        return CrossInkFile::Invalid;
    }
    CrossInkFile::Valid
}
/// Classify a bounded read using the file's full observed size.
pub(crate) fn classify_cross_ink_global_prefix(prefix: &[u8], size: usize) -> CrossInkFile {
    if size > GLOBAL_BYTES {
        CrossInkFile::Newer
    } else {
        prefix
            .get(..size)
            .map_or(CrossInkFile::Invalid, classify_cross_ink_global)
    }
}
pub(crate) fn valid_cross_ink_book_prefix(prefix: &[u8], size: usize) -> bool {
    prefix.get(..size).is_some_and(valid_cross_ink_book)
}
fn valid_date(b: &[u8]) -> bool {
    let year = u16::from_le_bytes([b[0], b[1]]);
    let month = b[2];
    let day = b[3];
    if year == 0 && month == 0 && day == 0 {
        return true;
    }
    if !(2000..=2099).contains(&year) || !(1..=12).contains(&month) {
        return false;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][usize::from(month - 1)]
        + u8::from(month == 2 && leap);
    day >= 1 && day <= days
}
pub fn valid_cross_ink_book(b: &[u8]) -> bool {
    if b.is_empty()
        || !matches!(
            (b[0], b.len()),
            (1, 11) | (2, 12) | (3, 16) | (4, 69) | (5, 73)
        )
    {
        return false;
    }
    if b.len() >= 12 && b[11] > 1 {
        return false;
    }
    b.len() < 69 || (b[16] <= 3 && valid_date(&b[17..21]) && valid_date(&b[21..25]))
}
pub fn cross_ink_book_completed(b: &[u8]) -> bool {
    valid_cross_ink_book(b) && b.len() >= 12 && b[11] == 1
}
pub fn decode_cross_ink_global(b: &[u8]) -> Option<CrossInkHistory> {
    if classify_cross_ink_global(b) != CrossInkFile::Valid {
        return None;
    }
    let mut read = CrossInkHistory {
        sessions: get32(b, 1),
        seconds: get32(b, 5),
        pages: get32(b, 9),
        ..CrossInkHistory::default()
    };
    if b.len() >= 17 {
        read.finished = get32(b, 13);
    }
    if b.len() == GLOBAL_BYTES {
        read.newest_day = get32(b, 61);
        read.days.copy_from_slice(&b[65..157]);
        read.longest_streak = u16::from_le_bytes([b[157], b[158]]);
    }
    Some(read)
}
/// A bounded name without its NUL terminator; record decoder checks trailing zeroes.
pub fn cross_ink_source_name(name: &[u8]) -> bool {
    if name.len() >= 32 {
        return false;
    }
    let letters = name.iter().take_while(|b| b.is_ascii_lowercase()).count();
    letters > 0
        && name.get(letters) == Some(&b'_')
        && letters + 1 < name.len()
        && name[letters + 1..].iter().all(u8::is_ascii_digit)
}
pub fn cross_ink_history(copy: &CrossInkGlobalCopy) -> CrossInkHistory {
    if copy.status == CrossInkStatus::Imported {
        copy.bytes
            .get(..usize::from(copy.size))
            .and_then(decode_cross_ink_global)
            .unwrap_or_default()
    } else {
        CrossInkHistory::default()
    }
}
fn seal(b: &mut [u8], magic: u32) {
    put32(b, 0, magic);
    put32(b, 4, 1);
    put32(b, 8, 0);
    let end = b.len() - 4;
    let sum = fnv1a(&b[..end]);
    put32(b, end, sum);
}
fn valid_record(b: &[u8], size: usize, magic: u32) -> bool {
    b.len() == size && classify_stats_record(b, magic) == StatsDecode::Valid
}
fn put_copy<const N: usize>(
    b: &mut [u8],
    at: usize,
    file: usize,
    copy: &CrossInkCopy<N>,
) -> Option<()> {
    let size = usize::from(copy.size);
    if size > N {
        return None;
    }
    b[at] = copy.status as u8;
    b[at + 1] = copy.candidate as u8;
    b[at + 2] = copy.size;
    b[file..file + size].copy_from_slice(&copy.bytes[..size]);
    Some(())
}
fn get_copy<const N: usize>(b: &[u8], at: usize, file: usize) -> Option<CrossInkCopy<N>> {
    let status = match b[at] {
        1 => CrossInkStatus::Imported,
        2 => CrossInkStatus::Unrecoverable,
        3 => CrossInkStatus::Newer,
        _ => return None,
    };
    let candidate = match b[at + 1] {
        0 => CrossInkCandidate::None,
        1 => CrossInkCandidate::Primary,
        2 => CrossInkCandidate::Temp,
        3 => CrossInkCandidate::Backup,
        4 => CrossInkCandidate::Previous,
        5 => CrossInkCandidate::Legacy,
        _ => return None,
    };
    let size = usize::from(b[at + 2]);
    if size > N || b[file + size..file + N].iter().any(|&x| x != 0) {
        return None;
    }
    if status == CrossInkStatus::Imported {
        if candidate == CrossInkCandidate::None {
            return None;
        }
    } else if size != 0
        || (status == CrossInkStatus::Newer) != (candidate != CrossInkCandidate::None)
    {
        return None;
    }
    let mut copy = CrossInkCopy {
        status,
        candidate,
        size: size as u8,
        bytes: [0; N],
    };
    copy.bytes[..size].copy_from_slice(&b[file..file + size]);
    Some(copy)
}
pub fn encode_cross_ink_global(copy: &CrossInkGlobalCopy) -> Option<[u8; GLOBAL_RECORD_BYTES]> {
    let mut b = [0; GLOBAL_RECORD_BYTES];
    put_copy(&mut b, 12, 16, copy)?;
    seal(&mut b, GLOBAL_MAGIC);
    Some(b)
}
pub fn decode_cross_ink_global_record(b: &[u8]) -> Option<CrossInkGlobalCopy> {
    if !valid_record(b, GLOBAL_RECORD_BYTES, GLOBAL_MAGIC) || b[15] != 0 || b[175] != 0 {
        return None;
    }
    let copy = get_copy(b, 12, 16)?;
    if !matches!(
        copy.candidate,
        CrossInkCandidate::None | CrossInkCandidate::Primary | CrossInkCandidate::Backup
    ) {
        return None;
    }
    if copy.status == CrossInkStatus::Imported
        && classify_cross_ink_global(&copy.bytes[..usize::from(copy.size)]) != CrossInkFile::Valid
    {
        return None;
    }
    Some(copy)
}
pub fn encode_cross_ink_source(source: &CrossInkSource) -> Option<[u8; SOURCE_RECORD_BYTES]> {
    let mut b = [0; SOURCE_RECORD_BYTES];
    b[12..44].copy_from_slice(&source.name);
    put_copy(&mut b, 44, 48, &source.copy)?;
    b[47] = source.binding as u8;
    b[124..156].copy_from_slice(&source.digest);
    seal(&mut b, SOURCE_MAGIC);
    Some(b)
}
pub fn decode_cross_ink_source(b: &[u8]) -> Option<CrossInkSource> {
    if !valid_record(b, SOURCE_RECORD_BYTES, SOURCE_MAGIC) || b[121..124] != [0; 3] {
        return None;
    }
    let name = &b[12..44];
    let len = name.iter().position(|&x| x == 0)?;
    if b[43] != 0 || !cross_ink_source_name(&name[..len]) || name[len..].iter().any(|&x| x != 0) {
        return None;
    }
    let copy: CrossInkCopy<BOOK_BYTES> = get_copy(b, 44, 48)?;
    if copy.status == CrossInkStatus::Newer {
        return None;
    }
    if copy.status == CrossInkStatus::Imported
        && (!valid_cross_ink_book(&copy.bytes[..usize::from(copy.size)])
            || (copy.candidate == CrossInkCandidate::Temp && usize::from(copy.size) != BOOK_BYTES))
    {
        return None;
    }
    let binding = match b[47] {
        0 => CrossInkBinding::None,
        1 => CrossInkBinding::Unverified,
        2 => CrossInkBinding::Manifest,
        _ => return None,
    };
    let unbound = b[124..156].iter().all(|&x| x == 0);
    if (binding == CrossInkBinding::None) != unbound {
        return None;
    }
    let mut source = CrossInkSource {
        copy,
        binding,
        ..CrossInkSource::default()
    };
    source.name.copy_from_slice(name);
    source.digest.copy_from_slice(&b[124..156]);
    Some(source)
}
pub fn encode_import(record: &ImportRecord) -> [u8; IMPORT_RECORD_BYTES] {
    let mut b = [0; IMPORT_RECORD_BYTES];
    put32(&mut b, 12, record.sources);
    b[16..48].copy_from_slice(&record.manifest);
    seal(&mut b, IMPORT_MAGIC);
    b
}
pub fn decode_import(b: &[u8]) -> Option<ImportRecord> {
    if !valid_record(b, IMPORT_RECORD_BYTES, IMPORT_MAGIC) || b[48..52] != [0; 4] {
        return None;
    }
    let mut record = ImportRecord {
        sources: get32(b, 12),
        ..ImportRecord::default()
    };
    record.manifest.copy_from_slice(&b[16..48]);
    Some(record)
}
