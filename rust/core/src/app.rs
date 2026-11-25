//! Application screen dispatch policy. MIT.
//! Drawing, firmware-owned settings commits and physical power are separate.
#![forbid(unsafe_code)]

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HomeSection { #[default] Books, Apps, Settings }
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action { Previous, Next, Activate, Back, Sleep, Read }
/// Whether `screen` currently shows a clockwise book frame on the panel.
pub fn clockwise_book(screen: Screen, shown: crate::preferences::Orientation) -> bool {
    matches!(screen, Screen::EpubReading | Screen::Reading)
        && shown == crate::preferences::Orientation::LandscapeCw
}
/// A press as the reader sees it. Clockwise landscape holds the X3 half a turn
/// from counterclockwise, so on a book screen the four front buttons mirror
/// (Back <-> Next, Activate <-> Previous — keeping Page forward at the top, as
/// in counterclockwise) while the two side page buttons keep their half-turn
/// swap (Previous <-> Next). Book screens follow the frame on the panel
/// (`shown`). Every other screen is portrait, even while a book's frame is
/// still shown: presses queued after leaving a book reach Home before its
/// first repaint.
pub fn facing(action: Action, front: bool, screen: Screen, shown: crate::preferences::Orientation) -> Action {
    if !clockwise_book(screen, shown) { return action; }
    match (action, front) {
        (Action::Back, true) => Action::Next,
        (Action::Activate, true) => Action::Previous,
        (Action::Previous, true) => Action::Activate,
        (Action::Next, true) => Action::Back,
        (Action::Previous, false) => Action::Next,
        (Action::Next, false) => Action::Previous,
        _ => action,
    }
}
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Screen { #[default] Home, Reading, Device, Sleeping, EpubReading, TinyApp, Library, QuickStart }
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevicePage { Settings, Diagnostics, About, FirstNewt, ReadingStats, FirmwareSwitch, WhatsNew, SleepScreen, Developer, HolidayOutfits }
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AboutView { #[default] Closed, Version, WhatsNewRow, WhatsNew, Updated }
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsRow { #[default] TimeZone, TimeFormat, SleepScreen, Diagnostics, ReadingStats, FirmwareSwitch, Update, Wifi, About, QuickStart }
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsNotice { #[default] None, Saved, SaveFailed, FormatSaved, FormatSaveFailed, UpdateNotStarted, WifiNotStarted, SleepProgressSaveFailed, SleepWeekSaveFailed, SleepHabitSaveFailed, QuickStartSaveFailed, TestBuildsSaveFailed, HolidayHalloweenSaveFailed, HolidayThanksgivingSaveFailed, HolidayChristmasSaveFailed }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AppState {
    pub screen: Screen,
    pub resume_screen: Screen,
    pub selected: u32,
    pub page: u32,
    pub about_presses: u8,
    pub about: AboutView,
    pub first_newt: bool,
    /// Presses of DEVELOPER_UNLOCK matched so far on the first newt. Never
    /// saved: the hidden Developer page needs the sequence on every visit.
    pub unlock_presses: u8,
    pub developer: bool,
    pub holiday_outfits: bool,
    pub row_open: bool,
    pub settings_row: SettingsRow,
    pub zone_list_open: bool,
    pub list_focus: u8,
    pub settings_notice: SettingsNotice,
}

pub fn home_section(selected: u32) -> HomeSection {
    match selected { 0 => HomeSection::Books, 1 => HomeSection::Apps, _ => HomeSection::Settings }
}
pub fn home_section_title(section: HomeSection) -> &'static str {
    match section { HomeSection::Books => "Books", HomeSection::Apps => "Apps", HomeSection::Settings => "Settings" }
}
pub fn home_section_route(section: HomeSection) -> Screen {
    match section { HomeSection::Books => Screen::Library, HomeSection::Apps => Screen::TinyApp, HomeSection::Settings => Screen::Device }
}
/// On the first newt, these presses in order open the hidden Developer page.
pub const DEVELOPER_UNLOCK: [Action; 4] = [Action::Previous, Action::Next, Action::Previous, Action::Next];
pub fn sleep_destination(screen: Screen) -> Screen {
    if screen == Screen::EpubReading { screen } else { Screen::Home }
}
fn close_device(state: &mut AppState) {
    state.first_newt = false;
    state.unlock_presses = 0;
    state.developer = false;
    state.holiday_outfits = false;
    state.row_open = false;
    state.zone_list_open = false;
    state.about = AboutView::Closed;
    state.about_presses = 0;
    state.list_focus = 0;
    state.settings_row = SettingsRow::TimeZone;
    state.settings_notice = SettingsNotice::None;
}
pub fn device_page(state: &AppState) -> DevicePage {
    if state.first_newt && state.developer { DevicePage::Developer }
    else if state.first_newt { DevicePage::FirstNewt }
    else if matches!(state.about, AboutView::WhatsNew | AboutView::Updated) { DevicePage::WhatsNew }
    else if state.about != AboutView::Closed { DevicePage::About }
    else if !state.row_open { DevicePage::Settings }
    else { match state.settings_row {
        SettingsRow::ReadingStats => DevicePage::ReadingStats,
        SettingsRow::FirmwareSwitch => DevicePage::FirmwareSwitch,
        SettingsRow::SleepScreen => if state.holiday_outfits { DevicePage::HolidayOutfits } else { DevicePage::SleepScreen },
        _ => DevicePage::Diagnostics,
    }}
}
fn settings(state: &mut AppState, action: Action) -> bool {
    const ROWS: [SettingsRow; 10] = [SettingsRow::TimeZone, SettingsRow::TimeFormat,
        SettingsRow::SleepScreen, SettingsRow::Diagnostics, SettingsRow::ReadingStats,
        SettingsRow::FirmwareSwitch, SettingsRow::Update, SettingsRow::Wifi, SettingsRow::About, SettingsRow::QuickStart];
    let position = state.settings_row as usize;
    match action {
        Action::Previous if position > 0 => state.settings_row = ROWS[position - 1],
        Action::Next if position + 1 < ROWS.len() => state.settings_row = ROWS[position + 1],
        Action::Activate if matches!(state.settings_row, SettingsRow::SleepScreen | SettingsRow::Diagnostics | SettingsRow::ReadingStats) => {
            state.row_open = true;
            state.list_focus = 0;
        }
        Action::Activate if state.settings_row == SettingsRow::About => {
            state.about = AboutView::Version;
            state.about_presses = 0;
        }
        _ => return false,
    }
    if matches!(state.settings_notice, SettingsNotice::Saved | SettingsNotice::FormatSaved) {
        state.settings_notice = SettingsNotice::None;
    }
    true
}
fn about(state: &mut AppState, action: Action) -> bool {
    let version = state.about == AboutView::Version;
    if action == Action::Activate && version {
        state.about_presses = state.about_presses.wrapping_add(1);
        if state.about_presses < 5 { return false; }
        state.first_newt = true;
        return true;
    }
    state.about_presses = 0;
    match action {
        Action::Activate => state.about = AboutView::WhatsNew,
        Action::Next if version => state.about = AboutView::WhatsNewRow,
        Action::Previous if !version => state.about = AboutView::Version,
        _ => return false,
    }
    true
}
/// A wrong press restarts the sequence, counting itself if it is the first
/// step. Only the last press changes the screen.
fn first_newt(state: &mut AppState, action: Action) -> bool {
    if DEVELOPER_UNLOCK.get(usize::from(state.unlock_presses)) != Some(&action) {
        state.unlock_presses = u8::from(action == DEVELOPER_UNLOCK[0]);
        return false;
    }
    state.unlock_presses += 1;
    if usize::from(state.unlock_presses) < DEVELOPER_UNLOCK.len() { return false; }
    state.unlock_presses = 0;
    state.developer = true;
    // A new visit never shows an earlier visit's unconfirmed save.
    state.settings_notice = SettingsNotice::None;
    true
}
fn sleep_screen(state: &mut AppState, action: Action) -> bool {
    match action {
        Action::Previous if state.list_focus > 0 => state.list_focus -= 1,
        Action::Next if u32::from(state.list_focus) + 1 < 4 => state.list_focus += 1,
        Action::Activate if state.list_focus == 3 => {
            state.holiday_outfits = true;
            state.list_focus = 0;
            state.settings_notice = SettingsNotice::None;
        }
        _ => return false,
    }
    true
}
fn holiday_outfits(state: &mut AppState, action: Action) -> bool {
    match action {
        Action::Previous if state.list_focus > 0 => state.list_focus -= 1,
        Action::Next if u32::from(state.list_focus) + 1 < 3 => state.list_focus += 1,
        _ => return false,
    }
    true
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatsGraphView { Summary, Days, Periods }
pub struct StatsGraphPosition { pub view: StatsGraphView, pub back: u8 }
pub fn stats_graph_position(packed: u8) -> StatsGraphPosition {
    let view = match packed & 3 {
        1 => StatsGraphView::Days,
        2 => StatsGraphView::Periods,
        _ => StatsGraphView::Summary,
    };
    StatsGraphPosition { view, back: packed >> 2 }
}
pub fn step_stats_graph(packed: &mut u8, action: Action) -> bool {
    let position = stats_graph_position(*packed);
    let (mut view, mut back) = (position.view as u8, position.back);
    match action {
        Action::Previous if view > 0 => { view -= 1; back = 0; }
        Action::Next if view < 2 => { view += 1; back = 0; }
        Action::Activate if position.view == StatsGraphView::Days => back = (back + 1) % 64,
        _ => return false,
    }
    *packed = view | (back << 2);
    true
}

pub fn dispatch(state: &mut AppState, action: Action) -> bool {
    if state.screen == Screen::Sleeping { return false; }
    if action == Action::Sleep {
        state.resume_screen = sleep_destination(state.screen);
        if state.resume_screen == Screen::Home { state.selected = 0; }
        close_device(state);
        state.screen = Screen::Sleeping;
        return true;
    }
    // Explicit #272 main 9c627fe5 overlay: its owner handles every press but Sleep.
    if state.screen == Screen::QuickStart { return false; }
    if state.screen == Screen::Device {
        let page = device_page(state);
        if action == Action::Back && state.about == AboutView::Updated {
            state.screen = Screen::Home;
            close_device(state);
            return true;
        }
        if action == Action::Back && page != DevicePage::Settings {
            match page {
                DevicePage::FirstNewt => state.first_newt = false,
                DevicePage::Developer => state.developer = false,
                DevicePage::HolidayOutfits => {
                    state.holiday_outfits = false;
                    state.list_focus = 3;
                    state.settings_notice = SettingsNotice::None;
                }
                DevicePage::WhatsNew => state.about = AboutView::WhatsNewRow,
                DevicePage::About => state.about = AboutView::Closed,
                _ => state.row_open = false,
            }
            state.about_presses = 0;
            state.unlock_presses = 0;
            return true;
        }
        if page == DevicePage::Settings && state.zone_list_open { return false; }
        if page == DevicePage::Settings && action != Action::Back { return settings(state, action); }
        if page == DevicePage::About { return about(state, action); }
        if page == DevicePage::FirstNewt { return first_newt(state, action); }
        if page == DevicePage::SleepScreen { return sleep_screen(state, action); }
        if page == DevicePage::HolidayOutfits { return holiday_outfits(state, action); }
        if page == DevicePage::ReadingStats { return step_stats_graph(&mut state.list_focus, action); }
        if action == Action::Activate && page == DevicePage::Diagnostics {
            state.about = AboutView::Version;
            state.about_presses = 0;
            return true;
        }
        if action == Action::Activate { return false; }
    }
    if action == Action::Back && state.screen != Screen::Home {
        state.screen = Screen::Home;
        close_device(state);
        return true;
    }
    if state.screen == Screen::Home {
        match action {
            Action::Previous if state.selected > 0 => state.selected -= 1,
            Action::Next if state.selected < 2 => state.selected += 1,
            Action::Activate => state.screen = home_section_route(home_section(state.selected)),
            Action::Read => state.screen = Screen::EpubReading,
            _ => return false,
        }
        return true;
    }
    if state.screen == Screen::Reading {
        match action {
            Action::Previous if state.page > 0 => state.page -= 1,
            Action::Next if state.page < 2 => state.page += 1,
            _ => return false,
        }
        return true;
    }
    false
}
pub fn wake(state: &mut AppState) {
    if state.screen == Screen::Sleeping { state.screen = sleep_destination(state.resume_screen); }
}

/// Sealed #272 src/app.cpp::leaveQuickStart. The Quick Start owner clears its
/// separately borrowed State before returning this route; Settings keeps its
/// place, whereas Home closes Device and focuses Books.
pub fn leave_quick_start(state: &mut AppState, to_settings: bool) {
    if to_settings {
        state.screen = Screen::Device;
        state.settings_row = SettingsRow::QuickStart;
        return;
    }
    state.screen = Screen::Home;
    state.selected = 0;
    close_device(state);
}

#[cfg(test)]
mod facing_tests {
    use super::{Action, Screen, facing};
    use crate::preferences::Orientation;

    const ALL: [Action; 6] = [Action::Previous, Action::Next, Action::Activate, Action::Back, Action::Sleep, Action::Read];
    const SCREENS: [Screen; 8] = [Screen::Home, Screen::Reading, Screen::Device, Screen::Sleeping,
        Screen::EpubReading, Screen::TinyApp, Screen::Library, Screen::QuickStart];

    #[test]
    fn only_clockwise_book_screens_mirror_front_and_swap_side_page_buttons() {
        for screen in SCREENS {
            for shown in [Orientation::Portrait, Orientation::LandscapeCcw] {
                for front in [true, false] {
                    for action in ALL { assert_eq!(facing(action, front, screen, shown), action); }
                }
            }
            let front_turned = ALL.map(|action| facing(action, true, screen, Orientation::LandscapeCw));
            let side_turned = ALL.map(|action| facing(action, false, screen, Orientation::LandscapeCw));
            let book = matches!(screen, Screen::EpubReading | Screen::Reading);
            // Front buttons 1-4 (Back, Activate, Previous, Next) mirror to Next, Previous, Activate, Back.
            assert_eq!(front_turned == [Action::Activate, Action::Back, Action::Previous, Action::Next, Action::Sleep, Action::Read],
                book, "{screen:?}");
            // Side buttons keep their Previous <-> Next swap and never become Activate or Back;
            // CancelBack (Action::Back with front=false) stays Action::Back.
            assert_eq!(side_turned == [Action::Next, Action::Previous, Action::Activate, Action::Back, Action::Sleep, Action::Read],
                book, "{screen:?}");
            assert_eq!(front_turned == ALL && side_turned == ALL, !book, "{screen:?}");
        }
    }

    #[test]
    fn holiday_outfits_submenu_follows_sleep_screen_row_and_keeps_state_size() {
        use super::{AppState, DevicePage, SettingsNotice, SettingsRow, device_page, dispatch};
        assert_eq!(size_of::<AppState>(), 24);
        let mut state = AppState {
            screen: Screen::Device,
            row_open: true,
            settings_row: SettingsRow::SleepScreen,
            list_focus: 2,
            ..AppState::default()
        };
        assert!(dispatch(&mut state, Action::Next));
        assert_eq!((device_page(&state), state.list_focus), (DevicePage::SleepScreen, 3));
        assert!(!dispatch(&mut state, Action::Next));
        assert!(dispatch(&mut state, Action::Activate));
        assert_eq!((device_page(&state), state.list_focus), (DevicePage::HolidayOutfits, 0));
        assert!(dispatch(&mut state, Action::Next));
        assert!(dispatch(&mut state, Action::Next));
        assert_eq!(state.list_focus, 2);
        assert!(!dispatch(&mut state, Action::Next));
        state.settings_notice = SettingsNotice::HolidayChristmasSaveFailed;
        assert!(dispatch(&mut state, Action::Back));
        assert_eq!(
            (device_page(&state), state.list_focus, state.settings_notice),
            (DevicePage::SleepScreen, 3, SettingsNotice::None)
        );
        assert!(dispatch(&mut state, Action::Activate));
        assert!(dispatch(&mut state, Action::Sleep));
        assert!(!state.holiday_outfits);
    }
}
