// SPDX-License-Identifier: MIT
//! The actual image's boot-owned application components and synchronous UI loop.
//! Native acquisition stays in lib.rs; no frame, reader or durable owner copy.
#![forbid(unsafe_code)]
pub use crate::app_runtime::run_loop::run;
use crate::{
    app_runtime::{AppRuntime, Press, display::{self, DisplayReceipt}, home_cover::HomeCover, home_refresh::HomeRefresh, package},
    dev_usb::{Bridge, Diagnostics, PanelRing},
    home_startup::{FirmwareCard, HomeRuntime},
    nvs::NvsBlobBackend,
    rest_app::{Dispatch, RestApp, Work},
    retained_runtime::record::BootSnapshot,
    sdk::Sdk552,
    serial_console::SerialConsole,
};
use core::fmt::Write;
use newt_c3_board::{Board, Error, Probe, hardware::{HardwareError, HardwareReader}};
use newt_core::{
    app::{self, Action, AppState, Screen},
    frame::Frame,
    preferences::ViewSettings,
    quick_start,
    refresh::UiRefresh,
    save_notice::SaveOutcome,
    sd_resume::SdResumeRecord,
    update_confirm::{self, FirstScreen, Image, ImageState},
};
use newt_device_ui::{DeviceInfo, BuildIdentity};
use newt_input_policy::{UiControl, native::UiQueue};
use newt_library_current::Catalog;
use newt_library_current_ui::LibraryUiState;
#[path = "app_input.rs"]
mod input_events;
pub use input_events::TaskAccess;
use newt_sdk_heap::Fixed;
use newt_stage_home::{HeaderInfo, HomeData, HomeStyle};
use newt_tiny_app::{Definition, State as TinyState, pause::PocketPause};
use crate::app_runtime::platform::DevicePlatform;
use crate::reader_action::{ActionStage, Trace};

pub type Home<'sdk> = HomeRuntime<'sdk, SerialConsole, Sdk552, FirmwareCard>;

/// A short view of the real input task/queue, never a second input owner.
pub struct Events<'a, 'task> {
    pub action: crate::reader_action::Trace<'a>,
    pub queue: &'a mut UiQueue<'task>,
    pub control: &'a mut UiControl<'task>,
    pub task: TaskAccess<'a, 'task>,
}

#[path = "app_runtime/reader_facts.rs"]
mod facts;
pub use facts::ReaderFacts;
/// The title/identity remain borrowed from the selection owner for one paint.
pub struct HomeView<'a> {
    pub data: HomeData<'a>,
    pub identity: Option<&'a SdResumeRecord>,
    pub cover_eligible: bool,
    pub cover_generation: u32,
}

/// Required physical/session operations supplied by the persistent W3 owner.
/// No operation has a fallback implementation or manufactured readiness fact.
pub trait Reader {
    fn boot(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>, boot: &BootSnapshot,
        logical: &mut Frame, trace: crate::reader_action::Trace<'_>) -> bool;
    fn open_boot(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>, logical: &mut Frame,
        events: &mut Events<'_, '_>);
    fn work(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>, work: Work,
        logical: &mut Frame, events: &mut Events<'_, '_>) -> bool;
    fn paint(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>, logical: &mut Frame,
        events: &mut Events<'_, '_>, force: bool) -> Result<bool, Error>;
    fn visit_home(&mut self, paint: &mut dyn FnMut(HomeView<'_>));
    fn facts(&self) -> ReaderFacts;
    fn diagnostics(&self, out: &mut Diagnostics);
    fn idle(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>, logical: &mut Frame,
        events: &mut Events<'_, '_>) -> (bool, u32);
    fn save_exit(&mut self, home: &mut Home<'_>, now: u32, trace: crate::reader_action::Trace<'_>) -> SaveOutcome;
    fn close_source(&mut self, home: &mut Home<'_>) -> Result<(), Error>;
    fn preferences_result(&mut self, saved: bool, settings: ViewSettings);
    fn same_books(&mut self, io: &newt_native_io::UiIo, kept: &[u8], copy: &[u8]) -> bool;
    fn purge_cache(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>) -> bool;
    fn settle_press(&mut self, cleanup: bool);
    fn finish_sleep(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>, logical: &mut Frame,
        events: &mut Events<'_, '_>, now: u32) -> bool;
    fn prepare_sleep(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>, logical: &mut Frame,
        events: &mut Events<'_, '_>, now: u32) -> crate::retained_runtime::record::SleepRecord;
    fn cancel_sleep(&mut self, home: &mut Home<'_>, now: u32);
    fn note_input(&mut self, now: u32);
    fn note_paint(&mut self, screen: Screen, generation: u32, includes_cover: bool, now: u32);
    #[cfg(feature = "dev-usb")]
    fn consumed_input(&mut self, physical: bool, button: u8, action: Action, now: u32,
        before: Screen, after: Screen, reader_was_open: bool);
    #[cfg(feature = "dev-usb")]
    fn completed_paint(&mut self, screen: Screen, generation: u32);
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum PressState { #[default] Idle, Accepted, Deferred, AnsweredFast, AnsweredCleanup }

pub struct Application<'sdk> {
    pub app: AppState,
    pub library: LibraryUiState<'static>,
    pub catalog: Option<Fixed<Catalog>>,
    pub tiny: TinyState,
    pub pause: PocketPause,
    pub quick_start: quick_start::State,
    pub definition: Option<&'static mut Definition>,
    pub app_unavailable: Option<&'static [u8]>,
    pub info: DeviceInfo<'static>,
    pub displayed: DisplayReceipt,
    pub ui_refresh: UiRefresh,
    pub home_refresh: &'static mut HomeRefresh,
    pub home_cover: HomeCover,
    pub hardware: HardwareReader<'sdk>,
    pub last_battery_read: u32,
    pub first_screen: FirstScreen,
    pub press: PressState,
    pub firmware_switch: crate::firmware_switch::State,
    #[cfg(feature = "dev-usb")]
    pub bridge: &'static mut Bridge,
    #[cfg(feature = "dev-usb")]
    pub panels: &'static mut PanelRing,
    #[cfg(feature = "dev-usb")]
    pub runtime_lows: crate::runtime_lows::RuntimeLows,
    #[cfg(feature = "dev-usb")]
    pub device_fingerprint: u64,
}

impl<'sdk> Application<'sdk> {
    /// Package and Pause precede reader preference/retained restoration, as
    /// frozen setup. Battery/Wire initialization waits until input has begun.
    pub fn new(board: &mut Board<'sdk>, nvs: &mut Sdk552, probe: &Probe,
        definition: &'static mut core::mem::MaybeUninit<Definition>,
        home_refresh: &'static mut core::mem::MaybeUninit<HomeRefresh>,
        #[cfg(feature = "dev-usb")] bridge: &'static mut core::mem::MaybeUninit<Bridge>,
        #[cfg(feature = "dev-usb")] panels: &'static mut core::mem::MaybeUninit<PanelRing>,
    ) -> Result<Self, HardwareError> {
        let mut hardware = board.hardware_reader();
        #[cfg(feature = "dev-usb")]
        let boot = hardware.random_word()?;
        #[cfg(feature = "dev-usb")]
        let device_fingerprint = crate::app_dev::fingerprint(&mut hardware);
        let definition = crate::reading_storage::initialize(definition, Definition::default);
        let loaded = package::load_boot(&mut NvsBlobBackend::new(nvs), definition);
        let mut pause = PocketPause::default();
        pause.load(&mut NvsBlobBackend::new(nvs));
        let mut info = DeviceInfo::default();
        info.firmware_switch = if crate::firmware_switch::found(board, &mut info.other_firmware) {
            newt_device_ui::FirmwareSwitch::Available
        } else { newt_device_ui::FirmwareSwitch::Missing };
        info.panel = Some(if probe.verdict == 2 { b"X3 / panel probe inconclusive".as_slice() }
            else if probe.promoted != 0 { b"X3 / UC8279 detected" } else { b"X3 / UC8253 assumed" });
        Ok(Self {
            app: AppState::default(), library: LibraryUiState::default(), catalog: None,
            tiny: TinyState::default(), pause, quick_start: quick_start::State::default(),
            definition: loaded.is_ok().then_some(definition), app_unavailable: loaded.err(), info,
            displayed: DisplayReceipt::default(), ui_refresh: UiRefresh::default(),
            home_refresh: crate::reading_storage::initialize(home_refresh, HomeRefresh::default),
            home_cover: HomeCover::default(), hardware,
            last_battery_read: 0, first_screen: FirstScreen::Owed, press: PressState::Idle,
            firmware_switch: crate::firmware_switch::State::new(),
            #[cfg(feature = "dev-usb")]
            bridge: crate::reading_storage::initialize(bridge, || Bridge::new(boot)),
            #[cfg(feature = "dev-usb")]
            panels: crate::reading_storage::initialize(panels, PanelRing::default),
            #[cfg(feature = "dev-usb")]
            runtime_lows: crate::runtime_lows::RuntimeLows::default(),
            #[cfg(feature = "dev-usb")]
            device_fingerprint,
        })
    }

    pub fn runtime(&mut self) -> AppRuntime<'_, 'static> {
        AppRuntime { rest: RestApp { app: &mut self.app, library: &mut self.library,
            tiny: &mut self.tiny, pause: &mut self.pause, definition: self.definition.as_deref() },
            quick_start: &mut self.quick_start }
    }

    pub fn press_answered(&self) -> bool {
        matches!(self.press, PressState::AnsweredFast | PressState::AnsweredCleanup)
    }
    pub fn settle_press(&mut self, reader: &mut impl Reader) {
        if self.press_answered() && self.app.screen != Screen::Sleeping {
            let cleanup = self.press == PressState::AnsweredCleanup;
            if self.app.screen == Screen::EpubReading { reader.settle_press(cleanup); }
            else { self.ui_refresh.painted(cleanup); }
        }
        self.press = PressState::Idle;
    }
    pub fn begin_press(&mut self, reader: &mut impl Reader) {
        self.settle_press(reader);
        self.press = PressState::Accepted;
    }

    /// One real BatteryMonitor::readStatus followed by the raw free-heap read.
    pub fn read_info(&mut self, home: &mut Home<'_>, trace: Trace<'_>) -> Result<(), HardwareError> {
        self.read_info_with_clock(&home.bus.board.monotonic_clock(), trace)
    }

    /// Producer pauses can refresh the same facts while Home's state is lent.
    pub fn read_info_with_clock(&mut self, clock: &newt_c3_board::MonotonicClock, trace: Trace<'_>) -> Result<(), HardwareError> {
        let _action = trace.scope(ActionStage::DeviceInfo);
        self.last_battery_read = crate::app_runtime::info::read_device_info(&mut self.info, &mut self.hardware, clock)?;
        Ok(())
    }

    /// One press answers once. Refresh credit remains owed until settle_press,
    /// after the real open has reset its destination's existing budget.
    pub fn acknowledge_reader_work(&mut self, home: &mut Home<'_>, work: newt_reader_ui::SlowWork,
        settings: ViewSettings, logical: &mut Frame, trace: Trace<'_>,
    ) -> Result<bool, Error> {
        if !matches!(self.press, PressState::Accepted | PressState::Deferred) { return Ok(false); }
        let started = display::refresh_start(&mut home.bus.board);
        self.home_refresh.invalidate();
        self.home_cover.release();
        let bar = if matches!(work, newt_reader_ui::SlowWork::FirstOpen) {
            home.clock.read_system_bar(&mut home.bus, &self.info, false)
        } else {
            newt_system_ui::SystemBar::new(None, home.clock.format(), self.info.battery_known, self.info.battery_percent)
        };
        #[cfg(feature = "dev-usb")]
        let panels = Some(&mut *self.panels);
        #[cfg(not(feature = "dev-usb"))]
        let panels = None;
        let cleanup = display::working(&mut home.bus.board, &mut self.displayed,
            work, &bar, settings, logical, panels, started, trace)?;
        self.press = if cleanup { PressState::AnsweredCleanup } else { PressState::AnsweredFast };
        Ok(true)
    }

    pub fn dispatch(
        &mut self, home: &mut Home<'sdk>, reader: &mut impl Reader, logical: &mut Frame, action: Action,
        events: &mut Events<'_, '_>,
    ) -> Result<Dispatch, HardwareError> {
        let count = self.catalog.as_ref().map_or(0, |catalog| catalog.size());
        let random = if self.app.screen == Screen::TinyApp
            || self.app.screen == Screen::Home && self.app.selected == 1 && action == Action::Activate {
            self.hardware.random_word()?
        } else { 0 }; // Not consumed outside Tiny/Pause; no entropy is promised.
        let was_home = self.app.screen == Screen::Home;
        let settings_quick = self.app.settings_row == app::SettingsRow::QuickStart;
        let quick_press = self.runtime().rest.quick_start_press(action, settings_quick);
        let wifi_saved = quick_press
            && matches!(self.quick_start.step, quick_start::Step::Wifi | quick_start::Step::Done)
            && home.wifi_network_saved();
        let press = Press { action, catalog_count: count, random, wifi_saved };
        #[cfg(feature = "dev-usb")]
        let facts = crate::app_runtime::publication::Facts::current(self);
        let Self { app, library, tiny, pause, definition, quick_start, info,
            displayed, ui_refresh, firmware_switch, press: press_state, hardware,
            #[cfg(feature = "dev-usb")] bridge,
            #[cfg(feature = "dev-usb")] panels,
            .. } = self;
        let mut runtime = AppRuntime { rest: RestApp { app, library, tiny, pause,
            definition: definition.as_deref() }, quick_start };
        let quick = runtime.dispatch_quick_with(home, press, &mut |home, effect| match effect {
            crate::quick_start_ports::Effect::Preferences { saved, settings } => {
                reader.preferences_result(saved, settings);
                saved
            }
            crate::quick_start_ports::Effect::EnterWifi => {
                let mut ports = crate::app_runtime::restart::Exit { home, reader, logical,
                    events, displayed,
                    #[cfg(feature = "dev-usb")] hardware,
                    #[cfg(feature = "dev-usb")] bridge,
                    #[cfg(feature = "dev-usb")] panels,
                    #[cfg(feature = "dev-usb")] facts };
                match crate::update_entry::enter_update_mode(&mut ports,
                    newt_core::update_mode_request::UpdateModeRequest::QuickStartWifi, "Wi-Fi setup") {
                    Ok(entered) => entered, Err(_) => crate::app_power::stop(),
                }
            }
        });
        let result = if let Some(quick) = quick { quick } else {
            #[cfg(feature = "dev-usb")]
            let panels = Some(&mut **panels);
            #[cfg(not(feature = "dev-usb"))]
            let panels = None;
            let mut platform = DevicePlatform { reader, info, displayed, budget: ui_refresh,
                switch: firmware_switch, logical, panels, press: press_state, events, hardware,
                #[cfg(feature = "dev-usb")] bridge,
                #[cfg(feature = "dev-usb")] facts };
            runtime.dispatch_after_quick(home, press, &mut platform)
        };
        if was_home && self.app.screen != Screen::Home { self.home_cover.release(); }
        Ok(result)
    }

    pub fn paint_sample(&mut self, home: &mut Home<'_>, logical: &mut Frame, force: bool, trace: Trace<'_>) -> Result<bool, Error> {
        if self.app.screen != Screen::Reading { return Ok(false); }
        let started = display::refresh_start(&mut home.bus.board);
        self.home_refresh.invalidate();
        self.home_cover.release();
        let composition = display::Composition::new(trace);
        if !crate::sample_scene::paint(&self.app, Some(logical)) { return Ok(false); }
        #[cfg(feature = "dev-usb")]
        let panels = Some(&mut *self.panels);
        #[cfg(not(feature = "dev-usb"))]
        let panels = None;
        self.displayed.binary(&mut home.bus.board, logical, &mut self.ui_refresh, panels, force, started, composition)?;
        #[cfg(feature = "dev-usb")]
        self.runtime_lows.painted(self.app.screen);
        Ok(true)
    }

    pub fn paint_system(&mut self, home: &mut Home<'_>, logical: &mut Frame,
        build: BuildIdentity<'_>, force: bool, trace: Trace<'_>,
    ) -> Result<bool, Error> {
        let started = display::refresh_start(&mut home.bus.board);
        let Self { app, library, tiny, pause, definition, quick_start, info, displayed,
            ui_refresh, home_refresh, home_cover, app_unavailable, .. } = self;
        let runtime = AppRuntime { rest: RestApp { app, library, tiny, pause, definition: definition.as_deref() }, quick_start };
        let composition = display::Composition::new(trace);
        let Some(prepared) = runtime.prepare_actual_system(home, info, false) else { return Ok(false); };
        home_refresh.invalidate();
        home_cover.release();
        let stats = runtime.rest.app.screen == Screen::Device
            && app::device_page(runtime.rest.app) == app::DevicePage::ReadingStats;
        let painted = if stats {
            runtime.paint_system_ready(prepared, info, Some(logical), None,
                Some(&mut |shown, count| home.read_stats_screen(shown, count)), build, *app_unavailable)
        } else {
            home.bus.board.with_build_storage(|scratch| {
                let scratch: &mut Frame = scratch.try_into().map_err(|_| Error::Length)?;
                Ok::<_, Error>(runtime.paint_system_ready(prepared, info, Some(logical), Some(scratch), None, build, *app_unavailable))
            })??
        };
        if painted {
            #[cfg(feature = "dev-usb")]
            let panels = Some(&mut *self.panels);
            #[cfg(not(feature = "dev-usb"))]
            let panels = None;
            displayed.binary(&mut home.bus.board, logical, ui_refresh, panels, force, started, composition)?;
            #[cfg(feature = "dev-usb")]
            self.runtime_lows.painted(self.app.screen);
        }
        Ok(painted)
    }

    /// Some reports a completed display and whether its cover region was painted.
    pub fn paint_home(&mut self, home: &mut Home<'_>, mut view: HomeView<'_>, logical: &mut Frame,
        force: bool, status_only: bool, trace: Trace<'_>,
    ) -> Result<Option<bool>, Error> {
        let _action = trace.scope(ActionStage::HOME_PAINT);
        let book = view.identity.map_or(0, SdResumeRecord::book_id);
        self.home_cover.observe(view.cover_eligible, book, view.cover_generation, &mut self.home_refresh);
        view.data.clock = None;
        view.data.set_clock_format(home.clock.format());
        let header = HeaderInfo { battery_known: self.info.battery_known, battery_percent: self.info.battery_percent };
        let Self { app, library, tiny, pause, definition, quick_start, home_cover, home_refresh,
            ui_refresh, displayed, .. } = self;
        let runtime = AppRuntime { rest: RestApp { app, library, tiny, pause, definition: definition.as_deref() }, quick_start };
        #[cfg(feature = "dev-usb")]
        let panels = Some(&mut *self.panels);
        #[cfg(not(feature = "dev-usb"))]
        let panels = None;
        let Some(painted) = display::home(&runtime, home, home_refresh, ui_refresh, displayed,
            &header, view.data, logical, &HomeStyle::default(), home_cover, view.identity, force, status_only, panels)? else {
            return Ok(None);
        };
        #[cfg(feature = "dev-usb")]
        self.runtime_lows.stage_painted(painted.grayscale, painted.full, painted.overlay);
        Ok(Some(painted.full))
    }

    /// HELLO stays unanswered until the existing first-finished-paint policy
    /// confirms the actual running OTA image. No completed frame means no call.
    pub fn confirm_first_paint(&mut self, home: &mut Home<'_>) {
        if self.displayed.valid && self.press == PressState::Idle && self.first_screen == FirstScreen::Owed {
            self.first_screen = if update_confirm::confirm_image(&mut RunningImage(home)) {
                FirstScreen::Confirmed
            } else { FirstScreen::Unconfirmed };
        }
    }
}

struct RunningImage<'a, 'sdk>(&'a mut Home<'sdk>);
impl Image for RunningImage<'_, '_> {
    fn state(&mut self) -> u8 {
        match self.0.bus.board.running_image_state() {
            Ok(1) => ImageState::Pending as u8,
            Ok(_) | Err(Error::Native(0x105)) => ImageState::Settled as u8,
            Err(_) => ImageState::Unreadable as u8,
        }
    }
    fn mark_valid(&mut self) -> bool {
        let marked = self.0.bus.board.mark_running_image_valid().is_ok();
        let _ = writeln!(self.0.bus.log, "Newt image {} after its first finished screen", if marked { "marked valid" } else { "not marked valid" });
        marked
    }
}
