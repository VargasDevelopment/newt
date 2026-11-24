// SPDX-License-Identifier: MIT
//! Cold component reset through the shipped Home restore, over existing Board/FAT ports.
extern crate alloc;
pub use newt_native_storage::{Directory, Error, Kind, Native, Open, Result, Stat};
#[path = "../../firmware/board_clock.rs"] pub mod board_clock;
#[path = "../../firmware/nvs/nvs.rs"] pub mod nvs;
#[path = "../../firmware/home_startup.rs"] pub mod home_startup;
#[path = "../../firmware/stats_clock.rs"] pub mod stats_clock;
#[path = "../sd_reading_state/model.rs"] pub mod model;
#[path = "../quick_start_ports/transport.rs"] pub mod transport;
pub mod flash;
use flash::Flash;
use home_startup::HomeRuntime;
use newt_c3_board::Board;
use newt_core::{preferences::{Orientation, Preferences, ViewSettings}, wire::{crc32, put32},
    progress::{Progress, Locator}, external_progress_store::ExternalProgressStore,
    durable::{Backend, Load, StorageOpen}};
use std::collections::BTreeMap;
use transport::Card;

unsafe extern "C" {
    fn fixture_fault(value: u32);
    fn checkpoint_cold_reset();
}
fn start<'a>(tokens: &'a mut [u8; 2], flash: Flash, card: Card, present: bool)
    -> HomeRuntime<'a, String, Flash, Card> {
    let [display, input] = tokens;
    // SAFETY: the existing host Board fake owns these distinct test tokens;
    // all retained/global test state is discarded before every startup.
    let (board, pending, _) = unsafe {
        checkpoint_cold_reset();
        fixture_fault(if present { 0 } else { 24 });
        Board::acquire_x3_optional_sd((display as *mut u8).cast(), (input as *mut u8).cast())
    }.unwrap();
    drop(pending);
    HomeRuntime::restore_durable(board_clock::BoardBus { board, log: String::new() }, flash, card)
}
fn legacy(flash: &mut Flash) -> (String, [u8; 32]) {
    let digest = [17; 32];
    let name = ExternalProgressStore::new(&mut nvs::NvsBlobBackend::new(flash), Some(&digest))
        .storage_namespace().to_owned();
    let progress = Progress { book: u64::from_le_bytes(digest[..8].try_into().unwrap()), sequence: 7,
        locator: Locator { spine: 2, character: 21 }, ..Progress::default() };
    flash.spaces.insert(name.clone(), BTreeMap::from([
        ("digest".into(), digest.to_vec()), ("slot0".into(), progress.encode().unwrap().to_vec()),
    ]));
    (name, digest)
}
/// Clockwise needs NWTP v3. Each row starts from durable bytes alone, as a cold boot does.
fn clockwise(tokens: &mut [u8; 2]) {
    let clockwise = ViewSettings { orientation: Orientation::LandscapeCw, ..Default::default() };
    let fallback = ViewSettings { orientation: Orientation::LandscapeCcw, ..clockwise };
    let durable = Preferences { sequence: 4, settings: clockwise }.encode().unwrap();
    let mut downgraded = durable;
    put32(&mut downgraded, 4, 2);
    let checksum = crc32(&downgraded[..20]);
    put32(&mut downgraded, 20, checksum);
    for case in ["saved", "failed-commit", "v2-header"] {
        // Room for a few 24-byte records; no legacy keys to reclaim here.
        let mut flash = Flash { free_entries: 16, ..Flash::default() };
        let bytes = if case == "v2-header" { downgraded } else { durable };
        flash.spaces.insert("newt_prefs".into(), BTreeMap::from([("slot0".into(), bytes.to_vec())]));
        let mut home = start(tokens, flash, Card::new(), true);
        if case == "v2-header" {
            assert_eq!(home.preferences.settings(), ViewSettings::default(), "clockwise under v2 is corrupt");
            assert!(!home.preferences.writable());
            continue;
        }
        assert_eq!(home.preferences.settings(), clockwise, "durable clockwise choice must survive a cold reset");
        home.nvs.fail_commit = case == "failed-commit";
        assert_eq!(home.preferences.save(&mut home.nvs, fallback), case == "saved");
        // The runtime keeps the old choice until the commit verifies.
        assert_eq!(home.preferences.settings(), if case == "saved" { fallback } else { clockwise });
        let flash = core::mem::take(&mut home.nvs);
        let card = home.card.clone();
        drop(home);
        let restored = start(tokens, flash, card, true).preferences.settings();
        if case == "saved" {
            assert_eq!(restored, fallback, "explicit fallback must replace the clockwise choice");
        } else {
            assert!([clockwise, fallback].contains(&restored));
        }
    }
}
fn main() {
    clockwise(&mut [0; 2]);
    // One row per startup invariant: no cross-product of unrelated faults.
    for case in ["first-write", "full", "near-full", "failed-commit", "missing", "corrupt", "sd-absent", "sd-readonly", "sd-sync"] {
        let mut flash = Flash { free_entries: usize::from(case == "near-full"), ..Flash::default() };
        let (name, digest) = legacy(&mut flash);
        let choice = Preferences { sequence: 4, settings: newt_core::preferences::ViewSettings {
            orientation: Orientation::Portrait, ..Default::default()
        }};
        flash.spaces.insert("newt_prefs".into(), BTreeMap::from([
            ("slot0".into(), choice.encode().unwrap().to_vec()),
        ]));
        if case == "first-write" {
            flash.spaces.remove("newt_prefs");
            flash.require_reclaim_before_write = true;
        }
        if case == "missing" { flash.spaces.remove(&name); }
        if case == "corrupt" { flash.unavailable = true; }
        let card = Card::new();
        if case == "sd-readonly" || case == "sd-sync" {
            card.0.borrow_mut().faults.push(model::Fault::new(
                if case == "sd-sync" { model::Op::Sync } else { model::Op::Open },
                1, 1000, model::Effect::Errno(if case == "sd-sync" { "EIO" } else { "EACCES" }), 0));
        }
        let legacy_before = flash.spaces.get(&name).cloned();
        let mut tokens = [0; 2];
        let mut home = start(&mut tokens, flash, card, case != "sd-absent");
        assert_eq!(home.preferences.settings(), if case == "corrupt" || case == "first-write" { Default::default() } else { choice.settings });
        let migrated = matches!(case, "first-write" | "full" | "near-full" | "failed-commit");
        if migrated {
            assert!(home.nvs.spaces[&name].is_empty());
            let mut proposed = choice.settings;
            proposed.orientation = Orientation::LandscapeCcw; // Explicit durable fallback.
            home.nvs.fail_commit = case == "failed-commit";
            assert_eq!(home.preferences.save(&mut home.nvs, proposed), case != "failed-commit");
            assert_eq!(home.preferences.settings(), if case == "failed-commit" { choice.settings } else { proposed });
        } else {
            assert_eq!(home.nvs.spaces.get(&name), legacy_before.as_ref());
            assert_eq!(home.nvs.erased, 0);
        }
        if case == "missing" { assert_eq!(home.nvs.scans, 1); }
        if case == "sd-absent" { assert_eq!((home.nvs.scans, home.nvs.legacy_opens), (0, 0)); }
        let mut found = Progress::default();
        home.with_state_store(|_, store| {
            if migrated { assert_eq!(ExternalProgressStore::new(store, Some(&digest)).load(&mut found), Load::Loaded(())); }
            else if case == "sd-absent" { assert_eq!(store.begin(&name, true), StorageOpen::Missing); store.end(); }
        });
        if migrated { assert_eq!(found.locator.character, 21); }
        if migrated {
            let flash = core::mem::take(&mut home.nvs);
            let card = home.card.clone();
            drop(home);
            card.0.borrow_mut().power();
            let restored = start(&mut tokens, flash, card, true);
            let mut fallback = choice.settings;
            fallback.orientation = Orientation::LandscapeCcw;
            if case == "failed-commit" {
                assert!([choice.settings, fallback].contains(&restored.preferences.settings()));
            } else { assert_eq!(restored.preferences.settings(), fallback); }
        }
    }
}
