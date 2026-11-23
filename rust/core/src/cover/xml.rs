// SPDX-License-Identifier: MIT
//! Exact cover-only namespace/attribute policy, supplied Expat callbacks.
//!
//! A package names its cover in ranked ways; the best one present wins:
//! 0. the `cover-image` item (EPUB 3);
//! 1. the image item `<meta name="cover">` names (EPUB 2);
//! 2. the guide's cover reference: an image item, or an XHTML cover page whose
//!    first image is the cover.
//! A cover page is parsed only when it outranks every image; when it gives no
//! image, the best image is the fallback.
use super::{MetadataStatus as S, discovery::{path, cbytes}};
use crate::xml_session::{Attributes, Handler};
#[derive(Clone, Copy)]
#[repr(u8)]
enum Role { Other, Container, Rootfiles, Package, Metadata, Manifest, Guide, Html, Body }
/// The document one parse reads.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind { Container, Package, Page }
/// What the package offers. A container parse leaves the package path in
/// `image`. Every byte zero is a valid empty value.
#[derive(Clone, Copy)]
pub struct Found { pub image: [u8; 512], pub page: [u8; 512] }
/// Bytes of the per-parse scratch block the caller lends to `State::new`.
pub const SCRATCH: usize = 3 * 512;
const NONE: u8 = u8::MAX;
const META_ID: u8 = 1;
const GUIDE: u8 = 2;
pub struct State<'a> {
    pub status: S,
    roles: [Role; 32], depth: usize,
    kind: Kind, pub resolve: bool, pub saw_root: bool,
    opf: &'a [u8], found: &'a mut Found,
    meta: &'a mut [u8; 512], guide: &'a mut [u8; 512], item: &'a mut [u8; 512],
    image_rank: u8, page_rank: u8, images: u8,
}
fn carve<'a>(bytes: &mut &'a mut [u8]) -> Option<&'a mut [u8; 512]> {
    let (head, tail) = core::mem::take(bytes).split_first_chunk_mut::<512>()?;
    *bytes = tail;
    Some(head)
}
impl<'a> State<'a> {
    /// A page parse keeps `found`; the others start it empty. None when
    /// `scratch` is shorter than `SCRATCH`.
    pub fn new(kind: Kind, opf: &'a [u8], found: &'a mut Found, mut scratch: &'a mut [u8]) -> Option<Self> {
        let (meta, guide, item) = (carve(&mut scratch)?, carve(&mut scratch)?, carve(&mut scratch)?);
        (meta[0], guide[0], item[0]) = (0, 0, 0);
        if kind != Kind::Page { found.image[0] = 0; found.page[0] = 0; }
        Some(Self { status: S::Ok, roles: [Role::Other; 32], depth: 0, kind, resolve: false,
            saw_root: false, opf, found, meta, guide, item, image_rank: NONE, page_rank: NONE, images: 0 })
    }
    pub fn complete(&self) -> S {
        if self.status != S::Ok { self.status }
        else if self.saw_root && self.depth == 0 { S::Ok } else { S::Malformed }
    }
    /// After pass 1: whether a lower-ranked candidate needs the manifest again.
    pub fn second_pass(&self) -> bool { self.image_rank != 0 && (self.meta[0] != 0 || self.guide[0] != 0) }
    /// After the package: keep the page only when it outranks every image.
    pub fn settle(&mut self) -> S {
        if self.page_rank >= self.image_rank { self.found.page[0] = 0; }
        if self.found.image[0] == 0 && self.found.page[0] == 0 { S::NotFound } else { S::Ok }
    }
    /// After a page: its first image becomes the cover.
    pub fn take_page_image(&mut self) -> S {
        if self.item[0] == 0 { return S::NotFound; }
        self.found.image = *self.item;
        S::Ok
    }
    /// Only a cover page may carry a DOCTYPE, and never an internal subset:
    /// entities and DTDs are never expanded.
    pub fn doctype(&mut self, internal_subset: bool) {
        if self.kind != Kind::Page || internal_subset { self.status = S::Unsupported; }
    }
    pub fn reject(&mut self) { self.status = S::Unsupported; }
    fn package_item(&mut self, attrs: &mut dyn Attributes) {
        let media = attrs.exact(b"media-type");
        // "image/jpg" is not a registered type, but common in books.
        let image = matches!(media, Some(b"image/png" | b"image/jpeg" | b"image/jpg"));
        if !self.resolve {
            if attrs.exact(b"properties").is_some_and(|p|
                p.split(|c| b" \t\r\n".contains(c)).any(|p| p == b"cover-image")) {
                if self.image_rank == 0 || !image
                    || !attrs.exact(b"href").is_some_and(|href| path(self.opf, href, &mut self.found.image)) {
                    self.status = S::Unsupported;
                }
                self.image_rank = 0;
            }
            return;
        }
        // An item whose href cannot be a book path names nothing.
        if !attrs.exact(b"href").is_some_and(|href| path(self.opf, href, self.item)) { return; }
        let rank = if image && self.meta[0] != 0 && attrs.exact(b"id") == Some(cbytes(self.meta)) { META_ID }
            else if self.guide[0] != 0 && cbytes(self.item) == cbytes(self.guide) { GUIDE }
            else { NONE };
        if image && rank < self.image_rank { self.found.image = *self.item; self.image_rank = rank; }
        else if media == Some(b"application/xhtml+xml") && rank < self.page_rank {
            self.found.page = *self.item; self.page_rank = rank;
        }
    }
    fn page_image(&mut self, name: &[u8], attrs: &mut dyn Attributes) {
        let source = if tag(name, b"img", XHTML) { attrs.exact(b"src") }
            else if tag(name, b"image", SVG) { attrs.exact(XLINK_HREF).or_else(|| attrs.exact(b"href")) }
            else { return };
        self.images = self.images.saturating_add(1);
        if self.images == 1 && !source.is_some_and(|href| path(&self.found.page, href, self.item)) {
            self.item[0] = 0;
        }
    }
}
fn tag(name: &[u8], plain: &[u8], namespace: &[u8]) -> bool {
    name == plain || (name.len() == namespace.len() + 1 + plain.len()
        && name.starts_with(namespace) && name[namespace.len()] == b'}'
        && &name[namespace.len() + 1..] == plain)
}
const CONTAINER: &[u8] = b"urn:oasis:names:tc:opendocument:xmlns:container";
const OPF: &[u8] = b"http://www.idpf.org/2007/opf";
const XHTML: &[u8] = b"http://www.w3.org/1999/xhtml";
const SVG: &[u8] = b"http://www.w3.org/2000/svg";
const XLINK_HREF: &[u8] = b"http://www.w3.org/1999/xlink}href";
impl Handler for State<'_> {
    fn start_element(&mut self, name: &[u8], attrs: &mut dyn Attributes) {
        if self.status != S::Ok { return; }
        if self.depth == 32 || name.len() > 511 { self.status = S::Unsupported; return; }
        while let Some((key, value)) = attrs.next() {
            if key.len() > 511 || value.len() > 2048 { self.status = S::Unsupported; return; }
        }
        let parent = if self.depth == 0 { Role::Other } else { self.roles[self.depth - 1] };
        let role = if self.depth == 0 {
            self.saw_root = true;
            match self.kind {
                Kind::Container if tag(name, b"container", CONTAINER) => Role::Container,
                Kind::Package if tag(name, b"package", OPF) => Role::Package,
                Kind::Page if tag(name, b"html", XHTML) => Role::Html,
                _ => { self.status = S::Malformed; return; }
            }
        } else if matches!(parent, Role::Container) && tag(name, b"rootfiles", CONTAINER) { Role::Rootfiles }
        else if matches!(parent, Role::Package) && tag(name, b"metadata", OPF) { Role::Metadata }
        else if matches!(parent, Role::Package) && tag(name, b"manifest", OPF) { Role::Manifest }
        else if matches!(parent, Role::Package) && tag(name, b"guide", OPF) { Role::Guide }
        else if matches!(parent, Role::Html) && tag(name, b"body", XHTML) { Role::Body }
        // Everything under body counts as body, so nested images count.
        else if matches!(parent, Role::Body) { Role::Body }
        else { Role::Other };
        self.roles[self.depth] = role; self.depth += 1;
        if matches!(parent, Role::Rootfiles) && tag(name, b"rootfile", CONTAINER) {
            if attrs.exact(b"media-type") == Some(b"application/oebps-package+xml") && self.found.image[0] == 0
                && !attrs.exact(b"full-path").is_some_and(|href| path(b"", href, &mut self.found.image)) {
                self.status = S::Unsupported;
            }
        } else if !self.resolve && matches!(parent, Role::Metadata) && tag(name, b"meta", OPF) {
            // The first usable cover meta counts; a broken one names nothing.
            if attrs.exact(b"name") == Some(b"cover") && self.meta[0] == 0
                && let Some(id) = attrs.exact(b"content") && id.len() < 512 {
                self.meta[..id.len()].copy_from_slice(id); self.meta[id.len()] = 0;
            }
        } else if !self.resolve && matches!(parent, Role::Guide) && tag(name, b"reference", OPF) {
            if attrs.exact(b"type").is_some_and(|t| t.eq_ignore_ascii_case(b"cover")) && self.guide[0] == 0
                && let Some(href) = attrs.exact(b"href") {
                let file = &href[..href.iter().position(|&c| c == b'#').unwrap_or(href.len())];
                if !path(self.opf, file, self.guide) { self.guide[0] = 0; }
            }
        } else if matches!(parent, Role::Manifest) && tag(name, b"item", OPF) {
            self.package_item(attrs);
        } else if matches!(parent, Role::Body) {
            self.page_image(name, attrs);
        }
    }
    fn end_element(&mut self, _name: &[u8]) { self.depth = self.depth.saturating_sub(1); }
    fn stop_parse(&self) -> bool { self.status != S::Ok }
}
