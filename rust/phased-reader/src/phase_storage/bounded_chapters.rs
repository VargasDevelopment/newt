// SPDX-License-Identifier: MIT
//! Blocking SDK chapter work in the sample's existing layout and parse banks.
#![forbid(unsafe_code)]
use super::{Parts, ParseResources};
use crate::{bounded::{self, Chapters, LaidOut, LAYOUT_BYTES, PARSE_BYTES},
    reader::{reader_parameters, validate_exact_anchor_stream}, BookLanguage, xml_input::{self, EntityFilter},
    xml_session::{self, ParserBackend, XmlStream}, zip_container::{Entry, Reader, Status}, bounded::source::Source};
use newt_core::{chapter_layout::{images::{self, BookImages, Catalog, Collector, Probed},
    types::{self, LineBreak, Sink}}, css::{CssRule, CssStylesheetBuilder, MAX_RULES},
    preferences::ViewSettings};
use newt_reader_navigation::{BookView, metadata::Metadata};

/// One sample-memory loan. The two source views below borrow these same
/// immutable bytes; neither owns another book, cursor, file, or identity.
pub struct Work<'a, B: ParserBackend + ?Sized> {
    epub: &'a [u8],
    layout: &'a mut [u8; LAYOUT_BYTES],
    parse: &'a mut [u8; PARSE_BYTES],
    backend: &'a mut B,
    breaker: &'a mut dyn LineBreak,
}
impl<'a, B: ParserBackend + ?Sized> Work<'a, B> {
    pub fn new(epub: &'a [u8], layout: &'a mut [u8; LAYOUT_BYTES],
        parse: &'a mut [u8; PARSE_BYTES], backend: &'a mut B,
        breaker: &'a mut dyn LineBreak,
    ) -> Self {
        Self { epub, layout, parse, backend, breaker }
    }
}
impl<B: ParserBackend + ?Sized> Chapters for Work<'_, B> {
    fn with_mapping<R>(&mut self, count: usize,
        operation: impl FnOnce(&mut [u32]) -> R) -> Result<R, Status> {
        let mut parts = Parts(&mut *self.parse);
        Ok(operation(parts.array::<u32>(count)?))
    }
    fn layout(&mut self, book: &Metadata<'_, '_>, spine: u32,
        settings: ViewSettings, sink: &mut dyn Sink,
    ) -> Result<LaidOut, Status> {
        let href = book.spine_item(spine as usize).and_then(|item| item.href)
            .ok_or(Status::NotFound)?;
        let catalog = Members(book);
        let entry = catalog.find(href).ok_or(Status::NotFound)?;
        let directory = directory(href);
        let language = BookLanguage::from_opf(book.package.metadata.language
            .and_then(|h| book.arena.c_string(h)));
        let params = reader_parameters(settings.page_layout(), None, language);
        // The raw detector precedes the optional table allocation, as in the
        // SDK. Failed optional work leaves inline probes to the main pass.
        if mentions(self.epub, &entry, self.parse) {
            let mut parts = Parts(&mut *self.layout);
            if let Ok(table) = parts.array::<Probed>(96) {
                let count = prescan(self.epub, &entry, directory, &catalog,
                    self.parse, self.backend, table);
                if count > 0 {
                    return run(self.epub, &entry, directory, &catalog, params,
                        parts.0, &table[..count], self.parse, self.backend,
                        self.breaker, sink);
                }
            }
        }
        // All table views have ended; the empty/failed prescan's full prefix
        // is available again, rather than charging dead bytes to layout.
        run(self.epub, &entry, directory, &catalog, params, &mut *self.layout,
            &[], self.parse, self.backend, self.breaker, sink)
    }

    fn validate_anchor(&mut self, book: &Metadata<'_, '_>, spine: u32,
        fragment: &[u8],
    ) -> Status {
        let Some(href) = book.spine_item(spine as usize).and_then(|item| item.href) else {
            return Status::NotFound;
        };
        let Some(entry) = Members(book).find(href) else { return Status::NotFound; };
        if fragment.is_empty() { return Status::Ok; }
        let mut parts = Parts(&mut *self.parse);
        let resources = (|| Ok::<_, Status>((parts.fixed::<2048>()?,
            parts.one(EntityFilter::new())?)))();
        let (buffer, filter) = match resources {
            Ok(resources) => resources,
            Err(status) => return status,
        };
        let mut source = Source(self.epub);
        let mut io = ParseResources { backend: self.backend, decoder: None,
            xml_buffer: Some(buffer), filter: Some(filter) };
        validate_exact_anchor_stream(&mut source, &entry, fragment, &mut io, None)
    }
}

struct Members<'a, 'book, 'storage>(&'a Metadata<'book, 'storage>);
impl Catalog for Members<'_, '_, '_> {
    fn find(&self, path: &[u8]) -> Option<Entry> {
        let hash = crate::zip_container::hash_path(path);
        self.0.entries.iter().find(|entry| entry.name_hash == hash
            && entry.name.and_then(|name| self.0.arena.c_string(name))
                .is_some_and(|name| xml_input::c_bytes(name) == xml_input::c_bytes(path)))
            .copied()
    }
    fn find_hash(&self, hash: u32) -> Option<Entry> {
        self.0.entries.iter().find(|entry| entry.name_hash == hash).copied()
    }
}
fn directory(href: &[u8]) -> &[u8] {
    let href = xml_input::c_bytes(href);
    match href.iter().rposition(|&byte| byte == b'/') {
        Some(end) => &href[..end],
        None => &href[..0],
    }
}
fn mentions(epub: &[u8], entry: &Entry, parse: &mut [u8; PARSE_BYTES]) -> bool {
    let mut parts = Parts(parse);
    let buffer = parts.fixed::<2048>();
    let Ok(buffer) = buffer else { return true; };
    let mut source = Source(epub);
    let mut reader = Reader::new(&mut source, None);
    reader.open(entry) != Status::Ok || images::mentions(&mut reader, buffer)
}

struct Parse<'a> {
    buffer: &'a mut [u8; 2048],
    filter: &'a mut EntityFilter,
    skip: &'a mut [u8; 1024],
    paths: &'a mut [u8],
}
fn parse(bank: &mut [u8; PARSE_BYTES])
    -> Result<Parse<'_>, Status>
{
    let mut parts = Parts(&mut *bank);
    let buffer = parts.fixed::<2048>()?;
    let filter = parts.one(EntityFilter::new())?;
    let skip = parts.fixed::<1024>()?;
    Ok(Parse { buffer, filter, skip, paths: parts.0 })
}
fn prescan<B: ParserBackend + ?Sized>(epub: &[u8], entry: &Entry,
    directory: &[u8], catalog: &dyn Catalog, bank: &mut [u8; PARSE_BYTES],
    backend: &mut B, table: &mut [Probed],
) -> usize {
    let Ok(io) = parse(bank) else { return 0; };
    let mut source = Source(epub);
    let mut book_source = Source(epub);
    let mut access = BookImages { source: &mut book_source, catalog: Some(catalog),
        directory, decoder: None, skip: io.skip, path_scratch: io.paths, table: &[] };
    let mut collector = Collector { images: &mut access, table: &mut *table,
        count: 0, stopped: false };
    let mut stream = XmlStream::new(&mut source, None, backend);
    let mut status = stream.open(entry, xml_session::Resources {
        buffer: Some(io.buffer), filter: Some(io.filter) }, true);
    let mut end = false;
    while status == Status::Ok && !end && !collector.stopped {
        status = stream.feed_chunk(&mut collector, Some(&mut end));
    }
    stream.close();
    drop(stream);
    let count = collector.count;
    drop(collector);
    if images::keep_prescan(status, count) {
        images::probe_collected(&mut access, &mut table[..count]);
        count
    } else { 0 }
}

struct Layout<'a> { storage: types::Storage<'a>, chapter_css: CssStylesheetBuilder<'a> }
fn storage<'a>(parts: &mut Parts<'a>) -> Result<Layout<'a>, Status> {
    // These are inline members of the SDK LayoutEngine. Rust lends the actual
    // typed arrays from the same bank before init's remaining allocations.
    let elements = parts.array::<types::Element>(16)?;
    let paragraph_links = parts.array::<types::ParLink>(6)?;
    let text = parts.fixed::<3072>()?;
    let breaks = parts.fixed::<3072>()?;
    let levels = parts.fixed::<3072>()?;
    let spans = parts.array::<types::Span>(96)?;
    let runs = parts.array::<types::Run>(304)?;
    let images = parts.array::<types::Image>(8)?;
    let links = parts.array::<types::Link>(16)?;
    let lines = parts.array::<types::Line>(176)?;
    let style_text = parts.bytes(3072, 1).unwrap_or(&mut []);
    let mut chapter_css = CssStylesheetBuilder::default();
    // SDK init attempts its optional stylesheet even when embeddedStyles=false.
    // A denied table leaves the cursor unchanged and does not deny the page.
    if !style_text.is_empty() {
        if let Ok(rules) = parts.array::<CssRule>(MAX_RULES) { chapter_css.begin(rules); }
    }
    // The SDK's page arena is max_align_t-aligned on both supported targets.
    let page = parts.bytes(7168, 16)?;
    Ok(Layout { storage: types::Storage { text, breaks, levels, spans, lines,
        elements, runs, images, links, page, style_text, paragraph_links }, chapter_css })
}
fn run<B: ParserBackend + ?Sized>(epub: &[u8], entry: &Entry,
    directory: &[u8], catalog: &dyn Catalog, params: types::Params<'_>,
    bank: &mut [u8], table: &[Probed], parse_bank: &mut [u8; PARSE_BYTES],
    backend: &mut B, breaker: &mut dyn LineBreak, sink: &mut dyn Sink,
) -> Result<LaidOut, Status> {
    let mut parts = Parts(bank);
    let Layout { storage, chapter_css } = storage(&mut parts)?;
    let io = parse(parse_bank)?;
    let mut source = Source(epub);
    let mut book_source = Source(epub);
    // A full table may omit later images; BookImages probes misses inline
    // against the same metadata catalog and same immutable EPUB bytes.
    let mut access = BookImages { source: &mut book_source, catalog: Some(catalog),
        directory, decoder: None, skip: io.skip, path_scratch: io.paths, table };
    bounded::layout::Chapter { source: &mut source, entry, params, storage, chapter_css,
        breaker, images: &mut access, parser: backend, buffer: io.buffer,
        filter: io.filter }.run(sink)
}

#[cfg(test)]
#[path = "bounded_chapters_cases.rs"]
mod cases;
