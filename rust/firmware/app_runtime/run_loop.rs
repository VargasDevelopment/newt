// SPDX-License-Identifier: MIT
//! Safe startup and the real queue/DEV/paint loop over one persistent reader.
use crate::{
    app_image::{Application, Events, Home, PressState, Reader, TaskAccess},
    board_clock::BoardBus,
    dev_usb,
    home_startup::{FirmwareCard, HomeRuntime},
    rest_app::Work,
    retained_runtime::record::BootSnapshot,
    sdk::Sdk552,
    serial_console::SerialConsole,
    start_failure::{RETAINED, stop},
};
use core::{convert::Infallible, fmt::Write};
use newt_c3_board::{Board, Error, PendingInput, Probe, hardware::HardwareError};
use newt_core::{app::{self, Action, Screen}, update_confirm};
use newt_input_policy::{self as input, Consumption, native::QueueItem};
use newt_native_input_owner::{InputStart, StartFailure, TaskNotRunning};
use newt_ui_canvas::Frame;

#[derive(Debug)]
pub enum ImageError { Board(Error), Hardware(HardwareError), InputTask(TaskNotRunning), SleepReturned }
impl From<Error> for ImageError {
    fn from(error: Error) -> Self { Self::Board(error) }
}
impl From<HardwareError> for ImageError {
    fn from(error: HardwareError) -> Self { Self::Hardware(error) }
}
impl From<TaskNotRunning> for ImageError {
    fn from(error: TaskNotRunning) -> Self { Self::InputTask(error) }
}

pub fn run<'sdk>(mut board: Board<'sdk>, pending: PendingInput<'sdk>, start: InputStart<'_>,
    probe: Probe, mut log: SerialConsole, logical: &mut Frame, boot: BootSnapshot, mut nvs: Sdk552,
    wifi_lend: crate::wifi_memory::WifiLend,
    storage: crate::reading_storage::ReadingStorage, action: crate::reader_action::Owner,
) -> StartFailure {
    #[cfg(feature = "dev-usb")]
    let mut wifi_report = crate::wifi_memory::report::WifiLendReport::new(wifi_lend);
    #[cfg(not(feature = "dev-usb"))]
    let _ = wifi_lend;
    let crate::reading_storage::ReadingStorage { definition, home_refresh,
        #[cfg(feature = "dev-usb")] bridge,
        #[cfg(feature = "dev-usb")] panels, title, home: home_data, picture } = storage;
    let mut ui = match Application::new(&mut board, &mut nvs, &probe, definition, home_refresh,
        #[cfg(feature = "dev-usb")] bridge,
        #[cfg(feature = "dev-usb")] panels,
    ) {
        Ok(ui) => ui,
        Err(_) => stop(&mut log, Some((&mut board, logical)), "hardware unavailable"),
    };
    #[cfg(feature = "wake-timing")]
    board.wake_stamp(newt_core::wake_timing::Stage::Application);
    let mut home = HomeRuntime::restore_durable(BoardBus { board, log }, nvs, FirmwareCard);
    #[cfg(feature = "wake-timing")]
    home.bus.board.wake_stamp(newt_core::wake_timing::Stage::HomeRestore);
    crate::reader_image::with_runtime(title, home_data, picture, |reader| {
        // Reader restore cannot depend on an input queue/task that does not
        // exist yet. Both one-shot magics are consumed once after arbitration.
        let trace = crate::reader_action::Trace::borrow(&action, home.bus.board.monotonic_clock());
        let restored = reader.boot(&mut home, &mut ui, &boot, logical, trace);
        #[cfg(feature = "wake-timing")]
        {
            home.bus.board.wake_stamp(newt_core::wake_timing::Stage::ReaderRestore);
            home.bus.board.wake_timeline().restored(ui.app.screen as u8, restored);
        }
        if home.consume_retained_boot().is_err() {
            stop(&mut home.bus.log, Some((&mut home.bus.board, logical)), RETAINED);
        }
        let failure = start.run(pending, |mut queue, mut control, mut task| {
            let mut events = Events { action: crate::reader_action::Trace::borrow(&action, home.bus.board.monotonic_clock()), queue: &mut queue, control: &mut control, task: TaskAccess::Running(&mut task) };
            match after_input(&mut home, &mut ui, reader, logical, &mut events, restored,
                #[cfg(feature = "dev-usb")] &mut wifi_report,
            ) {
                Ok(never) => never,
                Err(_) => crate::app_power::stop(),
            }
        });
        stop(&mut home.bus.log, Some((&mut home.bus.board, logical)), crate::app_startup::reason(&failure))
    })
}

fn after_input<'sdk>(home: &mut Home<'sdk>, ui: &mut Application<'sdk>, reader: &mut impl Reader,
    logical: &mut Frame, events: &mut Events<'_, '_>, restored: bool,
    #[cfg(feature = "dev-usb")] wifi_report: &mut crate::wifi_memory::report::WifiLendReport,
) -> Result<Infallible, ImageError> {
    ui.read_info(home, events.action)?;
    home.begin_clock();
    ui.begin_press(reader);
    #[cfg(feature = "wake-timing")]
    home.bus.board.wake_stamp(newt_core::wake_timing::Stage::InputReady);
    if restored {
        events.action.begin();
        reader.open_boot(home, ui, logical, events);
    }
    else {
        let Application { app, quick_start, .. } = ui;
        home.begin_quick_start(app, quick_start, &mut newt_core::quick_start::Facts::default(), false);
    }
    loop_body(home, ui, reader, logical, events,
        #[cfg(feature = "dev-usb")] wifi_report,
    )
}

pub fn loop_body<'sdk>(home: &mut Home<'sdk>, ui: &mut Application<'sdk>, reader: &mut impl Reader,
    logical: &mut Frame, events: &mut Events<'_, '_>,
    #[cfg(feature = "dev-usb")] wifi_report: &mut crate::wifi_memory::report::WifiLendReport,
) -> Result<Infallible, ImageError> {
    let force = !ui.press_answered();
    if ui.app.screen != Screen::Sleeping && !continues_reopen(ui, reader) {
        paint(home, ui, reader, logical, events, force, false)?;
    }
    finish_action(events, home);
    loop {
        if ui.app.screen == Screen::Sleeping {
            sleep(home, ui, reader, logical, events, None)?;
            continue;
        }
        finish_first_paint(home, ui, reader, logical, &events)?;
        #[cfg(feature = "dev-usb")]
        poll(home, ui, reader, logical, events)?;
        #[cfg(feature = "dev-usb")]
        let generation_before = ui.displayed.generation;
        #[cfg(feature = "dev-usb")]
        let mut synthetic_consumed = false;
        let mut changed = false;
        let mut moved = false;
        while let Some(item) = events.queue.try_receive() {
            // Frozen noteCoverInput precedes ticket admission and navigation.
            reader.note_input(home.bus.board.millis());
            let button = match &item {
                QueueItem::Physical(event) => event.queue_code(),
                QueueItem::Synthetic { button, .. } => *button,
            };
            #[cfg(feature = "dev-usb")]
            let physical = matches!(&item, QueueItem::Physical(_));
            #[cfg(feature = "dev-usb")]
            let mut cancellation = false;
            let action = match item {
                QueueItem::Physical(event) => {
                    #[cfg(feature = "dev-usb")]
                    ui.bridge.physical_input();
                    // Publish Back's pending metadata before accepting a press
                    // queued behind it. Work::None performs no navigation.
                    reader.work(home, ui, Work::None, logical, events);
                    ui.begin_press(reader);
                    match events.control.consume_physical(event, input_screen(ui.app.screen)) {
                        Consumption::Action(action) => Some(action),
                        Consumption::CancelBack { dispatch_back } => {
                            #[cfg(feature = "dev-usb")]
                            { cancellation = true; }
                            changed = true;
                            dispatch_back.then_some(input::Action::Back)
                        }
                        Consumption::Ignored => None,
                    }
                }
                QueueItem::Synthetic { ticket, button } => {
                    #[cfg(feature = "dev-usb")]
                    {
                        let clock = home.bus.board.monotonic_clock();
                        let connected = home.bus.log.dev_channel(clock).connected();
                        let now = clock.millis();
                        let accepted = events.task.gate().with_pending(|pending| {
                            ui.bridge.consume_pending(pending, ticket, connected, now)
                        });
                        if !accepted || !acknowledge(home, ui, reader, logical, events, ticket)? { continue; }
                        synthetic_consumed = true;
                        reader.work(home, ui, Work::None, logical, events);
                        ui.begin_press(reader);
                        if button == dev_usb::PURGE_CACHE_BUTTON {
                            changed |= reader.purge_cache(home, ui);
                            continue;
                        }
                        synthetic_action(button, ui.app.screen)
                    }
                    #[cfg(not(feature = "dev-usb"))]
                    { let _ = (ticket, button); None }
                }
            };
            let Some(action) = action else { continue; };
            // Book screens follow the frame on the panel, so a clockwise book's
            // four front buttons (Back..=Right, 0..=3) mirror counterclockwise
            // while side page buttons keep their half-turn swap and CancelBack
            // (0x80) stays Back. Other screens never remap, even before their
            // first repaint after leaving the book.
            let front = button <= input::Button::Right as u8;
            let action = app::facing(core_action(action), front, ui.app.screen, ui.displayed.orientation());
            #[cfg(feature = "dev-usb")]
            let before = (ui.app.screen, reader.facts().opened, home.bus.board.millis());
            events.action.begin();
            let zone_move = ui.app.screen == Screen::Device && ui.app.zone_list_open
                && matches!(action, Action::Previous | Action::Next);
            let dispatched = ui.dispatch(home, reader, logical, action, events)?;
            let work_changed = match dispatched.work {
                Work::None => false,
                work => reader.work(home, ui, work, logical, events),
            };
            if zone_move { moved |= dispatched.changed; } else { changed |= dispatched.changed; }
            changed |= work_changed || ui.press_answered();
            sync_back_button(ui, events.control);
            #[cfg(feature = "dev-usb")]
            if !cancellation {
                reader.consumed_input(physical, button, action, before.2, before.0, ui.app.screen, before.1);
            }
            if ui.app.screen == Screen::Sleeping { break; }
        }
        let dropped = events.control.take_dropped();
        if dropped != 0 { let _ = writeln!(home.bus.log, "Input queue overflow: {} presses lost", dropped); }
        if ui.app.screen == Screen::Sleeping {
            #[cfg(feature = "dev-usb")]
            let completion = synthetic_consumed.then_some(generation_before);
            #[cfg(not(feature = "dev-usb"))]
            let completion = None;
            sleep(home, ui, reader, logical, events, completion)?;
            continue;
        }
        if changed || moved {
            ui.read_info(home, events.action)?;
            if !continues_reopen(ui, reader) {
                paint(home, ui, reader, logical, events, false, !changed)?;
            }
        } else if background_ready(ui) && home.bus.board.millis().wrapping_sub(ui.last_battery_read) >= 60_000 {
            let previous = (ui.info.battery_known, ui.info.battery_percent);
            ui.read_info(home, events.action)?;
            let different = previous.0 != ui.info.battery_known || ui.info.battery_known && previous.1 != ui.info.battery_percent;
            if ui.app.screen == Screen::Home || different || ui.app.screen == Screen::Device {
                paint(home, ui, reader, logical, events, false, ui.app.screen == Screen::Home)?;
            }
        }
        // Frozen completeReaderHousekeeping/report precede action completion
        // and deferred metadata/idle work. Only the actual Runtime write arms
        // this one report; its own backpressure guard runs every pass.
        #[cfg(feature = "dev-usb")]
        {
            if ui.runtime_lows.publish(&ui.bridge, &mut home.bus.log, &mut ui.hardware) { wifi_report.arm(); }
            wifi_report.publish(&mut home.bus.log, &ui.bridge);
        }
        events.action.complete(&mut home.bus.log, || {
            #[cfg(feature = "dev-usb")]
            if synthetic_consumed {
                ui.bridge.render_returned(ui.displayed.generation, ui.displayed.generation != generation_before);
            }
        });
        if !background_ready(ui) { crate::app_power::delay_ms(10); continue; }
        let (repaint, delay) = reader.idle(home, ui, logical, events);
        if repaint && !continues_reopen(ui, reader) {
            paint(home, ui, reader, logical, events, false, false)?;
        }
        if events.action.running() { finish_action(events, home); }
        crate::app_power::delay_ms(delay);
    }
}

fn finish_action(events: &Events<'_ , '_>, home: &mut Home<'_>) {
    events.action.complete(&mut home.bus.log, || {});
}

fn continues_reopen(ui: &Application<'_>, reader: &impl Reader) -> bool {
    let facts = reader.facts();
    facts.defers_paint(ui.app.screen)
}
fn background_ready(ui: &Application<'_>) -> bool {
    #[cfg(feature = "dev-usb")]
    { ui.bridge.background_work_ready() }
    #[cfg(not(feature = "dev-usb"))]
    { let _ = ui; true }
}
pub fn paint(home: &mut Home<'_>, ui: &mut Application<'_>, reader: &mut impl Reader,
    logical: &mut Frame, events: &mut Events<'_, '_>, force: bool, status_only: bool,
) -> Result<(), Error> {
    ui.settle_press(reader);
    let result = match ui.app.screen {
        Screen::Home => {
            let mut result = Ok(None);
            reader.visit_home(&mut |view| result = ui.paint_home(home, view, logical, force, status_only, events.control, events.action));
            result
        }
        Screen::Library => {
            let shelf_state = reader.library_cover_state();
            if let Some(painted) = ui.paint_library(home, logical, force, super::display::Composition::new(events.action), shelf_state, events.control)? {
                if painted.includes_cover {
                    reader.note_library_paint(painted.view, painted.any_making, home.bus.board.millis());
                } else {
                    reader.note_paint(ui.app.screen, ui.displayed.generation, false, home.bus.board.millis());
                }
                #[cfg(feature = "dev-usb")]
                reader.completed_paint(ui.app.screen, ui.displayed.generation);
            }
            return Ok(());
        }
        Screen::Reading => ui.paint_sample(home, logical, force, events.control, events.action).map(|painted| painted.then_some(true)),
        Screen::EpubReading | Screen::Sleeping => reader.paint(home, ui, logical, events, force)
            .map(|painted| painted.then_some(true)),
        _ => ui.paint_system(home, logical, crate::app_dev::build_identity(), force, events.control, events.action).map(|painted| painted.then_some(true)),
    };
    if let Some(includes_cover) = result? {
        reader.note_paint(ui.app.screen, ui.displayed.generation, includes_cover, home.bus.board.millis());
        #[cfg(feature = "dev-usb")]
        reader.completed_paint(ui.app.screen, ui.displayed.generation);
    }
    Ok(())
}

fn sync_back_button(ui: &Application<'_>, control: &input::UiControl<'_>) {
    control.set_clockwise_back(app::clockwise_book(ui.app.screen, ui.displayed.orientation()));
}

fn finish_first_paint(home: &mut Home<'_>, ui: &mut Application<'_>, reader: &mut impl Reader,
    logical: &mut Frame, events: &Events<'_, '_>,
) -> Result<(), Error> {
    if !ui.displayed.valid || ui.press != PressState::Idle { return Ok(()); }
    ui.confirm_first_paint(home);
    if ui.first_screen != update_confirm::FirstScreen::Confirmed || ui.app.screen != Screen::Home { return Ok(()); }
    ui.first_screen = update_confirm::FirstScreen::Noted;
    home.choose_reader_orientation();
    let running = crate::app_dev::image_identity();
    let loaded = home.load_seen_build();
    if loaded == newt_core::durable::Load::Loaded(running) { return Ok(()); }
    if matches!(loaded, newt_core::durable::Load::Loaded(_)) {
        ui.app.screen = Screen::Device;
        ui.app.about = app::AboutView::Updated;
        if ui.paint_system(home, logical, crate::app_dev::build_identity(), true, events.control, events.action)? {
            reader.note_paint(ui.app.screen, ui.displayed.generation, true, home.bus.board.millis());
        }
    }
    home.note_seen_build(running);
    Ok(())
}

#[cfg(feature = "dev-usb")]
pub fn poll(home: &mut Home<'_>, ui: &mut Application<'_>, reader: &impl Reader,
    logical: &Frame, events: &mut Events<'_, '_>,
) -> Result<(), ImageError> {
    let facts = super::publication::Facts::current(ui);
    super::publication::Published { facts, displayed: &ui.displayed, hardware: &mut ui.hardware,
        bridge: &mut ui.bridge, panels: &ui.panels, writes: true }.poll(home, reader, logical, events).map_err(ImageError::from)
}

#[cfg(feature = "dev-usb")]
fn acknowledge(home: &mut Home<'_>, ui: &mut Application<'_>, reader: &impl Reader,
    logical: &Frame, events: &mut Events<'_, '_>, ticket: u32,
) -> Result<bool, ImageError> {
    let clock = home.bus.board.monotonic_clock();
    let started = clock.millis();
    let mut elapsed = 0;
    let mut accepted = false;
    while ui.bridge.consumption_pending(ticket) {
        elapsed = clock.millis().wrapping_sub(started);
        if elapsed >= 1000 { ui.bridge.physical_input(); break; }
        poll(home, ui, reader, logical, events)?;
        elapsed = clock.millis().wrapping_sub(started);
        if elapsed >= 1000 { ui.bridge.physical_input(); break; }
        if ui.bridge.consumption_acknowledged(ticket) { accepted = true; break; }
        crate::app_power::delay_ms(1);
    }
    if elapsed > 200 {
        let _ = super::trace::row(&mut home.bus.log, 80, format_args!("BG slow: work=input-ack ms={}\n", elapsed));
    }
    Ok(accepted)
}

fn sleep(home: &mut Home<'_>, ui: &mut Application<'_>, reader: &mut impl Reader,
    logical: &mut Frame, events: &mut Events<'_, '_>, completion: Option<u32>,
) -> Result<(), ImageError> {
    let now = home.bus.board.millis();
    let Events { queue, control, task, action: trace } = events;
    let entered = task.with_suspended(|suspended, input| {
        let mut events = Events { queue, control, action: *trace, task: suspended };
        // Frozen sleep_input_runtime.h retires cancellation before a live
        // sleep image. The same native suspension lasts through power entry.
        while let Some(item) = events.queue.try_receive() {
            if let QueueItem::Physical(event) = item {
                let _ = events.control.consume_physical(event, input_screen(ui.app.screen));
            }
        }
        {
        let _exit = if ui.app.resume_screen == Screen::EpubReading {
            events.action.boundary(crate::reader_action::ActionStage::MarkClose as u8);
            Some(events.action.scope(crate::reader_action::ActionStage::READER_EXIT))
        } else { None };
        paint(home, ui, reader, logical, &mut events, true, false)?;
        if reader.finish_sleep(home, ui, logical, &mut events, now) {
            paint(home, ui, reader, logical, &mut events, true, false)?;
        }
        }
        finish_first_paint(home, ui, reader, logical, &events)?;
        events.action.complete(&mut home.bus.log, || {
            #[cfg(feature = "dev-usb")]
            if let Some(before) = completion {
                ui.bridge.render_returned(ui.displayed.generation, ui.displayed.generation != before);
            }
        });
        #[cfg(feature = "dev-usb")]
        {
            ui.bridge.going_to_sleep();
            let clock = home.bus.board.monotonic_clock();
            let started = clock.millis();
            while (ui.bridge.output_pending() || ui.bridge.request_pending()) && clock.millis().wrapping_sub(started) < 250 {
                poll(home, ui, reader, logical, &mut events)?;
                crate::app_power::delay_ms(1);
            }
        }
        #[cfg(not(feature = "dev-usb"))]
        let _ = completion;
        let entered = crate::app_power::enter_sleep_quiesced(home, &input, &mut |home| {
            let now = home.bus.board.millis();
            reader.prepare_sleep(home, ui, logical, &mut events, now)
        });
        if !entered {
            app::wake(&mut ui.app);
            let now = home.bus.board.millis();
            reader.cancel_sleep(home, now);
            let _ = home.bus.board.with_retained(|mut words| words.clear_sd_resume_magic());
            ui.info.wake = Some(b"sleep wake pin unavailable");
        }
        Ok::<_, ImageError>(entered)
    })??;
    if entered { return Err(ImageError::SleepReturned); }
    // An abandoned attempt resumed the existing task after restoring the
    // live app/tracker; its replacement frame is outside that suspension.
    paint(home, ui, reader, logical, events, true, false)?;
    Ok(())
}

fn input_screen(screen: Screen) -> input::Screen {
    match screen { Screen::Home => input::Screen::Home, Screen::EpubReading => input::Screen::EpubReading, _ => input::Screen::Other }
}
fn core_action(action: input::Action) -> Action {
    match action { input::Action::Previous => Action::Previous, input::Action::Next => Action::Next,
        input::Action::Activate => Action::Activate, input::Action::Back => Action::Back,
        input::Action::Sleep => Action::Sleep, input::Action::Read => Action::Read }
}
#[cfg(feature = "dev-usb")]
fn synthetic_action(button: u8, screen: Screen) -> Option<input::Action> {
    let button = match button { 0 => input::Button::Back, 1 => input::Button::Confirm,
        2 => input::Button::Left, 3 => input::Button::Right, 4 => input::Button::Up,
        5 => input::Button::Down, 6 => input::Button::Power, _ => return None };
    Some(input::action_for_button(button, input_screen(screen)))
}
