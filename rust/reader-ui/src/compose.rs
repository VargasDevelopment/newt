use crate::*;
use newt_core::{progress::Progress, zip_container::Status, zip_window::source::BookSource};
use newt_phased_reader::{ChapterMenuView, PageInfo, PhasedReader};
use newt_reader_navigation::Section;
/// External SampleReader paintWithChapterMenu preparation. Construct/drop the
/// actual OperationReader for EACH call, before borrowing any paint frame.
/// First: prepare(selected), paint. Retry: prepare(selected); paint only on
/// success. A null frame must skip this entire sequence at the call site.
#[allow(clippy::too_many_arguments)]
pub fn prepare_reader_ui_paint<
    B: newt_core::xml_session::ParserBackend + ?Sized,
    S: BookSource + ?Sized,
>(
    op: newt_phased_reader::OperationReader<'_, '_, '_, '_, '_, '_, B>,
    source: &mut S,
    owner: &mut ReaderOwnerState,
    ui: &mut ReaderUiState,
    pass: ReaderPaintPass,
    directory: &mut [u8; 512],
    mapping: &mut [u32],
) -> PreparedReaderPaint {
    prepare_reader_ui_paint_with(&mut crate::lifecycle::LegacyMenu {
        op, source, directory, mapping,
    }, owner, ui, pass)
}
pub(crate) fn prepare_reader_ui_paint_with(
    op: &mut impl crate::lifecycle::MenuOperation,
    owner: &mut ReaderOwnerState, ui: &mut ReaderUiState, pass: ReaderPaintPass,
) -> PreparedReaderPaint {
    let retry = matches!(pass, ReaderPaintPass::RetryAfterReadyPage(_));
    let chapters = ui.panel == ReaderPanel::Chapters;
    let menu = chapters.then(|| {
        crate::lifecycle::prepare_menu(op, owner, ui, ui.selected)
    });
    if menu.is_some_and(|result| result.valid) {
        ui.selected = op.menu().info().focus;
    }
    let paint = !retry || menu.is_some_and(|result| result.valid);
    PreparedReaderPaint {
        menu,
        paint,
        retry_if_missing: chapters && !retry,
    }
}
/// Retry is requested only by RetryChapterMenu after the first paint succeeded.
#[derive(Debug, Eq, PartialEq)]
pub enum ReaderPaintPass {
    First,
    RetryAfterReadyPage(ReaderPaintRetry),
}
/// Consumed once within the same paint transaction; only successful first paint
/// can issue this continuation. It retains no source, parser or frame loan.
#[derive(Debug, Eq, PartialEq)]
pub struct ReaderPaintRetry(());
#[must_use]
pub struct PreparedReaderPaint {
    /// Err in its status: a cancel stopped the Contents load, so the paint
    /// is cancelled too.
    pub menu: Option<MenuPreparation>,
    pub(crate) paint: bool,
    retry_if_missing: bool,
}
#[must_use]
#[derive(Debug, Eq, PartialEq)]
pub enum ReaderPaintWork {
    Complete {
        ready: bool,
    },
    RetryChapterMenu(ReaderPaintRetry),
    /// A cancel stopped the paint. It drew nothing and says nothing about the
    /// book: nothing latches, and no frame or picture publishes (#199).
    Cancelled,
    /// The real revision Session repaint/show fallback needs its own operation.
    /// Keep this work pending; do not compose or publish a successful page.
    OwnerPaint {
        status: Status,
        completion: ReaderPaintCompletion,
    },
}
/// One outstanding fallback after an actual retained repaint attempt.
#[must_use]
#[derive(Debug, Eq, PartialEq)]
pub struct ReaderPaintCompletion {
    retry_if_missing: bool,
}
/// Retained-plan path only. Requires completed preparation and teardown above.
/// Unsupported is explicit owner work, not a latched parser failure: the real
/// Session may need its resource-backed show fallback before status is final.
#[allow(clippy::too_many_arguments)]
pub fn repaint_reader_ui<S: BookSource + ?Sized>(
    reader: &mut PhasedReader<'_, '_>,
    source: &mut S,
    owner: &mut ReaderOwnerState,
    prepared: PreparedReaderPaint,
    frame: &mut Frame,
    tracker: &mut newt_core::tracker::ReadingTracker,
    now: u32,
) -> ReaderPaintWork {
    repaint_reader_ui_with(reader, owner, prepared, tracker, now,
        |reader| Ok(reader.repaint(source, frame)))
}
pub(crate) fn repaint_reader_ui_with(
    reader: &mut PhasedReader<'_, '_>, owner: &mut ReaderOwnerState,
    prepared: PreparedReaderPaint, tracker: &mut newt_core::tracker::ReadingTracker,
    now: u32,
    repaint: impl FnOnce(&mut PhasedReader<'_, '_>) -> Result<Status, crate::external_owner::Cancelled>,
) -> ReaderPaintWork {
    // Retain first-paint readiness when retry preparation fails, even on a
    // fatal result. The owner latch still blocks routing and tracker resume.
    if prepared.menu.is_some_and(|menu| menu.status.is_err()) {
        tracker.pause(now);
        return ReaderPaintWork::Cancelled;
    }
    if !prepared.paint {
        if !owner.opened() {
            tracker.pause(now);
        }
        return ReaderPaintWork::Complete { ready: true };
    }
    if !owner.opened() {
        tracker.pause(now);
        return ReaderPaintWork::Complete { ready: false };
    }
    let Ok(status) = repaint(reader) else {
        tracker.pause(now);
        return ReaderPaintWork::Cancelled;
    };
    if status == Status::Unsupported {
        return ReaderPaintWork::OwnerPaint {
            status,
            completion: ReaderPaintCompletion {
                retry_if_missing: prepared.retry_if_missing,
            },
        };
    }
    finish_paint(
        reader,
        owner,
        status,
        prepared.retry_if_missing,
        tracker,
        now,
    )
}
/// Consumes the pending fallback after the actual Session paint has finished
/// and all Session/Action/resource loans have ended.
pub fn finish_reader_ui_paint(
    reader: &PhasedReader<'_, '_>,
    owner: &mut ReaderOwnerState,
    completion: ReaderPaintCompletion,
    status: Status,
    tracker: &mut newt_core::tracker::ReadingTracker,
    now: u32,
) -> ReaderPaintWork {
    finish_paint(
        reader,
        owner,
        status,
        completion.retry_if_missing,
        tracker,
        now,
    )
}
fn finish_paint(
    reader: &PhasedReader<'_, '_>,
    owner: &mut ReaderOwnerState,
    status: Status,
    retry_if_missing: bool,
    tracker: &mut newt_core::tracker::ReadingTracker,
    now: u32,
) -> ReaderPaintWork {
    owner.note_failure(status);
    if !owner.opened() {
        tracker.pause(now);
    }
    if owner.opened() && retry_if_missing && !reader.chapter_menu().info().valid {
        ReaderPaintWork::RetryChapterMenu(ReaderPaintRetry(()))
    } else {
        ReaderPaintWork::Complete {
            ready: owner.opened(),
        }
    }
}
/// Compose only after Complete, using its actual paint readiness. A fatal retry
/// can retain first-paint pixels/readiness while `owner.status()` is failed;
/// this never permits navigation or tracker resume.
/// Does not publish frames, read storage, acknowledge choices or mutate progress.
#[allow(clippy::too_many_arguments)]
pub fn compose_reader_ui(
    reader: &PhasedReader<'_, '_>,
    owner: &ReaderOwnerState,
    ui: &ReaderUiState,
    settings: ViewSettings,
    info: &DeviceInfo<'_>,
    progress: &Progress,
    ready: bool,
    frame: &mut Frame,
    failure: Option<&ReaderChapterFailure>,
    last_failure: Option<&ReaderChapterFailure>,
    book: Option<&BookStatsView>,
) {
    compose_reader_ui_with(
        Some(ReaderPaintProjection {
            page: owner.position(reader).page,
            section: owner.opened().then(|| reader.current_section()),
            title: reader.selection().title(),
            menu: reader.chapter_menu(),
        }), ReaderPaintContext { ui, settings, info, progress, ready }, frame,
        failure, last_failure, book,
    );
}
/// The bounded sample lends its actual projections to the same renderer. The
/// owner supplies the package's spine count; the reader retains no metadata.
pub struct SamplePaintSource<'a> {
    pub reader: &'a newt_phased_reader::bounded::Reader,
    pub title: &'a [u8],
    pub spines: u32,
    pub owner: &'a ReaderOwnerState,
}
pub fn compose_sample_reader_ui(source: SamplePaintSource<'_>,
    context: ReaderPaintContext<'_, '_>, frame: &mut Frame, book: Option<&BookStatsView>,
) {
    let SamplePaintSource { reader, title, spines, owner } = source;
    compose_reader_ui_with(
        Some(ReaderPaintProjection {
            page: owner.position_info(reader.info(), spines).page,
            section: owner.opened().then(|| reader.current_section()),
            title,
            menu: reader.chapter_menu(),
        }), context, frame,
        None, None, book,
    );
}
/// The same composition when reader storage is unavailable. Menu and Settings
/// remain usable without inventing a book, page or chapter menu.
pub fn compose_unavailable_reader_ui(
    ui: &ReaderUiState,
    settings: ViewSettings,
    info: &DeviceInfo<'_>,
    progress: &Progress,
    frame: &mut Frame,
    failure: Option<&ReaderChapterFailure>,
) {
    compose_reader_ui_with(
        None, ReaderPaintContext { ui, settings, info, progress, ready: false },
        frame, failure, None, None,
    );
}
pub struct ReaderPaintContext<'a, 'info> {
    pub ui: &'a ReaderUiState,
    pub settings: ViewSettings,
    pub info: &'a DeviceInfo<'info>,
    pub progress: &'a Progress,
    pub ready: bool,
}
struct ReaderPaintProjection<'a> {
    page: PageInfo,
    section: Option<Section<'a>>,
    title: &'a [u8],
    menu: ChapterMenuView<'a>,
}
fn compose_reader_ui_with(
    reader: Option<ReaderPaintProjection<'_>>,
    context: ReaderPaintContext<'_, '_>,
    frame: &mut Frame,
    failure: Option<&ReaderChapterFailure>,
    last_failure: Option<&ReaderChapterFailure>,
    book: Option<&BookStatsView>,
) {
    let ReaderPaintContext { ui, settings, info, progress, ready } = context;
    let chapters = ui.panel == ReaderPanel::Chapters;
    let page = reader.as_ref().map_or_else(PageInfo::default, |reader| reader.page);
    let section = reader.as_ref().and_then(|reader| reader.section);
    let title = reader.as_ref().map(|reader| reader.title);
    let menu = reader.map(|reader| reader.menu);
    let failed_jump = matches!(
        ui.notice,
        ReaderUiNotice::ChapterLocationUnresolved | ReaderUiNotice::ChapterJumpFailed
            | ReaderUiNotice::ChapterJumpTimedOut
    );
    let retained = !chapters || !failed_jump || menu.as_ref().is_some_and(|menu| menu.info().valid);
    let label = if ui.notice == ReaderUiNotice::BookChanged {
        Some(if ready {
            b"Book changed. Reopened.".as_slice()
        } else {
            b"Book changed. Reopen required.".as_slice()
        })
    } else {
        Some(section.map_or(b"".as_slice(), |section| section.label))
    };
    let footer = ReaderFooter {
        spine: section.map_or(0, |section| section.spine),
        page: page.page,
        pages: page.page_count,
        overall_known: page.overall_known,
        overall_percent: page.overall_percent,
        section_label: label,
    };
    render_reader_chrome(&footer, info, settings, ready && retained, Some(frame));
    render_verification_notice(ui.notice, settings, ready, Some(frame));
    if !(ready && retained) {
        if let Some(failure) = failure {
            render_chapter_failure(failure, info, settings, Some(frame));
        }
    }
    if ui.panel != ReaderPanel::Page
        && ((ready && retained) || matches!(ui.panel, ReaderPanel::Menu | ReaderPanel::Settings))
    {
        let generic = ReaderChapterFailure {
            spine: 0,
            status: Status::Ok,
            invalid_text: false,
        };
        let line = (ui.notice == ReaderUiNotice::ChapterJumpFailed)
            .then(|| chapter_failure_message(last_failure.unwrap_or(&generic)));
        render_reader_overlay(
            ui,
            settings,
            title,
            progress,
            ready,
            Some(frame),
            menu,
            line,
            if ui.panel == ReaderPanel::Statistics && ready {
                book
            } else {
                None
            },
        );
    }
}
/// Pure resolution failure must not call discard_chapter_publication: it did
/// not switch selection or consume the previous capture. Repaint failures do.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChapterJumpOutcome {
    Moved,
    Unresolved,
    Failed,
}
pub fn settle_chapter_jump(ui: &mut ReaderUiState, outcome: ChapterJumpOutcome) -> ReaderUiIntent {
    match outcome {
        ChapterJumpOutcome::Moved => {
            ui.selected = 0;
            ui.notice = ReaderUiNotice::None;
        }
        ChapterJumpOutcome::Unresolved => {
            ui.panel = ReaderPanel::Chapters;
            ui.notice = ReaderUiNotice::ChapterLocationUnresolved;
        }
        ChapterJumpOutcome::Failed => {
            ui.panel = ReaderPanel::Chapters;
            ui.notice = ReaderUiNotice::ChapterJumpFailed;
        }
    }
    ReaderUiIntent::Repaint
}

/// Only the actual paint sequence constructs this proof. Consumed once by final
/// logical composition + inversion; never used as an arbitrary success flag.
#[must_use]
pub struct PhysicalPaint(pub(crate) Painted);
#[derive(Clone, Copy)]
pub(crate) enum Painted {
    Drawn { ready: bool },
    Cancelled,
}
impl PhysicalPaint {
    /// Read readiness without consuming the proof needed by final composition.
    pub fn ready(&self) -> bool {
        matches!(self.0, Painted::Drawn { ready: true })
    }
}
/// What final composition left in the native frame. A cancelled paint
/// composes nothing: its caller publishes neither the frame nor a resume
/// picture, and the last frame stays on screen until a real paint (#199).
#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Composed {
    Drawn,
    Cancelled,
}
