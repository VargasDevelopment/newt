use newt_core::chapter_layout::types::{Font, Page, Sink};
use newt_phased_sink::{Frame, Target};
use newt_reader_atlas::{Geometry, ReaderAtlas};
use newt_reader_navigation::Navigation;

pub(super) struct Selecting<'a> {
    pub(super) found: bool,
    pub(super) selected_page: u32,
    pub(super) selected_char: u32,
    target: u32,
    mode: Target,
    landscape: bool,
    frame: Option<&'a mut Frame>,
    navigation: &'a mut Navigation,
}

impl<'a> Selecting<'a> {
    pub(super) fn new(
        target: u32,
        mode: Target,
        landscape: bool,
        frame: Option<&'a mut Frame>,
        navigation: &'a mut Navigation,
    ) -> Self {
        Self {
            found: false,
            selected_page: 0,
            selected_char: 0,
            target,
            mode,
            landscape,
            frame,
            navigation,
        }
    }
}

impl Sink for Selecting<'_> {
    fn anchor(&mut self, hash: u32, start: u32) {
        self.navigation.observe_anchor(hash, start);
    }

    fn page(&mut self, page: Page<'_>) -> bool {
        let mut anchor = self.target;
        let resolving = self.mode == Target::TocResolve;
        if resolving {
            let section = self.navigation.target_by_toc(self.target as u16);
            if !section.target_known || !section.anchor_known {
                return true;
            }
            anchor = section.char_start;
        }
        let by_anchor = resolving || self.mode == Target::Anchor;
        if (by_anchor && (!self.found || page.char_start <= anchor))
            || (!by_anchor && page.index == self.target)
        {
            if !resolving {
                if let Some(frame) = self.frame.as_deref_mut() {
                    draw_page(&page, frame, Geometry::reader(self.landscape));
                }
            }
            self.selected_page = page.index;
            self.selected_char = page.char_start;
            self.found = true;
        }
        true
    }
}

pub(super) struct Counting;

impl Sink for Counting {
    fn page(&mut self, _page: Page<'_>) -> bool {
        true
    }
}

fn draw_page(page: &Page<'_>, frame: &mut Frame, geometry: Geometry) {
    frame.fill(0);
    let mut font = ReaderAtlas;
    for run in page.runs {
        let text = run.text.get(page.bytes);
        let mut x = i32::from(run.x);
        let mut offset = 0;
        // Layout measures each emitted run with fresh kerning state.
        let mut previous = 0;
        while offset < text.len() {
            let cp = next_codepoint(text, &mut offset);
            if previous != 0 {
                x += i32::from(font.kerning(previous, cp, run.size, run.flags));
            }
            // The typed frame and reader geometry establish the draw domain.
            if font
                .draw(
                    frame,
                    geometry,
                    x,
                    i32::from(run.baseline),
                    cp,
                    run.size,
                    run.flags,
                )
                .is_err()
            {
                return;
            }
            x += i32::from(font.advance(cp, run.size, run.flags));
            previous = cp;
        }
    }
}

// Preserve reader.cpp's permissive decoder, including partial prefix consumption.
fn next_codepoint(text: &[u8], offset: &mut usize) -> u32 {
    let first = text[*offset];
    *offset += 1;
    if first < 128 {
        return u32::from(first);
    }
    let extra = match first {
        0xc2..=0xdf => 1,
        0xe0..=0xef => 2,
        0xf0..=0xf4 => 3,
        _ => 0,
    };
    if extra == 0 || *offset + extra > text.len() {
        return u32::from(b'?');
    }
    let mut cp = u32::from(first & (0x7f >> (extra + 1)));
    for _ in 0..extra {
        let byte = text[*offset];
        if byte & 0xc0 != 0x80 {
            return u32::from(b'?');
        }
        *offset += 1;
        cp = (cp << 6) | u32::from(byte & 0x3f);
    }
    cp
}
