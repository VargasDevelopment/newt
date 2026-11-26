//! The existing cache scalars and native ownership, separate from short loans.
use super::{FilePortState, Native, Route, CloseOutcome, LoanError, ChapterCacheStorage};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Lookup {
    #[default]
    Unknown,
    Missing,
    Invalid,
}

#[cfg(feature = "page-turn-trace")]
#[derive(Clone, Copy, Default)]
pub(super) struct SegmentTraceState {
    pub(super) active: bool,
    pub(super) idx: u32,
    pub(super) bytes: u32,
    pub(super) started_us: u32,
    pub(super) layout_us: u32,
    pub(super) tee_us: u32,
    pub(super) seal: newt_core::page_turn_trace::SealBreakdown,
}

/// Scalar state kept between loans; the descriptor stays in the FilePort.
#[derive(Default)]
pub(super) struct State {
    /// Three remembered absent names (folded FNV-1a, never zero). Chapter
    /// pages and metadata share this port, and an anchor's miss also proves
    /// its map absent (#192). A collision can only skip an optional cache.
    pub(super) absent: [u16; 3],
    pub(super) next_absent: u8,
    /// Census entries left in this producer grant, at most PAGE_ENTRY_GRANT.
    pub(super) entries: u8,
    pub(super) read_active: bool,
    /// The open record is a pre-#179 Pages/ name, copied to Meta/ on close.
    pub(super) legacy: bool,
    pub(super) lookup: Lookup,
    pub(super) last_published: bool,
    pub(super) payload: u32,
    pub(super) crc: u32,
    #[cfg(feature = "page-turn-trace")]
    pub(super) trace: SegmentTraceState,
}

/// Persistent cache state: the existing descriptor and existing cache scalars.
/// No backend/route/policy borrow survives a synchronous loan.
#[must_use = "close the cache at the enclosing source boundary"]
pub struct ChapterCacheState {
    pub(super) file: FilePortState,
    pub(super) state: State,
}
impl ChapterCacheState {
    pub fn new() -> Self { Self { file: FilePortState::new(), state: State::default() } }
    pub fn close<S: Native>(
        &mut self, native: &mut S, route: &Route<'_>,
    ) -> core::result::Result<CloseOutcome, LoanError> {
        let mut view = ChapterCacheStorage::lend(self, native, route)?;
        let mut access = view.access();
        // Keep legacy adoption and its native ordering in the existing close.
        access.close();
        let generation = access.file.generation();
        access.forget_absent();
        Ok(CloseOutcome { generation, clean: access.file.released_cleanly(generation) })
    }
}
