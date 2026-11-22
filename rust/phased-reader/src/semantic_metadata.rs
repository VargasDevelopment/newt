//! MIT; faithful pinned ReaderSemanticMetadata source port.
//! No retained storage reference, index, title, count table or allocation.
#![forbid(unsafe_code)]
use crate::SemanticProjection;
use newt_core::{
    book_semantic_index::{Digest, Index, MAX_SPINE_ITEMS, POLICY_VERSION},
    page_cache::CacheStorage,
    utf8,
};
pub type Title = [u8; 256];
pub type Key = [u8; 123];
pub const HEADER_BYTES: u32 = 328;
pub const READ_ATTEMPTS: u32 = 32 + (MAX_SPINE_ITEMS as u32 + 7) / 8;
pub const SDK_COMMIT: &[u8; 40] = b"cd6f5b831c8fad899723b6d406586f00c575f021";
const SDK: [u8; 20] = [
    0xcd, 0x6f, 0x5b, 0x83, 0x1c, 0x8f, 0xad, 0x89, 0x97, 0x23, 0xb6, 0xd4, 0x06, 0x58, 0x6f, 0x00,
    0xc5, 0x75, 0xf0, 0x21,
];
pub struct ReaderSemanticMetadata;
/// Count-only closed-Home projection; it cannot be applied as reader authority.
#[derive(Default)]
pub struct PositionProjection {
    pub canonical_spines: u32,
    pub chapter_chars: u32,
    pub prefix: u64,
    pub total: u64,
}
#[derive(Default)]
struct Header {
    spines: u32,
    title_length: u32,
}
fn unknown() -> SemanticProjection {
    SemanticProjection {
        digest: [0; 32],
        canonical_spines: 0,
        selected_spine: 0,
        chapter_chars: 0,
        prefix: 0,
        total: 0,
    }
}
pub fn make_key(digest: &Digest, key: &mut Key) -> bool {
    let hex = b"0123456789abcdef";
    key[..3].copy_from_slice(b"sm-");
    for (i, b) in digest.iter().enumerate() {
        key[3 + 2 * i] = hex[(b >> 4) as usize];
        key[4 + 2 * i] = hex[(b & 15) as usize];
    }
    key[67] = b'-';
    key[68..108].copy_from_slice(SDK_COMMIT);
    key[108] = b'-';
    for i in 0..8 {
        key[109 + i] = hex[((POLICY_VERSION >> (28 - 4 * i)) & 15) as usize];
    }
    key[117..].copy_from_slice(b".ntsm\0");
    true
}
pub fn invalidate(title: &mut Title, index: &mut Index) {
    title.fill(0);
    index.invalidate();
}
fn valid_title(title: &[u8]) -> bool {
    let mut at = 0;
    let mut scalar = 0;
    while at < title.len() {
        if !utf8::next(Some(title), &mut at, &mut scalar) || scalar == 0 {
            return false;
        }
    }
    true
}
fn get32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
fn read(s: &mut impl CacheStorage, key: &Key, offset: u32, out: &mut [u8]) -> bool {
    s.read_at(&key[..122], offset, out) == out.len() as i32
}
fn binding(
    s: &mut impl CacheStorage,
    d: &Digest,
    k: &Key,
    h: &mut Header,
    b: &mut [u8; 32],
) -> bool {
    if !read(s, k, 0, &mut b[..16]) || &b[..8] != b"NTSM0001" || get32(&b[8..]) != POLICY_VERSION {
        return false;
    }
    h.spines = get32(&b[12..]);
    if !read(s, k, 16, b)
        || b != d
        || !read(s, k, 48, &mut b[..20])
        || b[..20] != SDK
        || !read(s, k, 68, &mut b[..4])
        || b[2] != 0
        || b[3] != 0
    {
        return false;
    }
    h.title_length = u32::from(b[0]) | (u32::from(b[1]) << 8);
    true
}
#[inline(never)]
fn begin(
    s: &mut impl CacheStorage,
    d: &Digest,
    canonical: u32,
    t: &mut Title,
    k: &mut Key,
    h: &mut Header,
    b: &mut [u8; 32],
    preserve: bool,
) -> bool {
    make_key(d, k);
    let bytes = s.file_size(&k[..122]);
    if bytes < i64::from(HEADER_BYTES)
        || bytes > i64::from(HEADER_BYTES) + 4 * MAX_SPINE_ITEMS as i64
        || !binding(s, d, k, h, b)
    {
        return false;
    }
    if h.spines > MAX_SPINE_ITEMS as u32
        || (h.spines != 0 && canonical != 0 && h.spines != canonical)
        || bytes != i64::from(HEADER_BYTES) + 4 * i64::from(h.spines)
        || h.title_length >= 256
    {
        return false;
    }
    if preserve {
        for offset in (0..256).step_by(32) {
            if !read(s, k, 72 + offset as u32, b) || b[..] != t[offset..offset + 32] {
                return false;
            }
        }
    } else if !read(s, k, 72, t) {
        return false;
    }
    valid_title(&t[..h.title_length as usize])
        && t[h.title_length as usize..].iter().all(|v| *v == 0)
}
fn count(
    s: &mut impl CacheStorage,
    k: &Key,
    spine: u32,
    spines: u32,
    b: &mut [u8; 32],
) -> Option<u32> {
    if spine >= spines {
        return None;
    }
    if spine % 8 == 0 {
        let take = 8.min(spines - spine) as usize * 4;
        if !read(s, k, HEADER_BYTES + 4 * spine, &mut b[..take]) {
            return None;
        }
    }
    Some(get32(&b[(spine % 8) as usize * 4..]))
}
fn finish(s: &mut impl CacheStorage, d: &Digest, k: &Key, h: &Header, b: &mut [u8; 32]) -> bool {
    let mut checked = Header::default();
    if !binding(s, d, k, &mut checked, b)
        || checked.spines != h.spines
        || checked.title_length != h.title_length
    {
        return false;
    }
    read(s, k, 0, &mut [])
}
impl ReaderSemanticMetadata {
    /// `title` models a readable nullable C string, including its NUL. Rust
    /// requires a terminator inside the supplied slice and first 256 bytes.
    pub fn save(
        s: &mut impl CacheStorage,
        d: &Digest,
        canonical: u32,
        title: Option<&[u8]>,
        index: Option<&Index>,
    ) -> bool {
        let Some(title) = title else {
            return false;
        };
        let Some(length) = title.iter().take(256).position(|v| *v == 0) else {
            return false;
        };
        if !valid_title(&title[..length]) {
            return false;
        }
        let mut spines = 0;
        if let Some(index) = index.filter(|i| i.complete(d, POLICY_VERSION)) {
            if canonical == 0
                || canonical > MAX_SPINE_ITEMS as u32
                || index.chapter_count(d, POLICY_VERSION, canonical).is_some()
            {
                return false;
            }
            for i in 0..canonical {
                if index.chapter_count(d, POLICY_VERSION, i).is_none() {
                    return false;
                }
            }
            spines = canonical;
        }
        let mut key = [0; 123];
        make_key(d, &mut key);
        let mut prefix = [0; 72];
        prefix[..8].copy_from_slice(b"NTSM0001");
        prefix[8..12].copy_from_slice(&POLICY_VERSION.to_le_bytes());
        prefix[12..16].copy_from_slice(&spines.to_le_bytes());
        prefix[16..48].copy_from_slice(d);
        prefix[48..68].copy_from_slice(&SDK);
        prefix[68..70].copy_from_slice(&(length as u16).to_le_bytes());
        if !s.begin_write(&key[..122]) {
            s.remove(&key[..122]);
            return false;
        }
        let mut good = s.write(&prefix) && s.write(&title[..length]);
        prefix.fill(0);
        let mut remaining = 256 - length;
        while good && remaining != 0 {
            let take = remaining.min(72);
            good = s.write(&prefix[..take]);
            remaining -= take;
        }
        let mut word = [0; 4];
        for spine in 0..spines {
            if !good {
                break;
            }
            let chars = index.and_then(|i| i.chapter_count(d, POLICY_VERSION, spine));
            good = chars.is_some();
            word.copy_from_slice(&chars.unwrap_or(0).to_le_bytes());
            if good {
                good = s.write(&word);
            }
        }
        if good {
            good = s.end_write();
        }
        if !good {
            s.remove(&key[..122]);
        }
        good
    }
    pub fn load(
        s: &mut impl CacheStorage,
        d: &Digest,
        canonical: u32,
        t: &mut Title,
        index: &mut Index,
    ) -> bool {
        invalidate(t, index);
        let mut key = [0; 123];
        let mut h = Header::default();
        let mut b = [0; 32];
        let mut good = begin(s, d, canonical, t, &mut key, &mut h, &mut b, false);
        if good && h.spines != 0 {
            good = index.bind(d, POLICY_VERSION, h.spines);
        }
        for spine in 0..h.spines {
            if !good {
                break;
            }
            good = count(s, &key, spine, h.spines, &mut b)
                .is_some_and(|chars| index.record(d, POLICY_VERSION, spine, chars));
        }
        good = good
            && (h.spines == 0 || index.complete(d, POLICY_VERSION))
            && finish(s, d, &key, &h, &mut b);
        if !good {
            invalidate(t, index);
        }
        good
    }
    #[inline(never)]
    pub fn load_projection(
        s: &mut impl CacheStorage,
        d: &Digest,
        canonical: u32,
        selected: u32,
        t: &mut Title,
        p: &mut SemanticProjection,
    ) -> bool {
        t.fill(0);
        *p = unknown();
        let mut position = PositionProjection::default();
        if !Self::projection_record(s, d, canonical, selected, t, &mut position, false) {
            t.fill(0);
            return false;
        }
        p.canonical_spines = position.canonical_spines;
        p.chapter_chars = position.chapter_chars;
        p.prefix = position.prefix;
        p.total = position.total;
        if p.canonical_spines != 0 {
            p.digest = *d;
            p.selected_spine = selected;
        }
        true
    }
    /// Position-only output has no digest or selected-spine authority fields.
    #[inline(never)]
    pub fn load_position(
        s: &mut impl CacheStorage,
        d: &Digest,
        canonical: u32,
        selected: u32,
        t: &mut Title,
        p: &mut PositionProjection,
    ) -> bool {
        let had_title = t[0] != 0;
        *p = PositionProjection::default();
        if !Self::projection_record(s, d, canonical, selected, t, p, had_title) {
            if !had_title {
                t.fill(0);
            }
            *p = PositionProjection::default();
            return false;
        }
        true
    }
    fn projection_record(
        s: &mut impl CacheStorage,
        d: &Digest,
        canonical: u32,
        selected: u32,
        t: &mut Title,
        p: &mut PositionProjection,
        preserve: bool,
    ) -> bool {
        let mut key = [0; 123];
        let mut h = Header::default();
        let mut b = [0; 32];
        if !begin(s, d, canonical, t, &mut key, &mut h, &mut b, preserve)
            || (h.spines != 0 && selected >= h.spines)
        {
            return false;
        }
        p.canonical_spines = h.spines;
        for spine in 0..h.spines {
            let Some(chars) = count(s, &key, spine, h.spines, &mut b) else {
                return false;
            };
            if spine < selected {
                p.prefix += u64::from(chars);
            }
            if spine == selected {
                p.chapter_chars = chars;
            }
            p.total += u64::from(chars);
        }
        finish(s, d, &key, &h, &mut b)
    }
}
