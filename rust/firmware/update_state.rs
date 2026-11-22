// SPDX-License-Identifier: MIT
//! Update's short credential transactions use the SAME reading-state router.
#![forbid(unsafe_code)]
use crate::{home_startup::{FirmwareCard, with_state_parts}, sdk::Sdk552,
    update_keys::NvsWifiKeyBackend, update_runtime::{self, UpdateView},
    wifi_secrets::{Scopes, Wipe}};
use newt_c3_board::Board;
use newt_core::wifi_network_store::{PLAIN_BYTES, workflow::{self, KeyRegion, NativeWipe,
    Secret, WifiChangeReport, WifiChoice, WifiRestoreReport, Work}};
use newt_native_storage::reading_state::StateFacts;

/// Only this selected boot owns these frozen store facts and the unique NVS
/// lease. It never constructs HomeRuntime or a second state-store policy.
pub struct State {
    nvs: Sdk552,
    card: FirmwareCard,
    facts: StateFacts,
}
impl State {
    pub fn new(nvs: Sdk552) -> Self { Self { nvs, card: FirmwareCard, facts: StateFacts::new() } }

    pub fn restore_clock<'sdk, C>(&mut self, board: &mut Board<'sdk>, clock: &mut newt_device_ui::home_clock::HomeClockSource<C>) -> bool {
        use newt_core::reading_stats_store::StatsBackend;
        with_state_parts(board, &mut self.nvs, &mut self.card, &mut self.facts,
            |store| { clock.restore_preferences(store); !store.reading_state_unavailable() })
    }

    pub fn restore(&mut self, board: &mut Board<'_>, choice: &mut WifiChoice,
        password: Option<&mut dyn Secret<64>>, sdk: &crate::update_native::Sdk,
    ) -> WifiRestoreReport {
        let mut cipher = sdk.cipher();
        let Self { nvs, card, facts } = self;
        nvs.with_ports(|mut card_port, mut key_port| {
            let mut keys = NvsWifiKeyBackend::new(&mut key_port);
            with_state_parts(board, &mut card_port, card, facts, |store| {
                with_work(|work| workflow::restore_wifi_network(store, &mut keys,
                    &mut cipher, &Scopes, &Wipe, choice, password, work))
            })
        })
    }

    pub fn change(&mut self, board: &mut Board<'_>, view: &mut UpdateView<'_>,
        ports: &mut crate::update_native::Sdk,
    ) -> WifiChangeReport {
        let mut cipher = ports.cipher();
        let Self { nvs, card, facts } = self;
        nvs.with_ports(|mut card_port, mut key_port| {
            let mut keys = NvsWifiKeyBackend::new(&mut key_port);
            with_state_parts(board, &mut card_port, card, facts, |store| {
                with_work(|work| update_runtime::change_network(view, store, &mut keys,
                    &mut cipher, ports, &Scopes, work))
            })
        })
    }
}

/// These are the existing workflow's temporary key/plain/readback regions,
/// not another codec or boot-held secret. The guard clears even an early exit.
struct Workspace {
    key: [u8; 32], observed: [u8; 32], readback: [u8; 32], plain: [u8; PLAIN_BYTES],
    key_size: usize, observed_size: usize,
}
impl Drop for Workspace {
    fn drop(&mut self) {
        Wipe.wipe(&mut self.key); Wipe.wipe(&mut self.observed);
        Wipe.wipe(&mut self.readback); Wipe.wipe(&mut self.plain);
        self.key_size = 0; self.observed_size = 0;
    }
}
fn with_work<R>(operation: impl FnOnce(&mut Work<'_>) -> R) -> R {
    let mut storage = Workspace { key: [0; 32], observed: [0; 32], readback: [0; 32],
        plain: [0; PLAIN_BYTES], key_size: 0, observed_size: 0 };
    operation(&mut Work {
        key: KeyRegion { bytes: &mut storage.key, length: &mut storage.key_size },
        observed: KeyRegion { bytes: &mut storage.observed, length: &mut storage.observed_size },
        readback: &mut storage.readback, plain: &mut storage.plain,
    })
}
