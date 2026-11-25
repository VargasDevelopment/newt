// SPDX-License-Identifier: MIT
//! Choose the separate update boot before SD, app, package or reader owners.
use core::ffi::c_void;
use core::mem::MaybeUninit;
use newt_c3_board::Board;
#[cfg(feature = "wifi-update")]
use newt_c3_board::{PendingInput, Probe};
#[cfg(feature = "wifi-update")]
use newt_core::frame::Frame;
use newt_native_input_owner::{StartFailure, run_acquired_deferred};
#[cfg(feature = "wifi-update")]
use newt_native_input_owner::InputStart;
use crate::{sdk::Sdk552, start_failure::{RETAINED, stop}, update_boot};

// Frozen loadedApp.app, homeRefresh, usbBridgeStorage, panelRing, sdSelection.title_, home, resumePicture.
// Only the one reading-startup boundary below may touch these slots or guard.
static mut READING_TAKEN: bool = false;
static mut READING_DEFINITION: MaybeUninit<newt_tiny_app::Definition> = MaybeUninit::uninit();
static mut READING_HOME_REFRESH: MaybeUninit<crate::app_runtime::home_refresh::HomeRefresh> = MaybeUninit::uninit();
#[cfg(feature = "dev-usb")]
static mut READING_BRIDGE: MaybeUninit<crate::dev_usb::Bridge> = MaybeUninit::uninit();
#[cfg(feature = "dev-usb")]
static mut READING_PANELS: MaybeUninit<crate::dev_usb::PanelRing> = MaybeUninit::uninit();
static mut READING_TITLE: MaybeUninit<[u8; 256]> = MaybeUninit::uninit();
static mut READING_HOME: MaybeUninit<newt_stage_home::HomeData<'static>> = MaybeUninit::uninit();
static mut READING_PICTURE: MaybeUninit<newt_core::resume_picture::Mark> = MaybeUninit::uninit();
#[cfg(feature = "wifi-update")]
use crate::{app_image::{Events, TaskAccess}, update_boot::UpdateModeRequest, update_mode::Online, update_ui::ModeUi};

// SAFETY: fixed entry.cpp gates over the existing console channels; no Rust
// memory is lent, and these synchronous calls retain no context.
unsafe extern "C" {
    #[cfg(feature = "wifi-update")]
    fn nb_withhold_console();
    fn nb_allow_reading_crash_output();
}
/// # Safety
/// Same one-shot SDK/display/input/global contract as newt_rust_app_run. No
/// Wi-Fi calls or early SD mount may precede this choice; panic cannot unwind.
/// The concrete Online and mode operations must edit the pinned view in place,
/// never move or replace its filled secret fields.
// SAFETY: only the sole native image entry supplies these exclusive owners.
pub unsafe fn run(display: *mut c_void, input: *mut c_void) -> ! {
    #[cfg(feature = "wake-timing")]
    newt_c3_board::wake_timing::begin();
    // SAFETY: raw retained evidence is sampled before Serial or acquisition.
    let boot = match unsafe { crate::retained_runtime::snapshot_before_board() } {
        Ok(boot) => boot,
        // SAFETY: this failure path begins Serial once, after the sampling and
        // before any other serial user; it never returns.
        Err(_) => stop(&mut unsafe { crate::serial_console::SerialConsole::begin() }, None, RETAINED),
    };
    // SAFETY: the unique entry begins Serial once before any other serial user.
    let mut log = unsafe { crate::serial_console::SerialConsole::begin() };
    #[cfg(feature = "wake-timing")]
    newt_c3_board::wake_timing::serial_ready();
    let action = crate::reader_action::allocate();
    // SAFETY: caller grants sole initialized SDK/display/input access.
    let (mut board, pending, probe) = match unsafe { Board::acquire_x3_panel_deferred_input(display, input) } {
        // Frozen fail("display buffer unavailable") had no panel to paint.
        Ok(acquired) => acquired, Err(_) => stop(&mut log, None, "display buffer unavailable"),
    };
    // SAFETY: the native app is excluded and no NVS owner was created yet.
    let nvs = unsafe { Sdk552::from_exclusive_firmware_lease() };
    #[cfg(feature = "wake-timing")]
    board.wake_timeline().boot(boot.reset, boot.cause, if probe.promoted != 0 { 2 } else { 1 });
    // SAFETY: sole firmware entry; no other logical frame loan exists.
    let failure = unsafe { newt_c3_board::app_frame::with_logical(|logical| {
        let _request = match update_boot::take(&mut board, boot.software_reset) {
            Ok(request) => request, Err(_) => stop(&mut log, Some((&mut board, logical)), RETAINED),
        };
        // SAFETY: panel acquisition above granted the same sole SDK lease;
        // this starts only the deferred owner, not the input queue or task.
        run_acquired_deferred(board, pending, probe, move |mut board, pending, start, probe| {
            #[cfg(feature = "wifi-update")]
            if _request != UpdateModeRequest::None {
                return run_update(board, pending, start, probe, log, nvs, logical, _request,
                    &mut crate::update_online_runtime::Online, action);
            }
            nb_allow_reading_crash_output();
            // SAFETY: the reading boot never enters the update SDK scope;
            // this one lend precedes SD or input allocation.
            let lend = crate::wifi_memory::lend();
            match board.ensure_sd_mounted() {
                Ok(()) | Err(newt_c3_board::Error::Io) => {}
                Err(_) => stop(&mut log, Some((&mut board, logical)), "storage unavailable"),
            }
            #[cfg(feature = "wake-timing")]
            board.wake_stamp(newt_core::wake_timing::Stage::Sd);
            // SAFETY: this is the sole firmware startup thread and the native
            // owner's FnOnce callback, before queue/task creation. The guard
            // fails closed before any second reference can be formed. No other
            // code names these slots; all later access uses the returned loans.
            if READING_TAKEN { stop(&mut log, Some((&mut board, logical)), reason(&StartFailure::Contract)); }
            READING_TAKEN = true;
            let storage = crate::reading_storage::ReadingStorage {
                definition: &mut *(&raw mut READING_DEFINITION),
                home_refresh: &mut *(&raw mut READING_HOME_REFRESH),
                #[cfg(feature = "dev-usb")]
                bridge: &mut *(&raw mut READING_BRIDGE),
                #[cfg(feature = "dev-usb")]
                panels: &mut *(&raw mut READING_PANELS),
                title: &mut *(&raw mut READING_TITLE),
                home: &mut *(&raw mut READING_HOME),
                picture: &mut *(&raw mut READING_PICTURE),
            };
            crate::app_image::run(board, pending, start, probe, log, logical, boot, nvs, lend, storage, action)
        })
    }) };
    // Every startup failure above stops with its reason. Only a native owner
    // that never ran startup returns here; the log moved into that startup.
    let _ = failure;
    crate::start_failure::halt()
}

/// Frozen fail() reasons for the input start; the others name their step.
pub(crate) fn reason(failure: &StartFailure) -> &'static str {
    match failure {
        StartFailure::Queue => "input queue unavailable",
        StartFailure::Task => "input task unavailable",
        StartFailure::Board(_) => "input unavailable",
        StartFailure::Contract => "startup contract broken",
    }
}

/// The separate boot owns every station/TLS path. Keep its boundary visible
/// in the image, so the linked memory check can prove reading never reaches it.
// SAFETY: run's caller granted the same exclusive globals; this branch has
// neither lent Wi-Fi memory nor started input and it never enters reading.
#[cfg(feature = "wifi-update")]
#[inline(never)]
unsafe fn run_update<'sdk, O: Online>(board: Board<'sdk>, pending: PendingInput<'sdk>,
    start: InputStart<'_>, probe: Probe, log: crate::serial_console::SerialConsole,
    nvs: Sdk552, logical: &mut Frame, request: UpdateModeRequest, online: &mut O,
    action: crate::reader_action::Owner,
) -> StartFailure {
    // SAFETY: only the selected update boot reaches these SDK operations and
    // the filled view never moves; every success remains in this scope until reset.
    unsafe {
        nb_withhold_console();
        let mut ui = match ModeUi::new(board, log, &probe) {
            Ok(ui) => ui, Err(_) => crate::stop(),
        };
        if ui.trace_heap("start").is_err() { crate::stop(); }
        crate::wifi_memory::run_constructors();
        let mut sdk = crate::update_native::Sdk::for_update_boot(&ui.bus.board);
        let failure = start.run(pending, |mut queue, mut control, mut task| {
            let mut events = Events { action: crate::reader_action::Trace::borrow(&action, ui.bus.board.monotonic_clock()), queue: &mut queue, control: &mut control,
                task: TaskAccess::Running(&mut task) };
            if ui.read_info().is_err() { crate::stop(); }
            let mut state = crate::update_state::State::new(nvs);
            let card_ready = state.restore_clock(&mut ui.bus.board, &mut ui.clock);
            ui.begin_clock();
            // Only empty secret fields move into their final pinned slot.
            let mut view = core::pin::pin!(crate::update_runtime::empty_view(newt_c3_board::image::running_image_version()));
            let view = view.as_mut().get_unchecked_mut();
            crate::update_mode::run_started(&mut ui, &mut state, view, logical,
                &mut sdk, &mut events, request, card_ready, online)
        });
        stop(&mut ui.bus.log, Some((&mut ui.bus.board, logical)), reason(&failure))
    }
}
