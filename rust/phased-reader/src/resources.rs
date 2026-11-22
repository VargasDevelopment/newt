use crate::styles::BuildScratch;
use newt_core::{
    chapter_layout::types::{Images, LineBreak, Storage},
    zip_container::InflateScratch,
};

pub fn decoder<'a>(d: &'a mut Option<InflateScratch<'_>>) -> Option<InflateScratch<'a>> {
    d.as_mut().map(|d| InflateScratch {
        state: &mut *d.state,
        dictionary: &mut *d.dictionary,
        input: &mut *d.input,
    })
}
pub fn storage<'a>(s: &'a mut Storage<'_>) -> Storage<'a> {
    Storage {
        text: s.text,
        breaks: s.breaks,
        levels: s.levels,
        spans: s.spans,
        lines: s.lines,
        elements: s.elements,
        runs: s.runs,
        images: s.images,
        links: s.links,
        page: s.page,
        style_text: s.style_text,
        paragraph_links: s.paragraph_links,
    }
}
/// Layout-only resources. XML/backend/decoder are reborrowed sequentially from
/// canonical Resources, never held in two phase views at once.
pub struct LayoutResources<'a> {
    pub storage: Storage<'a>,
    pub breaker: &'a mut dyn LineBreak,
    pub images: CataloglessImages<'a>,
}
pub struct StyleResources<'a, 's> {
    pub directory: &'a mut [u8; 512],
    pub paths: &'a mut newt_core::zip_window::arena::Arena<'s>,
    pub build: Option<BuildScratch<'a>>,
}
/// Exact BookImages null-catalog branch: resolve paths/fragments with Core,
/// never probe the ZIP. No unused second source capability is required.
pub struct CataloglessImages<'a> {
    pub directory: &'a [u8],
    pub path_scratch: &'a mut [u8],
}
impl Images for CataloglessImages<'_> {
    fn resolve(&mut self, src: &[u8], path: &mut [u8], frag: &mut [u8]) -> Option<(usize, usize)> {
        let mut arena = newt_core::zip_window::arena::Arena::new(self.path_scratch);
        let mut fragment = None;
        let resolved = crate::xml_input::resolve_href(
            &mut arena,
            Some(self.directory),
            Some(src),
            if frag.is_empty() {
                None
            } else {
                Some(&mut fragment)
            },
        )?;
        let bytes = crate::xml_input::c_bytes(arena.view(resolved)?);
        if bytes.len() > path.len() {
            return None;
        }
        path[..bytes.len()].copy_from_slice(bytes);
        let n = bytes.len();
        let mut nf = 0;
        if let Some(f) = fragment {
            let bytes = crate::xml_input::c_bytes(arena.view(f)?);
            nf = bytes.len().min(frag.len());
            frag[..nf].copy_from_slice(&bytes[..nf]);
        }
        Some((n, nf))
    }
    fn probe(&mut self, _: &[u8]) -> Option<(u32, u32)> {
        None
    }
}
/// Exclusive resource receipt. The caller loans actual typed parse storage
/// only inside an operation closure and reclaims it when the closure returns.
/// `returned` runs after live parser/decoder use ends, before capture returns.
/// Actual typed loans return to the caller when capture returns. Drop those
/// views before reclaiming the physical owner and calling publish; no reentry.
pub struct Lease {
    pub acquire: fn() -> bool,
    pub returned: fn(),
}
pub(crate) struct Held<'a>(&'a Lease);
impl Lease {
    pub(crate) fn hold(&self) -> Result<Held<'_>, newt_core::zip_container::Status> {
        if (self.acquire)() {
            Ok(Held(self))
        } else {
            Err(newt_core::zip_container::Status::OutOfMemory)
        }
    }
}
impl Drop for Held<'_> {
    fn drop(&mut self) {
        (self.0.returned)();
    }
}

impl CataloglessImages<'_> {
    // Derive context from this operation's canonical selection, never a caller's
    // previous chapter. Borrowing the href adds no directory buffer or copy.
    pub(crate) fn for_chapter<'b>(&'b mut self, href: &'b [u8]) -> CataloglessImages<'b> {
        let href = crate::xml_input::c_bytes(href);
        let end = href.iter().rposition(|&b| b == b'/').unwrap_or(0);
        CataloglessImages {
            directory: &href[..end],
            path_scratch: &mut *self.path_scratch,
        }
    }
}
