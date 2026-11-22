//! MIT. Real borrowed entry points for root's native host / RV32 instantiation.
//! No synthetic reader, cache success, source, SHA or parser implementation.
use crate::CachePresence;
use crate::imported_revision::{
    Digest, ImportedRevision,
    session::{ParseLease, Session},
};
use crate::{OperationCache, OperationReader, xml_session::ParserBackend};
use crate::{zip_container::Status, zip_window::source::BookSource};
use newt_core::page_cache::CacheStorage;

#[inline(never)]
pub fn cold_import<'source, B: ParserBackend + ?Sized, C: CacheStorage + CachePresence>(
    revision: &mut ImportedRevision<'source>,
    source: &'source mut dyn BookSource,
    reader: &mut OperationReader<'_, '_, '_, '_, '_, '_, B>,
    cache: &mut OperationCache<'_, '_, C>,
) -> Status {
    Session {
        revision,
        reader,
        cache,
    }
    .import(source)
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub fn warm_select<'source, B: ParserBackend + ?Sized, C: CacheStorage + CachePresence>(
    revision: &mut ImportedRevision<'source>,
    source: &'source mut dyn BookSource,
    digest: Digest,
    spine: u32,
    lease: &mut impl ParseLease,
    reader: &mut OperationReader<'_, '_, '_, '_, '_, '_, B>,
    cache: &mut OperationCache<'_, '_, C>,
) -> Status {
    Session {
        revision,
        reader,
        cache,
    }
    .warm_open(source, digest, spine, lease)
}

#[inline(never)]
pub fn select_verified<B: ParserBackend + ?Sized, C: CacheStorage + CachePresence>(
    revision: &mut ImportedRevision<'_>,
    spine: u32,
    lease: &mut impl ParseLease,
    reader: &mut OperationReader<'_, '_, '_, '_, '_, '_, B>,
    cache: &mut OperationCache<'_, '_, C>,
) -> Status {
    Session {
        revision,
        reader,
        cache,
    }
    .select(spine, lease)
}

/// Borrowed RV32 presence body. Root instantiates with actual native store/SHA.
#[inline(never)]
pub fn stored_hint<B: ParserBackend + ?Sized, C: CacheStorage + CachePresence>(
    digest: &Digest,
    spine: u32,
    reader: &mut OperationReader<'_, '_, '_, '_, '_, '_, B>,
    cache: &mut OperationCache<'_, '_, C>,
) -> bool {
    crate::imported_revision::session::Cache::stored(cache, digest, spine, reader)
}
