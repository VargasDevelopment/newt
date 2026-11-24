// Copyright (c) 2026 FreeInk. MIT; see rust/THIRD_PARTY.md.
//! Pinned FreeInk ZipCatalog / ZipEntryReader translation. See rust/tests/zip_container/CONTRACT.md.
// Integrated as crate::zip_container; inflater is crate::inflate.
use crate::inflate::{DICTIONARY_SIZE, HAS_MORE_INPUT, State, Status as InflateStatus};
use crate::zip_window::{
    arena::{Arena, CStringRef},
    source::BookSource,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Status {
    Ok,
    IoError,
    NotZip,
    NotEpub,
    Encrypted,
    Truncated,
    Unsupported,
    OutOfMemory,
    ParseError,
    NotFound,
    Stale,
    MetadataTooLarge,
    PathTooLong,
}
impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Self::Ok => "Ok",
            Self::IoError => "IoError",
            Self::NotZip => "NotZip",
            Self::NotEpub => "NotEpub",
            Self::Encrypted => "Encrypted",
            Self::Truncated => "Truncated",
            Self::Unsupported => "Unsupported",
            Self::OutOfMemory => "OutOfMemory",
            Self::ParseError => "ParseError",
            Self::NotFound => "NotFound",
            Self::Stale => "Stale",
            Self::MetadataTooLarge => "MetadataTooLarge",
            Self::PathTooLong => "PathTooLong",
        }
    }
}
pub const INPUT_SIZE: usize = 2048;
pub const RAW_HEADER_OFFSET: u32 = u32::MAX;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Entry {
    pub name: Option<CStringRef>,
    pub name_hash: u32,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    pub local_header_offset: u32,
    pub method: u16,
}
impl Entry {
    pub const fn raw(size: u32) -> Self {
        Self {
            name: None,
            name_hash: 0,
            compressed_size: size,
            uncompressed_size: size,
            local_header_offset: RAW_HEADER_OFFSET,
            method: 0,
        }
    }
}
fn c_bytes(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len())]
}
pub fn hash_path(path: &[u8]) -> u32 {
    c_bytes(path).iter().fold(2166136261u32, |h, b| {
        (h ^ u32::from(*b)).wrapping_mul(16777619)
    })
}
fn le16(p: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([p[i], p[i + 1]])
}
fn le32(p: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([p[i], p[i + 1], p[i + 2], p[i + 3]])
}
fn read_fully<S: BookSource + ?Sized>(source: &mut S, off: u64, dst: &mut [u8]) -> bool {
    let mut got = 0;
    while got < dst.len() {
        let n = source.read_at(off + got as u64, &mut dst[got..]);
        if n <= 0 || n as usize > dst.len() - got {
            return false;
        }
        got += n as usize;
    }
    true
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Directory {
    pub offset: u32,
    pub size: u32,
    pub entries: u16,
}
/// Caller-owned 4,099-byte scan window avoids large stack frames.
/// `out` is unchanged on every failure, including short source reads.
pub fn locate<S: BookSource + ?Sized>(
    source: &mut S,
    scan: &mut [u8; 4099],
    out: &mut Directory,
) -> Status {
    let size = source.size();
    if size < 22 {
        return Status::NotZip;
    }
    let max_back = size.min(65557);
    let mut lowest = size;
    let mut found = None;
    while found.is_none() && size - lowest < max_back {
        let chunk = (max_back - (size - lowest)).min(4096);
        let off = lowest - chunk;
        let len = (chunk + (size - lowest).min(3)) as usize;
        if !read_fully(source, off, &mut scan[..len]) {
            return Status::IoError;
        }
        if len >= 4 {
            for i in (0..=len - 4).rev() {
                if le32(scan, i) == 0x06054b50 {
                    found = Some(off + i as u64);
                    break;
                }
            }
        }
        lowest = off;
    }
    let Some(at) = found.filter(|at| *at + 22 <= size) else {
        return Status::NotZip;
    };
    let mut eocd = [0; 22];
    if !read_fully(source, at, &mut eocd) {
        return Status::IoError;
    }
    let d = Directory {
        entries: le16(&eocd, 10),
        size: le32(&eocd, 12),
        offset: le32(&eocd, 16),
    };
    if d.entries == u16::MAX || d.size == u32::MAX || d.offset == u32::MAX {
        return Status::Unsupported;
    }
    if u64::from(d.offset) + u64::from(d.size) > size {
        return Status::Truncated;
    }
    *out = d;
    Status::Ok
}
struct SeqReader<'a, S: BookSource + ?Sized> {
    source: &'a mut S,
    buf: &'a mut [u8; 1024],
    pos: u64,
    buf_pos: usize,
    avail: usize,
}
impl<S: BookSource + ?Sized> SeqReader<'_, S> {
    fn read(&mut self, mut dst: &mut [u8]) -> bool {
        while !dst.is_empty() {
            if self.avail == 0 {
                let n = self.source.read_at(self.pos, self.buf);
                if n <= 0 || n as usize > self.buf.len() {
                    return false;
                }
                self.avail = n as usize;
                self.buf_pos = 0;
                self.pos += n as u64;
            }
            let take = self.avail.min(dst.len());
            dst[..take].copy_from_slice(&self.buf[self.buf_pos..self.buf_pos + take]);
            dst = &mut dst[take..];
            self.buf_pos += take;
            self.avail -= take;
        }
        true
    }
    fn skip(&mut self, len: usize) {
        let buffered = self.avail.min(len);
        self.buf_pos += buffered;
        self.avail -= buffered;
        self.pos += (len - buffered) as u64;
    }
}
/// The exclusive arena borrow prevents reset/release/reinitialization while names exist.
pub struct Catalog<'a, 'storage, S: BookSource + ?Sized> {
    source: &'a mut S,
    arena: &'a mut Arena<'storage>,
    entries: &'a mut [Entry],
    count: usize,
}
impl<'a, 'storage, S: BookSource + ?Sized> Catalog<'a, 'storage, S> {
    pub fn new(
        source: &'a mut S,
        arena: &'a mut Arena<'storage>,
        entries: &'a mut [Entry],
    ) -> Self {
        Self {
            source,
            arena,
            entries,
            count: 0,
        }
    }
    pub fn open(&mut self, scan: &mut [u8; 4099], sequential: &mut [u8; 1024]) -> Status {
        self.count = 0;
        let mut directory = Directory::default();
        let status = locate(self.source, scan, &mut directory);
        if status != Status::Ok {
            return status;
        }
        self.open_directory(directory, sequential)
    }
    /// Counted typed backing is obtained after validated EOCD location and
    /// before any central entry/name read, matching the original allocation.
    pub fn open_allocated(
        &mut self,
        scan: &mut [u8; 4099],
        sequential: &mut [u8; 1024],
        allocate: impl FnOnce(&mut Arena<'storage>, usize) -> Result<&'a mut [Entry], Status>,
    ) -> Status {
        self.count = 0;
        let mut directory = Directory::default();
        let status = locate(self.source, scan, &mut directory);
        if status != Status::Ok {
            return status;
        }
        self.entries = match allocate(self.arena, usize::from(directory.entries)) {
            Ok(entries) => entries,
            Err(status) => return status,
        };
        self.open_directory(directory, sequential)
    }
    fn open_directory(&mut self, directory: Directory, sequential: &mut [u8; 1024]) -> Status {
        let count = usize::from(directory.entries);
        if count > self.entries.len() {
            return Status::OutOfMemory;
        }
        let mut reader = SeqReader {
            source: self.source,
            buf: sequential,
            pos: u64::from(directory.offset),
            buf_pos: 0,
            avail: 0,
        };
        for slot in &mut self.entries[..count] {
            let mut hdr = [0; 46];
            if !reader.read(&mut hdr) {
                return Status::Truncated;
            }
            if le32(&hdr, 0) != 0x02014b50 {
                return Status::NotZip;
            }
            let len = usize::from(le16(&hdr, 28));
            if len == 0 || len > 1024 {
                return Status::Unsupported;
            }
            let Some(name) = self.arena.alloc(len + 1, 1) else {
                return Status::OutOfMemory;
            };
            let bytes = self
                .arena
                .view_mut(name)
                .expect("live exclusive arena allocation");
            if !reader.read(&mut bytes[..len]) {
                return Status::Truncated;
            }
            bytes[len] = 0;
            reader.skip(usize::from(le16(&hdr, 30)) + usize::from(le16(&hdr, 32)));
            let name_hash = hash_path(bytes);
            let name = self
                .arena
                .string_ref(name)
                .expect("terminated catalog name");
            *slot = Entry {
                name: Some(name),
                name_hash,
                compressed_size: le32(&hdr, 20),
                uncompressed_size: le32(&hdr, 24),
                local_header_offset: le32(&hdr, 42),
                method: le16(&hdr, 10),
            };
        }
        self.count = count;
        Status::Ok
    }
    /// Consume the catalog before extending its arena. No live catalog can reset it.
    pub fn into_parts(self) -> (&'a mut S, &'a mut Arena<'storage>, &'a mut [Entry]) {
        (self.source, self.arena, &mut self.entries[..self.count])
    }
    pub fn entry_count(&self) -> usize {
        self.count
    }
    pub fn entry(&self, index: usize) -> Option<&Entry> {
        self.entries[..self.count].get(index)
    }
    /// First-NUL name bytes without the terminator, within this arena scope.
    pub fn name(&self, entry: &Entry) -> Option<&[u8]> {
        self.arena.c_string(entry.name?)
    }
    pub fn find(&self, path: Option<&[u8]>) -> Option<&Entry> {
        let path = path?;
        let h = hash_path(path);
        self.entries[..self.count].iter().find(|e| {
            e.name_hash == h
                && self
                    .name(e)
                    .is_some_and(|name| c_bytes(name) == c_bytes(path))
        })
    }
    pub fn find_by_hash(&self, hash: u32) -> Option<&Entry> {
        self.entries[..self.count]
            .iter()
            .find(|e| e.name_hash == hash)
    }
    pub fn source(&mut self) -> &mut S {
        self.source
    }
    pub fn arena_used(&self) -> usize {
        self.arena.used()
    }
}
/// Initialized caller-owned storage, never constructed inside a reader/open call.
pub struct InflateScratch<'a> {
    pub state: &'a mut State,
    pub dictionary: &'a mut [u8; DICTIONARY_SIZE],
    pub input: &'a mut [u8; INPUT_SIZE],
}
pub struct Reader<'a, S: BookSource + ?Sized> {
    source: &'a mut S,
    entry: Option<&'a Entry>,
    scratch: Option<InflateScratch<'a>>,
    data_offset: u64,
    produced: u32,
    in_pos: usize,
    in_avail: usize,
    comp_consumed: u32,
    window_pos: usize,
    pending_pos: usize,
    pending_len: usize,
    done: bool,
    inflate_ready: bool,
}
impl<'a, S: BookSource + ?Sized> Reader<'a, S> {
    pub fn new(source: &'a mut S, scratch: Option<InflateScratch<'a>>) -> Self {
        Self {
            source,
            entry: None,
            scratch,
            data_offset: 0,
            produced: 0,
            in_pos: 0,
            in_avail: 0,
            comp_consumed: 0,
            window_pos: 0,
            pending_pos: 0,
            pending_len: 0,
            done: false,
            inflate_ready: false,
        }
    }
    pub fn open(&mut self, entry: &'a Entry) -> Status {
        self.entry = Some(entry);
        self.produced = 0;
        self.in_pos = 0;
        self.in_avail = 0;
        self.comp_consumed = 0;
        self.window_pos = 0;
        self.pending_pos = 0;
        self.pending_len = 0;
        self.done = false;
        self.inflate_ready = false;
        if entry.local_header_offset == RAW_HEADER_OFFSET {
            if entry.method != 0 || entry.compressed_size != entry.uncompressed_size {
                return Status::Unsupported;
            }
            self.data_offset = 0;
            return Status::Ok;
        }
        let mut local = [0; 30];
        if !read_fully(
            self.source,
            u64::from(entry.local_header_offset),
            &mut local,
        ) {
            return Status::IoError;
        }
        if le32(&local, 0) != 0x04034b50 {
            return Status::NotZip;
        }
        self.data_offset = u64::from(entry.local_header_offset)
            + 30
            + u64::from(le16(&local, 26))
            + u64::from(le16(&local, 28));
        if entry.method == 0 {
            return if entry.compressed_size != entry.uncompressed_size {
                Status::Truncated
            } else {
                Status::Ok
            };
        }
        if entry.method != 8 {
            return Status::Unsupported;
        }
        let Some(scratch) = self.scratch.as_mut() else {
            return Status::OutOfMemory;
        };
        scratch.state.reset();
        self.inflate_ready = true;
        Status::Ok
    }
    /// Lend the input only between reads; decoder offsets and state stay owned here.
    pub fn with_source<R>(&mut self, operation: impl FnOnce(&mut S) -> R) -> R {
        operation(self.source)
    }
    pub fn total_produced(&self) -> u32 {
        self.produced
    }
    pub fn entry(&self) -> Option<&Entry> {
        self.entry
    }
    pub fn read(&mut self, dst: &mut [u8]) -> i32 {
        let Some(entry) = self.entry else {
            return -1;
        };
        if dst.is_empty() {
            return 0;
        }
        // C's signed return domain requires representable positive counts.
        if dst.len() > i32::MAX as usize {
            return -1;
        }
        if entry.method == 0 {
            let want =
                (entry.uncompressed_size.wrapping_sub(self.produced) as usize).min(dst.len());
            if want == 0 {
                return 0;
            }
            let n = self.source.read_at(
                self.data_offset + u64::from(self.produced),
                &mut dst[..want],
            );
            if n <= 0 || n as usize > want {
                return -1;
            }
            self.produced = self.produced.wrapping_add(n as u32);
            return n;
        }
        if !self.inflate_ready {
            return -1;
        }
        let Some(scratch) = self.scratch.as_mut() else {
            return -1;
        };
        let mut out = 0;
        while out < dst.len() {
            if self.pending_len > 0 {
                let take = self.pending_len.min(dst.len() - out);
                dst[out..out + take].copy_from_slice(
                    &scratch.dictionary[self.pending_pos..self.pending_pos + take],
                );
                self.pending_pos += take;
                self.pending_len -= take;
                out += take;
                continue;
            }
            if self.done {
                break;
            }
            if self.in_avail == 0 && self.comp_consumed < entry.compressed_size {
                let want = ((entry.compressed_size - self.comp_consumed) as usize).min(INPUT_SIZE);
                let n = self.source.read_at(
                    self.data_offset + u64::from(self.comp_consumed),
                    &mut scratch.input[..want],
                );
                if n <= 0 || n as usize > want {
                    return -1;
                }
                self.in_avail = n as usize;
                self.in_pos = 0;
                self.comp_consumed += n as u32;
            }
            let flags = if self.comp_consumed < entry.compressed_size {
                HAS_MORE_INPUT
            } else {
                0
            };
            let progress = scratch.state.decompress(
                &scratch.input[self.in_pos..self.in_pos + self.in_avail],
                scratch.dictionary,
                self.window_pos,
                DICTIONARY_SIZE - self.window_pos,
                flags,
            );
            self.in_pos += progress.consumed;
            self.in_avail -= progress.consumed;
            self.pending_pos = self.window_pos;
            self.pending_len = progress.produced;
            self.window_pos = (self.window_pos + progress.produced) & (DICTIONARY_SIZE - 1);
            if progress.status == InflateStatus::Done {
                self.done = true;
            } else if (progress.status as i32) < 0 {
                return -1;
            } else if progress.consumed == 0 && progress.produced == 0 {
                if progress.status == InflateStatus::NeedsMoreInput
                    && self.comp_consumed >= entry.compressed_size
                {
                    return -1;
                }
                if progress.status != InflateStatus::NeedsMoreInput
                    && progress.status != InflateStatus::HasMoreOutput
                {
                    return -1;
                }
            }
        }
        self.produced = self.produced.wrapping_add(out as u32);
        out as i32
    }
}
