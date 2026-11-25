// SPDX-License-Identifier: MIT
//! Actual Library traversal and painting over the mounted UI I/O capability.
use super::AppRuntime;
use crate::{
    app_image::{Application, Home, Reader},
    library_covers::PreparedCovers,
    reader_image::cover_idle::{ShelfCoverState, shelf_view},
    rest_app::RestApp,
};
use newt_c3_board::{Error, MonotonicClock};
use newt_core::{app::Screen, frame::Frame};
use newt_input_policy::UiControl;
use newt_library_current::{Catalog, LibraryStatus, scan::{self, FirmwareProgress, PaceNative, Report}};
use newt_native_io::LibraryFiles;
use newt_sdk_heap::Fixed;
use crate::reader_action::Trace;

pub struct LibraryPaint {
    pub includes_cover: bool,
    pub view: u32,
    pub any_making: bool,
}

impl Application<'_> {
    /// browse has already ended the reader's physical use before entering.
    /// The catalog is its sole fallible owner, retained only until book open.
    pub fn scan_library(&mut self, home: &mut Home<'_>, reader: &mut impl Reader, logical: &mut Frame,
        control: &UiControl<'_>, trace: Trace<'_>,
    ) -> Result<(), Error> {
        self.library.message = None;
        self.library.no_card = false;
        newt_library_current_ui::invalidate_frame(&mut self.library);
        if home.bus.board.ensure_sd_mounted().is_err() {
            self.catalog = None;
            self.library.no_card = true;
            return Ok(());
        }
        let Some(mut catalog) = self.catalog.take().or_else(|| Fixed::try_new_with(Catalog::default)) else {
            self.library.message = Some(b"Library unavailable");
            return Ok(());
        };
        let clock = home.bus.board.monotonic_clock();
        #[cfg(feature = "dev-usb")]
        let started = clock.millis();
        let mut report = Report::default();
        let result = home.with_storage_io(|home, io| {
            io.with_library_folders(|folders| {
                let mut files = LibraryFiles::new(io);
                let mut native = ScanPace { clock, home, ui: self, reader, logical, control, trace, error: None };
                let mut pace = FirmwareProgress { native: &mut native, yielded: 0, loading_pending: true };
                let status = scan::scan(b"/newt-sd\0", &mut catalog, &mut report, folders, &mut files,
                    Some(&mut |kept, copy| crate::reader_image::same_books(io, kept, copy)), &mut pace, false);
                match native.error.take() { Some(error) => Err(error), None => Ok(status) }
            })
        });
        let result = match result {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(Error::Io),
            Err(error) => Err(error),
        };
        self.library.loading = false;
        if !matches!(result, Ok(LibraryStatus::Ok)) { catalog.reset(LibraryStatus::Unavailable); }
        self.library.message = catalog.notice().map(str::as_bytes);
        if catalog.size() == 0 { self.library.selected = 0; }
        else { self.library.selected = self.library.selected.min(catalog.size() - 1); }
        #[cfg(feature = "dev-usb")]
        {
            let omitted = catalog.omissions();
            let list = match report.list { scan::ListState::None => "none", scan::ListState::Kept => "kept",
                scan::ListState::Built => "built", scan::ListState::Ram => "ram" };
            let _ = super::trace::row(&mut home.bus.log, 159, format_args!(
                "LIBRARY result={} books={} outside={} same={} full={} names={} skipped={} stopped={} entries={} dirs={} cmp={} list={} walks={} runs={} ms={}\n",
                if catalog.status() == LibraryStatus::Ok { "ok" } else { "unavailable" }, catalog.size(),
                catalog.outside(), catalog.copies(), omitted.full, omitted.names, omitted.folders, u8::from(omitted.stopped),
                report.entries, report.folders, report.compared, list, report.walks, report.runs, clock.millis().wrapping_sub(started)));
        }
        self.catalog = Some(catalog);
        result.map(|_| ())
    }

    /// Some preserves the renderer's full-shelf versus focus-only result and shelf cover status.
    pub fn paint_library(
        &mut self,
        home: &mut Home<'_>,
        logical: &mut Frame,
        force: bool,
        composition: super::display::Composition<'_>,
        shelf_state: ShelfCoverState,
        control: &UiControl<'_>,
    ) -> Result<Option<LibraryPaint>, Error> {
        if self.app.screen != Screen::Library { return Ok(None); }
        let started = super::display::refresh_start(&mut home.bus.board);
        self.home_refresh.invalidate();
        self.home_cover.release();
        let bar = home.clock.read_system_bar(&mut home.bus, &self.info, false);
        let painted = if self.library.loading || self.library.no_card || self.catalog.is_none() {
            if !newt_library_current_ui::paint_without_files(&mut self.library, Some(logical), &bar) {
                return Ok(None);
            }
            LibraryPaint { includes_cover: true, view: 0, any_making: false }
        } else {
            let Self { app, library, tiny, pause, installed, prompt, quick_start, catalog, .. } = self;
            let mut runtime = AppRuntime { rest: RestApp { app, library, tiny, pause, definition: installed.running(), prompt }, quick_start };
            let Some(painted) = home.with_storage_io(|_, io| {
                let mut files = LibraryFiles::new(io);
                let view = catalog.as_deref_mut()
                    .filter(|c| c.status() == LibraryStatus::Ok && c.size() > 0)
                    .map_or(0, |c| {
                        let selected = runtime.rest.library.selected.min(c.size() - 1);
                        let first = newt_library_current_ui::shelf_first(selected);
                        c.with_shelf_names(&mut files, first, |names| shelf_view(names, false))
                    });
                let mut native = io.port();
                let mut covers = PreparedCovers::for_library(&mut native, shelf_state, view);
                let includes_cover = runtime.paint_library(&bar, catalog.as_deref_mut(), &mut files, Some(logical), Some(&mut covers))?;
                Some(LibraryPaint { includes_cover, view, any_making: covers.any_making() })
            })? else { return Ok(None); };
            painted
        };
        #[cfg(feature = "dev-usb")]
        let panels = Some(&mut *self.panels);
        #[cfg(not(feature = "dev-usb"))]
        let panels = None;
        self.displayed.binary(&mut home.bus.board, logical, &mut self.ui_refresh, panels, force, started, composition, control)?;
        #[cfg(feature = "dev-usb")]
        self.runtime_lows.painted(self.app.screen);
        Ok(Some(painted))
    }
}

struct ScanPace<'a, 'home, 'app, 'task, R> {
    clock: MonotonicClock,
    home: &'a mut Home<'home>,
    ui: &'a mut Application<'app>,
    reader: &'a mut R,
    logical: &'a mut Frame,
    control: &'a UiControl<'task>,
    trace: Trace<'a>,
    error: Option<Error>,
}
impl<R: Reader> PaceNative for ScanPace<'_, '_, '_, '_, R> {
    fn milliseconds(&mut self) -> u32 { self.clock.millis() }
    fn delay_one_millisecond(&mut self) { crate::app_power::delay_ms(1); }
    fn loading(&mut self) {
        self.ui.app.screen = Screen::Library;
        self.ui.library.loading = true;
        self.ui.press = crate::app_image::PressState::Idle;
        match self.ui.paint_library(self.home, self.logical, false, super::display::Composition::scoped(self.trace), ShelfCoverState::default(), self.control) {
            Ok(Some(painted)) => self.reader.note_library_paint(painted.view, painted.any_making, self.clock.millis()),
            Ok(None) => {}
            Err(error) => self.error = Some(error),
        }
        self.ui.library.loading = false;
    }
}
