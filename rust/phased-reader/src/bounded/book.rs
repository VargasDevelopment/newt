// SPDX-License-Identifier: MIT
//! Stored catalog preflight and the SDK Book's eager optional navigation parse.
use super::{MAX_ENTRIES, MAX_SOURCE_BYTES};
use crate::{canonical::{BookParts, CatalogView}, package_parsers::{self, ManifestItem,
        PackageResult, TocEntry, TocKind}, xml_session::ParserBackend,
    zip_container::{Catalog, Entry, Status}, zip_window::{arena::Arena, source::BookSource}};
use newt_reader_navigation::metadata::Metadata;

/// The caller supplies the catalog's real counted book-bank allocation. A
/// 17-entry catalog which fits must report Unsupported, not an artificial OOM
/// from a fixed 16-element replacement array.
pub fn preflight<'a, 'storage, S: BookSource + ?Sized>(
    catalog: &mut Catalog<'a, 'storage, S>, scan: &mut [u8; 4099],
    sequential: &mut [u8; 1024],
    allocate: impl FnOnce(&mut Arena<'storage>, usize) -> Result<&'a mut [Entry], Status>,
) -> Status {
    if catalog.source().size() > MAX_SOURCE_BYTES { return Status::Unsupported; }
    let status = catalog.open_allocated(scan, sequential, allocate);
    if status != Status::Ok { return status; }
    if catalog.entry_count() > MAX_ENTRIES { return Status::Unsupported; }
    for index in 0..catalog.entry_count() {
        let Some(entry) = catalog.entry(index) else { return Status::Unsupported; };
        if entry.method != 0 || entry.uncompressed_size > 8 * 1024 { return Status::Unsupported; }
    }
    Status::Ok
}

/// No source/parser capability survives eager metadata loading. These borrows
/// stay within the sample owner's one book-bank scope.
pub struct MetadataParts<'a, 'storage> {
    pub arena: &'a mut Arena<'storage>,
    pub entries: &'a [Entry],
    pub manifest: &'a mut [ManifestItem],
    pub spine: &'a mut [u16],
    pub toc: &'a mut [TocEntry],
    pub package: PackageResult,
    pub directory_offset: u32,
}
impl MetadataParts<'_, '_> {
    pub fn view(&self) -> Metadata<'_, '_> {
        Metadata { arena: self.arena, entries: self.entries, manifest: self.manifest,
            spine: self.spine, toc: self.toc, package: &self.package,
            directory_offset: self.directory_offset }
    }
}

/// Call after the same second catalog/container/OPF pass used by SDK Book::open.
/// The counted TOC allocation is retained in the book bank; its failure and
/// both parser passes have the SDK's original order.
pub fn load_toc<'a, 'storage, S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    parts: BookParts<'a, 'storage, S, B>,
    allocate: impl FnOnce(&mut Arena<'storage>, usize) -> Result<&'a mut [TocEntry], Status>,
) -> Result<MetadataParts<'a, 'storage>, Status> {
    let BookParts { mut workflow, arena, entries, manifest, spine, result, directory_offset, .. } = parts;
    let selected = result.nav_index.or(result.ncx_index)
        .and_then(|index| manifest.get(index)).and_then(|item| item.href)
        .and_then(|href| arena.c_string(href))
        .map(|href| CatalogView::new(entries, arena, directory_offset).member(workflow.source, href))
        .filter(|found| *found != Err(Status::NotFound))
        .transpose()?;
    let toc = if let Some((_, entry)) = selected {
        let index = result.nav_index.or(result.ncx_index).ok_or(Status::NotEpub)?;
        let href = manifest[index].href.and_then(|href| arena.c_string(href)).ok_or(Status::NotEpub)?;
        let mut directory = [0; 512];
        if !crate::xml_input::dir_name(href, &mut directory) { return Err(Status::Unsupported); }
        let directory = crate::xml_input::c_bytes(&directory);
        let kind = if result.nav_index.is_some() { TocKind::Nav } else { TocKind::Ncx };
        let count = package_parsers::count_toc(&mut workflow, &entry, directory, arena, kind)?;
        let toc = allocate(arena, count)?;
        let used = package_parsers::fill_toc(&mut workflow, &entry, directory, arena, kind, toc)?;
        toc.get_mut(..used).ok_or(Status::OutOfMemory)?
    } else { &mut [] };
    Ok(MetadataParts { arena, entries, manifest, spine, toc, package: result, directory_offset })
}
