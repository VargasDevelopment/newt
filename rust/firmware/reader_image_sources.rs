// SPDX-License-Identifier: MIT
//! The index pass and reader operations lend the same selected card owners.
use super::{millis, selected, session};
use crate::{reader_book::{self, BookFailure}, reader_hash::{MbedHasher, PackageHasher},
    reader_pages::ReaderPages, reader_verification::ReaderVerification, sd_revision::SdRevisionStorage};
use newt_core::zip_window::source::BookSource;
use newt_native_storage::{Authority, Native, page_entries::PageEntries, RouteState, chapter_cache::{ChapterCacheStorage, ChapterCacheState}};
use newt_phased_reader::imported_revision::RevisionPorts;
use newt_reader_ui::ReaderSource;

pub(super) enum Source<'loan, 'source> {
    Strict(&'loan mut dyn BookSource),
    Session(&'loan mut ReaderSource<'source>, &'loan mut ReaderVerification<MbedHasher>),
}

/// A typed pass can inline its consumer into the selected loan branch,
/// without keeping another callback frame under the index builder.
pub(super) trait Operation<N: Native, R> {
    fn run(self, pages: &mut ReaderPages<'_, N>, source: Source<'_, '_>) -> Result<R, BookFailure>;
}
struct Callback<F>(F);
impl<N: Native, R, F> Operation<N, R> for Callback<F>
where F: FnOnce(&mut ReaderPages<'_, N>, Source<'_, '_>) -> Result<R, BookFailure> {
    #[inline(always)]
    fn run(self, pages: &mut ReaderPages<'_, N>, source: Source<'_, '_>) -> Result<R, BookFailure> {
        (self.0)(pages, source)
    }
}

#[inline(always)]
pub(super) fn with_sources<R, N: Native + PageEntries>(context: Option<&mut session::Session>,
    selected: &mut selected::SelectedSource, filename: &str,
    source_native: &mut N, receipt_native: &mut N, pages_native: &mut N,
    cancel: &dyn Authority, observer: Option<&dyn selected::FileObserver>, yielded: &dyn Fn(),
    action_storage: Option<&mut [u8]>,
    operation: impl FnOnce(&mut ReaderPages<'_, N>, Source<'_, '_>) -> Result<R, BookFailure>,
) -> Result<R, BookFailure> {
    with_operation(context, selected, filename, source_native, receipt_native, pages_native,
        cancel, observer, yielded, action_storage, Callback(operation))
}

#[inline(always)]
pub(super) fn with_operation<R, N: Native + PageEntries>(context: Option<&mut session::Session>,
    selected: &mut selected::SelectedSource, filename: &str,
    source_native: &mut N, receipt_native: &mut N, pages_native: &mut N,
    cancel: &dyn Authority, observer: Option<&dyn selected::FileObserver>, yielded: &dyn Fn(),
    action_storage: Option<&mut [u8]>, operation: impl Operation<N, R>,
) -> Result<R, BookFailure> {
    match context {
        Some(context) => with_session(context, selected, source_native, receipt_native,
            pages_native, cancel, observer, yielded, action_storage, operation),
        None => without_session(selected, filename, source_native, pages_native,
            cancel, observer, operation),
    }
}

// Never keep the standalone route/key/cache state on the session's stack.
#[inline(never)]
fn without_session<R, N: Native + PageEntries>(selected: &mut selected::SelectedSource,
    filename: &str, source_native: &mut N, pages_native: &mut N,
    cancel: &dyn Authority, observer: Option<&dyn selected::FileObserver>,
    operation: impl Operation<N, R>,
) -> Result<R, BookFailure> {
    let mut key = [0; 64];
    let key = reader_book::route_key(filename, &mut PackageHasher::from_live_sdk(), &mut key)
        .ok_or(BookFailure::Route)?;
    let route_state = RouteState::new(reader_book::CACHE_ROOT, key);
    let route = route_state.lend(cancel, millis);
    let mut state = ChapterCacheState::new();
    let result = {
        let mut pages = ReaderPages(ChapterCacheStorage::lend(&mut state, pages_native, &route)
            .map_err(|_| BookFailure::Route)?);
        selected.with_source_observed(source_native, Some(cancel), observer,
            |raw| operation.run(&mut pages, Source::Strict(raw))).map_err(|_| BookFailure::Source)?
    };
    let closed = state.close(pages_native, &route);
    if closed.is_ok_and(|outcome| outcome.clean) { result } else { Err(BookFailure::Source) }
}

#[inline(never)]
fn with_session<R, N: Native + PageEntries>(context: &mut session::Session,
    selected: &mut selected::SelectedSource, source_native: &mut N,
    receipt_native: &mut N, pages_native: &mut N,
    cancel: &dyn Authority, observer: Option<&dyn selected::FileObserver>, yielded: &dyn Fn(),
    action_storage: Option<&mut [u8]>, operation: impl Operation<N, R>,
) -> Result<R, BookFailure> {
    let session::Session { route, receipt, pages, whole, block, scratch, source: state, verification } = context;
    let route = route.lend(cancel, millis);
    let mut receipt = SdRevisionStorage::lend(receipt, receipt_native, &route).map_err(|_| BookFailure::Route)?;
    let mut pages = ReaderPages(ChapterCacheStorage::lend(pages, pages_native, &route).map_err(|_| BookFailure::Route)?);
    let mut whole = whole.borrow_with_yield(yielded);
    let mut block = block.borrow_with_yield(yielded);
    selected.with_source_observed(source_native, Some(cancel), observer, |raw| {
        session::with_source_lent(state, RevisionPorts { source: Some(raw), store: &mut receipt,
            whole: &mut whole, block: Some(&mut block), scratch }, action_storage,
            |revision| operation.run(&mut pages, Source::Session(revision, verification)))
            .map_err(|_| BookFailure::Revision)?
    }).map_err(|_| BookFailure::Source)?
}
