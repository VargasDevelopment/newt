//! Focused native-free tests. Scripted line-break records are fixtures, not a
//! Unicode implementation. Native parity is exclusively the host/oracle path.
use newt_core::{
    css::{CssRule, CssStylesheetBuilder},
    chapter_layout::engine::Engine,
    chapter_layout::text,
    chapter_layout::types::*,
};
struct Fixed;
impl Font for Fixed {
    fn covers(&mut self, _: u32) -> bool {
        true
    }
    fn advance(&mut self, _: u32, _: u16, _: u8) -> i16 {
        4
    }
    fn line_height(&mut self, _: u16) -> i16 {
        8
    }
    fn ascent(&mut self, _: u16) -> i16 {
        8
    }
}
struct OneLine;
impl LineBreak for OneLine {
    fn breaks(&mut self, t: &[u8], _: Option<&core::ffi::CStr>, o: &mut [u8]) {
        assert_eq!(t.len(), o.len());
        o.fill(2);
        for (i, bytes) in t.windows(2).enumerate() {
            if bytes == [0xc2, 0xad] { o[i + 1] = 1; }
        }
        if let Some(last) = o.last_mut() {
            *last = 0
        }
    }
}
struct FixtureImages;
impl Images for FixtureImages {
    fn resolve(&mut self, href: &[u8], path: &mut [u8], fragment: &mut [u8]) -> Option<(usize, usize)> {
        let split = href.iter().position(|&byte| byte == b'#')?;
        let (target, hash_fragment) = href.split_at(split);
        let suffix = &hash_fragment[1..];
        let path_len = target.len().min(path.len());
        let fragment_len = suffix.len().min(fragment.len());
        path[..path_len].copy_from_slice(&target[..path_len]);
        fragment[..fragment_len].copy_from_slice(&suffix[..fragment_len]);
        Some((path_len, fragment_len))
    }
    fn probe(&mut self, _: &[u8]) -> Option<(u32, u32)> {
        None
    }
}
#[derive(Default)]
struct Capture {
    pages: Vec<(u32, u32, Vec<(Vec<u8>, i16, i16, u16, u8)>)>,
    page_byte_counts: Vec<usize>,
    links: Vec<(u32, Vec<u8>, Vec<u8>)>,
    link_rects: Vec<(u32, i16, i16, u16, u16)>,
    stop: bool,
}
impl Sink for Capture {
    fn page(&mut self, p: Page<'_>) -> bool {
        assert!(p.images.is_empty());
        self.links.extend(p.links.iter().map(|link| (
            p.index,
            link.target.get(p.bytes).to_vec(),
            link.fragment.get(p.bytes).to_vec(),
        )));
        self.link_rects.extend(p.links.iter().map(|link| (
            p.index,
            link.x,
            link.y,
            link.width,
            link.height,
        )));
        self.page_byte_counts.push(p.bytes.len());
        self.pages.push((
            p.index,
            p.char_start,
            p.runs
                .iter()
                .map(|r| {
                    (
                        r.text.get(p.bytes).to_vec(),
                        r.x,
                        r.baseline,
                        r.size,
                        r.flags,
                    )
                })
                .collect(),
        ));
        !self.stop
    }
}
fn fixture(sink: &mut Capture, page_cap: usize, f: impl FnOnce(&mut Engine<'_>)) {
    let mut font = Fixed;
    let mut backend = SplitBackend {
        font: &mut font,
        sink,
    };
    with_backend(&mut backend, page_cap, f)
}
fn with_backend(backend: &mut dyn Backend, page_cap: usize, f: impl FnOnce(&mut Engine<'_>)) {
    with_text_capacity(backend, page_cap, 128, f)
}
fn with_text_capacity(backend: &mut dyn Backend, page_cap: usize, text_cap: usize,
    f: impl FnOnce(&mut Engine<'_>)) {
    let mut text = [0; 256];
    let mut breaks = [0; 256];
    let mut levels = [0; 256];
    let mut spans = [Span::default(); 128];
    let mut lines = [Line::default(); 16];
    let mut elements = [Element::default(); 16];
    let mut runs = [Run::default(); 128];
    let mut images = [Image::default(); 2];
    let mut links = [Link::default(); 4];
    let mut page = vec![0; page_cap];
    let mut style = [0; 128];
    let mut pl = [ParLink::default(); 4];
    let mut rules = vec![CssRule::default(); 512];
    let mut builder = CssStylesheetBuilder::default();
    builder.begin(&mut rules);
    let storage = Storage {
        text: &mut text[..text_cap],
        breaks: &mut breaks[..text_cap],
        levels: &mut levels[..text_cap],
        spans: &mut spans,
        lines: &mut lines,
        elements: &mut elements,
        runs: &mut runs,
        images: &mut images,
        links: &mut links,
        page: &mut page,
        style_text: &mut style,
        paragraph_links: &mut pl,
    };
    let (mut breaker, mut img) = (OneLine, FixtureImages);
    let params = Params {
        width: 80,
        height: 32,
        left: 0,
        right: 0,
        top: 0,
        bottom: 0,
        base_size: 8,
        ..Params::default()
    };
    let mut engine = Engine::new(
        params,
        storage,
        backend,
        &mut breaker,
        &mut img,
        None,
        builder,
    )
    .unwrap();
    f(&mut engine)
}
#[test]
fn collapse_styles_offsets() {
    let mut sink = Capture::default();
    fixture(&mut sink, 128, |e| {
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, None, None, None, None);
        e.text(b"  a \t");
        e.start(b"b", None, None, None, None, None);
        e.text(b"b");
        e.end(b"b");
        e.text(b"c \n");
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
        assert_eq!(e.chars, 4)
    });
    assert_eq!(
        sink.pages,
        vec![(
            0,
            0,
            vec![
                (b"a".to_vec(), 0, 8, 8, 0),
                (b" b".to_vec(), 4, 8, 8, 1),
                (b"c".to_vec(), 12, 8, 8, 0)
            ]
        )]
    )
}
#[test]
fn arena_failure_is_not_a_partial_run() {
    let mut sink = Capture { stop: true, ..Capture::default() };
    fixture(&mut sink, 1, |e| {
        e.start(b"body", None, None, None, None, None);
        e.text(b"x");
        assert!(!e.finish());
        assert!(e.failed)
    });
    assert!(sink.pages.is_empty());
}

#[test]
fn run_limit_moves_the_whole_line_and_its_start() {
    two_lines_on_separate_pages(128, 2);
}

#[test]
fn pool_limit_rolls_back_the_whole_line() {
    // Each run reserves five bytes. Line two starts then runs out of bytes.
    two_lines_on_separate_pages(11, 16);
}

#[test]
fn link_pool_limit_moves_the_whole_line_with_its_link() {
    // Runs use five bytes each. The following page has room for both linked
    // line runs and both strings, even when the first page fits just the target.
    for page_cap in [15, 17] {
        let mut sink = Capture::default();
        fixture(&mut sink, page_cap, |e| {
            e.start(b"body", None, None, None, None, None);
            e.text(b"a");
            e.flush();
            e.text(b"b");
            e.start(b"a", None, None, None, Some(b"p#id"), None);
            e.text(b"c");
            e.end(b"a");
            assert!(e.finish());
        });
        assert_eq!(sink.links, [(1, b"p".to_vec(), b"id".to_vec())]);
        assert_eq!(sink.page_byte_counts, [5, 15]);
        assert_eq!(sink.pages, [
            (0, 0, vec![(b"a".to_vec(), 0, 8, 8, 0)]),
            (1, 1, vec![(b"b".to_vec(), 0, 8, 8, 0), (b"c".to_vec(), 4, 8, 8, 0)]),
        ]);
    }
}

#[path = "chapter_layout/links.rs"]
mod links;

fn two_lines_on_separate_pages(page_cap: usize, run_cap: usize) {
    let mut sink = Capture::default();
    fixture(&mut sink, page_cap, |e| {
        e.storage.runs = &mut core::mem::take(&mut e.storage.runs)[..run_cap];
        e.start(b"body", None, None, None, None, None);
        e.text(b"a");
        e.flush();
        e.text(b"b");
        e.start(b"b", None, None, None, None, None);
        e.text(b"c");
        e.end(b"b");
        assert!(e.finish());
    });
    assert_eq!(sink.pages, vec![
        (0, 0, vec![(b"a".to_vec(), 0, 8, 8, 0)]),
        (1, 1, vec![(b"b".to_vec(), 0, 8, 8, 0), (b"c".to_vec(), 4, 8, 8, BOLD)]),
    ]);
    assert_eq!(sink.page_byte_counts, [5, 10]);
}

#[test]
fn a_line_larger_than_an_empty_run_budget_fails_without_a_page() {
    let mut sink = Capture { stop: true, ..Capture::default() };
    fixture(&mut sink, 128, |e| {
        e.storage.runs = &mut core::mem::take(&mut e.storage.runs)[..2];
        e.start(b"body", None, None, None, None, None);
        e.text(b"a");
        e.start(b"b", None, None, None, None, None);
        e.text(b"b");
        e.end(b"b");
        e.text(b"c");
        assert!(!e.finish());
        assert!(e.failed);
    });
    assert!(sink.pages.is_empty());
}

#[test]
fn failed_final_flush_does_not_publish_buffered_page() {
    let mut sink = Capture::default();
    fixture(&mut sink, 128, |e| {
        e.start(b"body", None, None, None, None, None);
        e.text(b"a");
        e.flush();
        e.text(b"b");
        e.params.line_spacing = 0;
        assert!(!e.finish());
    });
    assert!(sink.pages.is_empty());
}

#[test]
fn forced_break_discards_space_at_start_of_continuation() {
    let mut sink = Capture::default();
    fixture(&mut sink, 1024, |e| {
        e.params.width = 320;
        e.params.height = 8;
        append_64_runs(e);
        e.text(b" c");
        assert!(e.finish());
    });
    assert_eq!(sink.pages.len(), 2);
    let (_, start, runs) = &sink.pages[1];
    assert_eq!((*start, runs[0].0.as_slice()), (65, b"c".as_slice()));
}

#[test]
fn soft_hyphen_stays_at_the_end_of_the_continuation() {
    let mut sink = Capture::default();
    fixture(&mut sink, 1024, |e| {
        e.params.width = 65 * 4;
        e.params.height = 8;
        append_64_runs(e);
        e.text("c\u{00ad}d".as_bytes());
        assert!(e.finish());
    });
    assert_eq!(sink.pages.len(), 3);
    assert_eq!(sink.pages[0].2.last().unwrap().0, b"b");
    assert_eq!(sink.pages[1].2[0].0, b"c-");
    assert_eq!(sink.pages[2].2[0].0, b"d");
}

fn append_64_runs(e: &mut Engine<'_>) {
    e.start(b"body", None, None, None, None, None);
    for _ in 0..32 {
        e.text(b"a");
        e.start(b"b", None, None, None, None, None);
        e.text(b"b");
        e.end(b"b");
    }
}

#[test]
fn justified_cjk_forces_a_line_break_after_64_segments() {
    let text = "文字".repeat(34);
    for (height, expected) in [(8, &[(0, 64), (64, 1), (67, 1)][..]), (24, &[(0, 66)][..])] {
        let mut sink = Capture::default();
        let mut font = Fixed;
        let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
        with_text_capacity(&mut backend, 1024, 256, |e| {
            // The 67-glyph measured line leaves three glyphs in a ragged continuation.
            e.params.width = 67 * 4 + 1;
            e.params.height = height;
            e.start(b"body", None, None, None, None, None);
            e.start(b"p", None, Some(b"text-align:justify"), None, None, None);
            e.text(text.as_bytes());
            e.end(b"p");
            assert!(e.finish());
        });
        let pages: Vec<_> = sink.pages.iter().map(|(_, start, runs)| (*start, runs.len())).collect();
        assert_eq!(pages, expected);
        let laid_out: Vec<_> = sink.pages.iter()
            .flat_map(|(_, _, runs)| runs.iter().flat_map(|(bytes, ..)| bytes.iter().copied()))
            .collect();
        assert_eq!(laid_out, text.as_bytes());
    }
}

#[test]
fn a_space_after_the_segment_break_keeps_hangul_gaps_excluded() {
    struct WideTail;
    impl Font for WideTail {
        fn covers(&mut self, _: u32) -> bool { true }
        fn advance(&mut self, cp: u32, _: u16, _: u8) -> i16 {
            if cp == u32::from(b'W') { 100 } else { 4 }
        }
        fn line_height(&mut self, _: u16) -> i16 { 8 }
        fn ascent(&mut self, _: u16) -> i16 { 8 }
    }
    let mut sink = Capture::default();
    let mut font = WideTail;
    let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
    let width = 350;
    let text = format!("한한{} aaW", "日".repeat(64));
    with_text_capacity(&mut backend, 1024, 256, |e| {
        e.params.width = width;
        e.params.height = 8;
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, Some(b"text-align:justify"), None, None, None);
        e.text(text.as_bytes());
        e.end(b"p");
        assert!(e.finish());
    });
    let runs = &sink.pages[0].2;
    assert_eq!(runs.len(), 64);
    assert_eq!(runs[0].0, "한한".as_bytes());
    assert_eq!(runs[63].1 + 4, width);
}
#[test]
fn private_decoder_and_visual_reverse() {
    let bytes = [0xf0, 0x90, b'!'];
    let mut at = 0;
    assert_eq!(text::decode(&bytes, &mut at), 16);
    assert_eq!(at, 2);
    let mut s = "aאב".as_bytes().to_vec();
    text::reverse(&mut s);
    assert_eq!(s, "באa".as_bytes());
    assert_eq!(text::count(&bytes), 2)
}
#[test]
fn exact_defaults() {
    use newt_core::chapter_layout::style;
    assert_eq!(style::defaults(b"h2").size_pct, 160);
    assert_eq!(style::defaults(b"p").margin_bottom_pct, 40);
    assert_eq!(style::defaults(b"sup").vert_align, 1);
    assert!(style::block(b"figcaption"));
    assert!(!style::block(b"span"))
}
#[test]
fn bounded_mock_backend_and_handler_reborrow() {
    use newt_core::{
        chapter_layout::session::{ChapterSession, LayoutHandler, State},
        xml_input::EntityFilter,
        xml_session::{Handler, ParserBackend, Resources, XmlStream},
        zip_container::{Entry, Status},
        zip_window::source::BookSource,
    };
    struct Source;
    impl BookSource for Source {
        fn size(&self) -> u64 {
            4096
        }
        fn read_at(&mut self, _: u64, o: &mut [u8]) -> i32 {
            o.fill(b' ');
            o.len() as i32
        }
    }
    #[derive(Default)]
    struct Backend {
        feeds: u32,
    }
    impl ParserBackend for Backend {
        fn create(&mut self) -> bool {
            true
        }
        fn feed(&mut self, _: &[u8], _: bool, _: &mut dyn Handler) -> bool {
            self.feeds += 1;
            true
        }
        fn close(&mut self) {}
    }
    let mut sink = Capture::default();
    fixture(&mut sink, 128, |e| {
        let (mut source, mut backend) = (Source, Backend::default());
        let entry = Entry::raw(4096);
        let mut buffer = [0; 2048];
        let mut filter = EntityFilter::new();
        let mut session = ChapterSession::new(XmlStream::new(&mut source, None, &mut backend));
        assert_eq!(
            session.begin(
                &entry,
                Resources {
                    buffer: Some(&mut buffer),
                    filter: Some(&mut filter)
                },
                e
            ),
            Status::Ok
        );
        {
            let mut handler = LayoutHandler { engine: e };
            assert_eq!(session.step_bounded(&mut handler, 0, 1), Status::Ok);
            assert_eq!(session.bytes_consumed(), 0);
            assert_eq!(session.step_bounded(&mut handler, 1, 0), Status::Ok);
            assert_eq!(session.bytes_consumed(), 0);
            assert_eq!(session.step_bounded(&mut handler, 1, 1), Status::Ok);
            assert_eq!(session.bytes_consumed(), 2048);
        }
        assert_eq!(e.pages, 0);
        assert_eq!(session.state(), State::Parsing);
        session.abort();
        drop(session);
        assert_eq!(backend.feeds, 1);
    });
}

#[test]
fn full_text_buffer_keeps_utf8_characters_whole() {
    const CAPACITY: usize = 8;
    let mut mismatches = Vec::new();
    for character in ['é', '漢', '🙂'] {
        for bytes_before_flush in 1..character.len_utf8() {
            let prefix = "a".repeat(CAPACITY - 1 - bytes_before_flush);
            let input = format!("{prefix}{character}z");
            let mut utf8 = [0; 4];
            let encoded = character.encode_utf8(&mut utf8).as_bytes();
            let (first, rest) = encoded.split_at(bytes_before_flush);
            let mut font = Fixed;
            let mut sink = Capture::default();
            let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
            with_text_capacity(&mut backend, 128, CAPACITY, |engine| {
                engine.start(b"body", None, None, None, None, None);
                engine.text(prefix.as_bytes());
                engine.start(b"b", None, None, None, None, None);
                engine.text(first);
                engine.text(rest);
                engine.end(b"b");
                engine.text(b"z");
                assert!(engine.finish());
                assert_eq!(engine.chars, input.chars().count() as u32);
            });
            let runs = sink.pages.iter().flat_map(|(_, _, runs)| runs);
            let output: Vec<_> = runs.clone().flat_map(|run| run.0.iter().copied()).collect();
            let bold: Vec<_> = runs
                .filter(|run| run.4 & BOLD != 0)
                .flat_map(|run| run.0.iter().copied())
                .collect();
            if output != input.as_bytes() || bold != encoded {
                mismatches.push((character, bytes_before_flush, output, bold));
            }
        }
    }
    assert!(mismatches.is_empty(), "text or style lost at flush boundaries: {mismatches:?}");
}

#[test]
fn full_text_run_preserves_failed_flush() {
    struct InvalidHeight(i16);
    impl Font for InvalidHeight {
        fn covers(&mut self, _: u32) -> bool { true }
        fn advance(&mut self, _: u32, _: u16, _: u8) -> i16 { 4 }
        fn line_height(&mut self, _: u16) -> i16 { self.0 }
        fn ascent(&mut self, _: u16) -> i16 { 8 }
    }
    for height in [0, -1] {
        for capacity in [2, 3] {
            let mut font = InvalidHeight(height);
            let mut sink = Capture::default();
            let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
            with_text_capacity(&mut backend, 128, capacity, |engine| {
                engine.start(b"body", None, None, None, None, None);
                engine.text(&b"abc"[..capacity]);
                assert!(engine.failed);
                assert!(!engine.finish());
                assert_eq!(engine.chars, 0);
            });
            assert!(sink.pages.is_empty());
        }
    }
}

struct PixelFont<'a>(&'a [u8]);
impl Font for PixelFont<'_> {
    fn covers(&mut self, cp: u32) -> bool {
        cp == 65
    }
    fn advance(&mut self, _: u32, _: u16, _: u8) -> i16 {
        2
    }
    fn line_height(&mut self, _: u16) -> i16 {
        8
    }
    fn ascent(&mut self, _: u16) -> i16 {
        8
    }
}
impl newt_core::page_renderer::RasterFont for PixelFont<'_> {
    fn has_glyph(&mut self, cp: u32) -> bool {
        self.covers(cp)
    }
    fn rasterize(
        &mut self,
        cp: u32,
        _: u16,
    ) -> Option<newt_core::page_renderer::GlyphBitmap<'_>> {
        self.covers(cp)
            .then_some(newt_core::page_renderer::GlyphBitmap {
                pixels: self.0,
                width: 1,
                height: 1,
                xoff: 0,
                yoff: -1,
                advance: 99,
            })
    }
}
struct Canvas<'a, 'f> {
    fonts: newt_core::page_renderer::FontChain<'a, 'f>,
    target: newt_core::page_renderer::FrameTarget<'a>,
    pages: u32,
}
impl Font for Canvas<'_, '_> {
    fn covers(&mut self, c: u32) -> bool {
        self.fonts.covers(c)
    }
    fn advance(&mut self, c: u32, s: u16, f: u8) -> i16 {
        self.fonts.advance(c, s, f)
    }
    fn line_height(&mut self, s: u16) -> i16 {
        self.fonts.line_height(s)
    }
    fn ascent(&mut self, s: u16) -> i16 {
        self.fonts.ascent(s)
    }
    fn kerning(&mut self, a: u32, b: u32, s: u16, f: u8) -> i16 {
        self.fonts.kerning(a, b, s, f)
    }
    fn ligature(&mut self, a: u32, b: u32, f: u8) -> u32 {
        self.fonts.ligature(a, b, f)
    }
}
impl Sink for Canvas<'_, '_> {
    fn page(&mut self, page: Page<'_>) -> bool {
        assert_eq!(
            newt_core::page_renderer::render_text(
                &page,
                &mut self.fonts,
                &mut self.target,
                None
            ),
            Ok(0)
        );
        self.pages += 1;
        true
    }
}
#[test]
fn layout_callback_renders_with_the_same_exclusively_owned_fonts() {
    use newt_core::page_renderer::{
        FontChain, FrameFormat, FrameRotation, FrameTarget, RasterFont,
    };
    let pixels = [255];
    let mut font = PixelFont(&pixels);
    let mut bank: [&mut dyn RasterFont; 1] = [&mut font];
    let mut fonts = FontChain::new(&mut bank);
    assert!(fonts.add(0, 0));
    let mut frame = [255; 32];
    let target = FrameTarget {
        framebuffer: &mut frame,
        width: 8,
        height: 32,
        width_bytes: 1,
        format: FrameFormat::Mono1Sharp,
        rotation: FrameRotation::None,
    };
    let mut canvas = Canvas {
        fonts,
        target,
        pages: 0,
    };
    with_backend(&mut canvas, 128, |e| {
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, None, None, None, None);
        e.text(b"AA");
        e.end(b"p");
        e.end(b"body");
        assert!(e.finish());
        assert_eq!(e.pages, 1);
        assert_eq!(e.chars, 2);
    });
    assert_eq!(canvas.pages, 1);
    drop(canvas);
    let mut expected = [255; 32];
    expected[7] = 0x5f;
    assert_eq!(frame, expected);
}
#[test]
fn malformed_offset_view_fails_before_any_frame_change() {
    use newt_core::page_renderer::{
        FontChain, FrameFormat, FrameRotation, FrameTarget, RasterFont, SafetyError, render_text,
    };
    let bytes = b"A";
    let runs = [
        Run {
            text: Bytes { start: 0, len: 1 },
            size: 8,
            baseline: 1,
            ..Run::default()
        },
        Run {
            text: Bytes { start: 1, len: 1 },
            ..Run::default()
        },
    ];
    let page = Page {
        bytes,
        runs: &runs,
        images: &[],
        links: &[],
        index: 0,
        char_start: 0,
    };
    let pixels = [255];
    let mut font = PixelFont(&pixels);
    let mut bank: [&mut dyn RasterFont; 1] = [&mut font];
    let mut fonts = FontChain::new(&mut bank);
    assert!(fonts.add(0, 0));
    let mut frame = [255; 1];
    let mut target = FrameTarget {
        framebuffer: &mut frame,
        width: 8,
        height: 1,
        width_bytes: 1,
        format: FrameFormat::Mono1Sharp,
        rotation: FrameRotation::None,
    };
    assert_eq!(
        render_text(&page, &mut fonts, &mut target, None),
        Err(SafetyError::TextLength)
    );
    assert_eq!(frame, [255]);
}
#[test]
fn invalid_second_image_prevents_text_and_decoder_writes() {
    use newt_core::page_renderer::{
        self as r, BookStatus, ByteArena, FontChain, FrameFormat, FrameRotation, FrameTarget,
        ImageScratch, RasterFont, SafetyError,
    };
    struct Decoder {
        calls: u32,
    }
    impl r::ImageDecoder for Decoder {
        fn render(
            &mut self,
            _: &r::PageImage<'_>,
            _: &mut ByteArena<'_>,
            sink: &mut dyn r::ImageRows,
        ) -> Result<BookStatus, SafetyError> {
            self.calls += 1;
            sink.on_row(0, &[0], 1)?;
            Ok(BookStatus::Ok)
        }
    }
    let bytes = b"A";
    let runs = [Run {
        text: Bytes { start: 0, len: 1 },
        baseline: 1,
        size: 8,
        ..Run::default()
    }];
    let images = [
        Image {
            href: Bytes { start: 0, len: 1 },
            width: 1,
            height: 1,
            ..Image::default()
        },
        Image {
            href: Bytes { start: 1, len: 1 },
            width: 1,
            height: 1,
            ..Image::default()
        },
    ];
    let page = Page {
        bytes,
        runs: &runs,
        images: &images,
        links: &[],
        index: 0,
        char_start: 0,
    };
    let pixels = [255];
    let mut font = PixelFont(&pixels);
    let mut bank: [&mut dyn RasterFont; 1] = [&mut font];
    let mut fonts = FontChain::new(&mut bank);
    assert!(fonts.add(0, 0));
    let mut frame = [255];
    let mut target = FrameTarget {
        framebuffer: &mut frame,
        width: 8,
        height: 1,
        width_bytes: 1,
        format: FrameFormat::Mono1Sharp,
        rotation: FrameRotation::None,
    };
    let mut storage = [0; 32];
    let mut err = [0; 3];
    let mut next = [0; 3];
    let mut scratch = ImageScratch {
        arena: ByteArena::new(&mut storage),
        err: &mut err,
        err_next: &mut next,
    };
    let mut decoder = Decoder { calls: 0 };
    assert_eq!(
        r::render(&page, &mut fonts, &mut decoder, &mut scratch, &mut target),
        Err(SafetyError::TextLength)
    );
    assert_eq!(decoder.calls, 0);
    assert_eq!(frame, [255]);
}
