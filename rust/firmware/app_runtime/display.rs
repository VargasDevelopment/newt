// SPDX-License-Identifier: MIT
//! Frozen Home/display publication over the one SDK frame and logical frame.
use super::{AppRuntime, home_refresh::HomeRefresh, home_cover::{HomeCover, Step}};
use crate::app_image::Home;
use crate::dev_usb::{PanelOutcome, PanelRing};
use crate::reader_action::{ActionStage, Trace};
use newt_sdk_heap::Bytes;
use newt_c3_board::{Board, ReaderPanel, Error, Refresh, grayscale::{GrayEncoding, GrayMode, GrayPlane}};
use newt_core::{
    app::home_section,
    frame::{self, Frame, NativeRegion},
    preferences::Orientation,
    refresh::UiRefresh,
    sd_resume::SdResumeRecord,
};
use newt_input_policy::UiControl;
use newt_stage_home::{HeaderInfo, HomeData, HomeFocusStyle, HomePlane, HomeStyle};

/// Frozen display_receipt.h: current only after a blocking display return.
#[derive(Default)]
pub struct DisplayReceipt {
    pub generation: u32,
    orientation: Orientation,
    pub valid: bool,
    pub grayscale: bool,
    pub overlay: bool,
    #[cfg(feature = "dev-usb")]
    pub refresh_ms: u32,
}

/// Non-Home composition ends after conversion, before the blocking refresh.
/// Early unavailable/error returns restore Idle without retaining the frame.
pub struct Composition<'a> {
    trace: Trace<'a>,
    #[cfg(feature = "dev-usb")]
    restore: bool,
}
impl<'a> Composition<'a> {
    pub fn new(trace: Trace<'a>) -> Self {
        trace.boundary(ActionStage::Composition as u8);
        Self { trace, #[cfg(feature = "dev-usb")] restore: false }
    }
    /// The existing rollback scan can paint Loading inside source selection;
    /// return to that enclosing stage instead of clearing its accounting.
    pub fn scoped(trace: Trace<'a>) -> Self {
        trace.boundary(ActionStage::Composition as u8 | ActionStage::Push as u8);
        Self { trace, #[cfg(feature = "dev-usb")] restore: true }
    }
}
impl Drop for Composition<'_> {
    fn drop(&mut self) {
        #[cfg(feature = "dev-usb")]
        self.trace.boundary(if self.restore { ActionStage::Restore } else { ActionStage::Idle } as u8);
    }
}
impl DisplayReceipt {
    pub fn orientation(&self) -> Orientation { self.orientation }

    pub fn completed(&mut self, board: &mut Board<'_>, orientation: Orientation, gray: bool, overlay: bool,
        started: u32, control: &UiControl<'_>,
    ) {
        #[cfg(feature = "dev-usb")]
        let refresh_ms = board.millis().wrapping_sub(started);
        #[cfg(not(feature = "dev-usb"))]
        let refresh_ms = { let _ = (board, started); 0 };
        self.completed_at(orientation, gray, overlay, refresh_ms, control);
    }

    /// Single frame-publication boundary for the app: keeps the input producer's
    /// clockwise Back button synchronized with the orientation shown on the panel.
    pub fn completed_at(&mut self, orientation: Orientation, gray: bool, overlay: bool,
        refresh_ms: u32, control: &UiControl<'_>,
    ) {
        self.finish(orientation, gray, overlay, refresh_ms);
        control.set_clockwise_back(orientation == Orientation::LandscapeCw);
    }

    /// Portrait-only completion for update mode, which has no book orientation.
    pub fn completed_portrait(&mut self, board: &mut Board<'_>, gray: bool, overlay: bool, started: u32) {
        #[cfg(feature = "dev-usb")]
        let refresh_ms = board.millis().wrapping_sub(started);
        #[cfg(not(feature = "dev-usb"))]
        let refresh_ms = { let _ = (board, started); 0 };
        self.finish(Orientation::Portrait, gray, overlay, refresh_ms);
    }

    fn finish(&mut self, orientation: Orientation, gray: bool, overlay: bool, refresh_ms: u32) {
        self.orientation = orientation;
        self.grayscale = gray;
        self.overlay = gray && overlay;
        self.valid = true;
        self.generation = self.generation.wrapping_add(1);
        #[cfg(feature = "dev-usb")]
        { self.refresh_ms = refresh_ms; }
        #[cfg(not(feature = "dev-usb"))]
        let _ = refresh_ms;
    }

    /// Caller has completed the pure renderer; every SDK scratch loan ended.
    pub fn binary(&mut self, board: &mut Board<'_>, logical: &Frame,
        budget: &mut UiRefresh, panels: Option<&mut PanelRing>, force: bool, started: u32,
        composition: Composition<'_>, control: &UiControl<'_>,
    ) -> Result<(), Error> {
        let full = force || !self.valid || self.grayscale
            || self.orientation != Orientation::Portrait || budget.needs_full();
        native(board, logical, false)?;
        let trace = composition.trace;
        drop(composition);
        let inverse = self.valid && self.overlay;
        {
            let _action = trace.scope(ActionStage::BlockingDisplay);
            if inverse { drive_from_inverse(board)?; }
            binary_panel(board, panels, if full && !inverse { Refresh::Full } else { Refresh::Fast },
                if inverse { "leave-overlay-inverse" } else { "ui" })?;
        }
        budget.painted(full);
        self.completed(board, Orientation::Portrait, false, false, started, control);
        Ok(())
    }
}

/// The same blocking binary SDK publication when composition already lives
/// in the native buffer (the update download's TLS-lent progress bar).
pub fn blocking_binary(board: &mut Board<'_>, panels: Option<&mut PanelRing>, refresh: Refresh, reason: &str) -> Result<(), Error> {
    binary_panel(board, panels, refresh, reason)
}

/// The same publication while native source descriptors retain the board loan.
pub fn blocking_binary_panel(panel: &mut ReaderPanel<'_, '_>, panels: Option<&mut PanelRing>, refresh: Refresh, reason: &str) -> Result<(), Error> {
    binary_panel(panel, panels, refresh, reason)
}

// Exact immutable native regions from home_display.h and the existing SDK
// template. Only these byte-aligned interiors participate in focus changes.
const FOCUS: [NativeRegion; 3] = [
    NativeRegion { x: 568, y: 374, width: 8, height: 108 },
    NativeRegion { x: 568, y: 210, width: 8, height: 108 },
    NativeRegion { x: 568, y: 46, width: 8, height: 108 },
];

/// Frozen book_open_display.h: a working frame owes its refresh credit to the
/// eventual destination. It does not advance either existing paint budget.
pub fn working(board: &mut Board<'_>, shown: &mut DisplayReceipt, work: newt_reader_ui::SlowWork,
    bar: &newt_system_ui::SystemBar, settings: newt_core::preferences::ViewSettings,
    logical: &mut Frame, panels: Option<&mut PanelRing>, started: u32, control: &UiControl<'_>, trace: Trace<'_>,
) -> Result<bool, Error> {
    working_common(board, shown, work, bar, settings, logical, panels, started, control, trace)
}

/// Working publication between synchronous source/frame loans.
pub fn working_panel(panel: &mut ReaderPanel<'_, '_>, shown: &mut DisplayReceipt, work: newt_reader_ui::SlowWork,
    bar: &newt_system_ui::SystemBar, settings: newt_core::preferences::ViewSettings,
    logical: &mut Frame, panels: Option<&mut PanelRing>, started: u32, control: &UiControl<'_>, trace: Trace<'_>,
) -> Result<bool, Error> {
    working_common(panel, shown, work, bar, settings, logical, panels, started, control, trace)
}

fn working_common(board: &mut impl BinaryPanel, shown: &mut DisplayReceipt, work: newt_reader_ui::SlowWork,
    bar: &newt_system_ui::SystemBar, settings: newt_core::preferences::ViewSettings,
    logical: &mut Frame, panels: Option<&mut PanelRing>, started: u32, control: &UiControl<'_>, trace: Trace<'_>,
) -> Result<bool, Error> {
    use newt_reader_ui::SlowWork;
    let orientation = if matches!(work, SlowWork::FirstOpen) { Orientation::Portrait }
        else { settings.orientation };
    {
        let _action = trace.scope(ActionStage::Composition);
        if matches!(work, SlowWork::FirstOpen) {
            newt_reader_ui::render_book_opening_ui(bar, Some(logical));
        } else {
            newt_reader_ui::render_reader_working_ui(work, settings, Some(logical));
        }
        frame::to_native_frame(logical, frame(board)?, orientation);
    }
    let inverse = shown.valid && shown.overlay;
    let fast = !inverse && shown.valid && !shown.grayscale;
    {
        let _action = trace.scope(ActionStage::BlockingDisplay);
        if inverse { drive_inverse(board)?; }
        binary_panel(board, panels, if inverse || fast { Refresh::Fast } else { Refresh::Full },
            match (work, inverse) {
            (SlowWork::FirstOpen, false) => "working-first-open",
            (SlowWork::FirstOpen, true) => "working-first-open-inverse",
            (SlowWork::Opening, false) => "working-opening",
            (SlowWork::Opening, true) => "working-opening-inverse",
            (SlowWork::Saving, false) => "working-saving",
            (SlowWork::Saving, true) => "working-saving-inverse",
            (SlowWork::Applying, false) => "working-applying",
            (SlowWork::Applying, true) => "working-applying-inverse",
            (SlowWork::Loading, false) => "working-loading",
            (SlowWork::Loading, true) => "working-loading-inverse",
            })?;
    }
    #[cfg(feature = "dev-usb")]
    let refresh_ms = board.millis().wrapping_sub(started);
    #[cfg(not(feature = "dev-usb"))]
    let refresh_ms = { let _ = started; 0 };
    shown.completed_at(orientation, false, false, refresh_ms, control);
    Ok(!fast)
}

/// The actual completed paint result, borrowed by no later operation.
pub struct HomePaintUpdate { pub grayscale: bool, pub full: bool, pub overlay: bool }

/// Focus returns before a clock/cover read. A rejected focus falls through to
/// exactly one full composition, whose planes share the actual prepared views.
pub fn home(
    runtime: &AppRuntime<'_, '_>, home: &mut Home<'_>, policy: &mut HomeRefresh,
    budget: &mut UiRefresh, shown: &mut DisplayReceipt, info: &HeaderInfo,
    data: HomeData<'_>, logical: &mut Frame, style: &HomeStyle<'_>,
    cover: &mut HomeCover, identity: Option<&SdResumeRecord>, force: bool, status_only: bool,
    mut panels: Option<&mut PanelRing>, control: &UiControl<'_>,
) -> Result<Option<HomePaintUpdate>, Error> {
    let marker = home.clock.begin_paint();
    if !policy.needs_paint(runtime.rest.app, info, &data, home.clock.shown(marker), force) || status_only { return Ok(None); }
    let started = refresh_start(&mut home.bus.board);
    let cleanup = force || budget.needs_full();
    let focus = !cleanup && policy.focus_only(runtime.rest.app, info, &data, home.clock.shown(marker));
    let quiet = style.focus == HomeFocusStyle::BinaryCursor;
    if focus && quiet && policy.selected() < 3
        && home.bus.board.gray_encoding(GrayMode::Direct)? == GrayEncoding::AbsolutePlanes {
        let next = home_section(runtime.rest.app.selected) as u32;
        if newt_stage_home::render_stage_home_focus(next, HomePlane::Msb, Some(logical)) {
            let ready = frame::to_native_selector_regions(logical, frame(&mut home.bus.board)?, &FOCUS);
            if ready && panel(&mut home.bus.board, panels.as_deref_mut(), PanelCall::Focus((1 << policy.selected()) | (1 << next)), "home-stage-focus")? {
                let data = HomeData { clock: home.clock.shown(marker), ..data };
                policy.remember(runtime.rest.app, info, &data);
                budget.painted(false);
                shown.completed(&mut home.bus.board, Orientation::Portrait, true, shown.overlay, started, control);
                return Ok(Some(HomePaintUpdate { grayscale: true, full: false, overlay: shown.overlay }));
            }
        }
    }
    home.clock.paint(&mut home.bus, marker);
    let data = HomeData { clock: home.clock.shown(marker), ..data };
    let board = &mut home.bus.board;
    let overlay = if quiet { overlay(runtime, board, info, &data, logical, style, cover, identity,
        shown.valid && shown.overlay && !cleanup, cleanup, &mut panels)? } else { false };
    let gray = if overlay { true } else {
        direct(runtime, board, info, &data, logical, style, cover, identity, &mut panels)?
    };
    policy.remember(runtime.rest.app, info, &data);
    budget.painted(true);
    shown.completed(board, Orientation::Portrait, gray, overlay, started, control);
    // One number-only row per full Home composition: why a cover is absent.
    #[cfg(feature = "dev-usb")]
    {
        let (eligible, attempted, step, ready) = cover.report();
        let _ = super::trace::row(&home.bus.log, 120, format_args!(
            "HOME_COVER eligible={} attempted={} step={} ready={} painted={}\n",
            u8::from(eligible), u8::from(attempted), step as u8, u8::from(ready), u8::from(ready && shown.valid)));
    }
    Ok(Some(HomePaintUpdate { grayscale: gray, full: true, overlay }))
}

fn overlay(runtime: &AppRuntime<'_, '_>, board: &mut Board<'_>, info: &HeaderInfo,
    data: &HomeData<'_>, logical: &mut Frame, style: &HomeStyle<'_>, cover: &mut HomeCover, identity: Option<&SdResumeRecord>,
    rebase: bool, cleanup: bool, panels: &mut Option<&mut PanelRing>,
) -> Result<bool, Error> {
    if board.gray_encoding(GrayMode::Overlay)? != GrayEncoding::OverlayMasks { return Ok(false); }
    // The frozen optional one-paint mask; failure enters the real Direct path.
    let Some(mut masks) = Bytes::try_new(frame::FRAME_BYTES) else { return Ok(false); };
    let masks: &mut Frame = masks.as_mut().try_into().map_err(|_| Error::Length)?;
    read_cover(board, cover, identity);
    let cover = cover.held();
    runtime.paint_home(info, data, HomePlane::Msb, Some(logical), style, cover.as_ref());
    native(board, logical, true)?;
    masks.copy_from_slice(frame(board)?);
    if rebase { drive_from_inverse(board)?; }
    else if cleanup { board.gray_resync()?; }
    if !panel(board, panels.as_deref_mut(), PanelCall::Base(GrayMode::Overlay, true, Refresh::Half),
        if cleanup { "home-cleanup" } else if rebase { "home-inverse-rebase" } else { "home-overlay-base" })? {
        return Ok(false);
    }
    runtime.paint_home(info, data, HomePlane::Lsb, Some(logical), style, cover.as_ref());
    native(board, logical, true)?;
    for (lsb, msb) in frame(board)?.iter_mut().zip(masks.iter_mut()) {
        let original = *lsb;
        *lsb &= !*msb;
        *msb ^= original;
    }
    board.gray_copy_frame(GrayPlane::Lsb)?;
    board.gray_copy_selectors(GrayPlane::Msb, masks)?;
    panel(board, panels.as_deref_mut(), PanelCall::Display, "home")?;
    Ok(true)
}

fn direct(runtime: &AppRuntime<'_, '_>, board: &mut Board<'_>, info: &HeaderInfo,
    data: &HomeData<'_>, logical: &mut Frame, style: &HomeStyle<'_>, cover: &mut HomeCover, identity: Option<&SdResumeRecord>,
    panels: &mut Option<&mut PanelRing>,
) -> Result<bool, Error> {
    let caps = board.gray_encoding(GrayMode::Direct)?;
    read_cover(board, cover, identity);
    let cover = cover.held();
    if caps == GrayEncoding::AbsolutePlanes {
        let mut base = false;
        if style.focus == HomeFocusStyle::BinaryCursor {
            base = panel(board, panels.as_deref_mut(), PanelCall::Base(GrayMode::Direct, true, Refresh::Half), "home-stage")?;
        }
        if base || panel(board, panels.as_deref_mut(), PanelCall::Base(GrayMode::Direct, false, Refresh::Half), "home-stage")? {
            for (plane, sdk) in [(HomePlane::Lsb, GrayPlane::Lsb), (HomePlane::Msb, GrayPlane::Msb)] {
                runtime.paint_home(info, data, plane, Some(logical), style, cover.as_ref());
                native(board, logical, true)?;
                board.gray_copy_frame(sdk)?;
            }
            panel(board, panels.as_deref_mut(), PanelCall::Display, "home-stage")?;
            return Ok(true);
        }
    }
    runtime.paint_home(info, data, HomePlane::Monochrome, Some(logical), style, cover.as_ref());
    native(board, logical, false)?;
    binary_panel(board, panels.as_deref_mut(), Refresh::Full, "home-fallback")?;
    Ok(false)
}

fn read_cover(board: &mut Board<'_>, cover: &mut HomeCover, identity: Option<&SdResumeRecord>) {
    if !board.sd_mounted() { cover.failed_attempt(Step::Unmounted); return; }
    if board.with_reader_io(|_, io| cover.read_once(io, &mut |out| {
        let Some(identity) = identity else { return false; };
        *out = *identity;
        identity.valid()
    })).is_err() { cover.failed_attempt(Step::Loan); }
}

// Only operations needed by working/binary publication with live source FDs.
trait BinaryPanel {
    fn frame(&mut self) -> Result<&mut [u8], Error>;
    fn millis(&mut self) -> u32;
    fn update(&mut self, refresh: Refresh) -> Result<(), Error>;
    fn cleanup_frame(&mut self) -> Result<(), Error>;
}
impl BinaryPanel for Board<'_> {
    fn frame(&mut self) -> Result<&mut [u8], Error> { Board::frame(self) }
    fn millis(&mut self) -> u32 { Board::millis(self) }
    fn update(&mut self, refresh: Refresh) -> Result<(), Error> { Board::update(self, refresh, false) }
    fn cleanup_frame(&mut self) -> Result<(), Error> { self.gray_cleanup_frame() }
}
impl BinaryPanel for ReaderPanel<'_, '_> {
    fn frame(&mut self) -> Result<&mut [u8], Error> { ReaderPanel::frame(self) }
    fn millis(&mut self) -> u32 { ReaderPanel::millis(self) }
    fn update(&mut self, refresh: Refresh) -> Result<(), Error> { ReaderPanel::update(self, refresh, false) }
    fn cleanup_frame(&mut self) -> Result<(), Error> { self.gray_cleanup_frame() }
}

fn binary_panel(board: &mut impl BinaryPanel, ring: Option<&mut PanelRing>, refresh: Refresh, reason: &str) -> Result<(), Error> {
    let mode = if matches!(refresh, Refresh::Full) { "full" } else { "fast" };
    traced_panel(board, ring, mode, reason, |board| board.update(refresh).map(|()| true)).map(|_| ())
}

fn traced_panel<P: BinaryPanel>(board: &mut P, mut ring: Option<&mut PanelRing>, mode: &str, reason: &str,
    call: impl FnOnce(&mut P) -> Result<bool, Error>,
) -> Result<bool, Error> {
    let token = ring.as_deref_mut().map(|ring| ring.begin(board.millis(), mode, reason));
    let result = call(board);
    if let (Some(ring), Some(token)) = (ring, token) {
        ring.finish(token, board.millis(), if matches!(result, Ok(true)) { PanelOutcome::Accepted } else { PanelOutcome::Rejected });
    }
    result
}

fn frame<'a>(board: &'a mut impl BinaryPanel) -> Result<&'a mut Frame, Error> {
    board.frame()?.try_into().map_err(|_| Error::Length)
}
fn native(board: &mut Board<'_>, logical: &Frame, selectors: bool) -> Result<(), Error> {
    if selectors { frame::to_native_selectors(logical, frame(board)?, Orientation::Portrait); }
    else { frame::to_native_frame(logical, frame(board)?, Orientation::Portrait); }
    Ok(())
}
pub fn drive_from_inverse(board: &mut Board<'_>) -> Result<(), Error> {
    drive_inverse(board)
}
fn drive_inverse(board: &mut impl BinaryPanel) -> Result<(), Error> {
    for byte in frame(board)? { *byte = !*byte; }
    board.cleanup_frame()?;
    for byte in frame(board)? { *byte = !*byte; }
    Ok(())
}

enum PanelCall { Base(GrayMode, bool, Refresh), Focus(u32), Display }
pub fn refresh_start(board: &mut Board<'_>) -> u32 {
    #[cfg(feature = "dev-usb")]
    { board.millis() }
    #[cfg(not(feature = "dev-usb"))]
    { let _ = board; 0 }
}
fn panel(board: &mut Board<'_>, ring: Option<&mut PanelRing>, call: PanelCall, reason: &str) -> Result<bool, Error> {
    let mode = match call {
        PanelCall::Base(GrayMode::Overlay, _, _) => "gray-base-half", PanelCall::Base(..) => "gray-base",
        PanelCall::Focus(_) => "binary-regions-fast", PanelCall::Display if reason == "home" => "gray-overlay",
        PanelCall::Display => "gray-direct" };
    traced_panel(board, ring, mode, reason, |board| match call {
        PanelCall::Base(mode, binary, refresh) => board.gray_base(mode, binary, refresh, false),
        PanelCall::Focus(mask) => board.gray_binary_focus(mask, true),
        PanelCall::Display => board.gray_display().map(|()| true),
    })
}
