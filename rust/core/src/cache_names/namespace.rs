use super::{cstr, identity};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Unknown,
    Page,
    /// A book index (`.IDX`): block envelope like a page, in its own folder.
    Index,
    Semantic,
    Progress,
    Identity,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Form {
    #[default]
    Canonical,
    Backup,
    Temporary,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageName {
    pub kind: Kind,
    pub form: Form,
    pub canonical_bytes: usize,
}
pub fn hex(text: &[u8]) -> bool {
    text.iter()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
}
impl PageName {
    pub fn parse(text: Option<&[u8]>) -> Self {
        let Some(text) = text else {
            return Self::default();
        };
        let text = cstr(text);
        if let Some(name)=crate::book_index::FileName::parse(text) {
            use crate::book_index::FileKind;
            return Self {kind:Kind::Index,form:match name.kind() {
                FileKind::Index=>Form::Canonical,FileKind::Backup=>Form::Backup,_=>Form::Temporary },canonical_bytes:12};
        }
        let mut bytes = text.len();
        let mut result = Self::default();
        if text.ends_with(b".bak") {
            result.form = Form::Backup;
            bytes -= 4;
        } else if bytes >= 13
            && text[bytes - 13] == b'.'
            && hex(&text[bytes - 12..bytes - 4])
            && &text[bytes - 4..] == b".tmp"
        {
            result.form = Form::Temporary;
            bytes -= 13;
        }
        if bytes < 65 || !hex(&text[..64]) || text[64] != b'-' {
            return Self::default();
        }
        let suffix = &text[65..bytes];
        result.kind = if let Some(kind) = envelope_kind(suffix) {
            kind
        } else if suffix.len() == 72
            && &suffix[..3] == b"pe-"
            && hex(&suffix[3..67])
            && &suffix[67..] == b".ntpe"
        {
            Kind::Progress
        } else if suffix.len() == 122
            && &suffix[..3] == b"sm-"
            && hex(&suffix[3..67])
            && suffix[67] == b'-'
            && hex(&suffix[68..108])
            && suffix[108] == b'-'
            && hex(&suffix[109..117])
            && &suffix[117..] == b".ntsm"
        {
            Kind::Semantic
        } else if identity::matches(suffix) {
            Kind::Identity
        } else {
            Kind::Unknown
        };
        if result.kind != Kind::Unknown {
            result.canonical_bytes = bytes;
        }
        result
    }
}
pub fn add(total: &mut u64, bytes: u64) -> bool {
    let Some(next) = total.checked_add(bytes) else {
        return false;
    };
    *total = next;
    true
}
/// Page hex routes and typed 8.3 index names. Legacy `.fibx` is retained
/// only for classification of bench files and format fixtures.
pub fn envelope_kind(suffix: &[u8]) -> Option<Kind> {
    if crate::book_index::FileName::parse(suffix).is_some() {return Some(Kind::Index);}
    if suffix.len() != 69 || !hex(&suffix[..64]) {
        return None;
    }
    match &suffix[64..] {
        b".fibp" => Some(Kind::Page),
        b".fibx" => Some(Kind::Index),
        _ => None,
    }
}
/// None is the writer's default folder (Pages).
pub fn cache_folder(suffix: Option<&[u8]>) -> Option<&'static str> {
    match suffix.map(cstr).and_then(envelope_kind) {
        Some(Kind::Page) => None,
        Some(_) => Some("Index"),
        None => Some("Meta"),
    }
}
pub const PAGE_QUOTA: u64 = 32 * 1024 * 1024;
pub const PAGE_CANDIDATE: u32 = crate::page_envelope::MAX_EXTENT;
pub const PAGE_CREDIT_UNIT: u32 = 65536;
pub const PAGE_REQUIRED_UNITS: u8 = (PAGE_CANDIDATE / PAGE_CREDIT_UNIT + 1) as u8;
pub const PAGE_ENTRY_CAP: u32 = 128;
pub const PAGE_ENTRY_RESERVE: u32 = 8;
pub const PAGE_ENTRY_GRANT: u8 = (PAGE_ENTRY_CAP - PAGE_ENTRY_RESERVE) as u8;
pub const PAGE_SEGMENT_BATCH: u8 = 4;
/// Book indexes have their own quota, outside Pages, evicted oldest-written
/// first: losing one costs a rebuild, so orphans of deleted books age out
/// under pressure.
pub const INDEX_QUOTA: u64 = 16 * 1024 * 1024;
/// Every FAT lookup scans the folder. Bound that per-file cost while keeping
/// the full census; it runs before a new index, never on a repeat open.
pub const INDEX_ENTRY_CAP: u32 = 32;
pub const INDEX_ENTRY_RESERVE: u32 = 8;
pub const INDEX_ENTRY_GRANT: u8 = (INDEX_ENTRY_CAP - INDEX_ENTRY_RESERVE) as u8;
/// Free credit for one largest index envelope, rounded up like pages.
pub const INDEX_CANDIDATE: u32 = {
    let largest = crate::book_index::payload_bytes(crate::book_index::MAX_MEMBERS, crate::book_index::MAX_SPINE);
    // Checked at compile time: the format's own limits always have a layout.
    assert!(largest.is_some(), "largest index layout");
    let payload = match largest { Some(payload) => payload, None => 0 };
    payload + 4 * crate::page_envelope::blocks(payload) + crate::page_envelope::MANIFEST_BYTES
};
pub const INDEX_REQUIRED_UNITS: u8 = (INDEX_CANDIDATE / PAGE_CREDIT_UNIT + 1) as u8;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageNameSet {
    words: [u32; 8],
}
impl PageNameSet {
    fn bits(name: &[u8]) -> [usize; 2] {
        let value = name.iter().fold(2166136261u32, |v, b| {
            (v ^ u32::from(*b)).wrapping_mul(16777619)
        });
        [(value & 255) as usize, ((value >> 16) & 255) as usize]
    }
    pub fn add(&mut self, name: &[u8]) {
        for bit in Self::bits(name) {
            self.words[bit / 32] |= 1u32 << (bit % 32);
        }
    }
    pub fn may_contain(&self, name: &[u8]) -> bool {
        Self::bits(name)
            .iter()
            .all(|&bit| self.words[bit / 32] & (1u32 << (bit % 32)) != 0)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageCacheNeed {
    pub files: u8,
    pub short_chapter: bool,
    pub keep: PageNameSet,
}
impl Default for PageCacheNeed {
    fn default() -> Self {
        Self {
            files: 1,
            short_chapter: false,
            keep: PageNameSet::default(),
        }
    }
}
