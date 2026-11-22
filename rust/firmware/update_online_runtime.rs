// SPDX-License-Identifier: MIT
//! Actual Check composition: saved credentials, one join, existing Core
//! release/download policy, short TLS-frame progress and Wi-Fi-off clock set.
#![forbid(unsafe_code)]
use core::{ffi::CStr, fmt::Write};
use crate::{app_image::Events, app_runtime::run_loop::ImageError,
    update_download::Download, update_https::{self, Https}, update_mode,
    update_native::Sdk, update_radio::{self, JoinDuty, WifiJoinReport},
    update_runtime::{FlashFence, UpdateView}, update_state::State, update_ui::{ModeUi, Parts}};
use newt_c3_board::{Board, Error, MonotonicClock};
use newt_core::{frame::Frame, network_clock::{self, NetworkTime}, update_card,
    update_manifest::{MAX_BYTES, UpdateManifest}, update_mode_request::UpdateModeRequest,
    update_online::{self, Clock, Http, OnlineCheck, OnlineWork, READ_BYTES, URL_BYTES},
    update_release::ReleaseJson, wifi_network_store::workflow::WifiChoice};
use newt_device_ui::update::{OnlineStatus, UpdatePage};
use newt_native_io::UpdateFiles;
use newt_sdk_heap::Fixed;

/// No retained state: all Check owners live only inside the actual operation.
pub struct Online;
impl update_mode::Online for Online {
    fn check(&mut self, ui: &mut ModeUi<'_>, state: &mut State, view: &mut UpdateView<'_>, logical: &mut Frame,
        sdk: &mut Sdk, events: &mut Events<'_, '_>,
    ) -> Result<(), ImageError> {
        ui.paint(view, Some(logical), sdk.flash_fenced_for_wifi())?;
        ui.trace_heap("online-before")?;
        let mut outcome = Outcome { result: OnlineCheck::NoJoin, manifest: UpdateManifest::default(),
            time: NetworkTime { seconds: 0, at_ms: 0 }, frame_kept: false };
        let mut network = WifiChoice::default();
        state.restore(&mut ui.bus.board, &mut network, Some(&mut view.password), sdk);
        // The inert lexical owner starts no SDK client/timer/loan. It spans
        // join cleanup so a kept frame can restart before permission returns.
        update_https::with_https(logical, |http| {
            let mut report = WifiJoinReport::not_started();
            let mut fault = None;
            if !network.saved { outcome.result = OnlineCheck::NoNetwork; }
            else {
                let private = newt_device_ui::update::update_frame_private(view);
                let mut page = Page { online: &mut view.online, percent: &mut view.percent,
                    password_on_panel: &mut view.password_on_panel, running: view.running,
                    #[cfg(feature = "dev-usb")] card_failure: &mut view.card_failure };
                report = ui.with_parts(|board, parts| sdk.with_join(|ports| {
                    update_radio::join_wifi_network_with(&network, &mut view.password, ports,
                        &mut |ports, duty| match duty {
                            JoinDuty::Wait => {
                                crate::app_power::delay_ms(10);
                                if http.with_frame(|frame| parts.poll_ready(board, private, frame.as_deref(), events)).is_err() {
                                    crate::app_power::stop();
                                }
                            }
                            JoinDuty::Online => {
                                if let Err(error) = fetch(board, parts, &mut page, http, events, &mut outcome) { fault = Some(error); }
                                // SDK deadline/client lifetime ends at the
                                // frozen local Http destructor, before teardown.
                                let _ = ports;
                            }
                        })
                }));
            }
            let _ = writeln!(ui.bus.log, "Newt update online: result {}, join {}, reason {}, error {}, join-ms {}, stop-ms {}, fenced-opens {}, fenced-writes {}",
                outcome.result as u8, report.result as u8, report.reason, report.run.error,
                report.join_ms, report.run.stop_ms, report.run.fenced_opens, report.run.fenced_writes);
            if !report.run.off {
                http.with_frame(|frame| update_mode::leave(ui, view, frame.as_deref(), sdk, events, "Newt"));
            }
            if outcome.frame_kept || http.kept() {
                if crate::update_boot::request(&mut ui.bus.board, UpdateModeRequest::Update).is_err() { crate::app_power::stop(); }
                update_mode::leave(ui, view, None, sdk, events, "Newt");
            }
            if let Some(error) = fault { return Err(error); }
            ui.trace_heap("online-stopped")?;
            view.online = outcome.result;
            // This is the actual final manifest after Core's download/abort,
            // not a copy of its parsed promise or an invented Ready offer.
            ui.bus.board.with_optional_sd_io(|_, io| {
                let mut files = UpdateFiles::new(io);
                let mut scratch = [0; MAX_BYTES + 1];
                view.offers[0] = update_card::read_newt_offer(&mut files, &mut scratch);
            });
            let floor = network_clock::network_clock_floor(CStr::from_bytes_with_nul(
                concat!(env!("NEWT_DEV_BUILD_DATE"), "\0").as_bytes()).ok());
            let now = ui.bus.board.millis();
            let clock = crate::update_network_clock::set(&mut ui.bus.board, outcome.time, floor, now);
            crate::update_network_clock::trace(&mut ui.bus.log, &clock);
            if outcome.result == OnlineCheck::Ready { view.page = UpdatePage::List; view.focus = 0; }
            http.with_frame(|frame| {
                let frame = frame.ok_or(Error::Mode)?;
                ui.paint(view, Some(frame), sdk.flash_fenced_for_wifi()).map_err(ImageError::from)
            })
        })
    }
}

/// Frozen UpdateOnlineRuntime's transient values, not another durable owner.
struct Outcome { result: OnlineCheck, manifest: UpdateManifest, time: NetworkTime, frame_kept: bool }
struct Page<'a> {
    online: &'a mut OnlineCheck,
    percent: &'a mut u8,
    password_on_panel: &'a mut bool,
    running: &'a CStr,
    #[cfg(feature = "dev-usb")] card_failure: &'a mut newt_device_ui::update::CardFailure,
}
impl Page<'_> {
    fn paint(&mut self, board: &mut Board<'_>, parts: &mut Parts<'_, '_>, frame: Option<&mut Frame>) -> Result<(), Error> {
        let status = OnlineStatus { online: *self.online, percent: *self.percent, running: self.running,
            #[cfg(feature = "dev-usb")] card_failure: self.card_failure };
        // This page is drawn during the actual addressed callback; the fence
        // is raised by the same join owner until SDK deinit proves off.
        parts.paint_online(board, &status, self.password_on_panel, frame, true)
    }
    fn progress(&mut self, board: &mut Board<'_>, parts: &mut Parts<'_, '_>, http: &mut Https<'_>,
        events: &mut Events<'_, '_>, done: u32, total: u32,
    ) -> Result<(), ImageError> {
        let percent = (u64::from(done) * 100 / u64::from(total)) as u8;
        if done == 0 {
            *self.online = OnlineCheck::Downloading; *self.percent = 0;
            if !http.kept() {
                http.with_frame(|frame| self.paint(board, parts, Some(frame.ok_or(Error::Mode)?)))?;
            }
        } else if percent / 20 != *self.percent / 20 {
            *self.percent = percent;
            // Exact frameLent=true: touch only the existing native bar.
            self.paint(board, parts, None)?;
        }
        http.with_frame(|frame| parts.poll_ready(board, false, frame.as_deref(), events))?;
        Ok(())
    }
}

#[inline(never)]
fn fetch(board: &mut Board<'_>, parts: &mut Parts<'_, '_>, page: &mut Page<'_>, http: &mut Https<'_>,
    events: &mut Events<'_, '_>, outcome: &mut Outcome,
) -> Result<(), ImageError> {
    parts.trace_heap("online-joined")?;
    *page.online = OnlineCheck::Looking;
    http.with_frame(|frame| page.paint(board, parts, Some(frame.ok_or(Error::Mode)?)))?;
    outcome.result = OnlineCheck::Unreachable;
    let Some(mut work) = Fixed::try_new_with(WorkOwner::new) else { return Ok(()); };
    let mut clock = UpdateClock(board.monotonic_clock());
    let fetched: Result<(), ImageError> = board.with_optional_sd_io(|board, io| {
        let mut card = Download::new(io);
        board.with_image_partitions(false, |slots| slots.with_hash(|slots, hash| {
            let board = slots.board();
            let running = page.running;
            outcome.result = update_online::fetch_online_update_with_http(http, &mut card, hash, &mut clock,
                &mut |http, done, total| {
                    if page.progress(board, parts, http, events, done, total).is_err() { crate::app_power::stop(); }
                }, &mut work.borrow(), Some(running), &mut outcome.manifest);
            http.close();
            #[cfg(feature = "dev-usb")]
            {
                *page.card_failure = card.failure;
                let _ = writeln!(parts.log, "Newt update mode online-card: tries {}, slowest-write-ms {}, step {}, error {}, at {}",
                    card.failure.tries, card.slowest_write_ms, card.failure.step as u8, card.failure.error, card.failure.at);
            }
            outcome.frame_kept = http.kept(); outcome.time = http.time();
            let _ = writeln!(parts.log, "Newt update mode online-frame: sessions {}, lowest {}, kept {}",
                http.sessions(), if http.sessions() == 0 { 0 } else { http.frame_lowest() }, u8::from(http.kept()));
            parts.trace_heap("online-fetched")?;
            Ok(())
        })).map_err(ImageError::from)?
    });
    // Hash then CardDownload have ended, as frozen. End the local HTTP SDK
    // lifetime before join cleanup; only the exclusive frame epoch remains.
    http.end_sessions();
    fetched
}

struct UpdateClock(MonotonicClock);
impl Clock for UpdateClock { fn millis(&mut self) -> u32 { self.0.millis() } }

/// Exact existing OnlineWork fields, allocated only for this one Check.
/// Core receives short descriptors over these regions, never another parser.
struct WorkOwner {
    url: [u8; URL_BYTES], location: [u8; URL_BYTES], release: ReleaseJson,
    manifest: [u8; MAX_BYTES], manifest_length: usize, buffer: [u8; READ_BYTES],
}
impl WorkOwner {
    fn new() -> Self { Self { url: [0; URL_BYTES], location: [0; URL_BYTES], release: ReleaseJson::default(),
        manifest: [0; MAX_BYTES], manifest_length: 0, buffer: [0; READ_BYTES] } }
    fn borrow(&mut self) -> OnlineWork<'_> {
        OnlineWork { url: &mut self.url, location: &mut self.location, release: &mut self.release,
            manifest: &mut self.manifest, manifest_length: &mut self.manifest_length, buffer: &mut self.buffer }
    }
}
