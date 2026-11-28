use crate::*;
use newt_core::app::step_stats_graph;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum ReaderPanel {
    #[default]
    Page,
    Menu,
    Chapters,
    Settings,
    Statistics,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ReaderUiIntent {
    None,
    Repaint,
    PreviousPage,
    NextPage,
    Home,
    Sleep,
    SaveSettings,
    MarkFinished,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum ReaderUiNotice {
    #[default]
    None,
    SaveFailed,
    ChapterLocationUnresolved,
    ChapterJumpFailed,
    BookChanged,
    VerificationFailed,
    VerificationTimedOut,
    FinishSaveFailed,
    FinishUnconfirmed,
    FinishOpening,
    /// A Contents jump ran out of time; the reader kept its page (#199).
    ChapterJumpTimedOut,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum ReaderFinishRow {
    #[default]
    Hidden,
    Mark,
    Undo,
}
/// Reading settings rows, top to bottom; `ReaderUiState::selected` indexes them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsRow {
    Orientation,
    TextSize,
    SlimProgressBar,
    Refresh,
    Save,
    Cancel,
}
impl SettingsRow {
    pub const ALL: [Self; 6] =
        [Self::Orientation, Self::TextSize, Self::SlimProgressBar, Self::Refresh, Self::Save, Self::Cancel];
    /// Out-of-range selections land on Cancel, the safe row.
    pub fn at(selected: u32) -> Self {
        Self::ALL.get(selected as usize).copied().unwrap_or(Self::Cancel)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReaderUiState {
    pub panel: ReaderPanel,
    pub selected: u32,
    pub draft: ViewSettings,
    pub preferences_writable: bool,
    pub notice: ReaderUiNotice,
    pub finish: ReaderFinishRow,
}
impl Default for ReaderUiState {
    fn default() -> Self {
        Self {
            panel: ReaderPanel::Page,
            selected: 0,
            draft: ViewSettings::default(),
            preferences_writable: true,
            notice: ReaderUiNotice::None,
            finish: ReaderFinishRow::Hidden,
        }
    }
}
/// Failed-open routing without reader storage, chapter recovery or tracker work.
pub fn dispatch_unavailable_reader_ui(
    ui: &mut ReaderUiState,
    settings: ViewSettings,
    action: Action,
) -> ReaderUiIntent {
    if ui.panel != ReaderPanel::Settings {
        if let Some(intent) = route_unavailable_reader_ui(ui, action) {
            return intent;
        }
    }
    dispatch_reader_ui_with_availability(ui, settings, action, 0, false)
}
/// A completed route returns before ordinary dispatch; Menu Activate/Back
/// continue with the restricted selection already applied.
pub(crate) fn route_unavailable_reader_ui(
    ui: &mut ReaderUiState,
    action: Action,
) -> Option<ReaderUiIntent> {
    use ReaderPanel as P;
    use ReaderUiIntent as I;
    if action == Action::Sleep {
        return Some(I::Sleep);
    }
    if ui.panel != P::Menu {
        if action == Action::Back {
            return Some(I::Home);
        }
        if ui.panel == P::Page && action == Action::Activate {
            ui.panel = P::Menu;
            ui.selected = 2;
            return Some(I::Repaint);
        }
        return Some(I::None);
    }
    if matches!(action, Action::Previous | Action::Next) {
        let old = ui.selected;
        ui.selected = if action == Action::Previous { 2 } else { 5 };
        return Some(if old == ui.selected { I::None } else { I::Repaint });
    }
    if action == Action::Activate {
        ui.selected = if ui.selected == 5 { 5 } else { 2 };
    } else if action != Action::Back {
        return Some(I::None);
    }
    None
}
pub(crate) fn dispatch_reader_ui_with_availability(
    ui: &mut ReaderUiState,
    settings: ViewSettings,
    action: Action,
    chapter_count: u32,
    opened: bool,
) -> ReaderUiIntent {
    let previous = ui.panel;
    let intent = dispatch_reader_ui(ui, settings, action, chapter_count);
    if !opened && previous == ReaderPanel::Settings && ui.panel == ReaderPanel::Menu {
        ui.panel = ReaderPanel::Page;
    }
    intent
}
/// Storage intents retain the draft. Only the owner may acknowledge a verified commit.
pub fn dispatch_reader_ui(
    ui: &mut ReaderUiState,
    settings: ViewSettings,
    action: Action,
    chapter_count: u32,
) -> ReaderUiIntent {
    use ReaderPanel as P;
    use ReaderUiIntent as I;
    use ReaderUiNotice as N;
    if matches!(ui.notice, N::BookChanged | N::VerificationTimedOut) {
        ui.notice = N::None;
    }
    if action == Action::Sleep {
        return I::Sleep;
    }
    if ui.panel == P::Page {
        return match action {
            Action::Previous => I::PreviousPage,
            Action::Next => I::NextPage,
            Action::Back => I::Home,
            Action::Activate => {
                ui.panel = P::Menu;
                ui.selected = 0;
                I::Repaint
            }
            _ => I::None,
        };
    }
    if action == Action::Back {
        ui.panel = if ui.panel == P::Menu {
            P::Page
        } else {
            P::Menu
        };
        ui.selected = 0;
        ui.notice = N::None;
        return I::Repaint;
    }
    if ui.panel == P::Statistics {
        let mut packed = ui.selected as u8;
        if !step_stats_graph(&mut packed, action) {
            return I::None;
        }
        ui.selected = u32::from(packed);
        return I::Repaint;
    }
    if ui.panel == P::Chapters && chapter_count == 0 {
        return I::Repaint;
    }
    let count = match ui.panel {
        P::Menu => 6,
        P::Chapters => chapter_count,
        P::Settings => SettingsRow::ALL.len() as u32,
        _ => 0,
    };
    let old = ui.selected;
    ui.selected = if count == 0 {
        0
    } else {
        ui.selected.min(count - 1)
    };
    let skip = ui.panel == P::Menu && ui.finish == ReaderFinishRow::Hidden;
    if skip && ui.selected == 4 {
        ui.selected = 5;
    }
    if matches!(action, Action::Previous | Action::Next) {
        if action == Action::Previous && ui.selected > 0 {
            ui.selected -= 1;
        }
        if action == Action::Next && count > 0 && ui.selected < count - 1 {
            ui.selected += 1;
        }
        if skip && ui.selected == 4 {
            ui.selected = if action == Action::Previous { 3 } else { 5 };
        }
        if old != ui.selected {
            ui.notice = N::None;
        }
        return if old == ui.selected {
            I::None
        } else {
            I::Repaint
        };
    }
    if action != Action::Activate {
        return I::None;
    }
    if ui.panel == P::Menu {
        match ui.selected {
            0 => ui.panel = P::Page,
            1 => {
                ui.panel = P::Chapters;
                ui.selected = newt_reader_navigation::FOCUS_CURRENT;
            }
            2 => {
                ui.panel = P::Settings;
                ui.draft = settings;
                ui.notice = N::None;
            }
            3 => ui.panel = P::Statistics,
            4 => {
                ui.notice = N::None;
                return I::MarkFinished;
            }
            5 => return I::Home,
            _ => {}
        }
        if ui.panel != P::Chapters {
            ui.selected = 0;
        }
        ui.notice = N::None;
        return I::Repaint;
    }
    if ui.panel == P::Chapters && chapter_count > 0 {
        ui.panel = P::Page;
        return I::Repaint;
    }
    if ui.panel == P::Settings {
        match SettingsRow::at(ui.selected) {
            SettingsRow::Orientation => {
                ui.draft.orientation = match ui.draft.orientation {
                    Orientation::Portrait => Orientation::LandscapeCcw,
                    Orientation::LandscapeCcw => Orientation::LandscapeCw,
                    Orientation::LandscapeCw => Orientation::Portrait,
                }
            }
            SettingsRow::TextSize => {
                ui.draft.text_size = match ui.draft.text_size {
                    TextSize::Px22 => TextSize::Px24,
                    TextSize::Px24 => TextSize::Px27,
                    TextSize::Px27 => TextSize::Px30,
                    TextSize::Px30 => TextSize::Px34,
                    TextSize::Px34 => TextSize::Px22,
                }
            }
            SettingsRow::SlimProgressBar => ui.draft.slim_progress_bar = !ui.draft.slim_progress_bar,
            SettingsRow::Refresh => {
                ui.draft.refresh_pages = match ui.draft.refresh_pages {
                    1 => 5,
                    5 => 10,
                    _ => 1,
                }
            }
            SettingsRow::Save => {
                if ui.preferences_writable {
                    return I::SaveSettings;
                }
                ui.notice = N::SaveFailed;
            }
            SettingsRow::Cancel => {
                ui.panel = P::Menu;
                ui.selected = 0;
                ui.notice = N::None;
            }
        }
        return I::Repaint;
    }
    I::None
}

#[cfg(test)]
mod unavailable_cases {
    use super::*;

    #[test]
    fn unavailable_page_opens_settings_recovery_without_navigation() {
        let settings = ViewSettings::default();
        let mut ui = ReaderUiState::default();
        for action in [Action::Previous, Action::Next] {
            assert_eq!(dispatch_unavailable_reader_ui(&mut ui, settings, action), ReaderUiIntent::None);
            assert_eq!(ui.panel, ReaderPanel::Page);
        }
        assert_eq!(dispatch_unavailable_reader_ui(&mut ui, settings, Action::Activate), ReaderUiIntent::Repaint);
        assert_eq!((ui.panel, ui.selected), (ReaderPanel::Menu, 2));
        assert_eq!(dispatch_unavailable_reader_ui(&mut ui, settings, Action::Activate), ReaderUiIntent::Repaint);
        assert_eq!((ui.panel, ui.selected, ui.draft), (ReaderPanel::Settings, 0, settings));
        assert_eq!(dispatch_unavailable_reader_ui(&mut ui, settings, Action::Back), ReaderUiIntent::Repaint);
        assert_eq!(ui.panel, ReaderPanel::Page);
    }

    #[test]
    fn unavailable_menu_limits_focus_and_rejects_stale_book_actions() {
        let settings = ViewSettings::default();
        let mut ui = ReaderUiState { panel: ReaderPanel::Menu, selected: 3, ..ReaderUiState::default() };
        assert_eq!(dispatch_unavailable_reader_ui(&mut ui, settings, Action::Previous), ReaderUiIntent::Repaint);
        assert_eq!(ui.selected, 2);
        assert_eq!(dispatch_unavailable_reader_ui(&mut ui, settings, Action::Previous), ReaderUiIntent::None);
        assert_eq!(dispatch_unavailable_reader_ui(&mut ui, settings, Action::Next), ReaderUiIntent::Repaint);
        assert_eq!(ui.selected, 5);
        assert_eq!(dispatch_unavailable_reader_ui(&mut ui, settings, Action::Activate), ReaderUiIntent::Home);
        ui.selected = 3;
        assert_eq!(dispatch_unavailable_reader_ui(&mut ui, settings, Action::Activate), ReaderUiIntent::Repaint);
        assert_eq!(ui.panel, ReaderPanel::Settings);
    }

    #[test]
    fn unavailable_settings_preserves_save_contract_and_cancels_to_page() {
        let settings = ViewSettings::default();
        let mut ui = ReaderUiState { panel: ReaderPanel::Settings, selected: SettingsRow::Save as u32, ..ReaderUiState::default() };
        ui.draft.slim_progress_bar = !settings.slim_progress_bar;
        let draft = ui.draft;
        assert_eq!(dispatch_unavailable_reader_ui(&mut ui, settings, Action::Activate), ReaderUiIntent::SaveSettings);
        assert_eq!((ui.panel, ui.draft), (ReaderPanel::Settings, draft));
        ui.preferences_writable = false;
        assert_eq!(dispatch_unavailable_reader_ui(&mut ui, settings, Action::Activate), ReaderUiIntent::Repaint);
        assert_eq!((ui.panel, ui.draft, ui.notice), (ReaderPanel::Settings, draft, ReaderUiNotice::SaveFailed));
        ui.selected = SettingsRow::Cancel as u32;
        assert_eq!(dispatch_unavailable_reader_ui(&mut ui, settings, Action::Activate), ReaderUiIntent::Repaint);
        assert_eq!(ui.panel, ReaderPanel::Page);
    }

    #[test]
    fn unavailable_early_routes_keep_notices_and_allow_exits() {
        let settings = ViewSettings::default();
        for panel in [ReaderPanel::Page, ReaderPanel::Menu, ReaderPanel::Chapters, ReaderPanel::Statistics] {
            let mut ui = ReaderUiState { panel, notice: ReaderUiNotice::BookChanged, ..ReaderUiState::default() };
            assert_eq!(dispatch_unavailable_reader_ui(&mut ui, settings, Action::Sleep), ReaderUiIntent::Sleep);
            assert_eq!(ui.notice, ReaderUiNotice::BookChanged);
            if panel != ReaderPanel::Menu {
                assert_eq!(dispatch_unavailable_reader_ui(&mut ui, settings, Action::Back), ReaderUiIntent::Home);
            }
        }
    }
}
