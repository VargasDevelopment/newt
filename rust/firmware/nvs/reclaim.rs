// SPDX-License-Identifier: MIT
//! Boot-only preservation, then removal of recognized legacy Newt progress.
use super::{Inventory, NvsBlobBackend};
use super::alloc::vec::Vec;
use newt_core::{
    durable::{Backend, BlobRead, StorageOpen, SLOT_KEYS},
    external_progress_store::ExternalProgressStore,
    progress::{self, Progress, Selection},
    reading_state_namespace::numbered_namespace,
};
use newt_native_storage::{Native, reading_state::{ReadingStateRouter, StateFacts, StateMedium}};

#[derive(Default, Debug, Eq, PartialEq)]
pub struct Report {
    pub entries: u32,
    pub preserved: u16,
    pub reclaimed: u16,
    pub kept: u16,
    pub complete: bool,
}

struct Candidate {
    name: [u8; 15],
    keys: u8,
    known: bool,
    digest: [u8; 32],
    had_progress: bool,
}
impl Candidate {
    fn name(&self) -> Option<&str> { core::str::from_utf8(&self.name).ok() }
}

pub fn admitted_name(name: &str) -> bool { numbered_namespace(name, *b"nb", 13, true) }

/// No candidate means one read-only inventory and no SD calls or allocation.
/// All temporary owners are dropped before startup restores settings/reader.
#[inline(never)]
pub fn run<I: Inventory, N: Native, M: StateMedium<N>>(
    nvs: &mut I, facts: &mut StateFacts, medium: &mut M, native: &mut N,
) -> Report {
    facts.set_legacy_pending(false);
    let mut report = Report::default();
    let mut candidates: Vec<Candidate> = Vec::new();
    if !nvs.storage_ready() || nvs.visit_entries(&mut |name, key, blob| {
        report.entries += 1;
        let Ok(text) = core::str::from_utf8(name) else { return true };
        if !admitted_name(text) { return true; }
        let index = match candidates.iter().position(|entry| entry.name == name) {
            Some(index) => index,
            None => {
                if candidates.try_reserve(1).is_err() { return false; }
                let mut stored = [0; 15];
                stored.copy_from_slice(name);
                candidates.push(Candidate { name: stored, keys: 0, known: true,
                    digest: [0; 32], had_progress: false });
                candidates.len() - 1
            }
        };
        let entry = &mut candidates[index];
        let bit = match key { b"digest" => 1, b"slot0" => 2, b"slot1" => 4, _ => 0 };
        entry.known &= blob && bit != 0 && entry.keys & bit == 0;
        entry.keys |= bit;
        true
    }).is_err() {
        return report;
    }
    report.kept = candidates.len() as u16;
    candidates.retain_mut(|entry| {
        let Some((digest, had_progress)) = source(nvs, entry) else { return false };
        entry.digest = digest;
        entry.had_progress = had_progress;
        true
    });
    facts.set_legacy_pending(!candidates.is_empty());
    if !candidates.is_empty() && !medium.mount(Some(native)) { return report; }
    let mut pending = false;
    // Only admissible records can delay a seed; each verified SD copy permits
    // its own cleanup, independently of every other namespace.
    for entry in &candidates {
        let Some(name) = entry.name() else { continue };
        let preserved = ReadingStateRouter::new(NvsBlobBackend::new(nvs), facts, medium, native)
            .preserve_legacy_progress(name, &entry.digest, entry.had_progress);
        if preserved {
            report.preserved += 1;
            if erase(nvs, entry) {
                report.reclaimed += 1;
                report.kept -= 1;
            }
        } else {
            pending = true;
        }
    }
    facts.set_legacy_pending(pending);
    report.complete = report.kept == 0;
    report
}

fn source<I: Inventory>(nvs: &mut I, entry: &Candidate) -> Option<([u8; 32], bool)> {
    if !entry.known || entry.keys & 1 == 0 { return None; }
    let name = entry.name()?;
    let mut store = NvsBlobBackend::new(nvs);
    let result = if store.begin(name, true) == StorageOpen::Opened {
        read_source(&mut store, name)
    } else { None };
    store.end();
    result
}

fn read_source<B: Backend>(store: &mut B, name: &str) -> Option<([u8; 32], bool)> {
    let mut digest = [0; 32];
    if store.read_blob("digest", &mut digest) != BlobRead::Present(32) { return None; }
    let book = {
        let owner = ExternalProgressStore::new(store, Some(&digest));
        if owner.storage_namespace() != name || owner.book_id() == 0 { return None; }
        owner.book_id()
    };
    let mut slots = [[0; progress::BYTES]; 2];
    let mut lengths = [0; 2];
    for (index, key) in SLOT_KEYS.into_iter().enumerate() {
        match store.read_blob(key, &mut slots[index]) {
            BlobRead::Missing => {}
            BlobRead::Present(progress::BYTES)
                if Progress::decode(&slots[index]).is_some_and(|value| value.book == book) => {
                    lengths[index] = progress::BYTES;
                }
            _ => return None,
        }
    }
    if progress::select(&slots[0][..lengths[0]], &slots[1][..lengths[1]]) == Selection::Ambiguous {
        return None;
    }
    Some((digest, lengths != [0; 2]))
}

fn erase<I: Inventory>(nvs: &mut I, entry: &Candidate) -> bool {
    let Some(name) = entry.name() else { return false };
    let mut store = NvsBlobBackend::new(nvs);
    let mut ok = store.begin(name, false) == StorageOpen::Opened;
    // Digest last: an interrupted cleanup remains discoverable next boot.
    for key in ["slot0", "slot1", "digest"] {
        if ok { ok = store.erase_key(key); }
    }
    store.end();
    if !ok || store.begin(name, true) != StorageOpen::Opened {
        store.end();
        return false;
    }
    ok = ["digest", "slot0", "slot1"].into_iter()
        .all(|key| store.read_blob(key, &mut []) == BlobRead::Missing);
    store.end();
    ok
}
