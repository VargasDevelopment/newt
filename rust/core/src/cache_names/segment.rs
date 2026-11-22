use super::Digest;
pub const PAGES: u32 = 128;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChapterSegment {
    pub index: u32,
    pub pages: u32,
    pub chars: u32,
}
impl ChapterSegment {
    pub fn count(pages: u32) -> u32 {
        if pages > PAGES {
            pages.wrapping_add(PAGES - 1) / PAGES
        } else {
            0
        }
    }
    pub fn first(self) -> u32 {
        self.index.wrapping_mul(PAGES)
    }
    pub fn file_pages(self) -> u32 {
        PAGES.min(self.pages.wrapping_sub(self.first()))
    }
}
/// Provider is borrowed for this operation only. Input is exactly 76 bytes;
/// failure leaves all 128 name bytes unchanged. Success rewrites bytes 0..70.
pub fn segment_name(
    segment: ChapterSegment,
    name: &mut [u8; 128],
    sha: &mut impl FnMut(&[u8], &mut Digest) -> bool,
) -> bool {
    let mut encoded = [0u8; 76];
    encoded[..64].copy_from_slice(&name[..64]);
    for (i, v) in [segment.index, segment.pages, segment.chars]
        .iter()
        .enumerate()
    {
        encoded[64 + i * 4..68 + i * 4].copy_from_slice(&v.to_le_bytes());
    }
    let mut key = [0; 32];
    if !sha(&encoded, &mut key) {
        return false;
    }
    for (i, b) in key.iter().enumerate() {
        name[i * 2] = b"0123456789abcdef"[(b >> 4) as usize];
        name[i * 2 + 1] = b"0123456789abcdef"[(b & 15) as usize];
    }
    name[64..70].copy_from_slice(b".fibp\0");
    true
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SegmentAnchor {
    pub segment: ChapterSegment,
    pub start: u32,
}
pub const MAP_INDEX: u32 = u32::MAX;
pub const MAP_VERSION: u32 = 1;
pub const MAX_SEGMENTS: usize = 32;
pub const MAX_BYTES: usize = 156;
pub fn map_name(chapter: &mut [u8; 128], sha: &mut impl FnMut(&[u8], &mut Digest) -> bool) -> bool {
    segment_name(
        ChapterSegment {
            index: MAP_INDEX,
            pages: 0,
            chars: 0,
        },
        chapter,
        sha,
    )
}
fn get(p: &[u8]) -> u32 {
    u32::from_le_bytes([p[0], p[1], p[2], p[3]])
}
fn put(p: &mut [u8], value: u32) {
    p[..4].copy_from_slice(&value.to_le_bytes());
}
fn check(input: &[u8], sha: &mut impl FnMut(&[u8], &mut Digest) -> bool) -> Option<u32> {
    let mut digest = [0; 32];
    if sha(input, &mut digest) {
        Some(get(&digest))
    } else {
        None
    }
}
/// Invalid count leaves output untouched. SHA failure leaves the written header
/// and starts in place, with the old checksum/tail, exactly as the original.
pub fn encode(
    out: &mut [u8; MAX_BYTES],
    generation: u32,
    pages: u32,
    chars: u32,
    starts: &[u32],
    sha: &mut impl FnMut(&[u8], &mut Digest) -> bool,
) -> u32 {
    let count = ChapterSegment::count(pages) as usize;
    if count == 0 || count > MAX_SEGMENTS || starts.len() < count {
        return 0;
    }
    out[..4].copy_from_slice(b"NTSM");
    for (i, v) in [MAP_VERSION, generation, pages, chars, count as u32]
        .iter()
        .enumerate()
    {
        put(&mut out[4 + i * 4..], *v);
    }
    for (k, v) in starts[..count].iter().enumerate() {
        put(&mut out[24 + k * 4..], *v);
    }
    let bytes = 24 + count * 4;
    let Some(sum) = check(&out[..bytes], sha) else {
        return 0;
    };
    put(&mut out[bytes..], sum);
    (bytes + 4) as u32
}
/// All failures preserve out. Target beyond chars and nonzero first starts are
/// accepted; equal starts choose the last. Count zero also follows the original.
pub fn find(
    input: &[u8],
    generation: u32,
    target: u32,
    out: &mut SegmentAnchor,
    sha: &mut impl FnMut(&[u8], &mut Digest) -> bool,
) -> bool {
    let bytes = input.len();
    if !(28..=MAX_BYTES).contains(&bytes)
        || &input[..4] != b"NTSM"
        || get(&input[4..]) != MAP_VERSION
        || get(&input[8..]) != generation
    {
        return false;
    }
    let (pages, chars, count) = (get(&input[12..]), get(&input[16..]), get(&input[20..]));
    if count != ChapterSegment::count(pages) || bytes != 28 + count as usize * 4 {
        return false;
    }
    let Some(sum) = check(&input[..bytes - 4], sha) else {
        return false;
    };
    if sum != get(&input[bytes - 4..]) {
        return false;
    }
    let mut k = 0;
    for i in 0..count as usize {
        let start = get(&input[24 + i * 4..]);
        if start > chars || (i != 0 && start < get(&input[20 + i * 4..])) {
            return false;
        }
        if start <= target {
            k = i;
        }
    }
    *out = SegmentAnchor {
        segment: ChapterSegment {
            index: k as u32,
            pages,
            chars,
        },
        start: get(&input[24 + k * 4..]),
    };
    true
}
