// SPDX-License-Identifier: MIT
//! update_wifi.h::joinWifiNetwork over the same per-run start/stop policy.
use super::{Ports, WifiRun};
use crate::wifi_secrets::{SecretBytes, Wipe};
use newt_core::wifi_network_store::workflow::{Secret, WifiChoice};
use newt_device_ui::update::WifiJoin;

pub const JOIN_TIMEOUT_MS: u32 = 20_000;

/// Root lends exactly one native WifiJoinEvents/config/default-station scope.
/// Observers record CONNECTED, GOT_IP and DISCONNECTED (zero reason becomes
/// one as frozen); they retain no UI/secret reference and unregister before
/// scope release. Individual loads preserve the frozen short-circuit order.
pub trait JoinPorts: Ports {
    fn netif_init(&mut self) -> i32;
    fn create_default_station(&mut self) -> bool;
    fn register_wifi_observer(&mut self) -> i32;
    fn register_ip_observer(&mut self) -> i32;
    fn prepare_config(&mut self, name: &[u8; 33], password: &[u8; 64], size: usize);
    fn set_prepared_config(&mut self) -> i32;
    fn wipe_prepared_config(&mut self);
    fn connect_station(&mut self) -> i32;
    fn addressed(&self) -> bool;
    fn disconnect_reason(&self) -> u16;
    fn connected(&self) -> bool;
    fn disconnect_station(&mut self) -> i32;
    fn destroy_default_station(&mut self);
    fn unregister_ip_observer(&mut self) -> i32;
    fn unregister_wifi_observer(&mut self) -> i32;
}

/// Values are pinned IDF esp_wifi_types_generic.h constants used by the
/// frozen firmware, not heuristic classification of an unavailable result.
pub fn wifi_join_result(reason: u16) -> WifiJoin {
    match reason {
        14 | 15 | 23 | 202 | 204 => WifiJoin::WrongPassword,
        201 | 210 | 211 | 212 => WifiJoin::NotFound,
        _ => WifiJoin::Failed,
    }
}
pub struct WifiJoinReport { pub run: WifiRun, pub result: WifiJoin, pub reason: u16, pub join_ms: u32 }
impl WifiJoinReport {
    /// The frozen report before any station attempt (no restored network).
    /// off is true because no SDK initialization has occurred, not a guess
    /// about a station whose teardown failed.
    pub fn not_started() -> Self {
        Self { run: WifiRun::new(), result: WifiJoin::Failed, reason: 0, join_ms: 0 }
    }
}

/// No saved-network/cipher policy lives here. Caller restores that authority
/// through Core into this same password first. The optional online operation
/// runs once with the real address and returns before any disconnect/cleanup.
pub fn join_wifi_network<P: JoinPorts>(network: &WifiChoice, password: &mut SecretBytes<64>,
    ports: &mut P, wait: &mut dyn FnMut(&mut P), mut online: Option<&mut dyn FnMut(&mut P)>,
) -> WifiJoinReport {
    join_wifi_network_with(network, password, ports, &mut |ports, duty| match duty {
        JoinDuty::Wait => wait(ports),
        JoinDuty::Online => if let Some(online) = online.as_mut() { online(ports); },
    })
}

pub enum JoinDuty { Wait, Online }

/// One required sequential callback may borrow the same UI/frame for waiting
/// and addressed work. The legacy two-callback API above shares this body.
pub fn join_wifi_network_with<P: JoinPorts>(network: &WifiChoice, password: &mut SecretBytes<64>,
    ports: &mut P, work: &mut dyn FnMut(&mut P, JoinDuty),
) -> WifiJoinReport {
    let mut report = WifiJoinReport::not_started();
    report.run.open(ports);
    let netif = report.run.event_loop && report.run.ok(ports.netif_init());
    let station = netif && ports.create_default_station();
    // ESP_FAIL=-1, pinned esp_err.h: failed actual station allocation.
    if netif && !station { report.run.ok(-1); }
    let heard = station && report.run.ok(ports.register_wifi_observer())
        && report.run.ok(ports.register_ip_observer());
    ports.prepare_config(&network.name, password.bytes(), password.size());
    password.clear(&Wipe);
    if heard { report.run.start(ports, Some(&mut |ports: &mut P| ports.set_prepared_config())); }
    ports.wipe_prepared_config();
    let at = ports.millis();
    if report.run.started && report.run.ok(ports.connect_station()) {
        while !ports.addressed() && ports.disconnect_reason() == 0
            && ports.millis().wrapping_sub(at) < JOIN_TIMEOUT_MS { work(ports, JoinDuty::Wait); }
        report.reason = ports.disconnect_reason();
        report.result = if ports.addressed() { WifiJoin::Connected }
            else if ports.disconnect_reason() != 0 { wifi_join_result(ports.disconnect_reason()) }
            else if ports.connected() { WifiJoin::NoAddress } else { WifiJoin::TimedOut };
    }
    report.join_ms = ports.millis().wrapping_sub(at);
    if report.result == WifiJoin::Connected {
        work(ports, JoinDuty::Online);
    }
    let at = ports.millis();
    if report.run.started { report.run.ok(ports.disconnect_station()); }
    // The same config was wiped, so this writes an actual empty config.
    if report.run.initialized { report.run.ok(ports.set_prepared_config()); }
    report.run.stop(ports);
    if station { ports.destroy_default_station(); }
    if report.run.event_loop {
        let _ = ports.unregister_ip_observer();
        let _ = ports.unregister_wifi_observer();
        report.run.ok(ports.delete_default_event_loop());
    }
    report.run.stop_ms = ports.millis().wrapping_sub(at);
    report
}
