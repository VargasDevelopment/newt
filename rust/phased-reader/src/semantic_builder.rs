//! Per-operation semantic counts from the verified index and checked chapters.
use newt_core::{admission::{Observer, ScheduledSource, WorkPhase, WorkScope},
    book_index, book_semantic_index::{Digest, Index, POLICY_VERSION},
    canonical::{self, Resources}, package_parsers::Workflow,
    xml_session::ParserBackend, zip_container::Status, zip_window::{arena::Arena, source::BookSource}};
use crate::phase_storage::ReadScope;

/// The owner binds source and digest before lending the parser resources.
/// Every index query and chapter stream ends before `returned` runs.
#[allow(clippy::too_many_arguments)]
pub fn build<B: ParserBackend + ?Sized, C: ReadScope>(source: &mut dyn BookSource,
    source_bytes: u64, spines: u32, digest: &Digest, store: &mut C, index: &mut Index,
    metadata: &mut Arena<'_>, parse: &mut Arena<'_>, resources: &mut Resources<'_, B>,
    on_yield: newt_core::admission::Yield<'_>, observer: Observer,
    discard_styles: impl FnOnce(), returned: impl FnOnce(),
) -> Status {
    index.invalidate();
    discard_styles();
    metadata.reset(); parse.reset();
    let result = (|| {
        let _work = WorkScope::new_scoped(on_yield, observer, WorkPhase::Metadata);
        let status = canonical::semantic_validate(source, source_bytes);
        if status != Status::Ok { return Err(status); }
        let query = metadata.alloc(4160 + 4096, 16).ok_or(Status::OutOfMemory)?;
        let query = metadata.view_mut(query).ok_or(Status::OutOfMemory)?;
        let name = parse.alloc(512, 1).ok_or(Status::OutOfMemory)?;
        let name = parse.view_mut(name).ok_or(Status::OutOfMemory)?;
        let count = crate::reader::query_book_index(source, store, digest, query,
            |book, _| Ok(book.header().spine))?;
        if count != spines { return Err(Status::Stale); }
        if !index.bind(digest, POLICY_VERSION, spines) { return Err(Status::Unsupported); }
        let mut directory = newt_core::zip_container::Directory::default();
        let status = newt_core::zip_container::locate(source, resources.scan, &mut directory);
        if status != Status::Ok { return Err(status); }
        let mut scheduled = ScheduledSource::new(source, on_yield, resources.policy);
        let mut workflow = Workflow { source: &mut scheduled, backend: &mut *resources.backend,
            decoder: crate::decoder(&mut resources.decoder), buffer: resources.xml_buffer.as_deref_mut(),
            filter: resources.filter.as_deref_mut(), policy: resources.policy };
        for spine in 0..spines {
            let member = crate::reader::query_book_index(workflow.source, store, digest, query, |book, store| {
                let item = book.spine(store, spine).ok_or(Status::IoError)?;
                if item.member == book_index::NONE { return Err(Status::NotFound); }
                book.member(store, item.member).ok_or(Status::IoError)
            })?;
            let entry = newt_core::member_check::check_indexed(workflow.source, &member, None, name, directory.offset)?;
            if !newt_core::phased_metadata::admissible_href(crate::xml_input::c_bytes(name), name.len()) {
                return Err(Status::Unsupported);
            }
            let chars = canonical::semantic_count_member(&mut workflow, &entry, on_yield)?;
            let status = canonical::semantic_validate(workflow.source, source_bytes);
            if status != Status::Ok { return Err(status); }
            if !index.record(digest, POLICY_VERSION, spine, chars) { return Err(Status::Stale); }
        }
        Ok(())
    })();
    metadata.reset(); parse.reset();
    returned();
    let mut status = result.map_or_else(|error| error, |_| canonical::semantic_validate(source, source_bytes));
    if status == Status::Ok && (!index.complete(digest, POLICY_VERSION)
        || index.spine_count(digest, POLICY_VERSION) != spines) { status = Status::Stale; }
    if status != Status::Ok { index.invalidate(); }
    status
}
