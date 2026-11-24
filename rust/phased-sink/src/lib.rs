//! Selecting page sink for phased EPUB layout (separate from the reader lifecycle). MIT.
#![no_std]
#![forbid(unsafe_code)]

use newt_core::{
    chapter_layout::types::{Font, Page, Sink},
    utf8,
};
use newt_reader_atlas::{Geometry, ReaderAtlas, effective_size};
use newt_reader_navigation::Navigation;

pub const RUNS: usize = 304;
pub const TEXT: usize = 7 * 1024;
pub use newt_core::frame::{FRAME_BYTES, Frame};

/// Same fields/order as PhasedPagePlan::Run; no redundant run length.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Run {
    pub offset: u16,
    pub x: i16,
    pub baseline: i16,
    pub size: u8,
    pub flags: u8,
}
const _: () = assert!(core::mem::size_of::<Run>() == 8);

/// Retained capture counts without a borrow of the caller's backing arrays.
pub struct PlanState {
    count: u16,
    bytes: u16,
}
impl PlanState {
    pub fn new() -> Self {
        Self { count: 0, bytes: 0 }
    }
}
impl Default for PlanState {
    fn default() -> Self {
        Self::new()
    }
}

/// Buffers belong to the caller. No large temporary, heap, or self-reference.
/// Metadata is private so repaint only sees successfully captured runs.
pub struct Plan<'a> {
    runs: &'a mut [Run; RUNS],
    text: &'a mut [u8; TEXT],
    count: u16,
    bytes: u16,
}
impl<'a> Plan<'a> {
    pub fn new(runs: &'a mut [Run; RUNS], text: &'a mut [u8; TEXT]) -> Self {
        Self::from_state(runs, text, PlanState::new())
    }
    /// Resume the retained capture over its unchanged backing arrays.
    pub fn from_state(
        runs: &'a mut [Run; RUNS],
        text: &'a mut [u8; TEXT],
        state: PlanState,
    ) -> Self {
        Self {
            runs,
            text,
            count: state.count,
            bytes: state.bytes,
        }
    }
    pub fn state(&self) -> PlanState {
        PlanState {
            count: self.count,
            bytes: self.bytes,
        }
    }
    pub fn into_state(self) -> PlanState {
        self.state()
    }
    /// Caller must establish that no page/candidate is published before lending.
    /// Both arrays are exclusive; the result cannot borrow them. Run fields have
    /// only integer values, so a temporary byte representation can be restored.
    pub fn with_unpublished_backing<R>(
        &mut self,
        operation: impl FnOnce(&mut [Run; RUNS], &mut [u8; TEXT]) -> R,
    ) -> R {
        self.count = 0;
        self.bytes = 0;
        operation(self.runs, self.text)
    }
    pub fn count(&self) -> u16 {
        self.count
    }
    pub fn bytes(&self) -> u16 {
        self.bytes
    }
    pub fn runs(&self) -> &[Run] {
        &self.runs[..usize::from(self.count)]
    }
    pub fn text(&self) -> &[u8] {
        &self.text[..usize::from(self.bytes)]
    }
    pub fn run_length(&self, i: u16) -> u16 {
        assert!(i < self.count);
        let next = if i + 1 < self.count {
            self.runs[usize::from(i + 1)].offset
        } else {
            self.bytes
        };
        next - self.runs[usize::from(i)].offset
    }
    pub fn paint(
        &self,
        frame: &mut Frame,
        landscape: bool,
        atlas: &ReaderAtlas,
        metrics: &mut ReaderAtlas,
    ) {
        frame.fill(0);
        for (i, run) in self.runs().iter().enumerate() {
            let start = usize::from(run.offset);
            draw_run(
                atlas,
                metrics,
                frame,
                Geometry::reader(landscape),
                &self.text[start..start + usize::from(self.run_length(i as u16))],
                run.x,
                run.baseline,
                u16::from(run.size),
                run.flags,
            );
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Page,
    Anchor,
    TocResolve,
    TocPage,
}
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    pub found: bool,
    pub reached: bool,
    pub malformed: bool,
    pub overflow: bool,
    pub page_index: u32,
    pub char_start: u32,
}
pub enum Output<'a, 'b> {
    Frame(&'a mut Frame),
    Plan(&'a mut Plan<'b>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MissingNavigation;

/// Pass directly as Engine::new's `&mut dyn Backend`. Metrics use the shared
/// Core Font trait, while pixels go through actual ReaderAtlas::draw.
pub struct SelectingSink<'a, 'b> {
    pub state: State,
    target: u32,
    mode: Target,
    landscape: bool,
    metrics: &'a mut ReaderAtlas,
    atlas: &'a ReaderAtlas,
    output: Output<'a, 'b>,
    navigation: Option<&'a mut Navigation>,
}
impl<'a, 'b> SelectingSink<'a, 'b> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        target: u32,
        mode: Target,
        landscape: bool,
        metrics: &'a mut ReaderAtlas,
        atlas: &'a ReaderAtlas,
        output: Output<'a, 'b>,
        navigation: Option<&'a mut Navigation>,
    ) -> Result<Self, MissingNavigation> {
        if mode == Target::TocResolve && navigation.is_none() {
            return Err(MissingNavigation);
        }
        Ok(Self {
            state: State::default(),
            target,
            mode,
            landscape,
            metrics,
            atlas,
            output,
            navigation,
        })
    }
    pub fn output(&self) -> &Output<'a, 'b> {
        &self.output
    }
    /// Restart changes only sink selection/error state, not buffers/navigation.
    pub fn restart(&mut self) {
        self.state = State::default();
    }
    /// Valid only for capture output, matching the original method precondition.
    pub fn paint_captured(&mut self, frame: &mut Frame) {
        let Output::Plan(plan) = &self.output else {
            panic!("capture output required")
        };
        plan.paint(frame, self.landscape, self.atlas, self.metrics);
    }
}
impl Font for SelectingSink<'_, '_> {
    fn advance(&mut self, cp: u32, px: u16, flags: u8) -> i16 {
        self.metrics.advance(cp, px, flags)
    }
    fn line_height(&mut self, px: u16) -> i16 {
        self.metrics.line_height(px)
    }
    fn ascent(&mut self, px: u16) -> i16 {
        self.metrics.ascent(px)
    }
    fn covers(&mut self, cp: u32) -> bool {
        self.metrics.covers(cp)
    }
    fn ligature(&mut self, a: u32, b: u32, flags: u8) -> u32 {
        self.metrics.ligature(a, b, flags)
    }
    fn kerning(&mut self, a: u32, b: u32, px: u16, flags: u8) -> i16 {
        self.metrics.kerning(a, b, px, flags)
    }
}
impl Sink for SelectingSink<'_, '_> {
    fn anchor(&mut self, hash: u32, start: u32) {
        if let Some(nav) = &mut self.navigation {
            nav.observe_anchor(hash, start);
        }
    }
    fn page(&mut self, page: Page<'_>) -> bool {
        let resolving = self.mode == Target::TocResolve;
        let mut anchor = self.target;
        if resolving {
            let section = self
                .navigation
                .as_ref()
                .expect("constructor invariant")
                .target_by_toc(self.target as u16);
            if !section.target_known || !section.anchor_known {
                return true;
            }
            anchor = section.char_start;
        }
        let by_anchor = resolving || self.mode == Target::Anchor;
        if by_anchor && self.state.found && page.char_start > anchor {
            self.state.reached = true;
        }
        let selected = if by_anchor {
            !self.state.found || page.char_start <= anchor
        } else {
            page.index == self.target
        };
        if !selected {
            return true;
        }
        self.state.found = true;
        self.state.page_index = page.index;
        self.state.char_start = page.char_start;
        if resolving {
            return true;
        }
        if self.mode == Target::Page {
            self.state.reached = true;
        }
        self.state.malformed = false;
        // Validate every selected run before touching either output, including
        // a malformed suffix after otherwise drawable valid text.
        for run in page.runs {
            let text = run.text.get(page.bytes);
            let (mut at, mut cp) = (0, 0);
            while at < text.len() {
                if !utf8::next(Some(text), &mut at, &mut cp) {
                    self.state.malformed = true;
                    return true;
                }
            }
        }
        match &mut self.output {
            Output::Plan(plan) => {
                plan.count = 0;
                plan.bytes = 0;
                if page.runs.len() > RUNS {
                    self.state.overflow = true;
                    return false;
                }
                for (i, run) in page.runs.iter().enumerate() {
                    let text = run.text.get(page.bytes);
                    if text.len() > TEXT - usize::from(plan.bytes) {
                        self.state.overflow = true;
                        return false;
                    }
                    plan.runs[i] = Run {
                        offset: plan.bytes,
                        x: run.x,
                        baseline: run.baseline,
                        size: effective_size(run.size) as u8,
                        flags: run.flags,
                    };
                    let start = usize::from(plan.bytes);
                    plan.text[start..start + text.len()].copy_from_slice(text);
                    plan.bytes += text.len() as u16;
                }
                plan.count = page.runs.len() as u16;
            }
            Output::Frame(frame) => {
                frame.fill(0);
                for run in page.runs {
                    draw_run(
                        self.atlas,
                        self.metrics,
                        frame,
                        Geometry::reader(self.landscape),
                        run.text.get(page.bytes),
                        run.x,
                        run.baseline,
                        run.size,
                        run.flags,
                    );
                }
            }
        }
        true
    }
}
#[allow(clippy::too_many_arguments)]
fn draw_run(
    atlas: &ReaderAtlas,
    metrics: &mut ReaderAtlas,
    frame: &mut Frame,
    geometry: Geometry,
    text: &[u8],
    x: i16,
    baseline: i16,
    px: u16,
    flags: u8,
) {
    let (mut at, mut cp, mut previous, mut x) = (0, 0, 0, i32::from(x));
    while at < text.len() {
        if !utf8::next(Some(text), &mut at, &mut cp) {
            return;
        }
        if previous != 0 {
            x += i32::from(metrics.kerning(previous, cp, px, flags));
        }
        // Geometry and frame extent are guaranteed by construction.
        atlas
            .draw(frame, geometry, x, i32::from(baseline), cp, px, flags)
            .expect("canonical frame/geometry");
        x += i32::from(metrics.advance(cp, px, flags));
        previous = cp;
    }
}
pub mod probe;
