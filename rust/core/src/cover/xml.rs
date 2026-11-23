// SPDX-License-Identifier: MIT
//! Exact cover-only namespace/attribute policy, supplied Expat callbacks.
use super::{MetadataStatus as S, discovery::{path, cbytes}};
use crate::xml_session::{Attributes, Handler};
#[derive(Clone, Copy)]
#[repr(u8)]
enum Role { Other, Container, Rootfiles, Package, Metadata, Manifest }
pub struct State<'a> {
    pub status: S,
    roles: [Role; 32], depth: usize,
    pub container: bool, pub resolve_id: bool, pub saw_root: bool,
    opf: &'a [u8], pub id: [u8; 512], pub path: [u8; 512],
}
impl<'a> State<'a> {
    pub fn new(container: bool, opf: &'a [u8]) -> Self {
        Self { status: S::Ok, roles: [Role::Other; 32], depth: 0, container,
            resolve_id: false, saw_root: false, opf, id: [0; 512], path: [0; 512] }
    }
    pub fn complete(&self) -> S {
        if self.status != S::Ok { self.status }
        else if self.saw_root && self.depth == 0 { S::Ok } else { S::Malformed }
    }
    pub fn reject_doctype(&mut self) { self.status = S::Unsupported; }
}
fn tag(name: &[u8], plain: &[u8], namespace: &[u8]) -> bool {
    name == plain || (name.len() == namespace.len() + 1 + plain.len()
        && name.starts_with(namespace) && name[namespace.len()] == b'}'
        && &name[namespace.len() + 1..] == plain)
}
const CONTAINER: &[u8] = b"urn:oasis:names:tc:opendocument:xmlns:container";
const OPF: &[u8] = b"http://www.idpf.org/2007/opf";
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
            if self.container && tag(name, b"container", CONTAINER) { Role::Container }
            else if !self.container && tag(name, b"package", OPF) { Role::Package }
            else { self.status = S::Malformed; return; }
        } else if matches!(parent, Role::Container) && tag(name, b"rootfiles", CONTAINER) { Role::Rootfiles }
        else if matches!(parent, Role::Package) && tag(name, b"metadata", OPF) { Role::Metadata }
        else if matches!(parent, Role::Package) && tag(name, b"manifest", OPF) { Role::Manifest }
        else { Role::Other };
        self.roles[self.depth] = role; self.depth += 1;
        if matches!(parent, Role::Rootfiles) && tag(name, b"rootfile", CONTAINER) {
            if attrs.exact(b"media-type") == Some(b"application/oebps-package+xml") && self.path[0] == 0
                && !attrs.exact(b"full-path").is_some_and(|href| path(b"", href, &mut self.path)) {
                self.status = S::Unsupported;
            }
        } else if !self.resolve_id && matches!(parent, Role::Metadata) && tag(name, b"meta", OPF) {
            if attrs.exact(b"name") == Some(b"cover") {
                match attrs.exact(b"content") {
                    Some(id) if !id.is_empty() && id.len() < 512 && self.id[0] == 0 => {
                        self.id[..id.len()].copy_from_slice(id); self.id[id.len()] = 0;
                    }
                    _ => self.status = S::Unsupported,
                }
            }
        } else if matches!(parent, Role::Manifest) && tag(name, b"item", OPF) {
            let selected = if self.resolve_id { attrs.exact(b"id") == Some(cbytes(&self.id)) }
                else { attrs.exact(b"properties").is_some_and(|p|
                    p.split(|c| b" \t\r\n".contains(c)).any(|p| p == b"cover-image")) };
            if selected && (self.path[0] != 0
                // "image/jpg" is not a registered type, but common in books.
                || !matches!(attrs.exact(b"media-type"), Some(b"image/png" | b"image/jpeg" | b"image/jpg"))
                || !attrs.exact(b"href").is_some_and(|href| path(self.opf, href, &mut self.path))) {
                self.status = S::Unsupported;
            }
        }
    }
    fn end_element(&mut self, _name: &[u8]) { self.depth = self.depth.saturating_sub(1); }
    fn stop_parse(&self) -> bool { self.status != S::Ok }
}
