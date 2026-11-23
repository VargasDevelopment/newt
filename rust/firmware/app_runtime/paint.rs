// SPDX-License-Identifier: MIT
//! Existing screen painters, with only synchronous loans of their real inputs.
//! Root owns refresh, Library preparation and the reader's source-paint fence.
use super::AppRuntime;
use crate::{home_startup::{CardPort, HomeRuntime}, nvs};
use core::fmt::Write;
use newt_core::{app::{self, DevicePage, Screen, StatsGraphPosition},
    preferences::Orientation, quick_start::Facts, time_zone::TimeZone, update_online::TestBuilds};
use newt_device_ui::{BuildIdentity, DeviceInfo, SleepScreenChoices, stats::StatsScreenView};
use newt_library_current_ui::{self as library, Catalog, LibraryCoverSource};
use newt_stage_home::{HeaderInfo, HomeData, HomePlane, HomeStyle, PreparedHomeCover};
use newt_ui_canvas::Frame;
use newt_system_ui::SystemBar;

/// One paint's existing bar and published scalar choices, prepared before
/// lending Board pixels. No Home/Board borrow, frame, statistics or cache.
/// Use immediately without an intervening dispatch or preference publication.
pub struct PreparedSystemPaint {
    bar: SystemBar,
    zone: TimeZone,
    sleep: SleepScreenChoices,
    test_builds: TestBuilds,
    orientation: Orientation,
    wifi_saved: bool,
}

impl AppRuntime<'_, '_> {
    /// Home data/cover are Root's prepared views, including its clock snapshot;
    /// no sample default or synthetic reading-available fact is supplied here.
    pub fn paint_home(
        &self,
        info: &HeaderInfo,
        data: &HomeData<'_>,
        plane: HomePlane,
        frame: Option<&mut Frame>,
        style: &HomeStyle<'_>,
        cover: Option<&PreparedHomeCover<'_>>,
    ) -> bool {
        if self.rest.app.screen != Screen::Home { return false; }
        newt_stage_home::render_home(self.rest.app, info, data, plane, frame, style, cover);
        true
    }

    /// Root reads the bar before lending its actual catalog/files; no Board
    /// loan coexists with a Home clock read. Preserve the existing focus-only/full-paint result for refresh.
    pub fn paint_library(
        &mut self,
        bar: &SystemBar,
        catalog: Option<&mut Catalog>,
        files: &mut dyn newt_library_current::list::Files,
        frame: Option<&mut Frame>,
        covers: Option<&mut dyn LibraryCoverSource>,
    ) -> Option<bool> {
        if self.rest.app.screen != Screen::Library { return None; }
        Some(library::paint(catalog, files, self.rest.library, frame, bar, covers))
    }

    /// Read the real bar once before any frame/scratch loan; getters only copy
    /// the choices already published by their existing Home owners.
    pub fn prepare_system<'sdk, L: Write, N: nvs::Native, P: CardPort>(
        &self,
        home: &mut HomeRuntime<'sdk, L, N, P>,
        info: &DeviceInfo<'_>,
        radio_on: bool,
        wifi_saved: bool,
    ) -> Option<PreparedSystemPaint> {
        if !matches!(self.rest.app.screen, Screen::Device | Screen::TinyApp | Screen::QuickStart) {
            return None;
        }
        let bar = home.clock.read_system_bar(&mut home.bus, info, radio_on);
        Some(PreparedSystemPaint {
            bar,
            zone: home.clock.zone(),
            sleep: home.clock.sleep_screen(),
            test_builds: home.clock.test_builds(),
            orientation: home.preferences.settings().orientation,
            wifi_saved,
        })
    }

    /// Actual firmware preparation preserves paintQuickStart's ordering:
    /// read the bar first, then its sealed network fact only on Wifi/Done.
    pub fn prepare_actual_system<L: Write, N: nvs::Native, P: CardPort>(
        &self, home: &mut HomeRuntime<'_, L, N, P>, info: &DeviceInfo<'_>, radio_on: bool,
    ) -> Option<PreparedSystemPaint> {
        let mut prepared = self.prepare_system(home, info, radio_on, false)?;
        if self.rest.app.screen == Screen::QuickStart
            && matches!(self.quick_start.step, newt_core::quick_start::Step::Wifi | newt_core::quick_start::Step::Done) {
            prepared.wifi_saved = home.wifi_network_saved();
        }
        Some(prepared)
    }

    /// Compatibility path for callers lending separate logical storage. The
    /// same renderer gets the actual Home statistics adapter, never a default.
    pub fn paint_system<'sdk, L: Write, N: nvs::Native, P: CardPort>(
        &self,
        home: &mut HomeRuntime<'sdk, L, N, P>,
        info: &DeviceInfo<'_>,
        radio_on: bool,
        wifi_saved: bool,
        frame: Option<&mut Frame>,
        scratch: Option<&mut Frame>,
        stats: &mut dyn FnMut(&mut HomeRuntime<'sdk, L, N, P>, StatsGraphPosition, u32) -> StatsScreenView,
        build: BuildIdentity<'_>,
        app_unavailable: Option<&[u8]>,
    ) -> bool {
        let Some(prepared) = self.prepare_system(home, info, radio_on, wifi_saved) else {
            return false;
        };
        self.paint_system_ready(prepared, info, frame, scratch,
            Some(&mut |position, days| stats(home, position, days)), build, app_unavailable)
    }

    /// Pure composition over prepared scalar inputs; no Home or native read.
    /// Only an actual ReadingStats page requires the borrowed real provider.
    /// Its absence is unhandled before any frame/scratch drawing. Root lends
    /// separate logical storage (no SDK scratch) when that callback uses Home.
    pub fn paint_system_ready(
        &self,
        prepared: PreparedSystemPaint,
        info: &DeviceInfo<'_>,
        frame: Option<&mut Frame>,
        scratch: Option<&mut Frame>,
        stats: Option<&mut dyn FnMut(StatsGraphPosition, u32) -> StatsScreenView>,
        build: BuildIdentity<'_>,
        app_unavailable: Option<&[u8]>,
    ) -> bool {
        let screen = self.rest.app.screen;
        if !matches!(screen, Screen::Device | Screen::TinyApp | Screen::QuickStart) {
            return false;
        }
        if screen == Screen::Device && app::device_page(self.rest.app) == DevicePage::ReadingStats
            && stats.is_none() {
            return false;
        }
        let PreparedSystemPaint { bar, zone, sleep, test_builds, orientation, wifi_saved } = prepared;
        match screen {
            Screen::Device => {
                newt_device_ui::render_device_ui(
                    self.rest.app, info, &bar, frame, zone, scratch,
                    stats, build, sleep, test_builds,
                );
            }
            Screen::TinyApp => match (self.rest.prompt.page(), self.rest.definition) {
                (Some(page), running) => newt_tiny_app::install::render(&page, running, &bar, frame),
                (None, Some(definition)) if newt_tiny_app::pause::is_pocket_pause(definition) => {
                    self.rest.pause.render(definition, &bar, frame);
                }
                (None, Some(definition)) => newt_tiny_app::render(definition, self.rest.tiny, &bar, frame),
                (None, None) => newt_tiny_app::render_unavailable(app_unavailable, &bar, frame),
            },
            Screen::QuickStart => {
                let facts = Facts {
                    orientation,
                    zone,
                    wifi_saved,
                    clock_set: bar.clock != *b"--:--\0",
                };
                self.rest.paint_quick_start(self.quick_start, &facts, &bar, frame, scratch);
            }
            _ => return false,
        }
        true
    }
}
