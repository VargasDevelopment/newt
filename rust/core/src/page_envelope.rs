//! Derived from Newt bc9 page_cache_envelope.h; MIT. CRC detects damaged
//! media and binds route/block position; it does not authenticate a writer.
#![forbid(unsafe_code)]

// Pages (.fibp) written under v1 can contain split lines and duplicate
// character starts; v2 can drop links past a page's link slots; v3 can place
// oversized inline runs over the previous line or past the top margin; v4
// predates CSS small-caps and text-transform layout; v5 skips pictures with no
// note. Book indexes (.IDX and legacy .fibx) carry no rendered page layout and
// stay valid from v3 onward.
const VERSION: u32 = 6;
const INDEX_MIN_VERSION: u32 = 3;

pub fn is_index_name(name: Option<&[u8]>) -> bool {
    name.map(c_string).is_some_and(|n| {
        crate::book_index::FileName::parse(n)
            .is_some_and(|f| f.kind() == crate::book_index::FileKind::Index)
            || (n.len() == 69
                && crate::cache_names::namespace::hex(&n[..64])
                && &n[64..69] == b".fibx")
    })
}

pub const BLOCK_BYTES: u32 = 4096;
pub const MANIFEST_BYTES: u32 = 64;
pub const MAX_PAYLOAD: u32 = 4 * 1024 * 1024;
pub const MAX_BLOCKS: u32 = MAX_PAYLOAD.div_ceil(BLOCK_BYTES);
pub const MAX_EXTENT: u32 = MAX_PAYLOAD + 4 * MAX_BLOCKS + MANIFEST_BYTES;
pub type Route = [u8; 32];

fn c_string(input: &[u8]) -> &[u8] {
    &input[..input.iter().position(|&b| b == 0).unwrap_or(input.len())]
}

#[inline(never)]
pub fn crc(mut value: u32, input: &[u8]) -> u32 {
    for &byte in input {
        value ^= u32::from(byte);
        for _ in 0..8 {
            value = (value >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(value & 1));
        }
    }
    value
}

pub const fn blocks(bytes: u32) -> u32 {
    bytes.wrapping_add(BLOCK_BYTES - 1) / BLOCK_BYTES
}

pub fn payload_for_extent(extent: u32) -> u32 {
    if !(MANIFEST_BYTES + 5..=MAX_EXTENT).contains(&extent) {
        return 0;
    }
    let body = extent - MANIFEST_BYTES;
    let count = (body + BLOCK_BYTES + 3) / (BLOCK_BYTES + 4);
    let payload = body - 4 * count;
    if blocks(payload) == count { payload } else { 0 }
}

/// Page and legacy bench names carry a full hex route. Product index queries
/// use Query::for_index with the verified digest; an 8.3 name cannot supply it.
pub fn route(name: Option<&[u8]>, out: &mut Route, canonical: bool) -> bool {
    let Some(name) = name.map(c_string) else {
        return false;
    };
    if (canonical && name.len() != 69) || name.len() < 69 || !matches!(&name[64..69], b".fibp" | b".fibx") {
        return false;
    }
    fn digit(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        }
    }
    for i in 0..32 {
        let (Some(hi), Some(lo)) = (digit(name[2 * i]), digit(name[2 * i + 1])) else {
            return false;
        };
        out[i] = (hi << 4) | lo;
    }
    true
}

pub fn block_seed(route: &Route, block: u32, bytes: u32) -> u32 {
    let mut fields = [0; 8];
    fields[..4].copy_from_slice(&block.to_le_bytes());
    fields[4..].copy_from_slice(&bytes.to_le_bytes());
    crc(crc(u32::MAX, route), &fields)
}

pub fn block_crc(route: &Route, block: u32, data: &[u8]) -> u32 {
    crc(block_seed(route, block, data.len() as u32), data) ^ u32::MAX
}

fn get32(input: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(
        input[at..at + 4]
            .try_into()
            .expect("fixed-width manifest field"),
    )
}

fn put32(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

pub fn manifest(
    out: &mut [u8; 64],
    bytes: u32,
    route: &Route,
    directory_crc: u32,
    payload_crc: u32,
) {
    out[..4].copy_from_slice(b"NTBE");
    put32(out, 4, VERSION);
    put32(out, 8, bytes);
    put32(out, 12, BLOCK_BYTES);
    put32(out, 16, blocks(bytes));
    put32(out, 20, directory_crc);
    put32(out, 24, payload_crc);
    out[28..60].copy_from_slice(route);
    let checksum = crc(u32::MAX, &out[..60]) ^ u32::MAX;
    put32(out, 60, checksum);
}

pub fn inspect(input: &[u8; 64], extent: u32, route: &Route, bytes: &mut u32) -> bool {
    inspect_for(None, input, extent, route, bytes)
}

pub fn inspect_for(
    name: Option<&[u8]>,
    input: &[u8; 64],
    extent: u32,
    route: &Route,
    bytes: &mut u32,
) -> bool {
    let version = get32(input, 4);
    let version_ok = version == VERSION
        || (is_index_name(name) && (INDEX_MIN_VERSION..=VERSION).contains(&version));
    if &input[..4] != b"NTBE"
        || !version_ok
        || get32(input, 12) != BLOCK_BYTES
        || get32(input, 60) != crc(u32::MAX, &input[..60]) ^ u32::MAX
        || &input[28..60] != route
    {
        return false;
    }
    let payload = get32(input, 8);
    if payload == 0
        || payload > MAX_PAYLOAD
        || get32(input, 16) != blocks(payload)
        || extent != payload + 4 * blocks(payload) + MANIFEST_BYTES
    {
        return false;
    }
    *bytes = payload;
    true
}

/// The native owner verifies its live descriptor generation on every raw call.
/// It lends that owner per operation; this state retains no mutable source loan.
pub trait RawRead {
    fn read(&mut self, query: &Query<'_>, offset: u32, out: &mut [u8]) -> bool;
}

pub struct Query<'n> {
    pub name: Option<&'n [u8]>,
    pub route: Route,
    pub generation: u64,
    pub extent: u32,
    pub payload: u32,
    pub active: bool,
    pub bound: bool,
}

impl<'n> Query<'n> {
    pub fn new(name: Option<&'n [u8]>) -> Self {
        let mut route_bytes = [0; 32];
        let active = route(name, &mut route_bytes, true);
        Self {
            name,
            route: route_bytes,
            generation: 0,
            extent: 0,
            payload: 0,
            active,
            bound: false,
        }
    }
    /// An 8.3 index name carries only forty bits. The caller supplies all
    /// thirty-two route bytes from its verified book digest instead.
    pub fn for_index(name:Option<&'n [u8]>,digest:&Route)->Self {
        let expected=crate::book_index::FileName::new(digest,crate::book_index::FileKind::Index);
        Self { name,route:*digest,generation:0,extent:0,payload:0,
            active:name.map(c_string)==Some(expected.as_bytes().as_slice()),bound:false }
    }
    pub fn raw(&self, from: &mut impl RawRead, offset: u32, out: &mut [u8]) -> bool {
        self.active
            && self.bound
            && offset <= self.extent
            && out.len() <= (self.extent - offset) as usize
            && from.read(self, offset, out)
    }
    pub fn finish(&mut self) {
        self.active = false;
        self.bound = false;
    }
}

/// Generation-bound raw reader plus bounded pause/cancellation poll used by
/// the 1 KiB streaming envelope verifier.
pub trait EnvelopeIo {
    fn read(&mut self, query: &Query<'_>, offset: u32, out: &mut [u8]) -> bool;
    fn poll(&mut self) -> bool {
        true
    }
}

/// Verify an envelope's manifest and block-CRC directory over a generation-bound
/// read lease using only a 1 KiB window (no 4 KiB block buffer or 4 KiB directory
/// buffer). Sets `query.payload` on success.
#[inline(never)]
pub fn stream_attach(
    query: &mut Query<'_>,
    window: &mut [u8; 1024],
    io: &mut dyn EnvelopeIo,
) -> bool {
    if !query.active || !query.bound || !io.poll() {
        return false;
    }
    let payload = payload_for_extent(query.extent);
    if payload == 0 {
        return false;
    }
    let dir_bytes = 4 * blocks(payload);
    if payload.checked_add(dir_bytes).and_then(|v| v.checked_add(MANIFEST_BYTES))
        != Some(query.extent)
    {
        return false;
    }
    let mut header = [0u8; MANIFEST_BYTES as usize];
    if !io.read(query, query.extent - MANIFEST_BYTES, &mut header) {
        return false;
    }
    let mut checked = 0u32;
    if !inspect_for(query.name, &header, query.extent, &query.route, &mut checked)
        || checked != payload
    {
        return false;
    }
    let expected_dir_crc = get32(&header, 20);
    let mut dir_crc = u32::MAX;
    let mut at = 0u32;
    while at < dir_bytes {
        if !io.poll() {
            return false;
        }
        let take = ((dir_bytes - at) as usize).min(window.len());
        if !io.read(query, payload + at, &mut window[..take]) {
            return false;
        }
        dir_crc = crc(dir_crc, &window[..take]);
        at += take as u32;
    }
    if dir_crc ^ u32::MAX != expected_dir_crc || !io.poll() {
        return false;
    }
    query.payload = payload;
    true
}

/// Stream every 4 KiB block overlapping `[offset .. offset + len)` through a
/// 1 KiB window, verifying each block's CRC against its directory entry.
/// `deliver` receives `(relative_offset_from_offset, slice)` for bytes inside
/// the requested range; callers must NOT act on delivered bytes until this
/// function returns `true` (every touched block verified).
#[inline(never)]
pub fn stream_verified_range(
    query: &Query<'_>,
    window: &mut [u8; 1024],
    offset: u32,
    len: u32,
    io: &mut dyn EnvelopeIo,
    deliver: &mut dyn FnMut(u32, &[u8]) -> bool,
) -> bool {
    if !query.active || !query.bound || query.payload == 0 {
        return false;
    }
    let Some(end) = offset.checked_add(len) else {
        return false;
    };
    if end > query.payload {
        return false;
    }
    if len == 0 {
        return io.poll();
    }
    let first_block = offset / BLOCK_BYTES;
    let last_block = (end - 1) / BLOCK_BYTES;
    for block in first_block..=last_block {
        if !io.poll() {
            return false;
        }
        let mut entry = [0u8; 4];
        if !io.read(query, query.payload + 4 * block, &mut entry) {
            return false;
        }
        let expected = u32::from_le_bytes(entry);
        let block_start = block * BLOCK_BYTES;
        let block_len = BLOCK_BYTES.min(query.payload - block_start);
        let mut bcrc = block_seed(&query.route, block, block_len);
        let mut pos = 0u32;
        while pos < block_len {
            if !io.poll() {
                return false;
            }
            let take = ((block_len - pos) as usize).min(window.len());
            if !io.read(query, block_start + pos, &mut window[..take]) {
                return false;
            }
            let chunk = &window[..take];
            bcrc = crc(bcrc, chunk);
            let chunk_start = block_start + pos;
            let chunk_end = chunk_start + take as u32;
            let ov_start = chunk_start.max(offset);
            let ov_end = chunk_end.min(end);
            if ov_start < ov_end {
                let lo = (ov_start - chunk_start) as usize;
                let hi = (ov_end - chunk_start) as usize;
                if !deliver(ov_start - offset, &chunk[lo..hi]) {
                    return false;
                }
            }
            pos += take as u32;
        }
        if bcrc ^ u32::MAX != expected {
            return false;
        }
    }
    io.poll()
}

/// Read an exact verified payload slice into `dst` via `stream_verified_range`.
#[inline(never)]
pub fn stream_verified_exact(
    query: &Query<'_>,
    window: &mut [u8; 1024],
    offset: u32,
    dst: &mut [u8],
    io: &mut dyn EnvelopeIo,
) -> bool {
    let Ok(len) = u32::try_from(dst.len()) else {
        return false;
    };
    let mut written = 0usize;
    let ok = stream_verified_range(
        query,
        window,
        offset,
        len,
        io,
        &mut |rel, slice| {
            let at = rel as usize;
            let Some(out) = dst.get_mut(at..at + slice.len()) else {
                return false;
            };
            out.copy_from_slice(slice);
            written += slice.len();
            true
        },
    );
    ok && written == dst.len()
}

/// Exactly the pinned directory and one block, borrowed from the parse loan.
/// The caller may reclaim both buffers after this store is dropped.
pub struct VerifiedStore<'n, 'a> {
    pub query: Query<'n>,
    directory: &'a mut [u8; (4 * MAX_BLOCKS + MANIFEST_BYTES) as usize],
    window: &'a mut [u8; BLOCK_BYTES as usize],
    loaded: u32,
    ready: bool,
}

impl<'n, 'a> VerifiedStore<'n, 'a> {
    pub fn new(
        name: Option<&'n [u8]>,
        directory: &'a mut [u8; 4160],
        window: &'a mut [u8; 4096],
    ) -> Self {
        Self {
            query: Query::new(name),
            directory,
            window,
            loaded: u32::MAX,
            ready: false,
        }
    }
    pub fn for_index(name:Option<&'n [u8]>,digest:&Route,
        directory:&'a mut [u8;4160],window:&'a mut [u8;4096])->Self {
        Self {query:Query::for_index(name,digest),directory,window,loaded:u32::MAX,ready:false}
    }
    pub fn attach(&mut self, from: &mut impl RawRead) -> bool {
        let payload = payload_for_extent(self.query.extent);
        if payload == 0 {
            return false;
        }
        let bytes = (4 * blocks(payload)) as usize;
        if !self
            .query
            .raw(from, payload, &mut self.directory[..bytes + 64])
        {
            return false;
        }
        let header: &[u8; 64] = self.directory[bytes..bytes + 64]
            .try_into()
            .expect("manifest extent");
        if !inspect_for(
            self.query.name,
            header,
            self.query.extent,
            &self.query.route,
            &mut self.query.payload,
        ) || crc(u32::MAX, &self.directory[..bytes]) ^ u32::MAX != get32(header, 20)
        {
            return false;
        }
        self.ready = true;
        true
    }
    pub fn file_size(&self, name: Option<&[u8]>) -> i64 {
        if self.ready
            && self.query.active
            && name.is_some()
            && name.map(c_string) == self.query.name.map(c_string)
        {
            i64::from(self.query.payload)
        } else {
            -1
        }
    }
    pub fn read_at(
        &mut self,
        from: &mut impl RawRead,
        name: Option<&[u8]>,
        mut offset: u32,
        out: &mut [u8],
    ) -> i32 {
        if self.file_size(name) < 0
            || offset > self.query.payload
            || out.len() > (self.query.payload - offset) as usize
        {
            return -1;
        }
        let requested = out.len();
        let mut dst = out;
        while !dst.is_empty() {
            let block = offset / BLOCK_BYTES;
            let start = block * BLOCK_BYTES;
            let length = BLOCK_BYTES.min(self.query.payload - start);
            if block != self.loaded {
                self.loaded = u32::MAX;
                if !self
                    .query
                    .raw(from, start, &mut self.window[..length as usize])
                {
                    self.ready = false;
                    return -1;
                }
                if block_crc(&self.query.route, block, &self.window[..length as usize])
                    != get32(self.directory, 4 * block as usize)
                {
                    self.ready = false;
                    return -1;
                }
                self.loaded = block;
            }
            let take = dst.len().min((length - (offset - start)) as usize);
            dst[..take].copy_from_slice(
                &self.window[(offset - start) as usize..(offset - start) as usize + take],
            );
            dst = &mut dst[take..];
            offset += take as u32;
        }
        requested as i32
    }
    pub fn remove(&self, _name: Option<&[u8]>) -> bool {
        false
    }
    pub fn begin_write(&self, _name: Option<&[u8]>) -> bool {
        false
    }
    pub fn write(&self, _bytes: &[u8]) -> bool {
        false
    }
    pub fn end_write(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod index_route_tests {
    use super::*;
    use crate::book_index::{FileName,FileKind};
    #[test]
    fn colliding_short_names_keep_distinct_full_digest_routes() {
        let a=[17;32];let mut b=a;b[31]^=1;
        let name=FileName::new(&a,FileKind::Index);
        assert_eq!(name,FileName::new(&b,FileKind::Index));
        let first=Query::for_index(Some(name.as_bytes()),&a);
        let second=Query::for_index(Some(name.as_bytes()),&b);
        assert!(first.active&&second.active);
        assert_eq!(first.route,a);assert_eq!(second.route,b);
        let mut footer=[0;64];manifest(&mut footer,1,&a,0,0);
        assert!(inspect(&footer,69,&first.route,&mut 0));
        assert!(!inspect(&footer,69,&second.route,&mut 0));
    }
    #[test]
    fn index_query_needs_the_canonical_name_for_its_digest() {
        let digest=[7;32];let name=FileName::new(&digest,FileKind::Index);
        assert!(Query::for_index(Some(name.as_bytes()),&digest).active);
        assert!(!Query::for_index(Some(name.with_kind(FileKind::Temporary).as_bytes()),&digest).active);
        assert!(!Query::for_index(Some(b"AAAAAAAA.IDX"),&digest).active);
        assert!(!Query::for_index(None,&digest).active);
    }
}
