//! Runtime borrowed probes; no target-native XML backend is supplied or claimed.
use newt_core::package_parsers::*;
use newt_core::xml_session::ParserBackend;
use newt_core::zip_container::{Entry, Status};
use newt_core::zip_window::{
    arena::{Arena, CStringRef},
    source::BookSource,
};
#[unsafe(no_mangle)]
#[inline(never)]
pub fn newt_rust_package_probe(
    workflow: &mut Workflow<'_, dyn BookSource + '_, dyn ParserBackend + '_>,
    entry: &Entry,
    dir: &[u8],
    arena: &mut Arena<'_>,
    storage: PackageStorage<'_>,
    out: &mut PackageResult,
) -> Status {
    parse_package(workflow, entry, dir, arena, storage, out)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn newt_rust_container_probe(
    workflow: &mut Workflow<'_, dyn BookSource + '_, dyn ParserBackend + '_>,
    entry: &Entry,
    arena: &mut Arena<'_>,
    out: &mut Option<CStringRef>,
) -> Status {
    parse_container(workflow, entry, arena, out)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn newt_rust_toc_probe(
    workflow: &mut Workflow<'_, dyn BookSource + '_, dyn ParserBackend + '_>,
    entry: &Entry,
    dir: &[u8],
    arena: &mut Arena<'_>,
    kind: TocKind,
    entries: &mut [TocEntry],
    count: &mut usize,
) -> Status {
    parse_toc(workflow, entry, dir, arena, kind, entries, count)
}
