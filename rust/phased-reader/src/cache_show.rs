//! MIT. Shared chapter page cache query and capture helpers.
use super::*;
use crate::imported_revision::{Digest};
use newt_core::{
    cache_names::segment::{self, ChapterSegment, SegmentAnchor},
    chapter_layout::types::Page,
    page_cache::{CacheStorage, PageCacheReader, PageMemory, ReaderMemory},
    preferences::ViewSettings,
    admission::{ActionScope, Yield},
    reader_action_profile::CacheMissReason,
};

// Frozen readEntry's one-byte stack miss survives query cleanup and capture.
#[derive(Default)]
pub(crate) struct Miss {
    #[cfg(feature = "dev-usb")]
    reason: core::cell::Cell<Option<CacheMissReason>>,
}
impl Miss {
    #[inline]
    pub(crate) fn set(&self, reason: CacheMissReason) {
        #[cfg(feature = "dev-usb")] self.reason.set(Some(reason));
        #[cfg(not(feature = "dev-usb"))] let _ = reason;
    }
    pub(crate) fn publish(&self, trace: Yield<'_>) {
        #[cfg(feature = "dev-usb")]
        if let Some(reason) = self.reason.get() { admission::cache_miss(trace, reason); }
        #[cfg(not(feature = "dev-usb"))] let _ = trace;
    }
}
#[cfg(feature = "dev-usb")]
const _: () = assert!(core::mem::size_of::<Miss>() == 1);
#[cfg(not(feature = "dev-usb"))]
const _: () = assert!(core::mem::size_of::<Miss>() == 0);

pub struct CachedPageMemory<'a> {
    pub scratch: Arena<'a>,
    pub records: PageMemory<'a>,
}
#[derive(Clone, Copy)]
pub struct CacheRequest {
    pub spine: u32,
    pub target: u32,
    pub anchor: bool,
    pub staged: bool,
    pub build_on_miss: bool,
    pub cache_only: bool,
}
#[derive(Debug, PartialEq, Eq)]
pub struct ShowResult {
    pub status: Status,
    pub hit: bool,
}
/// A cached page's existing action scope ends after its later logical paint.
/// The token holds only the operation's short borrowed hook, never page memory.
pub struct CapturedPage<'trace> {
    pub result: ShowResult,
    pub publication: ActionScope<'trace>,
}
impl CapturedPage<'_> {
    pub(crate) fn failed(status: Status) -> Self {
        Self { result: ShowResult { status, hit: false }, publication: ActionScope::inactive() }
    }
}
pub(crate) fn valid_index(cache: &PageCacheReader, memory: &ReaderMemory<'_>) -> bool {
    if cache.is_partial() || cache.page_count() == 0 || cache.page_count() > segment::MAX_PAGES {
        return false;
    }
    (0..cache.page_count()).all(|i| {
        let start = cache.char_start(memory, i);
        start <= cache.total_chars() && (i == 0 || start >= cache.char_start(memory, i - 1))
    })
}

pub(super) fn expanded(base: &[u8; 69]) -> [u8; 128] {
    let mut name = [0; 128];
    name[..69].copy_from_slice(base);
    name
}
pub(super) fn segment_of(
    reader: &PhasedReader<'_, '_>,
    spine: u32,
    target: u32,
) -> Option<ChapterSegment> {
    let info = reader.info;
    if !reader.has_page
        || info.spine != spine
        || info.page_count > segment::MAX_PAGES
        || ChapterSegment::count(info.page_count) == 0
        || target >= info.page_count
    {
        return None;
    }
    Some(ChapterSegment {
        index: target / segment::PAGES,
        pages: info.page_count,
        chars: info.total_chars,
    })
}

pub(crate) fn query_open_page<'page>(
    store: &mut impl CacheStorage,
    request: CacheRequest,
    located: Option<SegmentAnchor>,
    certified: Option<ChapterSegment>,
    cache: &PageCacheReader,
    names: &Arena<'_>,
    index: &ReaderMemory<'_>,
    memory: &'page mut CachedPageMemory<'_>,
    miss: &Miss,
) -> Result<(Page<'page>, u32, u32), Status> {
    if !valid_index(&cache, index) {
        miss.set(CacheMissReason::Index);
        return Err(Status::Stale);
    }
    let segment = located
        .map(|v| v.segment)
        .or(certified.filter(|s| !request.anchor && cache.page_count() != s.pages));
    let first = segment.map_or(0, |s| s.first());
    if let Some(s) = segment {
        if cache.total_chars() != s.chars
            || cache.page_count() != s.file_pages()
            || located.is_some_and(|v| cache.char_start(index, 0) != v.start)
        {
            miss.set(CacheMissReason::Segment);
            return Err(Status::Stale);
        }
    }
    let number = if request.anchor {
        cache.page_for_char(index, request.target)
    } else {
        request.target.wrapping_sub(first)
    };
    if number >= cache.page_count() {
        miss.set(CacheMissReason::Range);
        return Err(Status::NotFound);
    }
    if located
        .is_some_and(|v| number + 1 == cache.page_count() && first + number + 1 < v.segment.pages)
    {
        miss.set(CacheMissReason::Edge);
        return Err(Status::Stale);
    }
    let mut page = cache.read_page(
        store,
        index,
        names,
        number,
        &mut memory.scratch,
        &mut memory.records,
    ).map_err(|error| { miss.set(CacheMissReason::Read); error })?;
    if page.char_start != cache.char_start(index, number) {
        miss.set(CacheMissReason::CharStart);
        return Err(Status::Stale);
    }
    if !page.images.is_empty() {
        miss.set(CacheMissReason::Image);
        return Err(Status::Stale);
    }
    page.index = first + number;
    Ok((
        page,
        segment.map_or(cache.page_count(), |s| s.pages),
        cache.total_chars(),
    ))
}

pub(super) fn restore_identity(
    settings: ViewSettings,
    digest: &Digest,
    spine: u32,
    hash: &mut dyn crate::hash_provider::CacheHash,
    name: &mut [u8; 69],
    generation: &mut u32,
    language: BookLanguage,
) -> bool {
    match session_adapter::identity(settings, digest, spine, hash, language) {
        Ok((n, g)) => {
            *name = n;
            *generation = g;
            true
        }
        Err(_) => {
            name[0] = 0;
            false
        }
    }
}
