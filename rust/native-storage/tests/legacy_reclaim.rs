// SPDX-License-Identifier: MIT
//! Real NVS/router/reclaim policy over the existing crashable FAT model.
extern crate alloc;
pub use newt_native_storage::*;
#[allow(dead_code)] // Reuse the complete existing FAT model, including protocol-only operations.
#[path = "../../tests/sd_reading_state/model.rs"]
mod model;
#[path = "../../firmware/nvs/nvs.rs"]
pub mod nvs;
use std::collections::BTreeMap;
use newt_core::{
    durable::{Backend, BlobRead, Load, StorageOpen},
    external_progress_store::ExternalProgressStore,
    progress::{Locator, Progress},
    reading_stats_service::{StatsResult, seed_stats},
};
use reading_state::{ReadingStateRouter, StateFacts, StateMedium};

#[path = "../../tests/legacy_nvs/flash.rs"]
mod flash;
use flash::Flash;

#[test]
fn full_and_near_full_stores_confirm_a_changed_setting_after_reclaim() {
    use newt_core::{preferences::{Preferences, Orientation}, preferences_store::PreferencesStore};
    for free_entries in [0, 1] {
        let mut flash = Flash { free_entries, ..Flash::default() };
        legacy(&mut flash, [17; 32]);
        let original = Preferences::default();
        flash.spaces.insert("newt_prefs".into(), BTreeMap::from([
            ("slot0".into(), original.encode().unwrap().to_vec()),
        ]));
        let mut choice = original;
        choice.settings.orientation = Orientation::Portrait;
        assert_ne!(choice.settings, original.settings);
        assert!(PreferencesStore::new(&mut nvs::NvsBlobBackend::new(&mut flash)).save(&mut choice).is_err());
        assert!(run(&mut flash, &mut model::Model::default(), &mut StateFacts::new()).complete);
        assert!(PreferencesStore::new(&mut nvs::NvsBlobBackend::new(&mut flash)).save(&mut choice).is_ok());
        let loaded = PreferencesStore::new(&mut nvs::NvsBlobBackend::new(&mut flash)).load().unwrap();
        assert_eq!(loaded.settings, choice.settings);
    }
}

#[test]
fn existing_newer_sd_wins_and_a_valid_ledger_is_never_reseeded() {
    let digest = [17; 32];
    let mut flash = Flash::default();
    let name = legacy(&mut flash, digest);
    let mut sd = model::Model::default();
    let mut facts = StateFacts::new();
    assert!(run(&mut flash, &mut sd, &mut facts).complete);
    {
        let mut card = Card(true);
        let mut store = ReadingStateRouter::new(nvs::NvsBlobBackend::new(&mut flash), &mut facts, &mut card, &mut sd);
        let seeded = seed_stats(&mut store, None, None);
        assert_eq!((seeded.result, seeded.seconds, seeded.pages), (StatsResult::Done, 12, 3));
    }
    let ledger = format!("/Newt/Stats/{}.nrs",
        newt_core::reading_stats_store::LEDGER_NAMESPACE.replace('_', "-"));
    let before = sd.files[&ledger].durable.clone();
    legacy(&mut flash, digest);
    legacy(&mut flash, [18; 32]); // Additional counters must not replace a seeded ledger.
    flash.spaces.get_mut(&name).unwrap().insert("slot0".into(), progress(&digest, 2, 1).encode().unwrap().to_vec());
    flash.spaces.get_mut(&name).unwrap().insert("slot1".into(), progress(&digest, 3, 2).encode().unwrap().to_vec());
    assert!(run(&mut flash, &mut sd, &mut StateFacts::new()).complete);
    let mut card = Card(true);
    let mut store = ReadingStateRouter::new(nvs::NvsBlobBackend::new(&mut flash), &mut facts, &mut card, &mut sd);
    let mut restored = Progress::default();
    assert_eq!(ExternalProgressStore::new(&mut store, Some(&digest)).load(&mut restored), Load::Loaded(()));
    assert_eq!(restored, progress(&digest, 8, 34));
    assert_eq!(seed_stats(&mut store, None, None).result, StatsResult::Done);
    drop(store);
    assert_eq!(sd.files[&ledger].durable, before);
}

struct Card(bool);
impl StateMedium<model::Model> for Card {
    const STATE: &'static str = "/Newt/State";
    const STATS: &'static str = "/Newt/Stats";
    fn release_pages_for_state(&mut self) {}
    fn mount(&mut self, _: Option<&mut model::Model>) -> bool { self.0 }
}
fn progress(digest: &[u8; 32], sequence: u32, anchor: u32) -> Progress {
    Progress { book: u64::from_le_bytes(digest[..8].try_into().unwrap()), sequence,
        locator: Locator { spine: 2, character: anchor }, active_milliseconds: 12_000,
        forward_pages: 3, qualified_sessions: 1, ..Progress::default() }
}
fn legacy(flash: &mut Flash, digest: [u8; 32]) -> String {
    let name = ExternalProgressStore::new(&mut nvs::NvsBlobBackend::new(flash), Some(&digest))
        .storage_namespace().to_owned();
    flash.spaces.insert(name.clone(), BTreeMap::from([
        ("digest".into(), digest.to_vec()),
        ("slot0".into(), progress(&digest, 7, 21).encode().unwrap().to_vec()),
        ("slot1".into(), progress(&digest, 8, 34).encode().unwrap().to_vec()),
    ]));
    name
}
fn run(flash: &mut Flash, sd: &mut model::Model, facts: &mut StateFacts) -> nvs::reclaim::Report {
    nvs::reclaim::run(flash, facts, &mut Card(true), sd)
}

#[test]
fn empty_scan_has_no_sd_io_or_record_reads() {
    for namespace in ["newt_prefs", "newt_pause", "newt_apps", "newt_reader", "newt_wifi", "phy", "other", "sb0123456789abc", "nb0123456789ab", "nb0123456789abcd", "nb0123456789ABC"] {
        assert!(!nvs::reclaim::admitted_name(namespace), "foreign namespace entered reclaim admission");
        let mut flash = Flash::default();
        flash.spaces.insert(namespace.into(), BTreeMap::from([("slot0".into(), vec![1; 32])]));
        let mut sd = model::Model::default();
        let report = run(&mut flash, &mut sd, &mut StateFacts::new());
        assert!(report.complete, "foreign namespace entered reclaim admission");
        assert_eq!((flash.scans, flash.reads, flash.erased), (1, 0, 0));
        assert!(sd.trace.is_empty());
        let untouched = flash.spaces[namespace].clone();
        legacy(&mut flash, [17; 32]);
        assert!(run(&mut flash, &mut sd, &mut StateFacts::new()).complete);
        assert_eq!(flash.spaces[namespace], untouched);
        assert!(flash.handles.is_empty());
    }

}

#[test]
fn cold_migration_preserves_both_checkpoint_identities() {
    for sequences in [(7, 8), (u32::MAX, 0)] {
        let mut flash = Flash::default();
        let digest = [17; 32];
        let name = legacy(&mut flash, digest);
        let old = progress(&digest, sequences.0, 21);
        let new = progress(&digest, sequences.1, 34);
        flash.spaces.get_mut(&name).unwrap().insert("slot0".into(), old.encode().unwrap().to_vec());
        flash.spaces.get_mut(&name).unwrap().insert("slot1".into(), new.encode().unwrap().to_vec());
        let expected = flash.spaces[&name].clone();
        let mut sd = model::Model::default();
        assert!(run(&mut flash, &mut sd, &mut StateFacts::new()).complete);
        assert!(flash.spaces[&name].is_empty());
        sd.power();
        let mut facts = StateFacts::new();
        let mut card = Card(true);
        let mut store = ReadingStateRouter::new(nvs::NvsBlobBackend::new(&mut flash), &mut facts, &mut card, &mut sd);
        assert_eq!(store.begin(&name, true), StorageOpen::Opened);
        for (key, bytes) in expected {
            let mut found = vec![0; bytes.len()];
            assert_eq!(store.read_blob(&key, &mut found), BlobRead::Present(bytes.len()));
            assert_eq!(found, bytes);
        }
        store.end();
        let mut loaded = Progress::default();
        assert_eq!(ExternalProgressStore::new(&mut store, Some(&digest)).load(&mut loaded), Load::Loaded(()));
        assert_eq!(loaded, new);
        assert_eq!(ExternalProgressStore::new(&mut store, Some(&digest)).load_checkpoint(
            &mut loaded, newt_core::sleep_intent::progress_checkpoint(&old)), Load::Loaded(()));
        assert_eq!(loaded, old);
    }
}

#[test]
fn inadmissible_sources_do_not_block_other_namespaces_or_seed() {
    for failure in ["missing-digest", "bad-slot", "unknown-key", "non-blob", "digest-name"] {
        let mut flash = Flash::default();
        let name = legacy(&mut flash, [17; 32]);
        let good = legacy(&mut flash, [18; 32]);
        match failure {
            "missing-digest" => { flash.spaces.get_mut(&name).unwrap().remove("digest"); }
            "bad-slot" => { flash.spaces.get_mut(&name).unwrap().get_mut("slot0").unwrap()[10] ^= 1; }
            "unknown-key" => { flash.spaces.get_mut(&name).unwrap().insert("other".into(), vec![1]); }
            "non-blob" => flash.non_blob = Some((name.clone(), "slot0".into())),
            "digest-name" => { flash.spaces.get_mut(&name).unwrap().get_mut("digest").unwrap()[0] ^= 1; }
            _ => unreachable!(),
        }
        let retained = flash.spaces[&name].clone();
        let mut sd = model::Model::default();
        let report = run(&mut flash, &mut sd, &mut StateFacts::new());
        assert_eq!((report.preserved, report.reclaimed, report.kept), (1, 1, 1), "{failure}");
        assert_eq!(flash.spaces[&name], retained);
        assert!(flash.spaces[&good].is_empty());
        // Another cold boot keeps only the bad record; it must not touch SD.
        sd.power();
        sd.trace.clear();
        let mut facts = StateFacts::new();
        assert_eq!(run(&mut flash, &mut sd, &mut facts).kept, 1);
        assert!(sd.trace.is_empty(), "inadmissible namespace caused SD I/O: {failure}");
        let mut card = Card(true);
        let mut store = ReadingStateRouter::new(nvs::NvsBlobBackend::new(&mut flash), &mut facts, &mut card, &mut sd);
        let seeded = seed_stats(&mut store, None, None);
        assert_eq!((seeded.result, seeded.seconds, seeded.pages), (StatsResult::Done, 12, 3));
    }
}

#[test]
fn source_and_card_failures_keep_legacy_and_block_a_partial_seed() {
    for failure in ["scan", "nvs", "sd", "collision", "readonly", "sync"] {
        let mut flash = Flash::default();
        let name = legacy(&mut flash, [17; 32]);
        let good = legacy(&mut flash, [18; 32]);
        let retained = flash.spaces[&name].clone();
        let mut sd = model::Model::default();
        if matches!(failure, "scan" | "nvs" | "collision") {
            let mut previous = Flash::default();
            let mut digest = if failure == "collision" { [17; 32] } else { [19; 32] };
            if failure == "collision" { digest[31] ^= 1; } // Same namespace; different full identity.
            let name = legacy(&mut previous, digest);
            let mut facts = StateFacts::new();
            let mut card = Card(true);
            let mut store = ReadingStateRouter::new(nvs::NvsBlobBackend::new(&mut previous), &mut facts, &mut card, &mut sd);
            assert!(store.preserve_legacy_progress(&name, &digest, true));
        }
        flash.scan_error = failure == "scan";
        flash.unavailable = failure == "nvs";
        if failure == "readonly" || failure == "sync" {
            sd.faults.push(model::Fault::new(
                if failure == "sync" { model::Op::Sync } else { model::Op::Open },
                1, 1000, model::Effect::Errno(if failure == "sync" { "EIO" } else { "EACCES" }), 0));
        }
        let mut facts = StateFacts::new();
        let report = nvs::reclaim::run(&mut flash, &mut facts, &mut Card(failure != "sd"), &mut sd);
        assert!(!report.complete, "{failure}");
        assert_eq!(flash.spaces[&name], retained, "legacy keys erased before durable SD confirmation");
        if failure == "collision" { assert!(flash.spaces[&good].is_empty()); }
        else { assert_eq!(flash.erased, 0); }
        sd.faults.clear();
        let mut card = Card(true);
        let mut store = ReadingStateRouter::new(nvs::NvsBlobBackend::new(&mut flash), &mut facts, &mut card, &mut sd);
        if failure == "scan" || failure == "nvs" {
            assert_eq!(seed_stats(&mut store, None, None).result, StatsResult::Done,
                "unavailable NVS blocked existing SD seed: {failure}");
        } else {
            assert_eq!(seed_stats(&mut store, None, None).result, StatsResult::Unavailable,
                "incomplete legacy migration allowed statistics seed: {failure}");
        }
    }
}

#[test]
fn interrupted_nvs_cleanup_retries_from_the_durable_sd_authority() {
    for (kind, boundary) in [("erase", 1), ("commit", 1), ("erase", 2), ("commit", 2), ("erase", 3), ("commit", 3)] {
        let mut flash = Flash::default();
        let digest = [17; 32];
        let name = legacy(&mut flash, digest);
        if kind == "erase" { flash.fail_erase_at = boundary; }
        else { flash.fail_commit_at = boundary; }
        let mut sd = model::Model::default();
        assert!(!run(&mut flash, &mut sd, &mut StateFacts::new()).complete);
        assert!(flash.handles.is_empty());
        sd.power();
        flash.fail_erase_at = 0;
        flash.fail_commit_at = 0;
        assert!(run(&mut flash, &mut sd, &mut StateFacts::new()).complete);
        assert!(flash.spaces[&name].is_empty());
        let mut facts = StateFacts::new();
        let mut card = Card(true);
        let mut store = ReadingStateRouter::new(nvs::NvsBlobBackend::new(&mut flash), &mut facts, &mut card, &mut sd);
        let mut restored = Progress::default();
        assert_eq!(ExternalProgressStore::new(&mut store, Some(&digest)).load(&mut restored), Load::Loaded(()));
        assert_eq!(restored, progress(&digest, 8, 34));
    }
}

#[test]
fn cuts_after_each_sd_boundary_keep_progress_and_retry_on_a_cold_boot() {
    let mut baseline_flash = Flash::default();
    legacy(&mut baseline_flash, [17; 32]);
    let mut baseline_sd = model::Model::default();
    assert!(run(&mut baseline_flash, &mut baseline_sd, &mut StateFacts::new()).complete);
    assert!(baseline_sd.steps > 0);
    for boundary in 1..=baseline_sd.steps {
        let mut flash = Flash::default();
        let digest = [17; 32];
        let name = legacy(&mut flash, digest);
        let before = flash.spaces[&name].clone();
        let mut sd = model::Model::default();
        sd.cut_after = Some(boundary);
        run(&mut flash, &mut sd, &mut StateFacts::new());
        sd.power();
        if flash.erased == 0 { assert_eq!(flash.spaces[&name], before); }
        // A new boot discards facts and cached bytes; a completed SD copy is
        // the authority if cleanup began, otherwise the legacy copy remains.
        assert!(run(&mut flash, &mut sd, &mut StateFacts::new()).complete, "boundary {boundary}");
        assert!(flash.spaces[&name].is_empty());
        let mut facts = StateFacts::new();
        let mut card = Card(true);
        let mut store = ReadingStateRouter::new(nvs::NvsBlobBackend::new(&mut flash), &mut facts, &mut card, &mut sd);
        let mut restored = Progress::default();
        let mut owner = ExternalProgressStore::new(&mut store, Some(&digest));
        assert_eq!(owner.load(&mut restored), Load::Loaded(()));
        assert_eq!(restored, progress(&digest, 8, 34));
        assert_eq!(owner.load_checkpoint(&mut restored,
            newt_core::sleep_intent::progress_checkpoint(&progress(&digest, 7, 21))), Load::Loaded(()));
        assert_eq!(restored, progress(&digest, 7, 21));
    }
}
