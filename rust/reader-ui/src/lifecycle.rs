//! Partial concrete owner routing: turn/jump remain explicit owned work values.
use crate::*;
use newt_core::{
    tracker::ReadingTracker, xml_session::ParserBackend, zip_container::Status,
    zip_window::source::BookSource,
};
use newt_phased_reader::OperationReader;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReaderUiWork {
    Intent(ReaderUiIntent),
    Turn(Action),
    Jump(u32),
}
/// Includes optional-workspace denial even when the owner remains readable.
#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReaderUiResult {
    pub work: ReaderUiWork,
    pub menu: Option<MenuPreparation>,
}
/// Consumes/drops the real operation view for one call only. Returned work owns no
/// Action, source, parser or frame loan. Turn/jump completion belongs to the
/// SampleReader-equivalent owner and must not be published as success here.
#[allow(clippy::too_many_arguments)]
pub fn handle_reader_ui<B: ParserBackend + ?Sized, S: BookSource + ?Sized>(
    op: OperationReader<'_, '_, '_, '_, '_, '_, B>,
    source: &mut S,
    owner: &mut ReaderOwnerState,
    ui: &mut ReaderUiState,
    settings: ViewSettings,
    action: Action,
    now: u32,
    tracker: &mut ReadingTracker,
    chapter_failure: Option<&ReaderChapterFailure>,
    directory: &mut [u8; 512],
    mapping: &mut [u32],
) -> ReaderUiResult {
    handle_reader_ui_with(&mut LegacyMenu { op, source, directory, mapping },
        owner, ui, settings, action, now, tracker, chapter_failure)
}

pub(crate) fn handle_reader_ui_with(
    op: &mut impl MenuOperation,
    owner: &mut ReaderOwnerState, ui: &mut ReaderUiState, settings: ViewSettings,
    action: Action, now: u32, tracker: &mut ReadingTracker,
    chapter_failure: Option<&ReaderChapterFailure>,
) -> ReaderUiResult {
    use ReaderPanel as P;
    use ReaderUiIntent as I;
    let mut menu = None;
    if !owner.opened() && ui.panel != P::Settings {
        tracker.pause(now);
        if action != Action::Sleep
            && ui.panel == P::Page
            && chapter_failure.is_some()
            && matches!(action, Action::Previous | Action::Next)
        {
            return ReaderUiResult {
                work: ReaderUiWork::Turn(action),
                menu,
            };
        }
        if let Some(intent) = crate::dispatch::route_unavailable_reader_ui(ui, action) {
            if matches!(intent, I::Home | I::Sleep) {
                tracker.leave(now);
            }
            return ReaderUiResult {
                work: ReaderUiWork::Intent(intent),
                menu,
            };
        }
    }
    let previous = ui.panel;
    let retried = previous == P::Chapters && !op.menu().info().valid;
    if retried {
        let result = prepare_menu(
            op, owner, ui, newt_reader_navigation::FOCUS_CURRENT,
        );
        menu = Some(result);
        if result.valid {
            ui.selected = op.menu().info().focus;
        }
    }
    let shown_menu = op.menu().info();
    let intent = crate::dispatch::dispatch_reader_ui_with_availability(
        ui,
        settings,
        action,
        if shown_menu.valid {
            shown_menu.count
        } else {
            0
        },
        owner.opened(),
    );
    if matches!(intent, I::PreviousPage | I::NextPage) {
        return ReaderUiResult {
            work: ReaderUiWork::Turn(action),
            menu,
        };
    }
    if previous == P::Chapters && ui.panel == P::Page {
        tracker.pause(now);
        return ReaderUiResult {
            work: ReaderUiWork::Jump(ui.selected),
            menu,
        };
    }
    if previous == P::Menu && ui.panel == P::Chapters {
        let result = prepare_menu(
            op, owner, ui, newt_reader_navigation::FOCUS_CURRENT,
        );
        menu = Some(result);
        if result.valid {
            ui.selected = op.menu().info().focus;
        }
    }
    if matches!(intent, I::Home | I::Sleep) {
        tracker.leave(now);
        return ReaderUiResult {
            work: ReaderUiWork::Intent(intent),
            menu,
        };
    }
    if ui.panel != P::Page || !owner.opened() {
        tracker.pause(now);
    } else if previous != P::Page && owner.opened() {
        tracker.enter(now, op.shown(owner));
    }
    ReaderUiResult {
        work: ReaderUiWork::Intent(if retried && intent == I::None {
            I::Repaint
        } else {
            intent
        }),
        menu,
    }
}
pub(crate) trait MenuOperation {
    fn menu(&self) -> newt_phased_reader::ChapterMenuView<'_>;
    fn shown(&self, owner: &ReaderOwnerState) -> newt_core::tracker::ShownPage;
    fn prepare(&mut self, owner: &mut ReaderOwnerState, focus: u32) -> MenuPreparation;
}
// Own the legacy operation until shared policy returns. Its Drop-bearing fields
// remain intact; no lease is recreated or replaced with a dummy.
pub(crate) struct LegacyMenu<O, S, D, M> { pub op: O, pub source: S, pub directory: D, pub mapping: M }
impl<'sel, 'plan, B: ParserBackend + ?Sized, S: BookSource + ?Sized>
    MenuOperation for LegacyMenu<
        OperationReader<'_, 'sel, 'plan, '_, '_, '_, B>, &mut S, &mut [u8; 512], &mut [u32]>
{
    fn menu(&self) -> newt_phased_reader::ChapterMenuView<'_> { self.op.reader.chapter_menu() }
    fn shown(&self, owner: &ReaderOwnerState) -> newt_core::tracker::ShownPage {
        owner.position(self.op.reader).shown
    }
    fn prepare(&mut self, owner: &mut ReaderOwnerState, focus: u32) -> MenuPreparation {
        let status = legacy_prepare(&mut self.op, self.source, focus, self.directory, self.mapping);
        owner.menu_result(status);
        MenuPreparation { status: Ok(status), valid: status == Status::Ok && self.menu().info().valid }
    }
}
fn legacy_prepare<B: ParserBackend + ?Sized, S: BookSource + ?Sized>(
    op: &mut OperationReader<'_, '_, '_, '_, '_, '_, B>, source: &mut S,
    focus: u32, directory: &mut [u8; 512], mapping: &mut [u32],
) -> Status {
    op.reader.prepare_chapter_menu(source, focus, op.metadata, op.parse, op.resources,
        op.toc, directory, mapping, Some(op.lease))
}
/// A cancelled load keeps Contents open with the jump notice, which firmware
/// words as a timeout when the time budget ran out (#199).
pub(crate) fn prepare_menu(
    op: &mut impl MenuOperation, owner: &mut ReaderOwnerState, ui: &mut ReaderUiState, focus: u32,
) -> MenuPreparation {
    if !owner.opened() {
        return MenuPreparation { status: Ok(owner.status()), valid: false };
    }
    let result = op.prepare(owner, focus);
    if result.status.is_err() {
        ui.panel = ReaderPanel::Chapters;
        ui.notice = ReaderUiNotice::ChapterJumpFailed;
    }
    result
}

/// External readers expose the raw menu result. The bounded SDK bool API
/// preserves its existing owner status and reports optional readiness in valid.
#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MenuPreparation {
    /// Err: a cancel stopped the load. It says nothing about the book (#199).
    pub status: Result<Status, crate::Cancelled>,
    pub valid: bool,
}
