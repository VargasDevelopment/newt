// SPDX-License-Identifier: MIT
//! Non-reader application dispatch tail.
//! Settings commits run first in the caller; Library selection and reader,
//! sleep and scan work return to their existing owners. No scene or storage
//! adapter is implemented here. All state and definition storage is borrowed.
#![forbid(unsafe_code)]
use newt_core::{app::{self, Action, AppState, Screen}, durable::Backend};
use newt_library_current_ui::{self as library, LibraryIntent, LibraryUiState};
use newt_tiny_app::{self as tiny, Definition, State, install::Choice, pause::PocketPause};
use crate::app_runtime::card_app::Prompt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Work {
    None,
    EnterLibrary,
    OpenSelection(usize),
    OpenSample,
    OpenReader,
    ReaderAction(Action),
    PrepareSleep,
    /// The app owner applies sealed leaveQuickStart's Home/closeDevice transition.
    QuickStartHome,
    /// The app owner selects the frozen SettingsRow::QuickStart.
    QuickStartSettings,
    /// The app owner sets QuickStartSaveFailed, never generic SaveFailed.
    QuickStartOpeningUnconfirmed,
    /// Apps opened: look once for an app file on the card.
    EnterApps,
    /// The reader chose Install on the card app offer.
    InstallApp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dispatch {
    pub changed: bool,
    pub work: Work,
}

/// A short view of boot-owned component state; no new retained owner/buffer.
/// `definition == None` follows the existing unavailable-app route. A card
/// app page, while one is shown, takes Apps' presses before the app does.
pub struct RestApp<'a, 'message> {
    pub app: &'a mut AppState,
    pub library: &'a mut LibraryUiState<'message>,
    pub tiny: &'a mut State,
    pub pause: &'a mut PocketPause,
    pub definition: Option<&'a Definition>,
    pub prompt: &'a mut Prompt,
}

impl RestApp<'_, '_> {
    /// Sealed quickStartPress. The application supplies whether its actual
    /// Settings selection is QuickStart; no alternate row table lives here.
    pub fn quick_start_press(&self, action: Action, settings_quick_start: bool) -> bool {
        if self.app.screen == Screen::QuickStart { return action != Action::Sleep; }
        settings_quick_start && action == Action::Activate
            && self.app.screen == Screen::Device
            && app::device_page(self.app) == app::DevicePage::Settings
    }

    /// Before the ordinary settings/non-reader tail (main.cpp:640). None
    /// means the existing dispatcher must handle this press, including Sleep.
    /// Typed exits let the application finish the frozen return route before
    /// the next paint, without holding Home's durable store loan.
    pub fn dispatch_quick_start(
        &mut self,
        shown: &mut newt_core::quick_start::State,
        facts: &newt_core::quick_start::Facts,
        action: Action,
        settings_quick_start: bool,
        ports: &mut dyn crate::quick_start_owner::Ports,
    ) -> Option<Dispatch> {
        use crate::quick_start_owner::{self, Route};
        if !self.quick_start_press(action, settings_quick_start) { return None; }
        let opening = self.app.screen != Screen::QuickStart;
        let route = quick_start_owner::press(shown, facts, action, opening, ports);
        let work = match route {
            Route::Enter => {
                self.app.screen = Screen::QuickStart;
                self.app.settings_notice = app::SettingsNotice::None;
                Work::None
            }
            Route::Home => Work::QuickStartHome,
            Route::Settings => Work::QuickStartSettings,
            Route::OpeningUnconfirmed => Work::QuickStartOpeningUnconfirmed,
            _ => Work::None,
        };
        Some(Dispatch { changed: route != Route::Unchanged, work })
    }

    /// #272 paintNonHomeScreen/paintQuickStart. One synchronous borrow of the
    /// existing frame and optional native scratch; refresh stays with Root.
    pub fn paint_quick_start(
        &self,
        shown: &newt_core::quick_start::State,
        facts: &newt_core::quick_start::Facts,
        bar: &newt_system_ui::SystemBar,
        frame: Option<&mut newt_ui_canvas::Frame>,
        scratch: Option<&mut newt_ui_canvas::Frame>,
    ) -> bool {
        if self.app.screen != Screen::QuickStart { return false; }
        let mut painted = *facts;
        painted.clock_set = bar.clock != *b"--:--\0";
        tiny::quick_start::render(shown, &painted, bar, frame, scratch);
        true
    }

    /// Library precedes Settings/storage dispatch in frozen main.cpp. Its
    /// selection is borrowed directly; no Home backend is opened for a press.
    pub fn dispatch_library(&mut self, action: Action, count: usize) -> Dispatch {
            let intent = library::dispatch(self.library, count, action);
            let work = match intent {
                LibraryIntent::Open => Work::OpenSelection(self.library.selected),
                LibraryIntent::OpenSample => Work::OpenSample,
                LibraryIntent::Home => {
                    app::dispatch(self.app, Action::Back);
                    Work::None
                }
                LibraryIntent::Sleep => {
                    app::dispatch(self.app, Action::Sleep);
                    Work::PrepareSleep
                }
                _ => Work::None,
            };
            Dispatch {
                changed: matches!(intent, LibraryIntent::Repaint | LibraryIntent::Home | LibraryIntent::Sleep),
                work,
            }
    }

    /// Boot only, before dispatch or restore of any retained Tiny App selection.
    /// Uses the existing durable owner; no RAM snapshot is persistence authority.
    pub fn start<B: Backend>(&mut self, backend: &mut B) {
        self.pause.load(backend);
    }

    /// Call after the existing settings handlers, passing their consumed/change
    /// result. Reader and Library routes bypass those handlers in main.cpp.
    /// `count` is the current catalog size, never a second catalog snapshot.
    pub fn dispatch<B: Backend>(
        &mut self,
        backend: &mut B,
        action: Action,
        count: usize,
        random: u32,
        settings_changed: bool,
    ) -> Dispatch {
        let before = self.app.screen;
        if before == Screen::EpubReading {
            return Dispatch { changed: false, work: Work::ReaderAction(action) };
        }
        if before == Screen::Library {
            return self.dispatch_library(action, count);
        }
        let mut changed = settings_changed;
        if before == Screen::TinyApp && let Some(choice) = self.prompt.choose(action) {
            changed = choice != Choice::Stay;
            match choice {
                Choice::Install => return Dispatch { changed: false, work: Work::InstallApp },
                Choice::Open => {
                    // The app may be new; it starts fresh, as on entry.
                    *self.tiny = State::default();
                    if self.definition.is_some_and(tiny::pause::is_pocket_pause) {
                        self.pause.open(self.tiny, random);
                    }
                }
                Choice::Home => self.app.screen = Screen::Home,
                Choice::Sleep => changed = app::dispatch(self.app, Action::Sleep),
                Choice::Stay | Choice::Leave => {}
            }
        } else if before == Screen::TinyApp {
            if let Some(definition) = self.definition {
                if tiny::pause::is_pocket_pause(definition) {
                    changed = self.pause.dispatch(backend, self.app, self.tiny, action, random);
                } else {
                    let previous = *self.tiny;
                    let intent = tiny::dispatch(definition, self.tiny, action);
                    changed = previous != *self.tiny;
                    changed |= tiny::apply_intent(self.app, intent);
                }
            } else if !changed {
                changed = app::dispatch(self.app, action);
            }
        } else if !changed {
            changed = app::dispatch(self.app, action);
        }
        let work = if before != Screen::Library && self.app.screen == Screen::Library {
            Work::EnterLibrary
        } else if before != Screen::EpubReading && self.app.screen == Screen::EpubReading {
            Work::OpenReader
        } else if before != Screen::Sleeping && self.app.screen == Screen::Sleeping {
            Work::PrepareSleep
        } else if before != Screen::TinyApp && self.app.screen == Screen::TinyApp {
            Work::EnterApps
        } else {
            Work::None
        };
        if before != Screen::TinyApp && self.app.screen == Screen::TinyApp
            && self.definition.is_some_and(tiny::pause::is_pocket_pause)
        {
            self.pause.open(self.tiny, random);
        }
        Dispatch { changed, work }
    }
}
