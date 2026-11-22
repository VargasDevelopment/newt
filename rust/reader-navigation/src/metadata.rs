//! Borrowed adapter over the existing Rust package parser and ZIP table.
use super::*;
use newt_core::{
    package_parsers::{ManifestItem, PackageResult, TocEntry},
    zip_container::Entry,
    zip_window::arena::{Arena, CStringRef},
};

pub struct Metadata<'a, 'storage> {
    pub arena: &'a Arena<'storage>,
    pub package: &'a PackageResult,
    pub manifest: &'a [ManifestItem],
    pub spine: &'a [u16],
    pub toc: &'a [TocEntry],
    pub entries: &'a [Entry],
}
impl Metadata<'_, '_> {
    fn bytes(&self, value: Option<CStringRef>) -> &[u8] {
        value.and_then(|v| self.arena.c_string(v)).unwrap_or(b"")
    }
    fn href_eq(&self, value: Option<CStringRef>, href: Href<'_>) -> bool {
        let href = href.as_bytes();
        // Invalid handles retain the validated accessor's empty-string fallback.
        if href.first().is_none_or(|b| *b == 0) {
            self.bytes(value).is_empty()
        } else {
            value.is_some_and(|v| self.arena.c_string_eq(v, href))
        }
    }
    fn spine_manifest(&self, index: usize) -> Option<&ManifestItem> {
        if index >= self.package.spine_count {
            return None;
        }
        self.manifest.get(usize::from(*self.spine.get(index)?))
    }
}
impl BookView for Metadata<'_, '_> {
    fn spine_count(&self) -> usize {
        self.package.spine_count
    }
    fn spine_item(&self, index: usize) -> Option<Item<'_>> {
        let item = self.spine_manifest(index)?;
        Some(Item {
            href: item.href.map(|_| self.bytes(item.href)),
            // Package parser uses None for the static empty media type.
            media: Some(self.bytes(item.media_type)),
        })
    }
    fn toc_count(&self) -> usize {
        self.toc.len()
    }
    fn toc_entry(&self, index: usize) -> Option<Declaration<'_>> {
        let entry = self.toc.get(index)?;
        Some(Declaration {
            title: self.bytes(entry.title),
            href: self.bytes(entry.href),
            fragment: self.bytes(entry.fragment),
            depth: entry.depth,
        })
    }
    fn toc_title(&self, index: usize) -> Option<&[u8]> {
        Some(self.bytes(self.toc.get(index)?.title))
    }
    fn toc_href(&self, index: usize) -> Option<&[u8]> {
        Some(self.bytes(self.toc.get(index)?.href))
    }
    fn toc_href_eq(&self, index: usize, href: Href<'_>) -> bool {
        self.toc
            .get(index)
            .is_some_and(|entry| self.href_eq(entry.href, href))
    }
    fn spine_href_eq(&self, index: usize, href: Href<'_>) -> bool {
        self.spine_manifest(index)
            .is_some_and(|item| item.href.is_some() && self.href_eq(item.href, href))
    }
    fn member(&self, href: &[u8]) -> Option<Member> {
        let href = c_bytes(href);
        let hash = hash_path(href);
        self.entries
            .iter()
            .find(|e| {
                e.name_hash == hash && e.name.is_some_and(|n| self.arena.c_string_eq(n, href))
            })
            .map(|e| Member {
                method: e.method,
                uncompressed: e.uncompressed_size,
            })
    }
}
