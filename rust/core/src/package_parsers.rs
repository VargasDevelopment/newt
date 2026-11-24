// Derived from FreeInk cd6f5b83 (MIT); see rust/THIRD_PARTY.md.
//! Caller-owned package arrays and byte-string arena. Handles belong to this
//! attempt's arena initialization; reset/release invalidates them. Arrays, parser
//! resources, source and arena must be disjoint. Typed storage has its own cost.
#![forbid(unsafe_code)]
use crate::xml_input::{TextCapture, has_token, local_is};
use crate::xml_session::{Attributes, Handler, PARSE_CHUNK, ParserBackend, Resources};
use crate::zip_container::{Entry, InflateScratch, Status, hash_path};
use crate::zip_window::{
    arena::{Arena, CStringRef},
    source::BookSource,
};

include!("package_workflow.rs");

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Metadata {
    pub title: Option<CStringRef>,
    pub author: Option<CStringRef>,
    pub language: Option<CStringRef>,
    pub identifier: Option<CStringRef>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ManifestItem {
    pub id: Option<CStringRef>,
    pub href: Option<CStringRef>,
    /// None denotes the static empty media type, with no arena allocation.
    pub media_type: Option<CStringRef>,
    pub id_hash: u32,
    pub is_nav: bool,
    pub is_cover_image: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TocEntry {
    pub title: Option<CStringRef>,
    pub href: Option<CStringRef>,
    pub fragment: Option<CStringRef>,
    pub depth: u8,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PackageResult {
    pub metadata: Metadata,
    pub manifest_count: usize,
    pub spine_count: usize,
    pub nav_index: Option<usize>,
    pub ncx_index: Option<usize>,
}
/// Arrays may contain unpublished partial writes after failure; only the result
/// counts published on Ok are consumable. Spine hashes are temporary per attempt.
pub struct PackageStorage<'a> {
    pub manifest: &'a mut [ManifestItem],
    pub spine_hashes: &'a mut [u32],
    pub spine: &'a mut [u16],
}
/// Counted result excludes temporary hash backing, whose borrow ends at return.
pub struct RetainedPackageStorage<'a> {
    pub manifest: &'a mut [ManifestItem],
    pub spine: &'a mut [u16],
}
fn bytes<'a>(arena: &'a Arena<'_>, range: Option<CStringRef>) -> &'a [u8] {
    range.and_then(|r| arena.c_string(r)).unwrap_or(b"")
}
// Reuse the exact bounded path/fragment allocator; compact only its handles.
fn resolve_href(
    arena: &mut Arena<'_>,
    base: Option<&[u8]>,
    href: Option<&[u8]>,
    fragment: Option<&mut Option<CStringRef>>,
) -> Option<CStringRef> {
    resolve_href_checked(arena, base, href, fragment).ok()
}
fn resolve_href_checked(
    arena: &mut Arena<'_>, base: Option<&[u8]>, href: Option<&[u8]>,
    fragment: Option<&mut Option<CStringRef>>,
) -> Result<CStringRef, crate::xml_input::HrefError> {
    let mut raw_fragment = None;
    let path = crate::xml_input::resolve_href_checked(
        arena,
        base,
        href,
        if fragment.is_some() {
            Some(&mut raw_fragment)
        } else {
            None
        },
    );
    if let Some(output) = fragment {
        *output = raw_fragment.and_then(|range| arena.string_ref(range));
    }
    path.and_then(|range| arena.string_ref(range).ok_or(crate::xml_input::HrefError::Capacity))
}

// These allocations are caller-owned metadata buffers, not runtime XML heap.
pub(crate) fn metadata_capacity_error(status: Status) -> Status {
    if status == Status::OutOfMemory { Status::MetadataTooLarge } else { status }
}

struct Container<'a, 'b> {
    arena: &'a mut Arena<'b>,
    path: Option<CStringRef>,
    error: Status,
}
impl Handler for Container<'_, '_> {
    fn start_element(&mut self, name: &[u8], atts: &mut dyn Attributes) {
        if self.path.is_some() || self.error != Status::Ok || !local_is(name, b"rootfile") {
            return;
        }
        if atts
            .local(b"media-type")
            .is_some_and(|v| v != b"application/oebps-package+xml")
        {
            return;
        }
        if let Some(path) = atts.local(b"full-path") {
            self.path = self.arena.copy_nul_ref(Some(path));
            if self.path.is_none() { self.error = Status::MetadataTooLarge; }
        }
    }
}
pub fn parse_container<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    workflow: &mut Workflow<'_, S, B>,
    entry: &Entry,
    arena: &mut Arena<'_>,
    out: &mut Option<CStringRef>,
) -> Status {
    let mut handler = Container { arena, path: None, error: Status::Ok };
    let status = workflow.pass(entry, &mut handler);
    if status != Status::Ok {
        return status;
    }
    if handler.error != Status::Ok { return handler.error; }
    if handler.path.is_none() {
        return Status::NotEpub;
    }
    *out = handler.path;
    Status::Ok
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Count,
    Fill,
}
#[derive(Clone, Copy)]
enum Field {
    None,
    Title,
    Author,
    Language,
    Identifier,
}
struct Package<'a, 'b> {
    mode: Mode,
    arena: &'a mut Arena<'b>,
    dir: &'a [u8],
    metadata_depth: usize,
    manifest_depth: usize,
    spine_depth: usize,
    capture: TextCapture,
    field: Field,
    metadata: Metadata,
    toc_id: Option<CStringRef>,
    manifest_count: usize,
    spine_count: usize,
    manifest: &'a mut [ManifestItem],
    hashes: &'a mut [u32],
    manifest_used: usize,
    spine_used: usize,
    error: Status,
    cover_hash: u32,
    interned: [Option<CStringRef>; 12],
    interned_count: usize,
}
impl<'a, 'b> Package<'a, 'b> {
    fn new(
        mode: Mode,
        arena: &'a mut Arena<'b>,
        dir: &'a [u8],
        manifest: &'a mut [ManifestItem],
        hashes: &'a mut [u32],
    ) -> Self {
        Self {
            mode,
            arena,
            dir,
            metadata_depth: 0,
            manifest_depth: 0,
            spine_depth: 0,
            capture: TextCapture::default(),
            field: Field::None,
            metadata: Metadata::default(),
            toc_id: None,
            manifest_count: 0,
            spine_count: 0,
            manifest,
            hashes,
            manifest_used: 0,
            spine_used: 0,
            error: Status::Ok,
            cover_hash: 0,
            interned: [None; 12],
            interned_count: 0,
        }
    }
    fn fail(&mut self, status: Status) {
        if self.error == Status::Ok { self.error = status; }
    }
    fn intern(&mut self, media: &[u8]) -> Option<CStringRef> {
        for value in &self.interned[..self.interned_count] {
            if bytes(self.arena, *value) == media {
                return *value;
            }
        }
        let value = self.arena.copy_nul_ref(Some(media));
        if value.is_some() && self.interned_count < 12 {
            self.interned[self.interned_count] = value;
            self.interned_count += 1;
        }
        value
    }
}
impl Handler for Package<'_, '_> {
    fn start_element(&mut self, name: &[u8], atts: &mut dyn Attributes) {
        if local_is(name, b"metadata") {
            self.metadata_depth += 1;
        } else if local_is(name, b"manifest") {
            self.manifest_depth += 1;
        } else if local_is(name, b"spine") {
            self.spine_depth += 1;
            if self.mode == Mode::Count && self.toc_id.is_none() {
                if let Some(toc) = atts.local(b"toc") {
                    self.toc_id = self.arena.copy_nul_ref(Some(toc));
                }
            }
        } else if self.metadata_depth > 0 && local_is(name, b"meta") {
            if atts.local(b"name") == Some(b"cover".as_slice()) {
                if let Some(content) = atts.local(b"content") {
                    self.cover_hash = hash_path(content);
                }
            }
        } else if self.metadata_depth > 0 && self.mode == Mode::Count {
            self.field = if local_is(name, b"title") {
                Field::Title
            } else if local_is(name, b"creator") {
                Field::Author
            } else if local_is(name, b"language") {
                Field::Language
            } else if local_is(name, b"identifier") {
                Field::Identifier
            } else {
                Field::None
            };
            if !matches!(self.field, Field::None) {
                self.capture.begin();
            }
        } else if self.manifest_depth > 0 && local_is(name, b"item") {
            if self.mode == Mode::Count {
                self.manifest_count += 1;
            } else if self.manifest_used < self.manifest.len() {
                if let (Some(id), Some(href)) = (atts.local(b"id"), atts.local(b"href")) {
                    let mut item = ManifestItem {
                        id: self.arena.copy_nul_ref(Some(id)),
                        id_hash: hash_path(id),
                        ..ManifestItem::default()
                    };
                    if item.id.is_none() { self.fail(Status::MetadataTooLarge); }
                    item.href = match resolve_href_checked(self.arena, Some(self.dir), Some(href), None) {
                        Ok(path) => Some(path),
                        Err(error) => {
                            self.fail(match error {
                                crate::xml_input::HrefError::TooLong => Status::PathTooLong,
                                crate::xml_input::HrefError::Capacity => Status::MetadataTooLarge,
                                crate::xml_input::HrefError::Missing => Status::ParseError,
                            });
                            None
                        }
                    };
                    let media = atts.local(b"media-type");
                    item.media_type = media.and_then(|m| self.intern(m));
                    item.is_nav = has_token(atts.local(b"properties"), b"nav");
                    item.is_cover_image = has_token(atts.local(b"properties"), b"cover-image");
                    let failed = item.id.is_none()
                        || item.href.is_none()
                        || (media.is_some() && item.media_type.is_none());
                    self.manifest[self.manifest_used] = item;
                    if failed {
                        self.fail(Status::MetadataTooLarge);
                    } else {
                        self.manifest_used += 1;
                    }
                }
            }
        } else if self.spine_depth > 0 && local_is(name, b"itemref") {
            if self.mode == Mode::Count {
                self.spine_count += 1;
            } else if self.spine_used < self.hashes.len() {
                if let Some(id) = atts.local(b"idref") {
                    self.hashes[self.spine_used] = hash_path(id);
                    self.spine_used += 1;
                }
            }
        }
    }
    fn end_element(&mut self, name: &[u8]) {
        if local_is(name, b"metadata") {
            self.metadata_depth = self.metadata_depth.saturating_sub(1);
        } else if local_is(name, b"manifest") {
            self.manifest_depth = self.manifest_depth.saturating_sub(1);
        } else if local_is(name, b"spine") {
            self.spine_depth = self.spine_depth.saturating_sub(1);
        } else if self.capture.active && self.mode == Mode::Count {
            let text = self.capture.trimmed();
            let slot = match self.field {
                Field::Title => Some(&mut self.metadata.title),
                Field::Author => Some(&mut self.metadata.author),
                Field::Language => Some(&mut self.metadata.language),
                Field::Identifier => Some(&mut self.metadata.identifier),
                Field::None => None,
            };
            if !text.is_empty() {
                if let Some(slot) = slot {
                    if bytes(self.arena, *slot).is_empty() {
                        if let Some(value) = self.arena.copy_nul_ref(Some(text)) {
                            *slot = Some(value);
                        } else {
                            self.fail(Status::MetadataTooLarge);
                        }
                    }
                }
            }
            self.capture.end();
            self.field = Field::None;
        }
    }
    fn text(&mut self, text: &[u8]) {
        self.capture.add(text);
    }
}
pub fn parse_package<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    workflow: &mut Workflow<'_, S, B>,
    entry: &Entry,
    dir: &[u8],
    arena: &mut Arena<'_>,
    storage: PackageStorage<'_>,
    out: &mut PackageResult,
) -> Status {
    let PackageStorage {
        manifest,
        spine_hashes,
        spine,
    } = storage;
    match parse_package_allocated(
        workflow,
        entry,
        dir,
        arena,
        |_, _, _| Ok((manifest, spine_hashes)),
        |_, _| Ok(spine),
        out,
    ) {
        Ok(_) => Status::Ok,
        Err(status) => status,
    }
}

/// Allocate counted arrays at the original count/fill boundaries. The second
/// allocation occurs only after a successful fill, preserving error precedence.
/// Returned storage is the caller's initialized typed backing, never a byte-size
/// reservation for objects allocated elsewhere.
pub fn parse_package_allocated<
    'mem,
    'scratch,
    'arena,
    S: BookSource + ?Sized,
    B: ParserBackend + ?Sized,
>(
    workflow: &mut Workflow<'_, S, B>,
    entry: &Entry,
    dir: &[u8],
    arena: &mut Arena<'arena>,
    allocate_manifest: impl FnOnce(
        &mut Arena<'arena>,
        usize,
        usize,
    )
        -> Result<(&'mem mut [ManifestItem], &'scratch mut [u32]), Status>,
    allocate_spine: impl FnOnce(&mut Arena<'arena>, usize) -> Result<&'mem mut [u16], Status>,
    out: &mut PackageResult,
) -> Result<RetainedPackageStorage<'mem>, Status> {
    let mut counter = Package::new(Mode::Count, arena, dir, &mut [], &mut []);
    let status = workflow.pass(entry, &mut counter);
    if status != Status::Ok {
        return Err(status);
    }
    if counter.error != Status::Ok {
        return Err(counter.error);
    }
    let (manifest_count, spine_count, metadata, toc_id) = (
        counter.manifest_count,
        counter.spine_count,
        counter.metadata,
        counter.toc_id,
    );
    if manifest_count == 0 || spine_count == 0 {
        return Err(Status::NotEpub);
    }
    let (manifest, spine_hashes) = allocate_manifest(arena, manifest_count, spine_count).map_err(metadata_capacity_error)?;
    if manifest.len() < manifest_count || spine_hashes.len() < spine_count {
        return Err(Status::MetadataTooLarge);
    }
    manifest[..manifest_count].fill(ManifestItem::default());
    let mut filler = Package::new(
        Mode::Fill,
        arena,
        dir,
        &mut manifest[..manifest_count],
        &mut spine_hashes[..spine_count],
    );
    let status = workflow.pass(entry, &mut filler);
    if status != Status::Ok {
        return Err(status);
    }
    if filler.error != Status::Ok {
        return Err(filler.error);
    }
    if filler.cover_hash != 0 {
        if let Some(item) = filler
            .manifest
            .iter_mut()
            .find(|m| m.id_hash == filler.cover_hash)
        {
            item.is_cover_image = true;
        }
    }
    let spine = allocate_spine(filler.arena, filler.spine_used).map_err(metadata_capacity_error)?;
    if spine.len() < filler.spine_used {
        return Err(Status::MetadataTooLarge);
    }
    let mut used = 0;
    for hash in &filler.hashes[..filler.spine_used] {
        if let Some(index) = filler.manifest[..filler.manifest_used]
            .iter()
            .position(|m| m.id_hash == *hash)
        {
            spine[used] = index as u16;
            used += 1;
        }
    }
    if used == 0 {
        return Err(Status::NotEpub);
    }
    let mut result = PackageResult {
        metadata,
        manifest_count: filler.manifest_used,
        spine_count: used,
        nav_index: None,
        ncx_index: None,
    };
    for (index, item) in filler.manifest[..filler.manifest_used].iter().enumerate() {
        if result.nav_index.is_none() && item.is_nav {
            result.nav_index = Some(index);
        }
        let id_matches = toc_id.is_some()
            && item.id.is_some()
            && bytes(filler.arena, item.id) == bytes(filler.arena, toc_id);
        if result.ncx_index.is_none()
            && (id_matches || bytes(filler.arena, item.media_type) == b"application/x-dtbncx+xml")
        {
            result.ncx_index = Some(index);
        }
    }
    *out = result;
    Ok(RetainedPackageStorage { manifest, spine })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TocKind {
    Nav,
    Ncx,
}
struct Toc<'a, 'b> {
    kind: TocKind,
    mode: Mode,
    arena: &'a mut Arena<'b>,
    dir: &'a [u8],
    entries: &'a mut [TocEntry],
    count: usize,
    used: usize,
    oom: bool,
    nav_depth: usize,
    ol_depth: u8,
    in_toc: bool,
    in_anchor: bool,
    href: Option<CStringRef>,
    fragment: Option<CStringRef>,
    point_depth: u8,
    in_label: usize,
    title: [u8; 256],
    title_len: usize,
    capture: TextCapture,
}
impl<'a, 'b> Toc<'a, 'b> {
    fn new(
        kind: TocKind,
        mode: Mode,
        arena: &'a mut Arena<'b>,
        dir: &'a [u8],
        entries: &'a mut [TocEntry],
    ) -> Self {
        Self {
            kind,
            mode,
            arena,
            dir,
            entries,
            count: 0,
            used: 0,
            oom: false,
            nav_depth: 0,
            ol_depth: 0,
            in_toc: false,
            in_anchor: false,
            href: None,
            fragment: None,
            point_depth: 0,
            in_label: 0,
            title: [0; 256],
            title_len: 0,
            capture: TextCapture::default(),
        }
    }
    fn nav_start(&mut self, name: &[u8], atts: &dyn Attributes) {
        if local_is(name, b"nav") {
            self.nav_depth += 1;
            if !self.in_toc && self.nav_depth == 1 && has_token(atts.exact(b"epub:type"), b"toc") {
                self.in_toc = true;
            }
            return;
        }
        if !self.in_toc {
            return;
        }
        if local_is(name, b"ol") {
            self.ol_depth = self.ol_depth.saturating_add(1);
        } else if local_is(name, b"a") {
            if let Some(href) = atts.local(b"href") {
                if self.mode == Mode::Count {
                    self.count += 1;
                } else {
                    self.href = resolve_href(
                        self.arena,
                        Some(self.dir),
                        Some(href),
                        Some(&mut self.fragment),
                    );
                    if self.href.is_none() {
                        self.oom = true;
                    }
                    self.capture.begin();
                    self.in_anchor = true;
                }
            }
        }
    }
    fn nav_end(&mut self, name: &[u8]) {
        if local_is(name, b"nav") {
            self.nav_depth = self.nav_depth.saturating_sub(1);
            if self.nav_depth == 0 {
                self.in_toc = false;
            }
            return;
        }
        if !self.in_toc {
            return;
        }
        if local_is(name, b"ol") {
            self.ol_depth = self.ol_depth.saturating_sub(1);
        } else if local_is(name, b"a") && self.in_anchor {
            self.in_anchor = false;
            if self.mode == Mode::Fill && self.used < self.entries.len() && self.href.is_some() {
                let title = self.arena.copy_nul_ref(Some(self.capture.trimmed()));
                if title.is_none() {
                    self.oom = true;
                    return;
                }
                self.entries[self.used] = TocEntry {
                    title,
                    href: self.href,
                    fragment: self.fragment,
                    depth: self.ol_depth.saturating_sub(1),
                };
                self.used += 1;
            }
            self.capture.end();
            self.href = None;
            self.fragment = None;
        }
    }
    fn ncx_start(&mut self, name: &[u8], atts: &dyn Attributes) {
        if local_is(name, b"navPoint") {
            self.point_depth = self.point_depth.saturating_add(1);
            self.title_len = 0;
            self.title[0] = 0;
        } else if local_is(name, b"navLabel") {
            self.in_label += 1;
        } else if self.in_label > 0 && local_is(name, b"text") {
            self.capture.begin();
        } else if self.point_depth > 0 && local_is(name, b"content") {
            if self.mode == Mode::Count {
                self.count += 1;
            } else if self.used < self.entries.len() {
                if let Some(src) = atts.local(b"src") {
                    let mut fragment = None;
                    let href =
                        resolve_href(self.arena, Some(self.dir), Some(src), Some(&mut fragment));
                    // Title is attempted even when resolving the path failed.
                    let title = self.arena.copy_nul_ref(Some(&self.title[..self.title_len]));
                    if href.is_none() || title.is_none() {
                        self.oom = true;
                        return;
                    }
                    self.entries[self.used] = TocEntry {
                        title,
                        href,
                        fragment,
                        depth: self.point_depth - 1,
                    };
                    self.used += 1;
                }
            }
        }
    }
    fn ncx_end(&mut self, name: &[u8]) {
        if local_is(name, b"navPoint") {
            self.point_depth = self.point_depth.saturating_sub(1);
        } else if local_is(name, b"navLabel") {
            self.in_label = self.in_label.saturating_sub(1);
        } else if self.capture.active && local_is(name, b"text") {
            let text = self.capture.trimmed();
            self.title_len = text.len();
            self.title[..text.len()].copy_from_slice(text);
            self.title[text.len()] = 0;
            self.capture.end();
        }
    }
}
impl Handler for Toc<'_, '_> {
    fn start_element(&mut self, name: &[u8], atts: &mut dyn Attributes) {
        match self.kind {
            TocKind::Nav => self.nav_start(name, atts),
            TocKind::Ncx => self.ncx_start(name, atts),
        }
    }
    fn end_element(&mut self, name: &[u8]) {
        match self.kind {
            TocKind::Nav => self.nav_end(name),
            TocKind::Ncx => self.ncx_end(name),
        }
    }
    fn text(&mut self, text: &[u8]) {
        if self.kind == TocKind::Ncx || self.in_anchor {
            self.capture.add(text);
        }
    }
}
pub fn parse_toc<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    workflow: &mut Workflow<'_, S, B>,
    entry: &Entry,
    dir: &[u8],
    arena: &mut Arena<'_>,
    kind: TocKind,
    entries: &mut [TocEntry],
    count_out: &mut usize,
) -> Status {
    let count = match count_toc(workflow, entry, dir, arena, kind) {
        Ok(count) => count,
        Err(status) => return status,
    };
    if count > entries.len() {
        return Status::OutOfMemory;
    }
    match fill_toc(workflow, entry, dir, arena, kind, &mut entries[..count]) {
        Ok(used) => {
            *count_out = used;
            Status::Ok
        }
        Err(status) => status,
    }
}

/// Only the callback may borrow temporary TOC backing.
pub trait TocBacking {
    fn with_toc<R>(
        &mut self,
        arena: &mut Arena<'_>,
        count: usize,
        operation: impl FnOnce(&mut Arena<'_>, &mut [TocEntry]) -> R,
    ) -> Result<R, Status>;
}
impl TocBacking for [TocEntry] {
    fn with_toc<R>(
        &mut self,
        arena: &mut Arena<'_>,
        count: usize,
        operation: impl FnOnce(&mut Arena<'_>, &mut [TocEntry]) -> R,
    ) -> Result<R, Status> {
        let entries = self.get_mut(..count).ok_or(Status::OutOfMemory)?;
        Ok(operation(arena, entries))
    }
}

pub fn count_toc<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    workflow: &mut Workflow<'_, S, B>,
    entry: &Entry,
    dir: &[u8],
    arena: &mut Arena<'_>,
    kind: TocKind,
) -> Result<usize, Status> {
    let mut counter = Toc::new(kind, Mode::Count, arena, dir, &mut []);
    let status = workflow.pass(entry, &mut counter);
    if status != Status::Ok {
        return Err(status);
    }
    Ok(counter.count)
}

/// XML/source failure precedes handler OOM. Publish only a complete fill.
pub fn fill_toc<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    workflow: &mut Workflow<'_, S, B>,
    entry: &Entry,
    dir: &[u8],
    arena: &mut Arena<'_>,
    kind: TocKind,
    entries: &mut [TocEntry],
) -> Result<usize, Status> {
    entries.fill(TocEntry::default());
    let mut filler = Toc::new(kind, Mode::Fill, arena, dir, entries);
    let status = workflow.pass(entry, &mut filler);
    if status != Status::Ok {
        return Err(status);
    }
    if filler.oom {
        return Err(Status::OutOfMemory);
    }
    Ok(filler.used)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Attrs<'a> {
        pairs: &'a [(&'a [u8], &'a [u8])],
        cursor: usize,
    }
    impl Attributes for Attrs<'_> {
        fn next(&mut self) -> Option<(&[u8], &[u8])> {
            let value = self.pairs.get(self.cursor).copied();
            self.cursor += usize::from(value.is_some());
            value
        }
        fn local(&self, name: &[u8]) -> Option<&[u8]> {
            crate::xml_input::attr_local(self.pairs, name)
        }
        fn exact(&self, name: &[u8]) -> Option<&[u8]> {
            crate::xml_input::attr_exact(self.pairs, name)
        }
    }
    #[test]
    fn repeated_lookup_is_first_match_and_cursor_independent() {
        let mut a = Attrs {
            pairs: &[
                (b"x:href", b"first"),
                (b"href", b"second"),
                (b"epub:type", b"toc"),
            ],
            cursor: 0,
        };
        assert_eq!(a.local(b"href"), Some(b"first".as_slice()));
        a.next();
        a.next();
        assert_eq!(a.local(b"href"), Some(b"first".as_slice()));
        assert_eq!(a.exact(b"href"), Some(b"second".as_slice()));
        assert_eq!(a.exact(b"type"), None);
    }
    #[test]
    fn only_first_twelve_distinct_media_are_interned() {
        let mut memory = [0; 128];
        let mut arena = Arena::new(&mut memory);
        let mut p = Package::new(Mode::Fill, &mut arena, b"", &mut [], &mut []);
        let first = p.intern(b"a");
        for n in b'b'..=b'l' {
            p.intern(&[n]);
        }
        assert_eq!(p.interned_count, 12);
        let used = p.arena.used();
        assert_eq!(p.intern(b"a"), first);
        assert_eq!(p.arena.used(), used);
        let thirteenth = p.intern(b"m");
        let repeated = p.intern(b"m");
        assert_ne!(thirteenth, repeated);
        assert_eq!(p.arena.used(), used + 4);
    }
    #[test]
    fn count_includes_missing_attributes_but_fill_skips() {
        let mut memory = [0; 64];
        let mut arena = Arena::new(&mut memory);
        let mut empty = Attrs {
            pairs: &[],
            cursor: 0,
        };
        let mut p = Package::new(Mode::Count, &mut arena, b"", &mut [], &mut []);
        p.start_element(b"manifest", &mut empty);
        p.start_element(b"item", &mut empty);
        p.start_element(b"spine", &mut empty);
        p.start_element(b"itemref", &mut empty);
        assert_eq!((p.manifest_count, p.spine_count), (1, 1));
        let mut manifest = [ManifestItem::default(); 1];
        let mut hashes = [0; 1];
        let mut p = Package::new(Mode::Fill, &mut arena, b"", &mut manifest, &mut hashes);
        p.start_element(b"manifest", &mut empty);
        p.start_element(b"item", &mut empty);
        p.start_element(b"spine", &mut empty);
        p.start_element(b"itemref", &mut empty);
        assert_eq!((p.manifest_used, p.spine_used), (0, 0));
        assert_eq!(p.error, Status::Ok);
    }
    struct Source {
        calls: usize,
    }
    impl BookSource for Source {
        fn size(&self) -> u64 {
            1
        }
        fn read_at(&mut self, _at: u64, out: &mut [u8]) -> i32 {
            self.calls += 1;
            out[0] = b'x';
            1
        }
    }
    struct Backend {
        creates: usize,
        closes: usize,
        malformed: bool,
    }
    impl ParserBackend for Backend {
        fn create(&mut self) -> bool {
            self.creates += 1;
            true
        }
        fn close(&mut self) {
            self.closes += 1;
        }
        fn feed(&mut self, bytes: &[u8], _: bool, h: &mut dyn Handler) -> bool {
            if !bytes.is_empty() {
                let mut a = Attrs {
                    pairs: &[],
                    cursor: 0,
                };
                h.start_element(b"metadata", &mut a);
                h.start_element(b"title", &mut a);
                h.text(b"A");
                h.end_element(b"title");
                h.end_element(b"metadata");
                h.start_element(b"manifest", &mut a);
                let mut item = Attrs {
                    pairs: &[(b"id", b"a"), (b"href", b"a")],
                    cursor: 0,
                };
                h.start_element(b"item", &mut item);
                h.end_element(b"manifest");
                h.start_element(b"spine", &mut a);
                let mut idref = Attrs {
                    pairs: &[(b"idref", b"a")],
                    cursor: 0,
                };
                h.start_element(b"itemref", &mut idref);
                h.end_element(b"spine");
            }
            !self.malformed
        }
    }
    #[test]
    fn two_pass_reborrow_and_late_spine_admission_do_not_publish_failure() {
        let mut source = Source { calls: 0 };
        let mut backend = Backend {
            creates: 0,
            closes: 0,
            malformed: false,
        };
        let mut buffer = [0; PARSE_CHUNK];
        let mut memory = [0; 64];
        let mut arena = Arena::new(&mut memory);
        let mut workflow = Workflow {
                policy: None,
            source: &mut source,
            backend: &mut backend,
            decoder: None,
            buffer: Some(&mut buffer),
            filter: None,
        };
        let mut manifest = [ManifestItem::default(); 1];
        let mut hashes = [0; 1];
        let mut out = PackageResult {
            spine_count: 77,
            ..PackageResult::default()
        };
        assert_eq!(
            parse_package(
                &mut workflow,
                &Entry::raw(1),
                b"",
                &mut arena,
                PackageStorage {
                    manifest: &mut manifest,
                    spine_hashes: &mut hashes,
                    spine: &mut []
                },
                &mut out
            ),
            Status::MetadataTooLarge
        );
        assert_eq!(out.spine_count, 77);
        assert!(manifest[0].id.is_some());
        assert_eq!(workflow.source.calls, 2);
        assert_eq!(workflow.backend.creates, 2);
        assert_eq!(workflow.backend.closes, 2);
    }
    #[test]
    fn xml_error_precedes_metadata_oom_and_preserves_out() {
        let mut source = Source { calls: 0 };
        let mut backend = Backend {
            creates: 0,
            closes: 0,
            malformed: true,
        };
        let mut buffer = [0; PARSE_CHUNK];
        let mut arena = Arena::default();
        let mut workflow = Workflow {
                policy: None,
            source: &mut source,
            backend: &mut backend,
            decoder: None,
            buffer: Some(&mut buffer),
            filter: None,
        };
        let mut out = PackageResult {
            spine_count: 77,
            ..PackageResult::default()
        };
        assert_eq!(
            parse_package(
                &mut workflow,
                &Entry::raw(1),
                b"",
                &mut arena,
                PackageStorage {
                    manifest: &mut [],
                    spine_hashes: &mut [],
                    spine: &mut []
                },
                &mut out
            ),
            Status::ParseError
        );
        assert_eq!(out.spine_count, 77);
        assert_eq!(workflow.backend.closes, 1);
    }
    #[test]
    fn ncx_attempts_title_even_if_path_resolution_failed() {
        let mut memory = [0; 1];
        let mut arena = Arena::new(&mut memory);
        let mut entries = [TocEntry::default(); 1];
        let mut handler = Toc::new(TocKind::Ncx, Mode::Fill, &mut arena, b"", &mut entries);
        let mut empty = Attrs {
            pairs: &[],
            cursor: 0,
        };
        handler.start_element(b"navPoint", &mut empty);
        let overlong = [b'x'; 512];
        let pairs = [(b"src".as_slice(), overlong.as_slice())];
        let mut attrs = Attrs {
            pairs: &pairs,
            cursor: 0,
        };
        handler.start_element(b"content", &mut attrs);
        assert!(handler.oom);
        assert_eq!(handler.used, 0);
        assert_eq!(handler.arena.used(), 1);
    }
    struct TocBackend {
        creates: usize,
        closes: usize,
    }
    impl ParserBackend for TocBackend {
        fn create(&mut self) -> bool {
            self.creates += 1;
            true
        }
        fn close(&mut self) {
            self.closes += 1;
        }
        fn feed(&mut self, bytes: &[u8], final_chunk: bool, handler: &mut dyn Handler) -> bool {
            if !bytes.is_empty() {
                let mut nav = Attrs {
                    pairs: &[(b"epub:type", b"toc")],
                    cursor: 0,
                };
                let mut link = Attrs {
                    pairs: &[(b"href", b"a")],
                    cursor: 0,
                };
                handler.start_element(b"nav", &mut nav);
                handler.start_element(b"a", &mut link);
                handler.text(b"A");
                handler.end_element(b"a");
                handler.end_element(b"nav");
            }
            !(self.creates == 2 && final_chunk)
        }
    }
    #[test]
    fn malformed_fill_leaves_count_unpublished_and_typed_partial_writes() {
        let mut source = Source { calls: 0 };
        let mut backend = TocBackend {
            creates: 0,
            closes: 0,
        };
        let mut buffer = [0; PARSE_CHUNK];
        let mut memory = [0; 32];
        let mut arena = Arena::new(&mut memory);
        let mut workflow = Workflow {
                policy: None,
            source: &mut source,
            backend: &mut backend,
            decoder: None,
            buffer: Some(&mut buffer),
            filter: None,
        };
        let mut entries = [TocEntry::default(); 1];
        let mut count = 77;
        assert_eq!(
            parse_toc(
                &mut workflow,
                &Entry::raw(1),
                b"",
                &mut arena,
                TocKind::Nav,
                &mut entries,
                &mut count
            ),
            Status::ParseError
        );
        assert_eq!(count, 77);
        assert!(entries[0].href.is_some());
        assert_eq!(workflow.backend.closes, 2);
    }
    #[test]
    fn allocated_package_uses_actual_counts_and_preserves_metadata_handles() {
        let mut source = Source { calls: 0 };
        let mut backend = Backend {
            creates: 0,
            closes: 0,
            malformed: false,
        };
        let mut buffer = [0; PARSE_CHUNK];
        let mut memory = [0; 64];
        let mut arena = Arena::new(&mut memory);
        let mut workflow = Workflow {
                policy: None,
            source: &mut source,
            backend: &mut backend,
            decoder: None,
            buffer: Some(&mut buffer),
            filter: None,
        };
        let mut manifest = [ManifestItem::default(); 1];
        let mut hashes = [0; 1];
        let mut spine = [9; 1];
        let mut out = PackageResult::default();
        let phase = core::cell::Cell::new(0);
        let storage = parse_package_allocated(
            &mut workflow,
            &Entry::raw(1),
            b"",
            &mut arena,
            |arena, manifests, spines| {
                assert_eq!((manifests, spines), (1, 1));
                assert_eq!(arena.used(), 2); // Count's title, before typed allocation.
                phase.set(1);
                Ok((&mut manifest, &mut hashes))
            },
            |_, count| {
                assert_eq!(phase.get(), 1);
                assert_eq!(count, 1);
                phase.set(2);
                Ok(&mut spine)
            },
            &mut out,
        )
        .unwrap();
        assert_eq!(phase.get(), 2);
        assert_eq!((out.manifest_count, out.spine_count), (1, 1));
        assert_eq!(
            arena.c_string(out.metadata.title.unwrap()),
            Some(b"A".as_slice())
        );
        assert_eq!(
            arena.c_string(storage.manifest[0].href.unwrap()),
            Some(b"a".as_slice())
        );
        assert_eq!(storage.spine[0], 0);
        assert_eq!((workflow.backend.creates, workflow.backend.closes), (2, 2));
    }

    #[test]
    fn allocated_package_preserves_fill_error_before_spine_allocation() {
        struct SecondPassFailure(Backend);
        impl ParserBackend for SecondPassFailure {
            fn create(&mut self) -> bool {
                self.0.create()
            }
            fn close(&mut self) {
                self.0.close();
            }
            fn feed(&mut self, bytes: &[u8], final_chunk: bool, handler: &mut dyn Handler) -> bool {
                self.0.feed(bytes, final_chunk, handler) && !(self.0.creates == 2 && final_chunk)
            }
        }
        let mut source = Source { calls: 0 };
        let mut backend = SecondPassFailure(Backend {
            creates: 0,
            closes: 0,
            malformed: false,
        });
        let mut buffer = [0; PARSE_CHUNK];
        let mut memory = [0; 64];
        let mut arena = Arena::new(&mut memory);
        let mut workflow = Workflow {
                policy: None,
            source: &mut source,
            backend: &mut backend,
            decoder: None,
            buffer: Some(&mut buffer),
            filter: None,
        };
        let mut manifest = [ManifestItem::default(); 1];
        let mut hashes = [0; 1];
        let mut out = PackageResult {
            spine_count: 77,
            ..PackageResult::default()
        };
        let result = parse_package_allocated(
            &mut workflow,
            &Entry::raw(1),
            b"",
            &mut arena,
            |_, _, _| Ok((&mut manifest, &mut hashes)),
            |_, _| panic!("spine allocation must follow successful fill"),
            &mut out,
        );
        assert!(matches!(result, Err(Status::ParseError)));
        assert_eq!(out.spine_count, 77);
        assert_eq!(workflow.backend.0.closes, 2);
    }
}
