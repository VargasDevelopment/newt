//! Runtime borrowed code generation for the shared chapter core; no native backend or firmware.

use newt_core::member_check::Checked;
use newt_core::{
    css::CssStylesheetBuilder,
    chapter_layout::{engine::Engine, images::{self, BookImages, Probed},
        session::{ChapterSession, LayoutHandler},
        types::{Backend, Capacity, Images, LineBreak, Page, Params, Storage}},
    page_renderer, xml_input::EntityFilter,
    xml_session::{BorrowedPolicy, ProcessingPolicy, ProducerBackend, Resources, XmlStream},
    zip_container::{Status}, zip_window::source::BookSource,
};

#[unsafe(no_mangle)]
pub fn nrust_chapter_plain_borrowed(
    source: &mut dyn BookSource, engine: &mut Engine<'_>, buffer: &mut [u8; 4096],
) -> Status {
    newt_core::chapter_layout::plain::layout(source, engine, buffer)
}

#[unsafe(no_mangle)]
pub fn nrust_chapter_construct_borrowed<'a>(
    params: Params<'a>, storage: Storage<'a>, backend: &'a mut dyn Backend,
    breaker: &'a mut dyn LineBreak, images: &'a mut dyn Images,
    stylesheet: CssStylesheetBuilder<'a>, text: &[u8], output: &mut [u32; 2],
) -> Result<bool, Capacity> {
    let mut engine = Engine::new(params, storage, backend, breaker, images, None, stylesheet)?;
    engine.start(b"body", None, None, None, None, None);
    engine.start(b"p", None, None, None, None, None);
    engine.text(text);
    engine.end(b"p");
    engine.end(b"body");
    let finished = engine.finish();
    *output = [engine.pages, engine.chars];
    Ok(finished)
}

#[unsafe(no_mangle)]
pub fn nrust_chapter_xml_borrowed(
    source: &mut dyn BookSource, entry: &Checked, backend: &mut dyn ProducerBackend,
    policy: &mut dyn ProcessingPolicy, buffer: &mut [u8; 2048],
    filter: &mut EntityFilter, engine: &mut Engine<'_>, limits: [u32; 2], output: &mut u64,
) -> Status {
    let stream = XmlStream::with_policy(source, None, backend, BorrowedPolicy(policy));
    let mut session = ChapterSession::new(stream);
    let mut status = session.begin(entry, Resources { buffer: Some(buffer), filter: Some(filter) }, engine);
    if status == Status::Ok {
        status = session.step_bounded(&mut LayoutHandler { engine }, limits[0], limits[1]);
    }
    *output = session.bytes_consumed();
    status
}

#[unsafe(no_mangle)]
pub fn nrust_chapter_prescan_borrowed(
    source: &mut dyn BookSource, entry: &Checked, backend: &mut dyn ProducerBackend,
    policy: &mut dyn ProcessingPolicy, buffer: &mut [u8; 2048],
    filter: &mut EntityFilter, images: &mut BookImages<'_>, table: &mut [Probed],
) -> usize {
    images::prescan(source, entry, backend, None, buffer, filter, images, table, BorrowedPolicy(policy))
}

#[unsafe(no_mangle)]
pub fn nrust_chapter_page_view_borrowed(
    page: &Page<'_>, fonts: &mut page_renderer::FontChain<'_, '_>,
    decoder: &mut dyn page_renderer::ImageDecoder,
    scratch: &mut page_renderer::ImageScratch<'_>, target: &mut page_renderer::FrameTarget<'_>,
) -> Result<page_renderer::BookStatus, page_renderer::SafetyError> {
    page_renderer::render(page, fonts, decoder, scratch, target)
}
