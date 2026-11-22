// SPDX-License-Identifier: MIT
//! Frozen firmware/update_wifi.h scan/start/stop policy over synchronous SDK
//! primitives. Only an update boot calls this; no radio owner is retained.
#![forbid(unsafe_code)]
use crate::{update_runtime::UpdateView, wifi_secrets::Wipe};
use newt_core::wifi_network_store::workflow::NativeWipe;
use newt_device_ui::update::{add_wifi_network, WifiScan};
#[path = "update_radio/join.rs"]
mod join;
pub use join::{JoinDuty, JoinPorts, WifiJoinReport, join_wifi_network, join_wifi_network_with, wifi_join_result};

/// Each operation names one existing SDK step. Init applies the frozen SDK
/// default config with nvs_enable=0; no method may add retries or persistence.
/// Native fence access uses the same serialized NVS guard as all flash writes.
pub trait Ports {
    fn millis(&self) -> u32;
    fn fence_flash_for_wifi(&mut self, raised: bool);
    fn silence_wifi_log(&mut self);
    fn create_default_event_loop(&mut self) -> i32;
    fn init_station_without_nvs(&mut self) -> i32;
    fn enable_usb_bbpll(&mut self);
    fn set_storage_ram(&mut self) -> i32;
    fn set_station_mode(&mut self) -> i32;
    fn start_station(&mut self) -> i32;
    fn scan_blocking(&mut self) -> i32;
    fn scan_count(&mut self, count: &mut u16) -> i32;
    /// Raw SSID/rssi/open fields from ONE wifi_ap_record_t, no table/cache.
    fn scan_record(&mut self, name: &mut [u8; 33], rssi: &mut i32, open: &mut bool) -> i32;
    fn clear_ap_list(&mut self) -> i32;
    /// esp_fill_random's complete 32-byte output, called only while started.
    fn radio_draw(&mut self, fresh: &mut [u8; 32]);
    fn stop_station(&mut self) -> i32;
    fn deinit_station(&mut self) -> i32;
    fn flash_fence_refusals(&self) -> (u32, u32);
    fn delete_default_event_loop(&mut self) -> i32;
}

/// Per-run observations replace WifiRun, not another live radio controller.
/// off=true initially is the not-started case; after init only successful
/// actual deinit establishes it and permits the flash fence to come down.
pub struct WifiRun {
    pub error: i32,
    pub off: bool,
    pub start_ms: u32,
    pub stop_ms: u32,
    pub fenced_opens: u32,
    pub fenced_writes: u32,
    event_loop: bool,
    initialized: bool,
    started: bool,
}
impl WifiRun {
    fn new() -> Self {
        Self { error: 0, off: true, start_ms: 0, stop_ms: 0, fenced_opens: 0, fenced_writes: 0,
            event_loop: false, initialized: false, started: false }
    }
    fn ok(&mut self, step: i32) -> bool { if self.error == 0 { self.error = step; } step == 0 }
    fn open(&mut self, ports: &mut impl Ports) {
        ports.fence_flash_for_wifi(true);
        ports.silence_wifi_log();
        self.event_loop = self.ok(ports.create_default_event_loop());
    }
    fn start<P: Ports>(&mut self, ports: &mut P, mut config: Option<&mut dyn FnMut(&mut P) -> i32>) {
        let at = ports.millis();
        self.initialized = self.event_loop && self.ok(ports.init_station_without_nvs());
        if self.initialized { ports.enable_usb_bbpll(); }
        self.started = self.initialized && self.ok(ports.set_storage_ram())
            && self.ok(ports.set_station_mode())
            && match config.as_mut() { Some(set) => self.ok(set(ports)), None => true }
            && self.ok(ports.start_station());
        self.start_ms = ports.millis().wrapping_sub(at);
    }
    fn stop(&mut self, ports: &mut impl Ports) {
        if self.started { self.ok(ports.stop_station()); }
        self.off = !self.initialized || self.ok(ports.deinit_station());
        (self.fenced_opens, self.fenced_writes) = ports.flash_fence_refusals();
        if self.off { ports.fence_flash_for_wifi(false); }
    }
}
pub struct WifiScanReport { pub run: WifiRun, pub heard: u32, pub scan_ms: u32 }

/// One scan; errors never authorize a save. A caller seeing !run.off must
/// leave_update_mode immediately, keeping every flash mutation fenced.
pub fn scan_wifi_networks(view: &mut UpdateView<'_>, ports: &mut impl Ports) -> WifiScanReport {
    let mut report = WifiScanReport { run: WifiRun::new(), heard: 0, scan_ms: 0 };
    view.network_count = 0;
    report.run.open(ports);
    report.run.start(ports, None);
    if report.run.started {
        let at = ports.millis();
        let mut count = 0;
        if report.run.ok(ports.scan_blocking()) && report.run.ok(ports.scan_count(&mut count)) {
            let mut name = [0; 33]; let mut rssi = 0; let mut open = false;
            while report.heard < u32::from(count) {
                // Frozen get_ap_record refusal terminates the walk without
                // replacing the first reported SDK error.
                if ports.scan_record(&mut name, &mut rssi, &mut open) != 0 { break; }
                report.heard += 1;
                add_wifi_network(view, Some(&name), rssi, open);
            }
        }
        let _ = ports.clear_ap_list();
        let mut fresh = [0; 32];
        ports.radio_draw(&mut fresh);
        view.entropy.seed.mix(&fresh);
        Wipe.wipe(&mut fresh);
        report.scan_ms = ports.millis().wrapping_sub(at);
    }
    let at = ports.millis();
    report.run.stop(ports);
    if report.run.event_loop { report.run.ok(ports.delete_default_event_loop()); }
    report.run.stop_ms = ports.millis().wrapping_sub(at);
    view.scan = if report.run.error == 0 { WifiScan::Done } else { WifiScan::Failed };
    report
}
