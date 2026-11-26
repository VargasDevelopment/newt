//! v4 segments with a resume point, read through the verified envelope.
//! Page extents must come from the verified index and footer alone: a bad
//! point range costs only the pages it makes ambiguous, and a bad point
//! record costs only the point.
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
        for (page, offset) in offsets.iter().enumerate() {
            payload.extend_from_slice(&offset.to_le_bytes());
            payload.extend_from_slice(&(100 * page as u32).to_le_bytes());
        }
        let mut segment = Self { payload, records, offsets, index, point: range };
        segment.footer(version, partial, range);
        segment
    }
    fn footer(&mut self, version: u16, partial: bool, point: (u32, u32)) {
        let anchors = self.payload.len() as u32;
        let total = 100 * self.records.len() as u32;
        let mut fields = vec![self.index, self.records.len() as u32, anchors, 0, total];
        if partial {
            fields.extend([total, total + 50]);
        }
        if version == VERSION_POINT {
            fields.extend([point.0, point.1]);
        }
        for value in fields {
            self.payload.extend_from_slice(&value.to_le_bytes());
        }
        self.payload.extend_from_slice(if partial { b"FIBx" } else { b"FIBX" });
    }
    /// The same pages with another footer range (whole v4 only).
    fn with_footer(&self, point: (u32, u32)) -> Vec<u8> {
        let mut other = Self { payload: self.payload[..self.index as usize + 8 * self.records.len()].to_vec(),
            records: self.records.clone(), offsets: self.offsets.clone(), index: self.index, point };
        other.footer(VERSION_POINT, false, point);
        other.payload
    }
    /// The bytes from a page's entry to the next entry or the index.
    fn gap(&self, page: usize) -> &[u8] {
        let end = self.offsets.get(page + 1).copied().unwrap_or(self.index);
        &self.payload[self.offsets[page] as usize..end as usize]
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

#[derive(Debug, PartialEq)]
struct Outcome {
    point: Option<PointRange>,
    /// Whether the point's bytes pass block verification, after the pages.
    point_readable: Option<bool>,
    pages: Vec<Result<Vec<u8>, Status>>,
}

/// Attach once and read the pages in order through the product reader.
fn read(file: &[u8], pages: &[u32]) -> Result<Outcome, Status> {
    let mut raw = Raw(file.to_vec());
    let (mut directory, mut window) = ([0; 4160], [0; 4096]);
    let mut store = VerifiedStore::new(Some(NAME), &mut directory, &mut window);
    store.query.extent = file.len() as u32;
    store.query.bound = true;
    assert!(store.attach(&mut raw), "envelope must attach");
    let mut access = VerifiedAccess { store: &mut store, source: &mut raw };
    let (mut entries, mut anchors) = ([Entry::default(); 16], [Anchor::default(); 1]);
    let mut memory = ReaderMemory { pages: &mut entries, anchors: &mut anchors };
    let mut name = [0; 80];
    let mut names = Arena::new(&mut name);
    let mut reader = PageCacheReader::default();
    match reader.open(&mut access, NAME, HASH, &mut names, &mut memory) {
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
                .read_page(&mut access, &memory, &names, page, &mut scratch, &mut out)
                .map(|page| page.bytes.to_vec()),
        );
    }
    let point = reader.point(&memory);
    let point_readable = point.map(|p| {
        let mut bytes = vec![0; p.length as usize];
        access.read_at(NAME, p.offset, &mut bytes) == p.length as i32
    });
    Ok(Outcome { point, point_readable, pages: results })
}

fn all(segment: &Segment) -> Vec<u32> {
    (0..segment.records.len() as u32).collect()
}

#[test]
fn a_point_between_pages_or_next_to_the_index_ends_only_its_page() {
    for (partial, after) in [(false, 1), (true, 1), (false, 3), (true, 3)] {
        let segment = Segment::new(VERSION_POINT, partial, 4, Some((after, point_record(40))));
        let outcome = read(&envelope(&segment.payload), &all(&segment)).unwrap();
        let expected: Vec<_> = segment.records.iter().cloned().map(Ok).collect();
        assert_eq!(outcome.pages, expected, "partial={partial} after={after}");
        let (offset, length) = segment.point;
        assert_eq!(outcome.point, Some(PointRange { offset, length }));
        assert_eq!(outcome.point_readable, Some(true));
        // The layout check runs on the point's own bytes.
        let at = offset as usize;
        let header: &[u8; HEADER_BYTES] = segment.payload[at..at + HEADER_BYTES].try_into().unwrap();
        assert!(Layout::parse(header, length).is_some());
    }
}

#[test]
fn a_valid_range_around_a_bad_point_record_loses_only_the_point() {
    let mut bad = point_record(40);
    bad[0] = b'X';
    let segment = Segment::new(VERSION_POINT, false, 4, Some((1, bad)));
    let outcome = read(&envelope(&segment.payload), &all(&segment)).unwrap();
    assert_eq!(outcome.pages, segment.records.iter().cloned().map(Ok).collect::<Vec<_>>());
    let point = outcome.point.unwrap();
    let at = point.offset as usize;
    let header: &[u8; HEADER_BYTES] = segment.payload[at..at + HEADER_BYTES].try_into().unwrap();
    assert_eq!(Layout::parse(header, point.length), None);
}

/// Each footer range, the pages it makes ambiguous, and nothing else.
fn bad_ranges(segment: &Segment) -> Vec<((u32, u32), Vec<usize>)> {
    let (offset, length) = segment.point;
    let [first, second, ..] = segment.offsets[..] else { unreachable!() };
    vec![
        ((offset, 0), vec![]),
        ((offset - 5, length), vec![1]),
        ((offset, length + 10), vec![1, 2]),
        ((second, offset + length - second), vec![1]),
        ((offset + length, 4), vec![2]),
        ((segment.index - 4, 8), vec![3]),
        ((4, first - 4), vec![]),
        ((u32::MAX - 3, 8), vec![]),
        ((offset + 1, u32::MAX), vec![1, 2, 3]),
    ]
}

/// Ambiguous pages miss as stale; every other page returns its whole gap.
fn check_bad_ranges(read: impl Fn(&[u8], &[u32]) -> Result<Outcome, Status>) -> Result<(), String> {
    let segment = Segment::new(VERSION_POINT, false, 4, Some((1, point_record(40))));
    for (range, ambiguous) in bad_ranges(&segment) {
        let outcome = read(&envelope(&segment.with_footer(range)), &all(&segment)).map_err(|s| format!("{s:?}"))?;
        let expected: Vec<_> = (0..segment.records.len())
            .map(|page| if ambiguous.contains(&page) { Err(Status::Stale) } else { Ok(segment.gap(page).to_vec()) })
            .collect();
        if outcome.pages != expected || outcome.point.is_some() {
            return Err(format!("range {range:?}: {:?}", outcome.pages.iter().map(|p| p.as_ref().map(Vec::len)).collect::<Vec<_>>()));
        }
    }
    Ok(())
}

#[test]
fn bad_point_ranges_miss_only_the_pages_they_make_ambiguous() {
    check_bad_ranges(read).unwrap();
}

/// Negative control: a reader that ends a page wherever the footer's point
/// starts serves a truncated page instead of a miss.
#[test]
fn a_reader_trusting_the_unverified_range_fails_the_check() {
    let trusting = |file: &[u8], pages: &[u32]| -> Result<Outcome, Status> {
        let mut outcome = read(file, pages)?;
        let payload = &file[..page_envelope::payload_for_extent(file.len() as u32) as usize];
        let field = |from_end: usize| {
            let at = payload.len() - from_end;
            u32::from_le_bytes(payload[at..at + 4].try_into().unwrap())
        };
        let (index, point) = (field(32) as usize, field(12) as usize);
        for (page, result) in outcome.pages.iter_mut().enumerate() {
            let entry = |page: usize| u32::from_le_bytes(payload[index + 8 * page..][..4].try_into().unwrap()) as usize;
            let start = entry(page);
            let end = if page + 1 < pages.len() { entry(page + 1) } else { index };
            let end = if (start..end).contains(&point) && point > start { point } else { end };
            *result = Ok(payload[start..end].to_vec());
        }
        Ok(outcome)
    };
    assert!(check_bad_ranges(trusting).is_err());
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
    assert_eq!(outcome.point_readable, Some(false));
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
        let outcome = read(&envelope(&segment.payload), &all(&segment)).unwrap();
        assert_eq!(outcome.pages, segment.records.iter().cloned().map(Ok).collect::<Vec<_>>());
        assert_eq!(outcome.point, None);
        // A v4 reader must not find point fields in a v3 footer.
        let mut v4 = segment.payload.clone();
        v4[4] = VERSION_POINT as u8;
        assert_eq!(read(&envelope(&v4), &[0]), Err(Status::Stale));
        let mut v5 = segment.payload.clone();
        v5[4] = 5;
        assert_eq!(read(&envelope(&v5), &[0]), Err(Status::Stale));
    }
}
