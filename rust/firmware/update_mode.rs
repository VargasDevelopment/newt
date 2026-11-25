// SPDX-License-Identifier: MIT
//! The frozen separate-boot queue/dispatch/paint loop. The final entry lends
//! the already-placed view; this module never moves a filled secret owner.
#![forbid(unsafe_code)]
use core::fmt::Write;
use crate::{app_image::Events, app_runtime::run_loop::ImageError, update_install::Installer,
    update_native::Sdk, update_radio::{self, Ports, WifiJoinReport},
    update_runtime::{self, FlashFence, UpdateView}, update_state::State, update_ui::ModeUi,
    wifi_secrets::Wipe};
use newt_core::{frame::Frame, update_card::{self, slot_installer::InstallIssue},
    update_manifest::UpdateKind, update_mode_request::UpdateModeRequest,
    wifi_network_store::workflow::WifiChoice};
use newt_device_ui::update::{self, UpdateIntent};
use newt_native_io::{UpdateDirectory, UpdateFiles};

/// Required actual HTTPS/frame/download composition, still an explicit join
/// until its concrete implementation exists. There is no unavailable or
/// no-op implementation with which entry could silently enable Check.
pub trait Online {
    fn check(&mut self, ui: &mut ModeUi<'_>, state: &mut State, view: &mut UpdateView<'_>,
        logical: &mut Frame, sdk: &mut Sdk, events: &mut Events<'_, '_>) -> Result<(), ImageError>;
}

/// InputStart::run has begun the SAME input/queue/task. Root read info, restored
/// the real clock and placed the empty view before this nonreturning loan.
/// card_ready is the actual result of that same state router's clock restore.
pub fn run_started(ui: &mut ModeUi<'_>, state: &mut State, view: &mut UpdateView<'_>, logical: &mut Frame,
    sdk: &mut Sdk, events: &mut Events<'_, '_>, request: UpdateModeRequest, card_ready: bool,
    online: &mut impl Online,
) -> ! {
    let mut installer = Installer::new();
    view.quick_start = request == UpdateModeRequest::QuickStartWifi;
    if update_runtime::needs_card_offers(request) { scan_offers(ui, view, card_ready); }
    let report = state.restore(&mut ui.bus.board, &mut view.saved, None, sdk);
    let _ = writeln!(ui.bus.log, "Newt Wi-Fi network restore: record {}, network {}, key {}, opened {}",
        report.record as u8, u8::from(report.network), report.key as u8, u8::from(report.opened));
    let first = update_runtime::open_requested_page(view, request);
    let initialized = if first == Some(UpdateIntent::Scan) { scan_networks(ui, view, logical, sdk, events) }
        else { ui.paint(view, Some(logical), sdk.flash_fenced_for_wifi()).map_err(ImageError::from) };
    if initialized.is_err() { leave(ui, view, Some(logical), sdk, events, "Newt"); }
    let mut last_press = ui.bus.board.millis();
    loop {
        if ui.poll(view, Some(logical), events).is_err() { leave(ui, view, Some(logical), sdk, events, "Newt"); }
        let before = ui.generation();
        let mut synthetic = false;
        loop {
            let action = match ui.next_action(view, Some(logical), events, &mut synthetic) {
                Ok(Some(action)) => action,
                Ok(None) => break,
                Err(_) => leave(ui, view, Some(logical), sdk, events, "Newt"),
            };
            last_press = ui.bus.board.millis();
            let mut intent = update::dispatch_update(view, action.0, action.1, &Wipe);
            let result = match intent {
                UpdateIntent::Exit => leave(ui, view, Some(logical), sdk, events, "Newt"),
                UpdateIntent::Repaint => ui.paint(view, Some(logical), sdk.flash_fenced_for_wifi()).map_err(ImageError::from),
                UpdateIntent::Scan => scan_networks(ui, view, logical, sdk, events),
                UpdateIntent::Network => {
                    let changed = change_network(ui, state, view, logical, sdk);
                    if changed.is_ok() { intent = update::check_after_save(view); }
                    changed
                }
                _ => Ok(()),
            };
            if result.is_err() { leave(ui, view, Some(logical), sdk, events, "Newt"); }
            if intent == UpdateIntent::Join && connect(ui, state, view, logical, sdk, events).is_err() {
                leave(ui, view, Some(logical), sdk, events, "Newt");
            }
            if intent == UpdateIntent::Check {
                if online.check(ui, state, view, logical, sdk, events).is_err() {
                    leave(ui, view, Some(logical), sdk, events, "Newt");
                }
                discard_actions(ui, view, logical, sdk, events, &mut synthetic);
                last_press = ui.bus.board.millis();
            }
            if intent != UpdateIntent::Install { continue; }
            let issue = match installer.install(ui, view, logical, events, sdk.flash_fenced_for_wifi()) {
                Ok(issue) => issue,
                Err(_) => leave(ui, view, Some(logical), sdk, events, "Newt"),
            };
            if issue == InstallIssue::None {
                let into = update::focused_offer(view).map(|index| &view.offers[index]);
                let into = match into {
                    Some(offer) if offer.manifest.kind == UpdateKind::Firmware => match offer.name {
                        update_card::FirmwareName::CrossInk => "CrossInk",
                        update_card::FirmwareName::CrossPoint => "CrossPoint",
                        update_card::FirmwareName::Unknown => "the other firmware",
                    },
                    _ => "the new Newt",
                };
                leave(ui, view, Some(logical), sdk, events, into);
            }
            discard_actions(ui, view, logical, sdk, events, &mut synthetic);
            last_press = ui.bus.board.millis();
        }
        ui.render_returned(synthetic, before);
        if ui.bus.board.millis().wrapping_sub(last_press) >= 10 * 60 * 1000 {
            leave(ui, view, Some(logical), sdk, events, "Newt");
        }
        crate::app_power::delay_ms(10);
    }
}

fn discard_actions(ui: &mut ModeUi<'_>, view: &UpdateView<'_>, logical: &Frame, sdk: &mut Sdk,
    events: &mut Events<'_, '_>, synthetic: &mut bool,
) {
    loop {
        match ui.next_action(view, Some(logical), events, synthetic) {
            Ok(Some(_)) => (), Ok(None) => break,
            // There is no reader/secret transition in queue retirement. A
            // native observation failure stops before another action runs.
            Err(_) => { let _ = sdk; crate::app_power::stop(); }
        }
    }
}

fn scan_offers(ui: &mut ModeUi<'_>, view: &mut UpdateView<'_>, card_ready: bool) {
    view.card_ready = card_ready;
    view.official_return = update::ReturnOffer::None;
    let channel = ui.clock.test_builds();
    if card_ready {
        ui.bus.board.with_optional_sd_io(|board, io| {
            let mut files = UpdateFiles::new(io); let mut directory = UpdateDirectory::new(io);
            let mut scratch = [0; newt_core::update_manifest::MAX_BYTES + 1];
            board.with_image_partitions(true, |slots|
                update_runtime::scan_offers(view, card_ready, &mut files, &mut directory, slots, &mut scratch));
            view.official_return = crate::update_download::official_return::Store::new(io)
                .restore(&update_runtime::running_identity(view.running), &view.offers[0].manifest, channel)
                .map(update::ReturnOffer::Offered).unwrap_or_default();
        });
        return;
    }
    // Frozen card-ready=false skips offers. Slot/rollback observations still
    // use actual partitions; there is no fake Files provider or mount retry.
    ui.bus.board.with_image_partitions(true, |slots| update_runtime::scan_target(view, slots));
}

fn scan_networks(ui: &mut ModeUi<'_>, view: &mut UpdateView<'_>, logical: &mut Frame,
    sdk: &mut Sdk, events: &mut Events<'_, '_>,
) -> Result<(), ImageError> {
    ui.paint(view, Some(logical), sdk.flash_fenced_for_wifi())?;
    ui.trace_heap("wifi-before")?;
    let report = update_radio::scan_wifi_networks(view, sdk);
    let _ = writeln!(ui.bus.log, "Newt Wi-Fi scan: error {}, heard {}, listed {}, start-ms {}, scan-ms {}, stop-ms {}, fenced-opens {}, fenced-writes {}",
        report.run.error, report.heard, view.network_count, report.run.start_ms, report.scan_ms,
        report.run.stop_ms, report.run.fenced_opens, report.run.fenced_writes);
    if !report.run.off { leave(ui, view, Some(logical), sdk, events, "Newt"); }
    ui.trace_heap("wifi-stopped")?;
    ui.paint(view, Some(logical), sdk.flash_fenced_for_wifi())?;
    Ok(())
}

fn change_network(ui: &mut ModeUi<'_>, state: &mut State, view: &mut UpdateView<'_>, logical: &mut Frame,
    sdk: &mut Sdk,
) -> Result<(), ImageError> {
    ui.paint(view, Some(logical), sdk.flash_fenced_for_wifi())?;
    let report = state.change(&mut ui.bus.board, view, sdk);
    let _ = writeln!(ui.bus.log, "Newt Wi-Fi network {}: record {}, key-written {}, key {}",
        if view.forgetting { "forget" } else { "save" }, report.record as u8,
        u8::from(report.key_written), report.key as u8);
    ui.paint(view, Some(logical), sdk.flash_fenced_for_wifi())?;
    Ok(())
}

fn connect(ui: &mut ModeUi<'_>, state: &mut State, view: &mut UpdateView<'_>, logical: &mut Frame,
    sdk: &mut Sdk, events: &mut Events<'_, '_>,
) -> Result<(), ImageError> {
    ui.paint(view, Some(logical), sdk.flash_fenced_for_wifi())?;
    ui.trace_heap("join-before")?;
    let mut network = WifiChoice::default();
    state.restore(&mut ui.bus.board, &mut network, Some(&mut view.password), sdk);
    let mut report = WifiJoinReport::not_started();
    if network.saved {
        let private = update::update_frame_private(view);
        report = ui.with_parts(|board, parts| sdk.with_join(|ports| {
            update_radio::join_wifi_network(&network, &mut view.password, ports, &mut |_| {
                crate::app_power::delay_ms(10);
                if parts.poll_ready(board, private, Some(logical), events).is_err() { crate::app_power::stop(); }
            }, None)
        }));
    }
    view.join = report.result;
    let _ = writeln!(ui.bus.log, "Newt Wi-Fi join: result {}, reason {}, error {}, start-ms {}, join-ms {}, stop-ms {}, fenced-opens {}, fenced-writes {}",
        report.result as u8, report.reason, report.run.error, report.run.start_ms, report.join_ms,
        report.run.stop_ms, report.run.fenced_opens, report.run.fenced_writes);
    if !report.run.off { leave(ui, view, Some(logical), sdk, events, "Newt"); }
    ui.trace_heap("join-stopped")?;
    ui.paint(view, Some(logical), sdk.flash_fenced_for_wifi())?;
    Ok(())
}

/// All normal/uncertain exits use this one frozen teardown. None logical
/// means TLS kept its loan: drain/status may run, but FRAME cannot expose it.
pub fn leave(ui: &mut ModeUi<'_>, view: &mut UpdateView<'_>, logical: Option<&Frame>, sdk: &mut Sdk,
    events: &mut Events<'_, '_>, into: &'static str,
) -> ! {
    let _ = sdk.clear_config(); let _ = sdk.stop_station(); let _ = sdk.deinit_station();
    view.clear_secrets(&Wipe);
    ui.drain_before_restart(view, logical, events);
    if ui.bus.board.with_retained(|mut words| {
        words.clear_sleep_magic(); words.clear_sd_resume_magic();
    }).is_err() { crate::app_power::stop(); }
    let _ = ui.bus.board.panel_sleep();
    let _ = writeln!(ui.bus.log, "Newt restarting into {}", into);
    ui.bus.log.flush();
    ui.bus.board.restart();
    crate::app_power::stop()
}
