//! v4 segments with a resume point, read through the verified envelope.
//! Page extents must come from the verified index and footer alone: a bad
//! point range costs only the pages it makes ambiguous, and a bad point
//! record costs only the point. A hit is always the page's exact record.
use newt_core::page_cache::*;
use newt_core::page_cache_access::VerifiedAccess;
use newt_core::page_envelope::{self, Query, RawRead, VerifiedStore, BLOCK_BYTES};
use newt_core::resume_point::{Layout, HEADER_BYTES};
use newt_core::zip_window::arena::Arena;

const HASH: u32 = 0x1234_5678;
const NAME: &[u8; 69] =
    b"00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff.fibp";

/// One page record with a single run whose text fills `text` bytes.
fn record(page: usize, text: usize) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&(100 * page as u32).to_le_bytes());
    b.extend_from_slice(&[1, 0, 0, 0, 0, 0]);
    b.extend_from_slice(&[0; 8]);
    b.extend_from_slice(&(text as u16).to_le_bytes());
    b.extend(vec![b'a' + page as u8; text]);
    b
}

/// A point whose layout is valid. Its sections are filler: R1 reads no content.
fn point_record(extra: u32) -> Vec<u8> {
    let mut b = b"FIRP\x01\x00\x01\x00".to_vec();
    for length in [1, 1, 1, 1 + extra, 0, 1, 0] {
        b.extend_from_slice(&length.to_le_bytes());
    }
    b.resize(HEADER_BYTES + 5 + extra as usize + 4, 0x5a);
    b
}

struct Segment {
    payload: Vec<u8>,
    records: Vec<Vec<u8>>,
    offsets: Vec<u32>,
    index: u32,
    point: (u32, u32),
}

impl Segment {
    /// `point` places its bytes after the given page and records their range.
    fn new(version: u16, partial: bool, pages: usize, point: Option<(usize, Vec<u8>)>) -> Self {
        let mut payload = b"FIBP".to_vec();
        payload.extend_from_slice(&version.to_le_bytes());
        payload.extend_from_slice(&[0, 0]);
        payload.extend_from_slice(&HASH.to_le_bytes());
        let (mut records, mut offsets, mut range) = (Vec::new(), Vec::new(), (0, 0));
        for page in 0..pages {
            offsets.push(payload.len() as u32);
            records.push(record(page, 1200));
            payload.extend_from_slice(records.last().unwrap());
            if let Some((_, bytes)) = point.as_ref().filter(|(after, _)| *after == page) {
                range = (payload.len() as u32, bytes.len() as u32);
                payload.extend_from_slice(bytes);
            }
        }
        let index = payload.len() as u32;
        let mut segment = Self { payload, records, offsets, index, point: range };
        segment.payload = segment.rebuilt(version, partial, &segment.offsets, range);
        segment
    }
    /// The same page bytes with another index and footer. Checksums stay valid.
    fn rebuilt(&self, version: u16, partial: bool, entries: &[u32], point: (u32, u32)) -> Vec<u8> {
        let mut payload = self.payload[..self.index as usize].to_vec();
        payload[4..6].copy_from_slice(&version.to_le_bytes());
        for (page, offset) in entries.iter().enumerate() {
            payload.extend_from_slice(&offset.to_le_bytes());
            payload.extend_from_slice(&(100 * page as u32).to_le_bytes());
        }
        let total = 100 * entries.len() as u32;
        let mut fields = vec![self.index, entries.len() as u32, payload.len() as u32, 0, total];
        if partial {
            fields.extend([total, total + 50]);
        }
        if version == VERSION_POINT {
            fields.extend([point.0, point.1]);
        }
        for value in fields {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.extend_from_slice(if partial { b"FIBx" } else { b"FIBX" });
        payload
    }
    fn with_footer(&self, point: (u32, u32)) -> Vec<u8> {
        self.rebuilt(VERSION_POINT, false, &self.offsets, point)
    }
    fn hits(&self) -> Vec<Result<Vec<u8>, Status>> {
        self.records.iter().cloned().map(Ok).collect()
    }
    fn all(&self) -> Vec<u32> {
        (0..self.records.len() as u32).collect()
    }
}

/// Payload, block directory and manifest, as the firmware stores them.
fn envelope(payload: &[u8]) -> Vec<u8> {
    let route = Query::new(Some(NAME)).route;
    let mut directory = Vec::new();
    for (block, bytes) in payload.chunks(BLOCK_BYTES as usize).enumerate() {
        directory.extend_from_slice(&page_envelope::block_crc(&route, block as u32, bytes).to_le_bytes());
    }
    let directory_crc = page_envelope::crc(u32::MAX, &directory) ^ u32::MAX;
    let mut manifest = [0; 64];
    page_envelope::manifest(&mut manifest, payload.len() as u32, &route, directory_crc, 0);
    [payload, &directory, &manifest].concat()
}

struct Raw(Vec<u8>);
impl RawRead for Raw {
    fn read(&mut self, _: &Query<'_>, offset: u32, out: &mut [u8]) -> bool {
        let at = offset as usize;
        self.0.get(at..at + out.len()).map(|bytes| out.copy_from_slice(bytes)).is_some()
    }
}

/// Attach the envelope and hand the verified storage to `use_it`.
fn verified<T>(file: &[u8], use_it: impl FnOnce(&mut VerifiedAccess<'_, '_, '_, Raw>) -> T) -> T {
    let mut raw = Raw(file.to_vec());
    let (mut directory, mut window) = ([0; 4160], [0; 4096]);
    let mut store = VerifiedStore::new(Some(NAME), &mut directory, &mut window);
    store.query.extent = file.len() as u32;
    store.query.bound = true;
    assert!(store.attach(&mut raw), "envelope must attach");
    use_it(&mut VerifiedAccess { store: &mut store, source: &mut raw })
}

#[derive(Debug, PartialEq)]
struct Outcome {
    point: Option<PointRange>,
    /// The point's bytes through the verified store, read after the pages.
    point_bytes: Option<Result<Vec<u8>, ()>>,
    pages: Vec<Result<Vec<u8>, Status>>,
}

/// Attach once and read the pages in order through the product reader.
fn read(file: &[u8], pages: &[u32]) -> Result<Outcome, Status> {
    verified(file, |access| {
        let (mut entries, mut anchors) = ([Entry::default(); 16], [Anchor::default(); 1]);
        let mut memory = ReaderMemory { pages: &mut entries, anchors: &mut anchors };
        let mut name = [0; 80];
        let mut names = Arena::new(&mut name);
        let mut reader = PageCacheReader::default();
        match reader.open(access, NAME, HASH, &mut names, &mut memory) {
            Status::Ok => {}
            status => return Err(status),
        }
        let mut results = Vec::new();
        for &page in pages {
            let mut buffer = [0; 8192];
            let mut scratch = Arena::new(&mut buffer);
            let mut runs = [Default::default(); 2];
            let mut out = PageMemory { runs: &mut runs, images: &mut [], links: &mut [] };
            results.push(
                reader
                    .read_page(access, &memory, &names, page, &mut scratch, &mut out)
                    .map(|page| page.bytes.to_vec()),
            );
        }
        let point = reader.point();
        let point_bytes = point.map(|p| {
            let mut bytes = vec![0; p.length as usize];
            (access.read_at(NAME, p.offset, &mut bytes) == p.length as i32).then_some(bytes).ok_or(())
        });
        Ok(Outcome { point, point_bytes, pages: results })
    })
}

fn layout(bytes: &[u8]) -> Option<Layout> {
    Layout::parse(bytes[..HEADER_BYTES].try_into().unwrap(), bytes.len() as u32)
}

#[test]
fn a_point_between_pages_or_next_to_the_index_ends_only_its_page() {
    for (partial, after) in [(false, 1), (true, 1), (false, 3), (true, 3)] {
        let segment = Segment::new(VERSION_POINT, partial, 4, Some((after, point_record(40))));
        let outcome = read(&envelope(&segment.payload), &segment.all()).unwrap();
        assert_eq!(outcome.pages, segment.hits(), "partial={partial} after={after}");
        let (offset, length) = segment.point;
        assert_eq!(outcome.point, Some(PointRange { offset, length }));
        let bytes = outcome.point_bytes.unwrap().unwrap();
        assert!(layout(&bytes).is_some());
    }
}

#[test]
fn a_valid_range_around_a_bad_point_record_loses_only_the_point() {
    let mut bad = point_record(40);
    bad[0] = b'X';
    let segment = Segment::new(VERSION_POINT, false, 4, Some((1, bad)));
    let outcome = read(&envelope(&segment.payload), &segment.all()).unwrap();
    assert_eq!(outcome.pages, segment.hits());
    assert_eq!(layout(&outcome.point_bytes.unwrap().unwrap()), None);
}

/// Footer ranges around a point stored after page 1, and the pages that must
/// miss. Page 1 always misses: either the range makes it ambiguous, or its
/// extent keeps the point's bytes and so is not exactly its record.
fn bad_ranges(segment: &Segment) -> Vec<((u32, u32), Vec<usize>)> {
    let (offset, length) = segment.point;
    let [first, second, ..] = segment.offsets[..] else { unreachable!() };
    vec![
        ((offset, 0), vec![1]),
        ((offset - 5, length), vec![1]),
        ((offset, length + 10), vec![1, 2]),
        ((second, offset + length - second), vec![1]),
        ((offset + length, 4), vec![1, 2]),
        ((segment.index - 4, 8), vec![1, 3]),
        ((4, first - 4), vec![1]),
        ((u32::MAX - 3, 8), vec![1]),
        ((offset + 1, u32::MAX), vec![1, 2, 3]),
    ]
}

#[test]
fn bad_point_ranges_miss_only_the_pages_they_affect() {
    let segment = Segment::new(VERSION_POINT, false, 4, Some((1, point_record(40))));
    for (range, misses) in bad_ranges(&segment) {
        let outcome = read(&envelope(&segment.with_footer(range)), &segment.all()).unwrap();
        let expected: Vec<_> = (0..4)
            .map(|page| if misses.contains(&page) { Err(Status::Stale) } else { Ok(segment.records[page].clone()) })
            .collect();
        assert_eq!(outcome.pages, expected, "range {range:?}");
        assert_eq!(outcome.point, None, "range {range:?}");
    }
}

#[test]
fn an_unordered_index_gives_the_point_no_owner() {
    // Pages 0 and 2 share one gap that the point ends, so neither owns it.
    // Truncating either at the point would yield page 0's exact record.
    let segment = Segment::new(VERSION_POINT, false, 5, Some((0, point_record(40))));
    let o = &segment.offsets;
    let entries = [o[0], o[1], o[0], o[1], o[4]];
    let file = envelope(&segment.rebuilt(VERSION_POINT, false, &entries, segment.point));
    let outcome = read(&file, &[0, 1, 2, 3, 4]).unwrap();
    let stale = Err(Status::Stale);
    let hit = Ok(segment.records[4].clone());
    assert_eq!(outcome.pages, vec![stale.clone(), stale.clone(), stale.clone(), stale, hit]);
    assert_eq!(outcome.point, None);
}

#[test]
fn overlapping_or_header_gaps_miss_and_disjoint_pages_still_hit() {
    let stale = || Err(Status::Stale);
    let segment = Segment::new(VERSION_POINT, false, 4, Some((3, point_record(40))));
    let [a, b, c, d] = segment.offsets[..] else { unreachable!() };
    let hit = |page: usize| Ok(segment.records[page].clone());
    // Logical pages 0 and 2 share one gap and page 1 runs backwards. Pages 3..5
    // are disjoint, and page 5 owns the point, so they still hit.
    let file = envelope(&segment.rebuilt(VERSION_POINT, false, &[a, b, a, b, c, d], segment.point));
    let outcome = read(&file, &[0, 1, 2, 3, 4, 5]).unwrap();
    assert_eq!(outcome.pages, vec![stale(), stale(), stale(), hit(1), hit(2), hit(3)]);
    let (offset, length) = segment.point;
    assert_eq!(outcome.point, Some(PointRange { offset, length }));
    // A gap that starts in the header misses and cannot own a point there.
    let plain = Segment::new(VERSION_POINT, false, 4, None);
    let file = envelope(&plain.rebuilt(VERSION_POINT, false, &[0, a, b, c, d], (4, a - 4)));
    let outcome = read(&file, &[0, 1, 2, 3, 4]).unwrap();
    assert_eq!(outcome.pages, vec![stale(), hit(0), hit(1), hit(2), hit(3)]);
    assert_eq!(outcome.point, None);
    // Empty and backwards gaps inside the point hold no bytes, so they miss
    // without taking the point from page 2, which still owns it.
    let segment = Segment::new(VERSION_POINT, false, 4, Some((2, point_record(40))));
    let [_, _, c, d] = segment.offsets[..] else { unreachable!() };
    let inside = segment.point.0 + 1;
    let file = envelope(&segment.rebuilt(VERSION_POINT, false, &[inside, inside, c, d], segment.point));
    let outcome = read(&file, &[0, 1, 2, 3]).unwrap();
    let hit = |page: usize| Ok(segment.records[page].clone());
    assert_eq!(outcome.pages, vec![stale(), stale(), hit(2), hit(3)]);
    let (offset, length) = segment.point;
    assert_eq!(outcome.point, Some(PointRange { offset, length }));
}

/// Negative control: deciding ownership from page 2's gap alone, without the
/// index-wide checks, ends page 2 at the point. The real decoder then returns
/// page 0's complete record as page 2; the product reader misses instead.
#[test]
fn a_reader_deciding_ownership_per_page_serves_a_wrong_page() {
    let segment = Segment::new(VERSION_POINT, false, 5, Some((0, point_record(40))));
    let o = &segment.offsets;
    let entries = [o[0], o[1], o[0], o[1], o[4]];
    let file = envelope(&segment.rebuilt(VERSION_POINT, false, &entries, segment.point));
    let outcome = read(&file, &[2, 4]).unwrap();
    assert_eq!(outcome.pages, vec![Err(Status::Stale), Ok(segment.records[4].clone())]);
    let (offset, length) = segment.point;
    let local = verified(&file, |access| {
        // Page 2's gap is entries[2]..entries[3]; the point fills its tail.
        let (start, end) = (entries[2], entries[3]);
        assert!(offset > start && offset + length == end);
        let mut bytes = vec![0; (offset - start) as usize];
        assert_eq!(access.read_at(NAME, start, &mut bytes), bytes.len() as i32);
        bytes
    });
    let mut buffer = [0; 8192];
    let mut arena = Arena::new(&mut buffer);
    let mut runs = [Default::default(); 2];
    let mut out = PageMemory { runs: &mut runs, images: &mut [], links: &mut [] };
    let page = decode_record(&local, 2, &mut arena, &mut out).unwrap();
    assert_eq!(page.bytes, &segment.records[0][..]);
    assert_eq!(page.char_start, 0, "page 2 should start at 200 chars");
}

#[test]
fn a_bad_block_shared_by_the_point_and_a_page_misses_only_its_reads() {
    // Page 2 ends in block 0 and its point crosses into block 1, where page 3 starts.
    let segment = Segment::new(VERSION_POINT, false, 8, Some((2, point_record(600))));
    let (offset, length) = segment.point;
    assert!(offset < BLOCK_BYTES && offset + length > BLOCK_BYTES);
    assert!(segment.offsets[3] < 2 * BLOCK_BYTES && segment.offsets[7] >= 2 * BLOCK_BYTES);
    assert!(segment.index >= 2 * BLOCK_BYTES);
    let mut file = envelope(&segment.payload);
    file[BLOCK_BYTES as usize + 1000] ^= 1;
    let hit = |page: usize| Ok(segment.records[page].clone());
    // Pages wholly in good blocks hit, including the page the point ends.
    let outcome = read(&file, &[0, 2, 7]).unwrap();
    assert_eq!(outcome.pages, vec![hit(0), hit(2), hit(7)]);
    // The point's range is still structural; its bytes fail verification.
    assert_eq!(outcome.point, Some(PointRange { offset, length }));
    assert_eq!(outcome.point_bytes, Some(Err(())));
    // A page in the bad block misses, and the failed attach stays failed.
    let outcome = read(&file, &[3, 0]).unwrap();
    assert_eq!(outcome.pages, vec![Err(Status::IoError), Err(Status::IoError)]);
    // A fresh attach hits again outside the bad block.
    assert_eq!(read(&file, &[7]).unwrap().pages, vec![hit(7)]);
}

#[test]
fn v3_segments_still_hit_and_unknown_versions_miss() {
    for partial in [false, true] {
        let segment = Segment::new(VERSION_PAGES, partial, 4, None);
        let outcome = read(&envelope(&segment.payload), &segment.all()).unwrap();
        assert_eq!(outcome.pages, segment.hits());
        assert_eq!(outcome.point, None);
        // One index entry, so page 0's gap also holds pages 1..3. v3 keeps the
        // SDK's tolerance of trailing bytes; v4 requires the exact record.
        let one = &segment.offsets[..1];
        let page = read(&envelope(&segment.rebuilt(VERSION_PAGES, partial, one, (0, 0))), &[0]).unwrap();
        assert!(page.pages[0].as_ref().unwrap().starts_with(&segment.records[0]));
        let page = read(&envelope(&segment.rebuilt(VERSION_POINT, partial, one, (0, 0))), &[0]).unwrap();
        assert_eq!(page.pages, vec![Err(Status::Stale)]);
        // A v4 reader must not find point fields in a v3 footer.
        let mut v4 = segment.payload.clone();
        v4[4] = VERSION_POINT as u8;
        assert_eq!(read(&envelope(&v4), &[0]), Err(Status::Stale));
        let mut v5 = segment.payload.clone();
        v5[4] = 5;
        assert_eq!(read(&envelope(&v5), &[0]), Err(Status::Stale));
    }
}
