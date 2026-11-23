// SPDX-License-Identifier: MIT
//! One blocking Core ChapterLayout pass for the bounded reader. The bank
//! owner performs the original prescan before lending these typed resources.
use super::LaidOut;
use newt_core::{chapter_layout::{engine::Engine, session::{ChapterSession, LayoutHandler},
        types::{Images, LineBreak, Params, Sink, SplitBackend, Storage}},
    css::CssStylesheetBuilder,
    xml_input::EntityFilter, xml_session::{self, ParserBackend, XmlStream},
    zip_container::{Entry, Status}, zip_window::source::BookSource};
use newt_reader_atlas::ReaderAtlas;

/// Every field is a loan of the existing layout/parse banks or the real
/// source/parser/image service. No fallback resources or buffers are created.
pub struct Chapter<'a, 'entry, B: ParserBackend + ?Sized> {
    pub source: &'a mut dyn BookSource,
    pub entry: &'entry Entry,
    pub params: Params<'a>,
    pub storage: Storage<'a>,
    pub chapter_css: CssStylesheetBuilder<'a>,
    pub breaker: &'a mut dyn LineBreak,
    pub images: &'a mut dyn Images,
    pub parser: &'a mut B,
    pub buffer: &'a mut [u8; 2048],
    pub filter: &'a mut EntityFilter,
}
impl<B: ParserBackend + ?Sized> Chapter<'_, '_, B> {
    pub fn run(mut self, sink: &mut dyn Sink) -> Result<LaidOut, Status> {
        let mut font = ReaderAtlas;
        let mut backend = SplitBackend { font: &mut font, sink };
        let mut engine = Engine::new(self.params, crate::resources::storage(&mut self.storage),
            &mut backend, self.breaker, self.images, None, self.chapter_css)
            .map_err(|_| Status::OutOfMemory)?;
        let stream = XmlStream::new(self.source, None, self.parser);
        let mut session = ChapterSession::new(stream);
        let mut status = session.begin(self.entry, xml_session::Resources {
            buffer: Some(self.buffer), filter: Some(self.filter) }, &engine);
        while status == Status::Ok && !session.done() {
            status = session.step(&mut LayoutHandler { engine: &mut engine }, u32::MAX);
        }
        let layout = LaidOut { pages: engine.pages, characters: engine.chars };
        session.abort();
        if status == Status::Ok { Ok(layout) } else { Err(status) }
    }
}
