//! One-pass OPF index builder. All record and path storage is caller-lent.
#![forbid(unsafe_code)]
use crate::{
    book_index::{Header, Identity, Member, NONE, MAX_SPINE, HEADER_BYTES, SpineItem, Writer, WriteError, name_hash},
    member_check::Checked,
    package_parsers::Workflow,
    page_cache::CacheStorage,
    phased_metadata::{BookLanguage, copy_display_title},
    xml_input::{TextCapture, has_token, local_is, resolve_href_checked, HrefError},
    xml_session::{Attributes, Handler, ParserBackend},
    zip_container::Status,
    zip_window::{arena::Arena, source::BookSource},
};

const TEXT: u8 = 1;
const NAV: u8 = 2;
const NCX: u8 = 4;
const COVER: u8 = 8;

#[derive(Clone, Copy, Debug, Default)]
pub struct Manifest {
    id: u64,
    member: u32,
    sequence: u32,
    flags: u8,
}

/// Collect inside `inspect_with_visitor`, then call `finish` before OPF parsing.
pub struct Members<'a> {
    records: &'a mut [Member],
    used: usize,
}
impl<'a> Members<'a> {
    pub fn new(records: &'a mut [Member]) -> Self { Self { records, used: 0 } }
    pub fn visit(&mut self, member: Member) -> Status {
        let Some(slot) = self.records.get_mut(self.used) else { return Status::MetadataTooLarge; };
        *slot = member;
        self.used += 1;
        Status::Ok
    }
    pub fn finish(self) -> Result<&'a mut [Member], Status> {
        let records = &mut self.records[..self.used];
        records.sort_unstable_by_key(|m| m.hash);
        if records.windows(2).any(|pair| pair[0].hash == pair[1].hash) {
            return Err(Status::Unsupported);
        }
        Ok(records)
    }
}

pub struct Scratch<'a> {
    pub manifest: &'a mut [Manifest],
    pub idrefs: &'a mut [u64],
    pub spine: &'a mut [SpineItem],
    pub path: &'a mut [u8],
    pub capture: &'a mut TextCapture,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error { Book(Status), Write(WriteError) }

pub struct Report {
    pub header: Header,
    /// Used record extents plus peak path arena, capture and writer backing.
    pub peak_bytes: usize,
}

struct Package<'a> {
    members: &'a mut [Member],
    scratch: Scratch<'a>,
    dir: &'a [u8],
    metadata_depth: usize,
    manifest_depth: usize,
    spine_depth: usize,
    field: u8,
    title: [u8; 256],
    title_set: bool,
    language: BookLanguage,
    language_set: bool,
    toc: Option<u64>,
    cover: Option<u64>,
    manifests: usize,
    idrefs: usize,
    raw_items: usize,
    raw_refs: usize,
    path_peak: usize,
    error: Status,
}
impl Package<'_> {
    fn fail(&mut self, status: Status) {
        if self.error == Status::Ok { self.error = status; }
    }
    fn item(&mut self, atts: &mut dyn Attributes) {
        self.raw_items += 1;
        let (Some(id), Some(href)) = (atts.local(b"id"), atts.local(b"href")) else { return; };
        if self.manifests == self.scratch.manifest.len() {
            self.fail(Status::MetadataTooLarge);
            return;
        }
        let mut arena = Arena::new(self.scratch.path);
        let path = resolve_href_checked(&mut arena, Some(self.dir), Some(href), None);
        self.path_peak = self.path_peak.max(arena.used());
        let hash = match path {
            Ok(path) => name_hash(arena.view(path).unwrap()),
            Err(error) => {
                self.fail(match error {
                    HrefError::TooLong => Status::PathTooLong,
                    HrefError::Capacity => Status::MetadataTooLarge,
                    HrefError::Missing => Status::ParseError,
                });
                return;
            }
        };
        let member = self.members.binary_search_by_key(&hash, |m| m.hash)
            .map_or(NONE, |i| i as u32);
        let media = atts.local(b"media-type");
        let mut flags = 0;
        if matches!(media, Some(b"application/xhtml+xml" | b"text/html")) { flags |= TEXT; }
        if media == Some(b"application/x-dtbncx+xml") { flags |= NCX; }
        if has_token(atts.local(b"properties"), b"nav") { flags |= NAV; }
        if has_token(atts.local(b"properties"), b"cover-image") { flags |= COVER; }
        self.scratch.manifest[self.manifests] = Manifest {
            id: name_hash(id), member, sequence: self.manifests as u32, flags,
        };
        self.manifests += 1;
    }
}
impl Handler for Package<'_> {
    fn start_element(&mut self, name: &[u8], atts: &mut dyn Attributes) {
        if local_is(name, b"metadata") { self.metadata_depth += 1; }
        else if local_is(name, b"manifest") { self.manifest_depth += 1; }
        else if local_is(name, b"spine") {
            self.spine_depth += 1;
            if self.toc.is_none() { self.toc = atts.local(b"toc").map(name_hash); }
        } else if self.metadata_depth > 0 && local_is(name, b"meta") {
            if atts.local(b"name") == Some(b"cover") {
                if let Some(id) = atts.local(b"content") { self.cover = Some(name_hash(id)); }
            }
        } else if self.metadata_depth > 0 {
            self.field = if local_is(name, b"title") { 1 }
                else if local_is(name, b"language") { 2 } else { 0 };
            if self.field != 0 { self.scratch.capture.begin(); }
        } else if self.manifest_depth > 0 && local_is(name, b"item") { self.item(atts); }
        else if self.spine_depth > 0 && local_is(name, b"itemref") {
            self.raw_refs += 1;
            if let Some(id) = atts.local(b"idref") {
                if let Some(slot) = self.scratch.idrefs.get_mut(self.idrefs) {
                    *slot = name_hash(id);
                    self.idrefs += 1;
                } else { self.fail(Status::MetadataTooLarge); }
            }
        }
    }
    fn end_element(&mut self, name: &[u8]) {
        if local_is(name, b"metadata") { self.metadata_depth = self.metadata_depth.saturating_sub(1); }
        else if local_is(name, b"manifest") { self.manifest_depth = self.manifest_depth.saturating_sub(1); }
        else if local_is(name, b"spine") { self.spine_depth = self.spine_depth.saturating_sub(1); }
        else if self.scratch.capture.active {
            let text = self.scratch.capture.trimmed();
            if !text.is_empty() {
                if self.field == 1 && !self.title_set {
                    copy_display_title(&mut self.title, Some(text));
                    self.title_set = true;
                } else if self.field == 2 && !self.language_set {
                    self.language = BookLanguage::from_opf(Some(text));
                    self.language_set = true;
                }
            }
            self.scratch.capture.end();
            self.field = 0;
        }
    }
    fn text(&mut self, text: &[u8]) { self.scratch.capture.add(text); }
}

/// Members must come from a successful `Members::finish` in this attempt.
/// The OPF is read once; no metadata strings survive that pass.
pub fn build<S: BookSource + ?Sized, B: ParserBackend + ?Sized, C: CacheStorage + ?Sized>(
    workflow: &mut Workflow<'_, S, B>, entry: &Checked, dir: &[u8],
    identity: Identity, members: &mut [Member], scratch: Scratch<'_>,
    store: &mut C, write_buffer: &mut [u8], receipt_live: &mut dyn FnMut() -> bool,
) -> Result<Report, Error> {
    if write_buffer.len() < HEADER_BYTES as usize {
        return Err(Error::Book(Status::MetadataTooLarge));
    }
    let mut package = Package {
        members, scratch, dir, metadata_depth: 0, manifest_depth: 0, spine_depth: 0,
        field: 0, title: [0; 256], title_set: false,
        language: BookLanguage::Default, language_set: false, toc: None, cover: None,
        manifests: 0, idrefs: 0, raw_items: 0, raw_refs: 0, path_peak: 0, error: Status::Ok,
    };
    package.scratch.capture.end();
    let status = workflow.pass(entry, &mut package);
    if status != Status::Ok { return Err(Error::Book(status)); }
    if package.error != Status::Ok { return Err(Error::Book(package.error)); }
    if package.raw_items == 0 || package.raw_refs == 0 { return Err(Error::Book(Status::NotEpub)); }
    if !package.title_set { copy_display_title(&mut package.title, None); }
    let mut header = Header {
        identity, members: package.members.len() as u32, spine: 0, spine_bytes: 0,
        nav: NONE, ncx: NONE, cover: NONE, title: package.title, language: package.language,
    };
    let manifest = &mut package.scratch.manifest[..package.manifests];
    let nav = manifest.iter().find(|m| m.flags & NAV != 0).copied();
    let ncx = manifest.iter().find(|m| m.flags & NCX != 0 || Some(m.id) == package.toc).copied();
    let cover_id = manifest.iter().find(|m| Some(m.id) == package.cover).map(|m| m.sequence);
    header.cover = manifest.iter().find(|m| m.flags & COVER != 0 || Some(m.sequence) == cover_id)
        .map_or(NONE, |m| m.member);
    if let Some(nav) = nav { header.nav = nav.member; }
    else if let Some(ncx) = ncx { header.ncx = ncx.member; }
    manifest.sort_unstable_by_key(|m| (m.id, m.sequence));
    let mut valid = true;
    for id in &package.scratch.idrefs[..package.idrefs] {
        let at = manifest.partition_point(|m| m.id < *id);
        let Some(item) = manifest.get(at).filter(|m| m.id == *id) else { continue; };
        if header.spine == MAX_SPINE { return Err(Error::Book(Status::MetadataTooLarge)); }
        let Some(slot) = package.scratch.spine.get_mut(header.spine as usize) else {
            return Err(Error::Book(Status::MetadataTooLarge));
        };
        *slot = SpineItem { member: item.member, before: header.spine_bytes };
        if let Some(member) = package.members.get_mut(item.member as usize) {
            if member.first_spine == NONE { member.first_spine = header.spine; }
            if item.flags & TEXT != 0 {
                if !matches!(member.method, 0 | 8) || member.uncompressed > 4 * 1024 * 1024 { valid = false; }
                header.spine_bytes += u64::from(member.uncompressed);
            }
        } else if item.flags & TEXT != 0 { valid = false; }
        header.spine += 1;
    }
    if header.spine == 0 { return Err(Error::Book(Status::NotEpub)); }
    let spine = &mut package.scratch.spine[..header.spine as usize];
    if !valid || header.spine_bytes > 64 * 1024 * 1024 {
        header.spine_bytes = 0;
        for item in spine.iter_mut() { item.before = 0; }
    }
    let peak_bytes = core::mem::size_of_val(package.members)
        + core::mem::size_of_val(manifest)
        + package.idrefs * core::mem::size_of::<u64>()
        + core::mem::size_of_val(spine) + package.path_peak
        + core::mem::size_of::<TextCapture>() + write_buffer.len();
    let mut writer = Writer::begin(store, header, write_buffer).map_err(Error::Write)?;
    for &member in package.members.iter() { writer = writer.member(store, member).map_err(Error::Write)?; }
    for &item in spine.iter() { writer = writer.spine(store, item).map_err(Error::Write)?; }
    writer.finish(store, receipt_live).map_err(Error::Write)?;
    Ok(Report { header, peak_bytes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_capacity_sorting_and_collision_refusal() {
        let mut records = [Member::default(); 2];
        let mut members = Members::new(&mut records);
        assert_eq!(members.visit(Member { hash: 2, ..Member::default() }), Status::Ok);
        assert_eq!(members.visit(Member { hash: 1, ..Member::default() }), Status::Ok);
        assert_eq!(members.visit(Member::default()), Status::MetadataTooLarge);
        let records = members.finish().unwrap();
        assert_eq!((records[0].hash, records[1].hash), (1, 2));
        let mut members = Members::new(records);
        for _ in 0..2 { assert_eq!(members.visit(Member::default()), Status::Ok); }
        assert!(matches!(members.finish(), Err(Status::Unsupported)));
    }
}
