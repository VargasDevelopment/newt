// Copyright (c) 2026 FreeInk. MIT; see rust/THIRD_PARTY.md.
//! Initialize the existing layout owner without changing its representation.
use super::*;

impl<'a> EngineState<'a> {
    pub fn new(
        params: Params<'a>,
        storage: Storage<'a>,
        breaker: &'a mut dyn LineBreak,
        image_source: &'a mut dyn Images,
        hyphenator: Option<&'a Hyphenator<'a>>,
        chapter_css: CssStylesheetBuilder<'a>,
    ) -> Result<Self, Capacity> {
        storage.validate()?;
        let y = params.top;
        let mut e = Self {
            params,
            storage,
            breaker,
            image_source,
            hyphenator,
            chapter_css,
            stopped: false,
            failed: false,
            pages: 0,
            chars: 0,
            len: 0,
            span_count: 0,
            line_count: 0,
            top: 0,
            overflow: 0,
            depth: 0,
            suppress: 0,
            in_body: false,
            in_style: 0,
            style_len: 0,
            style_ok: true,
            pending_margin: 0,
            pending_space: false,
            continued: false,
            paragraph: Paragraph::default(),
            rtl: false,
            run_count: 0,
            image_count: 0,
            link_count: 0,
            pool_len: 0,
            y,
            page_chars: 0,
            par_link_count: 0,
            current_link: 0,
            last_shy: false,
            word_state: case::WS_SEP,
            par_case: false,
            par_dir: DIR_NONE,
            carried_margin_px: 0,
        };
        e.storage.elements[0] = Element {
            align: e.params.align,
            ..Element::default()
        };
        e.latch(b"p", CssDecl::default(), false);
        Ok(e)
    }
}
