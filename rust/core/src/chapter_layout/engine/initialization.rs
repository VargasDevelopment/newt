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
            picture_note_at: None,
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

    /// Reset all layout carry fields to the exact initial state of `new`
    /// without reallocating or replacing caller-lent storage.
    #[inline(never)]
    pub fn reset(&mut self) {
        self.chapter_css = CssStylesheetBuilder::default();
        self.stopped = false;
        self.failed = false;
        self.pages = 0;
        self.chars = 0;
        self.len = 0;
        self.span_count = 0;
        self.line_count = 0;
        self.top = 0;
        self.overflow = 0;
        self.depth = 0;
        self.suppress = 0;
        self.in_body = false;
        self.in_style = 0;
        self.style_len = 0;
        self.style_ok = true;
        self.pending_margin = 0;
        self.pending_space = false;
        self.continued = false;
        self.paragraph = Paragraph::default();
        self.rtl = false;
        self.run_count = 0;
        self.image_count = 0;
        self.link_count = 0;
        self.pool_len = 0;
        self.y = self.params.top;
        self.page_chars = 0;
        self.picture_note_at = None;
        self.par_link_count = 0;
        self.current_link = 0;
        self.last_shy = false;
        self.word_state = case::WS_SEP;
        self.par_case = false;
        self.par_dir = DIR_NONE;
        self.carried_margin_px = 0;
        self.storage.elements[0] = Element {
            align: self.params.align,
            ..Element::default()
        };
        self.latch(b"p", CssDecl::default(), false);
    }
}
