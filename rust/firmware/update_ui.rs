// SPDX-License-Identifier: MIT
//! The selected update boot's actual clock, input and completed publication.
//! Parts lends these owners while the same Board serves partition progress.
#![forbid(unsafe_code)]
use crate::{app_image::Events, app_runtime::{display::{self, DisplayReceipt}, run_loop::ImageError},
    board_clock::{BoardBus, read_board_clock}, serial_console::SerialConsole,
    update_runtime::UpdateView};
use newt_c3_board::{Board, Error, Probe, Refresh, hardware::{HardwareError, HardwareReader}};
use newt_core::{app::Action, frame::{self, Frame}, preferences::Orientation, refresh::UiRefresh};
use newt_device_ui::{DeviceInfo, home_clock::{ClockReason, HomeClockSource}, update::{self, UpdatePage}};
use newt_input_policy::{self as input, native::QueueItem};
#[cfg(feature = "dev-usb")]
use crate::dev_usb::{Bridge, Diagnostics, PanelRing, Status};

pub struct ModeUi<'sdk> {
    pub bus: BoardBus<'sdk, SerialConsole>,
    pub clock: HomeClockSource<BoardBus<'sdk, SerialConsole>>,
    info: DeviceInfo<'static>,
    displayed: DisplayReceipt,
    budget: UiRefresh,
    hardware: HardwareReader<'sdk>,
    #[cfg(feature = "dev-usb")]
    bridge: Bridge,
    #[cfg(feature = "dev-usb")]
    panels: PanelRing,
    #[cfg(feature = "dev-usb")]
    fingerprint: u64,
}

impl<'sdk> ModeUi<'sdk> {
    pub fn new(board: Board<'sdk>, log: SerialConsole, probe: &Probe) -> Result<Self, HardwareError> {
        let mut hardware = board.hardware_reader();
        #[cfg(feature = "dev-usb")]
        let boot = hardware.random_word()?;
        #[cfg(feature = "dev-usb")]
        let fingerprint = crate::app_dev::fingerprint(&mut hardware);
        let mut info = DeviceInfo::default();
        info.panel = Some(if probe.verdict == 2 { b"X3 / panel probe inconclusive".as_slice() }
            else if probe.promoted != 0 { b"X3 / UC8279 detected" } else { b"X3 / UC8253 assumed" });
        Ok(Self { bus: BoardBus { board, log }, clock: HomeClockSource::new(), info,
            displayed: DisplayReceipt::default(), budget: UiRefresh::default(), hardware,
            #[cfg(feature = "dev-usb")] bridge: Bridge::new(boot),
            #[cfg(feature = "dev-usb")] panels: PanelRing::default(),
            #[cfg(feature = "dev-usb")] fingerprint })
    }

    /// After InputStart::run, before the view: one readDeviceInfo, then the
    /// caller restores clock preferences through the actual state router.
    pub fn read_info(&mut self) -> Result<(), HardwareError> {
        crate::app_runtime::info::read_device_info(&mut self.info, &mut self.hardware,
            &self.bus.board.monotonic_clock()).map(|_| ())
    }
    pub fn begin_clock(&mut self) { self.clock.begin_reader(&mut self.bus, Some(read_board_clock)); }

    pub fn with_parts<R>(&mut self, work: impl FnOnce(&mut Board<'sdk>, &mut Parts<'_, 'sdk>) -> R) -> R {
        let Self { bus, clock, info, displayed, budget, hardware,
            #[cfg(feature = "dev-usb")] bridge,
            #[cfg(feature = "dev-usb")] panels,
            #[cfg(feature = "dev-usb")] fingerprint } = self;
        work(&mut bus.board, &mut Parts { log: &mut bus.log, clock, info, displayed, budget, hardware,
            #[cfg(feature = "dev-usb")] bridge,
            #[cfg(feature = "dev-usb")] panels,
            #[cfg(feature = "dev-usb")] fingerprint: *fingerprint })
    }
    pub fn generation(&self) -> u32 { self.displayed.generation }
    pub fn trace_heap(&mut self, when: &str) -> Result<(), HardwareError> {
        self.with_parts(|_, parts| parts.trace_heap(when))
    }
    pub fn render_returned(&mut self, synthetic: bool, before: u32) {
        #[cfg(feature = "dev-usb")]
        if synthetic { self.bridge.render_returned(self.displayed.generation, self.displayed.generation != before); }
        #[cfg(not(feature = "dev-usb"))]
        let _ = (synthetic, before);
    }
    pub fn drain_before_restart(&mut self, view: &UpdateView<'_>, logical: Option<&Frame>, events: &mut Events<'_, '_>) {
        #[cfg(feature = "dev-usb")]
        {
            let started = self.bus.board.millis();
            while (self.bridge.output_pending() || self.bridge.request_pending())
                && self.bus.board.millis().wrapping_sub(started) < 250 {
                if self.poll(view, logical, events).is_err() { break; }
                crate::app_power::delay_ms(1);
            }
        }
        #[cfg(not(feature = "dev-usb"))]
        let _ = (view, logical, events);
    }
    pub fn paint(&mut self, view: &mut UpdateView<'_>, logical: Option<&mut Frame>, radio_on: bool) -> Result<(), Error> {
        self.with_parts(|board, parts| parts.paint(board, view, logical, radio_on))
    }
    pub fn poll(&mut self, view: &UpdateView<'_>, logical: Option<&Frame>, events: &mut Events<'_, '_>) -> Result<(), HardwareError> {
        self.with_parts(|board, parts| parts.poll(board, view, logical, events))
    }

    /// Queue codes and ticket acknowledgement are the same input task/bridge
    /// protocol as the app. No press made during a blocking operation is lost.
    pub fn next_action(&mut self, view: &UpdateView<'_>, logical: Option<&Frame>, events: &mut Events<'_, '_>,
        synthetic: &mut bool,
    ) -> Result<Option<(Action, bool)>, ImageError> {
        while let Some(item) = events.queue.try_receive() {
            let button = match item {
                QueueItem::Physical(event) => {
                    #[cfg(feature = "dev-usb")]
                    self.bridge.physical_input();
                    let code = event.queue_code();
                    // Update has no reader open/cancel scope. Consume the same
                    // physical event to retire its real input bookkeeping.
                    let _ = events.control.consume_physical(event, input::Screen::Other);
                    code
                }
                QueueItem::Synthetic { ticket, button } => {
                    #[cfg(feature = "dev-usb")]
                    {
                        let clock = self.bus.board.monotonic_clock();
                        let connected = self.bus.log.dev_channel(clock).connected();
                        if !events.task.gate().with_pending(|pending|
                            self.bridge.consume_pending(pending, ticket, connected, clock.millis())) { continue; }
                        if !self.acknowledge(view, logical, events, ticket)? { continue; }
                        *synthetic = true;
                        button
                    }
                    #[cfg(not(feature = "dev-usb"))]
                    { let _ = (ticket, button, synthetic); continue; }
                }
            };
            let key = match button { 0 => input::Button::Back, 1 => input::Button::Confirm,
                2 => input::Button::Left, 3 => input::Button::Right, 4 => input::Button::Up,
                5 => input::Button::Down, 6 => input::Button::Power, _ => continue };
            let action = match input::action_for_button(key, input::Screen::Other) {
                input::Action::Previous => Action::Previous, input::Action::Next => Action::Next,
                input::Action::Activate => Action::Activate, input::Action::Back => Action::Back,
                input::Action::Sleep => Action::Sleep, input::Action::Read => Action::Read };
            return Ok(Some((action, button == 4 || button == 5)));
        }
        Ok(None)
    }

    #[cfg(feature = "dev-usb")]
    fn acknowledge(&mut self, view: &UpdateView<'_>, logical: Option<&Frame>, events: &mut Events<'_, '_>, ticket: u32) -> Result<bool, HardwareError> {
        let clock = self.bus.board.monotonic_clock();
        let started = clock.millis(); let mut elapsed = 0; let mut accepted = false;
        while self.bridge.consumption_pending(ticket) {
            elapsed = clock.millis().wrapping_sub(started);
            if elapsed >= 1000 { self.bridge.physical_input(); break; }
            self.poll(view, logical, events)?;
            elapsed = clock.millis().wrapping_sub(started);
            if elapsed >= 1000 { self.bridge.physical_input(); break; }
            if self.bridge.consumption_acknowledged(ticket) { accepted = true; break; }
            crate::app_power::delay_ms(1);
        }
        if elapsed > 200 {
            let _ = crate::app_runtime::trace::row(&mut self.bus.log, 80, format_args!("BG slow: work=input-ack ms={}\n", elapsed));
        }
        Ok(accepted)
    }
}

/// Short references only. ImagePartitions::board reborrows the same Board
/// between SDK calls; these references never alias the partition owner.
pub struct Parts<'a, 'sdk> {
    pub log: &'a mut SerialConsole,
    clock: &'a mut HomeClockSource<BoardBus<'sdk, SerialConsole>>,
    info: &'a DeviceInfo<'static>,
    displayed: &'a mut DisplayReceipt,
    budget: &'a mut UiRefresh,
    hardware: &'a mut HardwareReader<'sdk>,
    #[cfg(feature = "dev-usb")] bridge: &'a mut Bridge,
    #[cfg(feature = "dev-usb")] panels: &'a mut PanelRing,
    #[cfg(feature = "dev-usb")] fingerprint: u64,
}
struct Bus<'a, 'sdk> { board: &'a mut Board<'sdk>, log: &'a mut SerialConsole }
fn read_clock(bus: &mut Bus<'_, '_>, reason: ClockReason, zone: newt_core::time_zone::TimeZone,
    format: newt_core::clock_format::ClockFormat, text: &mut [u8; 5],
) {
    let snapshot = bus.board.clock_snapshot();
    crate::board_clock::finish_clock_read(bus.log, snapshot, reason, zone, format, text);
}
impl Parts<'_, '_> {
    pub fn paint_online(&mut self, board: &mut Board<'_>, status: &update::OnlineStatus<'_>,
        password_on_panel: &mut bool, logical: Option<&mut Frame>, radio_on: bool,
    ) -> Result<(), Error> {
        self.paint_body(board, logical, status.percent, radio_on, |bar, frame| {
            update::render_online(status, bar, Some(frame));
            let erase = *password_on_panel;
            *password_on_panel = false;
            erase
        })
    }
    pub fn trace_heap(&mut self, when: &str) -> Result<(), HardwareError> {
        use core::fmt::Write;
        let free = self.hardware.free_heap_bytes()?;
        let largest = self.hardware.largest_heap_block_bytes()?;
        let lowest = self.hardware.min_free_heap_bytes()?;
        let _ = writeln!(self.log, "Newt update mode {}: heap free {}, largest block {}, lowest {}", when, free, largest, lowest);
        Ok(())
    }
    pub fn paint_installing(&mut self, board: &mut Board<'_>, checking: bool, percent: u8,
        password_on_panel: &mut bool, logical: &mut Frame, radio_on: bool,
    ) -> Result<(), Error> {
        self.paint_body(board, Some(logical), percent, radio_on, |bar, frame| {
            update::render_installing(checking, percent, bar, Some(frame));
            let erase = *password_on_panel;
            *password_on_panel = false;
            erase
        })
    }
    pub fn paint(&mut self, board: &mut Board<'_>, view: &mut UpdateView<'_>, logical: Option<&mut Frame>, radio_on: bool) -> Result<(), Error> {
        let percent = view.percent;
        self.paint_body(board, logical, percent, radio_on, |bar, frame| {
            update::render_update(view, bar, Some(frame));
            let password = view.page == UpdatePage::Password;
            let erase = view.password_on_panel && !password;
            view.password_on_panel = password;
            erase
        })
    }
    fn paint_body(&mut self, board: &mut Board<'_>, logical: Option<&mut Frame>, percent: u8, radio_on: bool,
        draw: impl FnOnce(&newt_system_ui::SystemBar, &mut Frame) -> bool,
    ) -> Result<(), Error> {
        let started = display::refresh_start(board);
        let full = if let Some(logical) = logical {
            let bar = self.clock.with_context(read_clock, |clock| clock.read_system_bar(
                &mut Bus { board, log: self.log }, self.info, radio_on));
            let erase = draw(&bar, logical);
            let full = !self.displayed.valid || self.budget.needs_full() || erase;
            frame::to_native_frame(logical, board.frame()?.try_into().map_err(|_| Error::Length)?, Orientation::Portrait);
            full
        } else {
            let bar = update::update_bar(u32::from(percent));
            if !frame::fill_native_rect(board.frame()?.try_into().map_err(|_| Error::Length)?,
                bar.left, bar.top, bar.filled, bar.height) { return Err(Error::Length); }
            false
        };
        #[cfg(feature = "dev-usb")]
        let panels = Some(&mut *self.panels);
        #[cfg(not(feature = "dev-usb"))]
        let panels = None;
        display::blocking_binary(board, panels, if full { Refresh::Full } else { Refresh::Fast }, "ui")?;
        self.budget.painted(full);
        self.displayed.completed_portrait(board, false, false, started);
        Ok(())
    }

    pub fn poll(&mut self, board: &mut Board<'_>, view: &UpdateView<'_>, logical: Option<&Frame>, events: &mut Events<'_, '_>) -> Result<(), HardwareError> {
        self.poll_ready(board, update::update_frame_private(view), logical, events)
    }
    pub fn poll_ready(&mut self, board: &mut Board<'_>, private: bool, logical: Option<&Frame>, events: &mut Events<'_, '_>) -> Result<(), HardwareError> {
        #[cfg(feature = "dev-usb")]
        {
            events.task.gate().with_pending(|pending| self.bridge.drain_pending(pending));
            let free_heap = self.hardware.free_heap_bytes()?;
            let largest = self.hardware.largest_heap_block_bytes()?;
            // Frozen globals remain on Home with no reader/evidence/catalog
            // constructed in this boot. Update never introduces a DEV screen.
            let status = Status { generation: self.displayed.generation, refresh_ms: self.displayed.refresh_ms,
                free_heap, dropped: events.control.dropped(), screen: 0, width: frame::WIDTH as u32, height: frame::HEIGHT as u32,
                ready: self.displayed.valid, orientation: Orientation::Portrait, monochrome: true };
            let diagnostics = Diagnostics { record: 255, requested: 255,
                reader_status: newt_core::zip_container::Status::NotFound as u32,
                free_heap, largest_block: largest, opening: events.control.active(),
                device_fingerprint: self.fingerprint, ..Diagnostics::default() };
            let frame = if private { None } else { logical };
            let mut publication = crate::app_dev::Publication { status, frame, queue: events.queue,
                panels: self.panels, diagnostics, put: crate::app_dev::Put::Unavailable,
                #[cfg(feature = "wake-timing")]
                wake: board.wake_timeline(),
            };
            let mut channel = self.log.dev_channel(board.monotonic_clock());
            crate::app_dev::poll(self.bridge, &mut channel, &mut publication, crate::app_dev::BUILD_ID, true);
        }
        #[cfg(not(feature = "dev-usb"))]
        let _ = (board, private, logical, events, &self.hardware);
        Ok(())
    }
}
