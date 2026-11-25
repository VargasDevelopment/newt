// SPDX-License-Identifier: MIT
//! The index needs the held physical source and Pages, without receipt views.
use super::{millis, selected, session};
use crate::{reader_book::{self, BookFailure}, reader_hash::{MbedHasher, PackageHasher},
    reader_pages::ReaderPages, reader_verification::ReaderVerification};
use newt_phased_reader::index_build::ReleasableSource;
use newt_native_storage::{Authority, Native, page_entries::PageEntries, RouteState,
    chapter_cache::{ChapterCacheStorage, ChapterCacheState}};
use newt_phased_reader::phase_storage::scratch_value;

pub(super) enum Source<'loan> {
    Strict(&'loan mut dyn ReleasableSource),
    Session(&'loan mut dyn ReleasableSource, &'loan mut ReaderVerification<MbedHasher>),
}

pub(super) trait Operation<N: Native, R> {
    fn run(self, pages: &mut ReaderPages<'_, N>, source: Source<'_>) -> Result<R, BookFailure>;
}

#[inline(always)]
pub(super) fn with_operation<R, N: Native + PageEntries>(context: Option<&mut session::Session>,
    selected: &mut selected::SelectedSource, filename: &[u8],
    source_native: &mut N, pages_native: &mut N, scratch: &mut [u8],
    cancel: &dyn Authority, observer: Option<&dyn selected::FileObserver>,
    operation: impl Operation<N, R>,
) -> Result<R, BookFailure> {
    match context {
        Some(context) => with_session(context, selected, filename, source_native, pages_native,
            scratch, cancel, observer, operation),
        None => without_session(selected, filename, source_native, pages_native,
            scratch, cancel, observer, operation),
    }
}

struct Standalone {
    route: RouteState,
    pages: ChapterCacheState,
}

// Both owners occupy the existing logical-frame loan, not the build's stack.
// They stay live through the consumer. IndexPass receives only the separate
// ReaderMemory shared bank and Board primary-frame parse loan, never these
// logical-frame bytes. scratch_value outlines construction, not owner lifetime.
#[inline(never)]
fn without_session<R, N: Native + PageEntries>(selected: &mut selected::SelectedSource,
    filename: &[u8], source_native: &mut N, pages_native: &mut N, scratch: &mut [u8],
    cancel: &dyn Authority, observer: Option<&dyn selected::FileObserver>,
    operation: impl Operation<N, R>,
) -> Result<R, BookFailure> {
    let extent = core::mem::size_of::<Standalone>() + core::mem::align_of::<Standalone>() - 1;
    let (owners, source_scratch) = scratch.split_at_mut_checked(extent)
        .ok_or(BookFailure::Open(newt_core::zip_container::Status::OutOfMemory))?;
    let mut state = scratch_value(owners, || {
        let mut key = [0; 64];
        let name = newt_core::library_name::name_bytes(filename).and_then(|b| core::str::from_utf8(b).ok())
            .ok_or(newt_core::zip_container::Status::Unsupported)?;
        let key = reader_book::route_key(name, &mut PackageHasher::from_live_sdk(), &mut key)
            .ok_or(newt_core::zip_container::Status::Unsupported)?;
        Ok(Standalone { route: RouteState::new(reader_book::CACHE_ROOT, key), pages: ChapterCacheState::new() })
    }).map_err(|_| BookFailure::Route)?;
    let Standalone { route, pages: state } = &mut *state;
    let route = route.lend(cancel, millis);
    let result = {
        let mut pages = ReaderPages(ChapterCacheStorage::lend(state, pages_native, &route)
            .map_err(|_| BookFailure::Route)?);
        selected.with_index_source(source_native, filename, source_scratch, Some(cancel), observer,
            |raw| operation.run(&mut pages, Source::Strict(raw))).map_err(|_| BookFailure::Source)?
    };
    let closed = state.close(pages_native, &route);
    if closed.is_ok_and(|outcome| outcome.clean) { result } else { Err(BookFailure::Source) }
}

#[inline(never)]
fn with_session<R, N: Native + PageEntries>(context: &mut session::Session,
    selected: &mut selected::SelectedSource, filename: &[u8], source_native: &mut N, pages_native: &mut N,
    scratch: &mut [u8], cancel: &dyn Authority, observer: Option<&dyn selected::FileObserver>,
    operation: impl Operation<N, R>,
) -> Result<R, BookFailure> {
    let route = context.route.lend(cancel, millis);
    let mut pages = ReaderPages(ChapterCacheStorage::lend(&mut context.pages, pages_native, &route)
        .map_err(|_| BookFailure::Route)?);
    selected.with_index_source(source_native, filename, scratch, Some(cancel), observer,
        |raw| operation.run(&mut pages, Source::Session(raw, &mut context.verification)))
        .map_err(|_| BookFailure::Source)?
}
