// MIT. Per-operation preparation only; no retained Index attachment.
use newt_core::{
    admission::{Observer, ScheduledSource, WorkPhase, WorkScope},
    book_semantic_index::{Digest, Index, POLICY_VERSION},
    canonical::{self, Resources},
    xml_session::ParserBackend,
    zip_container::Status,
    zip_window::{arena::Arena, source::BookSource},
};

/// Call only after owner preflight and exclusive parse lease acquisition.
/// `returned` releases that lease after all Book/XML loans and arena resets.
/// Revision owner must bind digest AND source to the admitted immutable revision;
/// size/probes cannot authenticate a same-size replacement or unread suffix.
#[allow(clippy::too_many_arguments)]
pub fn build<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    source: &mut S,
    source_bytes: u64,
    spines: u32,
    digest: &Digest,
    index: &mut Index,
    metadata: &mut Arena<'_>,
    parse: &mut Arena<'_>,
    resources: &mut Resources<'_, B>,
    on_yield: newt_core::admission::Yield<'_>,
    observer: Observer,
    discard_styles: impl FnOnce(),
    returned: impl FnOnce(),
) -> Status {
    index.invalidate();
    let mut status = canonical::semantic_validate(source, source_bytes);
    discard_styles();
    metadata.reset();
    parse.reset();
    {
        let _work = WorkScope::new_scoped(on_yield, observer, WorkPhase::Metadata);
        if status == Status::Ok {
            let mut scheduled = ScheduledSource::new(source, on_yield, None);
            status = match canonical::open(
                &mut scheduled,
                metadata,
                resources,
                false,
                on_yield,
                observer,
            ) {
                Ok(book) => book.semantic_counts(spines, source_bytes, digest, index, on_yield),
                Err(e) => e,
            };
        }
    }
    metadata.reset();
    parse.reset();
    returned();
    if status == Status::Ok {
        status = canonical::semantic_validate(source, source_bytes);
    }
    // Equivalent to the COMPLETE-table part of attachSemanticIndex. This does
    // not implement retained ownership, page count checks, or page projection.
    if status == Status::Ok {
        if !index.complete(digest, POLICY_VERSION)
            || index.spine_count(digest, POLICY_VERSION) != spines
        {
            status = Status::Stale;
        } else if canonical::semantic_validate(source, source_bytes) != Status::Ok {
            // Validation failure on attach maps to Stale.
            status = Status::Stale;
        }
    }
    if status != Status::Ok {
        index.invalidate();
    }
    status
}
