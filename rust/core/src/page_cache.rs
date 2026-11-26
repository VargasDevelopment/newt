//! Safe source port of FreeInk PageCache.cpp, SDK cd6f5b831c8fad899723b6d406586f00c575f021.
//! MIT; frozen SDK sources/license: rust/tests/page_renderer.
#![forbid(unsafe_code)]
use crate::chapter_layout::types::{Bytes, Image, Link, Page, Params, Run};
use crate::zip_window::arena::{Arena, ByteRange};

pub use crate::zip_container::Status;
include!("page_cache_storage.rs");

fn u16_at(b: &[u8], p: usize) -> u16 {
    u16::from_le_bytes([b[p], b[p + 1]])
}
fn u32_at(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]])
}
fn put16(b: &mut [u8], p: usize, v: u16) {
    b[p..p + 2].copy_from_slice(&v.to_le_bytes());
}
fn put32(b: &mut [u8], p: usize, v: u32) {
    b[p..p + 4].copy_from_slice(&v.to_le_bytes());
}
fn cstr(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&v| v == 0).unwrap_or(b.len())]
}
fn mix(h: u32, v: u32) -> u32 {
    (h ^ v).wrapping_mul(16777619)
}
/// Params has no hyphenator field; caller supplies the real readiness bit.
pub fn layout_generation_hash(p: &Params<'_>, fingerprint: u32, hyphenator_ready: bool) -> u32 {
    let mut h = 2166136261;
    for v in [
        3,
        10,
        p.width as u16 as u32,
        p.height as u16 as u32,
        p.left as u16 as u32,
        p.right as u16 as u32,
        p.top as u16 as u32,
        p.bottom as u16 as u32,
        p.base_size as u32,
        fingerprint,
        p.align as u32,
        (p.line_spacing as u32) << 16 | p.paragraph_spacing as u32,
        p.embedded_styles as u32,
        p.focus as u32,
        (p.orphan as u32) << 8 | p.widow as u32,
        p.stylesheet.as_ref().map_or(0, |s| s.content_hash),
        hyphenator_ready as u32,
    ] {
        h = mix(h, v);
    }
    for &b in p.language.map_or(&b""[..], core::ffi::CStr::to_bytes) {
        h = mix(h, b as u32);
    }
    h
}
/// snprintf-compatible truncation, including NUL when capacity is nonzero.
pub fn page_cache_name(spine: u16, hash: u32, out: &mut [u8]) -> bool {
    let mut b = [0u8; 21];
    b[0] = b's';
    let digits = if spine >= 10000 { 5 } else { 4 };
    let mut n = spine;
    for i in (1..=digits).rev() {
        b[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    b[digits + 1] = b'-';
    for i in 0..8 {
        b[digits + 2 + i] = b"0123456789abcdef"[((hash >> (28 - i * 4)) & 15) as usize];
    }
    let end = digits + 10;
    b[end..end + 5].copy_from_slice(b".fibp");
    let len = end + 5;
    if !out.is_empty() {
        let count = len.min(out.len() - 1);
        out[..count].copy_from_slice(&b[..count]);
        out[count] = 0;
    }
    len < out.len()
}
#[derive(Clone, Copy, Default, Debug)]
pub struct Entry {
    pub offset: u32,
    pub chars: u32,
}
#[derive(Clone, Copy, Default, Debug)]
pub struct Anchor {
    pub hash: u32,
    pub chars: u32,
}
/// Activated in units of 128 entries, never one fixed max-pages allocation.
/// Caller may lend additional chunks on later operations. Preserve earlier chunks.
#[derive(Clone, Copy)]
pub struct IndexChunk {
    pub entries: [Entry; 128],
}
impl Default for IndexChunk {
    fn default() -> Self {
        Self {
            entries: [Entry::default(); 128],
        }
    }
}
#[derive(Clone, Copy)]
pub enum Profile {
    Small,
    Standard,
    Large,
}
impl Profile {
    pub fn max_pages(self) -> u32 {
        match self {
            Self::Large => 8192,
            _ => 4096,
        }
    }
    pub fn max_anchors(self) -> usize {
        match self {
            Self::Small => 192,
            Self::Standard => 512,
            Self::Large => 1024,
        }
    }
}
/// Caller-owned persistent metadata. Only activated prefixes are used.
pub struct WriterMemory<'a> {
    pub chunks: &'a mut [IndexChunk],
    pub anchors: &'a mut [Anchor],
}
fn entry(m: &WriterMemory<'_>, i: u32) -> Option<Entry> {
    m.chunks
        .get(i as usize / 128)
        .map(|c| c.entries[i as usize % 128])
}

pub struct PageCacheWriter {
    profile: Profile,
    count: u32,
    offset: u32,
    anchors: usize,
    total: u32,
    failed: bool,
    open: bool,
}
impl PageCacheWriter {
    pub fn new(profile: Profile) -> Self {
        Self {
            profile,
            count: 0,
            offset: 0,
            anchors: 0,
            total: 0,
            failed: false,
            open: false,
        }
    }
    pub fn failed(&self) -> bool {
        self.failed
    }
    pub fn page_count(&self) -> u32 {
        self.count
    }
    pub fn set_total_chars(&mut self, total: u32) {
        self.total = total;
    }
    /// Name is a per-operation loan; every operation must use the same name/store/resources.
    /// As SDK begin(), totalChars survives begin() on a reused writer.
    pub fn begin(
        &mut self,
        s: &mut impl CacheStorage,
        name: &[u8],
        hash: u32,
        m: &WriterMemory<'_>,
    ) -> bool {
        self.count = 0;
        self.offset = 0;
        self.failed = false;
        self.open = false;
        self.anchors = 0;
        if m.anchors.len() < self.profile.max_anchors() {
            self.failed = true;
            return false;
        }
        if !s.begin_write(cstr(name)) {
            self.failed = true;
            return false;
        }
        self.open = true;
        let mut h = *b"FIBP\x03\x00\x00\x00\x00\x00\x00\x00";
        put32(&mut h, 8, hash);
        self.raw(s, &h)
    }
    fn raw(&mut self, s: &mut impl CacheStorage, b: &[u8]) -> bool {
        if self.failed || !self.open {
            return false;
        }
        if !s.write(b) {
            self.failed = true;
            return false;
        }
        self.offset = self.offset.wrapping_add(b.len() as u32);
        true
    }
    pub fn on_anchor(&mut self, m: &mut WriterMemory<'_>, hash: u32, chars: u32) {
        if self.anchors < self.profile.max_anchors() {
            if let Some(a) = m.anchors.get_mut(self.anchors) {
                *a = Anchor { hash, chars };
                self.anchors += 1;
            }
        }
    }
    pub fn on_page(
        &mut self,
        s: &mut impl CacheStorage,
        m: &mut WriterMemory<'_>,
        p: &Page<'_>,
    ) -> bool {
        if self.failed {
            return false;
        }
        if self.count >= self.profile.max_pages() {
            self.failed = true;
            return false;
        }
        let Some(chunk) = m.chunks.get_mut(self.count as usize / 128) else {
            self.failed = true;
            return false;
        };
        chunk.entries[self.count as usize % 128] = Entry {
            offset: self.offset,
            chars: p.char_start,
        };
        self.count += 1;
        // Reject page element counts that exceed the 16-bit wire header.
        if p.runs.len() > 65535 || p.images.len() > 65535 || p.links.len() > 65535 {
            self.failed = true;
            return false;
        }
        let mut h = [0; 10];
        put32(&mut h, 0, p.char_start);
        put16(&mut h, 4, p.runs.len() as u16);
        put16(&mut h, 6, p.images.len() as u16);
        put16(&mut h, 8, p.links.len() as u16);
        if !self.raw(s, &h) {
            return false;
        }
        for r in p.runs {
            let Some(text) = get_bytes(p.bytes, r.text) else {
                self.failed = true;
                return false;
            };
            let mut b = [0; 10];
            put16(&mut b, 0, r.x as u16);
            put16(&mut b, 2, r.baseline as u16);
            put16(&mut b, 4, r.size);
            b[6] = r.flags;
            put16(&mut b, 8, r.text.len);
            if !self.raw(s, &b) || !self.raw(s, text) {
                return false;
            }
        }
        for i in p.images {
            let Some(href) = get_bytes(p.bytes, i.href) else {
                self.failed = true;
                return false;
            };
            let href = cstr(href);
            let mut b = [0; 10];
            put16(&mut b, 0, i.x as u16);
            put16(&mut b, 2, i.y as u16);
            put16(&mut b, 4, i.width);
            put16(&mut b, 6, i.height);
            put16(&mut b, 8, href.len() as u16);
            if !self.raw(s, &b) || !self.raw(s, href) {
                return false;
            }
        }
        for l in p.links {
            let (Some(t), Some(f)) = (get_bytes(p.bytes, l.target), get_bytes(p.bytes, l.fragment))
            else {
                self.failed = true;
                return false;
            };
            let (t, f) = (cstr(t), cstr(f));
            let mut b = [0; 12];
            put16(&mut b, 0, l.x as u16);
            put16(&mut b, 2, l.y as u16);
            put16(&mut b, 4, l.width);
            put16(&mut b, 6, l.height);
            put16(&mut b, 8, t.len() as u16);
            put16(&mut b, 10, f.len() as u16);
            if !self.raw(s, &b) || !self.raw(s, t) || !self.raw(s, f) {
                return false;
            }
        }
        true
    }
    pub fn char_start(&self, m: &WriterMemory<'_>, page: u32) -> u32 {
        if page >= self.count {
            0
        } else {
            entry(m, page).map_or(0, |e| e.chars)
        }
    }
    pub fn page_for_char(&self, m: &WriterMemory<'_>, chars: u32) -> u32 {
        let mut best = 0;
        for i in 0..self.count {
            let Some(e) = entry(m, i) else { break };
            if e.chars <= chars {
                best = i;
            }
        }
        best
    }
    pub fn char_for_anchor(&self, m: &WriterMemory<'_>, hash: u32) -> Option<u32> {
        m.anchors
            .get(..self.anchors)?
            .iter()
            .find(|a| a.hash == hash)
            .map(|a| a.chars)
    }
    pub fn finish(&mut self, s: &mut impl CacheStorage, name: &[u8], m: &WriterMemory<'_>) -> bool {
        self.seal(s, name, m, None)
    }
    pub fn suspend(
        &mut self,
        s: &mut impl CacheStorage,
        name: &[u8],
        m: &WriterMemory<'_>,
        consumed: u32,
        total: u32,
    ) -> bool {
        self.seal(s, name, m, Some((consumed, total)))
    }
    fn seal(
        &mut self,
        s: &mut impl CacheStorage,
        name: &[u8],
        m: &WriterMemory<'_>,
        progress: Option<(u32, u32)>,
    ) -> bool {
        if self.failed || !self.open {
            if self.open {
                s.end_write();
                s.remove(cstr(name));
                self.open = false;
            }
            return false;
        }
        let index = self.offset;
        for i in 0..self.count {
            if self.failed {
                break;
            }
            let Some(e) = entry(m, i) else {
                self.failed = true;
                break;
            };
            let mut b = [0; 8];
            put32(&mut b, 0, e.offset);
            put32(&mut b, 4, e.chars);
            self.raw(s, &b);
        }
        let anchors = self.offset;
        for i in 0..self.anchors {
            if self.failed {
                break;
            }
            let Some(a) = m.anchors.get(i) else {
                self.failed = true;
                break;
            };
            let mut b = [0; 8];
            put32(&mut b, 0, a.hash);
            put32(&mut b, 4, a.chars);
            self.raw(s, &b);
        }
        let mut f = [0; 32];
        put32(&mut f, 0, index);
        put32(&mut f, 4, self.count);
        put32(&mut f, 8, anchors);
        put32(&mut f, 12, self.anchors as u32);
        put32(&mut f, 16, self.total);
        let len = if let Some((consumed, total)) = progress {
            put32(&mut f, 20, consumed);
            put32(&mut f, 24, total);
            f[28..].copy_from_slice(b"FIBx");
            32
        } else {
            f[20..24].copy_from_slice(b"FIBX");
            24
        };
        self.raw(s, &f[..len]);
        // Preserve short circuit: stream failure here skips endWrite, unlike entry failure.
        let committed = !self.failed && s.end_write();
        self.open = false;
        if !committed {
            s.remove(cstr(name));
            self.failed = true;
        }
        !self.failed
    }
    pub fn read_page<'a>(
        &self,
        s: &mut impl CacheStorage,
        m: &WriterMemory<'_>,
        page: u32,
        scratch: &'a mut Arena<'_>,
        out: &'a mut PageMemory<'_>,
    ) -> Result<Page<'a>, Status> {
        if !self.open || self.failed || page >= self.count {
            return Err(Status::NotFound);
        }
        let e = entry(m, page).ok_or(Status::NotFound)?;
        let end = if page + 1 < self.count {
            entry(m, page + 1).ok_or(Status::NotFound)?.offset
        } else {
            self.offset
        };
        let range = allocate_blob(scratch, e.offset, end)?;
        read_fully(
            s,
            None,
            e.offset,
            scratch.view_mut(range).ok_or(Status::OutOfMemory)?,
        )?;
        decode_page(range, page, scratch, out)
    }
}
fn get_bytes(pool: &[u8], b: Bytes) -> Option<&[u8]> {
    pool.get(b.start as usize..(b.start as usize).checked_add(b.len as usize)?)
}
fn read_fully(
    s: &mut impl CacheStorage,
    name: Option<&[u8]>,
    offset: u32,
    dst: &mut [u8],
) -> Result<(), Status> {
    let mut got = 0;
    while got < dst.len() {
        let n = if let Some(name) = name {
            s.read_at(name, offset.wrapping_add(got as u32), &mut dst[got..])
        } else {
            s.read_back_at(offset.wrapping_add(got as u32), &mut dst[got..])
        };
        if n <= 0 || n as usize > dst.len() - got {
            return Err(Status::IoError);
        }
        got += n as usize;
    }
    Ok(())
}
fn allocate_blob(a: &mut Arena<'_>, start: u32, end: u32) -> Result<ByteRange, Status> {
    if end <= start || end - start > 128 * 1024 {
        return Err(Status::Stale);
    }
    a.alloc((end - start) as usize, 4)
        .ok_or(Status::OutOfMemory)
}

/// Allocation follows validated footer, page reads, then anchor allocation.
/// Short backing returns OutOfMemory before reading the corresponding index.
pub trait IndexAllocation<'a> {
    fn pages(&mut self, count: usize) -> Result<&'a mut [Entry], Status>;
    fn anchors(&mut self, count: usize) -> Result<&'a mut [Anchor], Status>;
}
pub struct ReaderMemory<'a> {
    pub pages: &'a mut [Entry],
    pub anchors: &'a mut [Anchor],
}
// Reborrow caller slices so both opening routes use one decoder and error order.
struct IndexSlices<'a> {
    pages: Option<&'a mut [Entry]>,
    anchors: Option<&'a mut [Anchor]>,
}
impl<'a> IndexAllocation<'a> for IndexSlices<'a> {
    fn pages(&mut self, _: usize) -> Result<&'a mut [Entry], Status> {
        self.pages.take().ok_or(Status::OutOfMemory)
    }
    fn anchors(&mut self, _: usize) -> Result<&'a mut [Anchor], Status> {
        self.anchors.take().ok_or(Status::OutOfMemory)
    }
}
/// Segment payload versions. A v4 footer adds one optional resume-point range
/// (`point_offset`, `point_length`) just before its magic:
///
/// ```text
/// whole   v3: index count anchors anchor_count total                        FIBX  (24 B)
///         v4: index count anchors anchor_count total             point len  FIBX  (32 B)
/// partial v3: index count anchors anchor_count total consumed all           FIBx  (32 B)
///         v4: index count anchors anchor_count total consumed all point len FIBx  (40 B)
/// ```
///
/// A point sits in the gap after a page and before the next page or the
/// index, so that page's extent ends at the point. Its content is never
/// trusted for page extents: see `page_extent` and `point`.
pub const VERSION_PAGES: u16 = 3;
pub const VERSION_POINT: u16 = 4;

/// A resume point's verified position in its segment payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointRange {
    pub offset: u32,
    pub length: u32,
}

#[derive(Default)]
pub struct PageCacheReader {
    name: Option<ByteRange>,
    count: u32,
    index: u32,
    anchors: usize,
    total: u32,
    partial: bool,
    consumed: u32,
    build_total: u32,
    point_offset: u32,
    point_length: u32,
}
impl PageCacheReader {
    pub fn page_count(&self) -> u32 {
        self.count
    }
    pub fn is_partial(&self) -> bool {
        self.partial
    }
    pub fn total_chars(&self) -> u32 {
        self.total
    }
    pub fn build_bytes_consumed(&self) -> u32 {
        self.consumed
    }
    pub fn build_bytes_total(&self) -> u32 {
        self.build_total
    }
    pub fn open(
        &mut self,
        s: &mut impl CacheStorage,
        name: &[u8],
        hash: u32,
        a: &mut Arena<'_>,
        m: &mut ReaderMemory<'_>,
    ) -> Status {
        let mut allocation = IndexSlices {
            pages: Some(&mut *m.pages),
            anchors: Some(&mut *m.anchors),
        };
        match self.open_allocated(s, name, hash, a, &mut allocation) {
            Ok(_) => Status::Ok,
            Err(e) => e,
        }
    }
    pub fn open_allocated<'index>(
        &mut self,
        s: &mut impl CacheStorage,
        name: &[u8],
        hash: u32,
        a: &mut Arena<'_>,
        allocation: &mut impl IndexAllocation<'index>,
    ) -> Result<ReaderMemory<'index>, Status> {
        let name = cstr(name);
        self.name = a.copy_nul(Some(name));
        self.count = 0;
        self.point_offset = 0;
        self.point_length = 0;
        if self.name.is_none() {
            return Err(Status::OutOfMemory);
        }
        let size = s.file_size(name);
        if size < 36 {
            return Err(Status::NotFound);
        }
        let mut h = [0; 12];
        read_fully(s, Some(name), 0, &mut h)?;
        let version = u16_at(&h, 4);
        if &h[..4] != b"FIBP"
            || !matches!(version, VERSION_PAGES | VERSION_POINT)
            || u32_at(&h, 8) != hash
        {
            return Err(Status::Stale);
        }
        let mut magic = [0; 4];
        read_fully(s, Some(name), (size as u32).wrapping_sub(4), &mut magic)?;
        self.partial = &magic == b"FIBx";
        self.consumed = 0;
        self.build_total = 0;
        if !self.partial && &magic != b"FIBX" {
            return Err(Status::Stale);
        }
        let point_fields = if version == VERSION_POINT { 8 } else { 0 };
        let fs = (if self.partial { 32 } else { 24 }) + point_fields;
        if size < 12 + fs {
            return Err(Status::Stale);
        }
        let mut f = [0; 40];
        read_fully(
            s,
            Some(name),
            (size as u32).wrapping_sub(fs as u32),
            &mut f[..fs as usize],
        )?;
        self.index = u32_at(&f, 0);
        let count = u32_at(&f, 4);
        let anchor_off = u32_at(&f, 8);
        let anchors = u32_at(&f, 12);
        self.total = u32_at(&f, 16);
        if self.partial {
            self.consumed = u32_at(&f, 20);
            self.build_total = u32_at(&f, 24);
        }
        if version == VERSION_POINT {
            let at = fs as usize - 12;
            (self.point_offset, self.point_length) = (u32_at(&f, at), u32_at(&f, at + 4));
        }
        if self.index < 12
            || anchor_off as i64 + anchors as i64 * 8 + fs != size
            || self.index as i64 + count as i64 * 8 != anchor_off as i64
        {
            return Err(Status::Stale);
        }
        let pages = allocation.pages(count as usize)?;
        if pages.len() < count as usize {
            return Err(Status::OutOfMemory);
        }
        for i in 0..count {
            let mut b = [0; 8];
            read_fully(
                s,
                Some(name),
                self.index.wrapping_add(i.wrapping_mul(8)),
                &mut b,
            )?;
            pages[i as usize] = Entry {
                offset: u32_at(&b, 0),
                chars: u32_at(&b, 4),
            };
        }
        let anchor_records = allocation.anchors(anchors as usize)?;
        if anchor_records.len() < anchors as usize {
            return Err(Status::OutOfMemory);
        }
        for i in 0..anchors {
            let mut b = [0; 8];
            read_fully(
                s,
                Some(name),
                anchor_off.wrapping_add(i.wrapping_mul(8)),
                &mut b,
            )?;
            anchor_records[i as usize] = Anchor {
                hash: u32_at(&b, 0),
                chars: u32_at(&b, 4),
            };
        }
        self.anchors = anchors as usize;
        self.count = count;
        Ok(ReaderMemory {
            pages,
            anchors: anchor_records,
        })
    }
    pub fn char_start(&self, m: &ReaderMemory<'_>, page: u32) -> u32 {
        if page >= self.count {
            0
        } else {
            m.pages.get(page as usize).map_or(0, |e| e.chars)
        }
    }
    pub fn char_for_anchor(&self, m: &ReaderMemory<'_>, hash: u32) -> Option<u32> {
        m.anchors
            .get(..self.anchors)?
            .iter()
            .find(|a| a.hash == hash)
            .map(|a| a.chars)
    }
    /// The page's bytes before any point: from its index entry to the next
    /// page's entry, or to the index for the last page.
    fn gap(&self, m: &ReaderMemory<'_>, page: u32) -> Result<(u32, u32), Status> {
        if page >= self.count {
            return Err(Status::NotFound);
        }
        let start = m.pages.get(page as usize).ok_or(Status::NotFound)?.offset;
        let end = if page + 1 < self.count {
            m.pages.get(page as usize + 1).ok_or(Status::NotFound)?.offset
        } else {
            self.index
        };
        Ok((start, end))
    }
    /// Page extents use only the verified index and footer. A point that fills
    /// the tail of this page's gap ends the page. A point range overlapping the
    /// page any other way is ambiguous, so this page misses; pages it does not
    /// touch still hit. An overflowing range reaches the end of the address space.
    fn page_extent(&self, m: &ReaderMemory<'_>, page: u32) -> Result<(u32, u32), Status> {
        let (start, end) = self.gap(m, page)?;
        let point_start = self.point_offset;
        let point_end = point_start.saturating_add(self.point_length);
        if self.point_length == 0 || point_end <= start || point_start >= end {
            return Ok((start, end));
        }
        if point_start > start && point_end == end {
            Ok((start, point_start))
        } else {
            Err(Status::Stale)
        }
    }
    /// The resume point, when its range is unambiguous: it ends exactly one
    /// page's gap, after at least one byte of that page, and touches no other
    /// page. A bad range loses only the point; its content is checked later.
    pub fn point(&self, m: &ReaderMemory<'_>) -> Option<PointRange> {
        let point = PointRange { offset: self.point_offset, length: self.point_length };
        let end = point.offset.checked_add(point.length)?;
        if point.length == 0 || end > self.index {
            return None;
        }
        let mut owners = 0;
        for page in 0..self.count {
            let (start, gap_end) = self.gap(m, page).ok()?;
            if end <= start || point.offset >= gap_end {
                continue;
            }
            if point.offset <= start || end != gap_end {
                return None;
            }
            owners += 1;
        }
        (owners == 1).then_some(point)
    }
    pub fn page_for_char(&self, m: &ReaderMemory<'_>, chars: u32) -> u32 {
        if self.count == 0 {
            return 0;
        }
        let (mut lo, mut hi) = (0, self.count - 1);
        while lo < hi {
            let mid = lo + (hi - lo + 1) / 2;
            if self.char_start(m, mid) <= chars {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        lo
    }
    pub fn read_page<'a>(
        &self,
        s: &mut impl CacheStorage,
        m: &ReaderMemory<'_>,
        names: &Arena<'_>,
        page: u32,
        scratch: &'a mut Arena<'_>,
        out: &'a mut PageMemory<'_>,
    ) -> Result<Page<'a>, Status> {
        let (start, end) = self.page_extent(m, page)?;
        let range = allocate_blob(scratch, start, end)?;
        let name = names
            .view(self.name.ok_or(Status::NotFound)?)
            .ok_or(Status::NotFound)?;
        read_fully(
            s,
            Some(cstr(name)),
            start,
            scratch.view_mut(range).ok_or(Status::OutOfMemory)?,
        )?;
        decode_page(range, page, scratch, out)
    }
}
/// Independent typed storage is required because safe Arena is byte-only.
/// No allocation/hidden heap. Returned Page loans these records and scratch.
pub struct PageMemory<'a> {
    pub runs: &'a mut [Run],
    pub images: &'a mut [Image],
    pub links: &'a mut [Link],
}
fn copy_string(a: &mut Arena<'_>, blob: ByteRange, pos: usize, len: usize) -> Result<(), Status> {
    let r = a.alloc(len + 1, 1).ok_or(Status::OutOfMemory)?;
    for i in 0..len {
        let b = a.view(blob).ok_or(Status::Stale)?[pos + i];
        a.view_mut(r).ok_or(Status::OutOfMemory)?[i] = b;
    }
    a.view_mut(r).ok_or(Status::OutOfMemory)?[len] = 0;
    Ok(())
}
fn decode_page<'a>(
    blob: ByteRange,
    index: u32,
    a: &'a mut Arena<'_>,
    out: &'a mut PageMemory<'_>,
) -> Result<Page<'a>, Status> {
    let b = a.view(blob).ok_or(Status::Stale)?;
    if b.len() < 10 {
        return Err(Status::Stale);
    }
    let chars = u32_at(b, 0);
    let nr = u16_at(b, 4) as usize;
    let ni = u16_at(b, 6) as usize;
    let nl = u16_at(b, 8) as usize;
    if out.runs.len() < nr {
        return Err(Status::OutOfMemory);
    }
    if out.images.len() < ni {
        return Err(Status::OutOfMemory);
    }
    let mut pos = 10;
    for r in &mut out.runs[..nr] {
        let b = a.view(blob).ok_or(Status::Stale)?;
        if pos + 10 > b.len() {
            return Err(Status::Stale);
        }
        let len = u16_at(b, pos + 8);
        *r = Run {
            text: Bytes {
                start: (pos + 10) as u32,
                len,
            },
            x: u16_at(b, pos) as i16,
            baseline: u16_at(b, pos + 2) as i16,
            size: u16_at(b, pos + 4),
            flags: b[pos + 6],
        };
        pos += 10;
        if pos + len as usize > b.len() {
            return Err(Status::Stale);
        }
        pos += len as usize;
    }
    for i in &mut out.images[..ni] {
        let b = a.view(blob).ok_or(Status::Stale)?;
        if pos + 10 > b.len() {
            return Err(Status::Stale);
        }
        let len = u16_at(b, pos + 8) as usize;
        *i = Image {
            x: u16_at(b, pos) as i16,
            y: u16_at(b, pos + 2) as i16,
            width: u16_at(b, pos + 4),
            height: u16_at(b, pos + 6),
            href: Bytes {
                start: (pos + 10) as u32,
                len: 0,
            },
        };
        pos += 10;
        if pos + len > b.len() {
            return Err(Status::Stale);
        }
        i.href.len = cstr(&b[pos..pos + len]).len() as u16;
        copy_string(a, blob, pos, len)?;
        pos += len;
    }
    if out.links.len() < nl {
        return Err(Status::OutOfMemory);
    }
    for l in &mut out.links[..nl] {
        let b = a.view(blob).ok_or(Status::Stale)?;
        if pos + 12 > b.len() {
            return Err(Status::Stale);
        }
        let tl = u16_at(b, pos + 8) as usize;
        let fl = u16_at(b, pos + 10) as usize;
        *l = Link {
            x: u16_at(b, pos) as i16,
            y: u16_at(b, pos + 2) as i16,
            width: u16_at(b, pos + 4),
            height: u16_at(b, pos + 6),
            target: Bytes::default(),
            fragment: Bytes::default(),
        };
        pos += 12;
        if pos + tl + fl > b.len() {
            return Err(Status::Stale);
        }
        l.target = Bytes {
            start: pos as u32,
            len: cstr(&b[pos..pos + tl]).len() as u16,
        };
        l.fragment = Bytes {
            start: (pos + tl) as u32,
            len: cstr(&b[pos + tl..pos + tl + fl]).len() as u16,
        };
        // SDK attempts both allocations even if the first fails.
        let t = copy_string(a, blob, pos, tl);
        let f = copy_string(a, blob, pos + tl, fl);
        t?;
        f?;
        pos += tl + fl;
    }
    // SDK accepts unconsumed trailing bytes and reserved bytes.
    Ok(Page {
        bytes: a.view(blob).ok_or(Status::Stale)?,
        runs: &out.runs[..nr],
        images: &out.images[..ni],
        links: &out.links[..nl],
        index,
        char_start: chars,
    })
}
/// Decode an externally supplied complete page record (also useful to transports).
/// The record is copied into scratch before any validation, matching cache reads.
pub fn decode_record<'a>(
    record: &[u8],
    index: u32,
    a: &'a mut Arena<'_>,
    out: &'a mut PageMemory<'_>,
) -> Result<Page<'a>, Status> {
    let r = a.alloc(record.len(), 4).ok_or(Status::OutOfMemory)?;
    a.view_mut(r)
        .ok_or(Status::OutOfMemory)?
        .copy_from_slice(record);
    decode_page(r, index, a, out)
}
/// Temporary callback adapter; retained writer state owns no parser/frame loans.
pub struct WriterSink<'a, S: CacheStorage> {
    pub writer: &'a mut PageCacheWriter,
    pub storage: &'a mut S,
    pub memory: WriterMemory<'a>,
}
impl<S: CacheStorage> crate::chapter_layout::types::Sink for WriterSink<'_, S> {
    fn anchor(&mut self, hash: u32, chars: u32) {
        self.writer.on_anchor(&mut self.memory, hash, chars);
    }
    fn page(&mut self, page: Page<'_>) -> bool {
        self.writer.on_page(self.storage, &mut self.memory, &page)
    }
}
