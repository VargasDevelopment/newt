//! Navigation borrows only the optional TOC strings and scalar index results.
use crate::{BookView, Declaration, Item, Member, NO_SPINE, ProgressInputs, usable_entry};
use newt_core::{book_index::Header, package_parsers::TocEntry, zip_window::arena::{Arena, CStringRef}};

/// A batch map lasts through bind. Above the scratch bound, `lookup` supplies
/// the same per-row answer under its own checked index-read scope.
pub struct IndexedBook<'a, 'storage> {
    pub header: &'a Header,
    pub arena: &'a Arena<'storage>,
    pub toc: &'a [TocEntry],
    pub mapping: Option<&'a [u32]>,
    pub lookup: &'a dyn Fn(&[u8]) -> u32,
    pub progress: ProgressInputs,
}
impl IndexedBook<'_, '_> {
    fn bytes(&self, value: Option<CStringRef>) -> &[u8] {
        value.and_then(|value| self.arena.c_string(value)).unwrap_or(b"")
    }
}
impl BookView for IndexedBook<'_, '_> {
    fn spine_count(&self) -> usize { self.header.spine as usize }
    // Index-backed navigation uses the projection hooks; it never needs to
    // retain every member name in the metadata arena.
    fn spine_item(&self, _: usize) -> Option<Item<'_>> { None }
    fn member(&self, _: &[u8]) -> Option<Member> { None }
    fn toc_count(&self) -> usize { self.toc.len() }
    fn toc_entry(&self, row: usize) -> Option<Declaration<'_>> {
        let entry = self.toc.get(row)?;
        Some(Declaration { title: self.bytes(entry.title), href: self.bytes(entry.href),
            fragment: self.bytes(entry.fragment), depth: entry.depth })
    }
    fn toc_title(&self, row: usize) -> Option<&[u8]> { Some(self.bytes(self.toc.get(row)?.title)) }
    fn toc_href(&self, row: usize) -> Option<&[u8]> { Some(self.bytes(self.toc.get(row)?.href)) }
    fn declared_spine(&self, row: usize) -> u32 {
        if let Some(mapping) = self.mapping { return mapping.get(row).copied().unwrap_or(NO_SPINE); }
        if !usable_entry(self, row) { return NO_SPINE; }
        let href = self.toc_href(row).expect("usable declaration");
        if href.first().is_none_or(|&byte| byte == 0) { return NO_SPINE; }
        (self.lookup)(href)
    }
    fn progress_inputs(&self, selected: u32) -> ProgressInputs {
        if selected == self.progress.index { self.progress } else { ProgressInputs::default() }
    }
}
