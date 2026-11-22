// SPDX-License-Identifier: MIT
//! Synthetic target retention only; caller owns every byte and output.
//! No native SD/NVS or SHA-256 provider is implemented by this probe.
use newt_core::{
    book_days::*,
    book_days_store::*,
    durable::{Backend, BlobRead, DurableWrite, StorageOpen},
    reading_stats::*,
    reading_stats_store::*,
    sleep_stats::StatsStreak,
};

pub const PROBE_BYTES: usize = BOOK_DAYS_BYTES;
/// One synthetic namespace at a time. Namespace isolation is tested by the
/// separate host transport, not claimed by this code-generation backend.
pub struct ProbeBackend {
    pub slots: [[u8; PROBE_BYTES]; 2],
    pub observed: [usize; 2],
    pub present: [bool; 2],
    pub open_result: StorageOpen,
    pub unavailable: bool,
    pub failed_read: bool,
    pub failed_write: bool,
    pub durable_readback: bool,
    pub repair_ok: bool,
    pub last_namespace: [u8; 16],
    pub calls: [u32; 6],
}
impl Backend for ProbeBackend {
    fn begin(&mut self, name: &str, _: bool) -> StorageOpen {
        self.calls[0] = self.calls[0].wrapping_add(1);
        self.last_namespace.fill(0);
        let count = name.len().min(15);
        self.last_namespace[..count].copy_from_slice(&name.as_bytes()[..count]);
        self.open_result
    }
    fn end(&mut self) {
        self.calls[1] = self.calls[1].wrapping_add(1);
    }
    fn read_blob(&mut self, key: &str, output: &mut [u8]) -> BlobRead {
        self.calls[2] = self.calls[2].wrapping_add(1);
        if self.failed_read {
            return BlobRead::Failed;
        }
        let slot = usize::from(key == "slot1");
        if !self.present[slot] {
            return BlobRead::Missing;
        }
        let count = output.len().min(self.observed[slot]).min(PROBE_BYTES);
        output[..count].copy_from_slice(&self.slots[slot][..count]);
        BlobRead::Present(self.observed[slot])
    }
    fn put_bytes(&mut self, _: DurableWrite, key: &str, bytes: &[u8]) -> usize {
        self.calls[3] = self.calls[3].wrapping_add(1);
        if bytes.len() > PROBE_BYTES {
            return 0;
        }
        let slot = usize::from(key == "slot1");
        self.slots[slot][..bytes.len()].copy_from_slice(bytes);
        self.observed[slot] = bytes.len();
        self.present[slot] = true;
        if self.failed_write { 0 } else { bytes.len() }
    }
    fn readback_durable(&self) -> bool {
        self.durable_readback
    }
}
impl StatsBackend for ProbeBackend {
    fn reading_state_unavailable(&self) -> bool {
        self.unavailable
    }
    fn set_aside(&mut self, _: &str) -> bool {
        self.calls[4] = self.calls[4].wrapping_add(1);
        if !self.repair_ok {
            return false;
        }
        self.present = [false; 2];
        true
    }
    fn repair_in_place(&mut self, _: &str, bytes: &[u8]) -> bool {
        self.calls[5] = self.calls[5].wrapping_add(1);
        if !self.repair_ok || bytes.len() > PROBE_BYTES {
            return false;
        }
        self.slots[0][..bytes.len()].copy_from_slice(bytes);
        self.observed = [bytes.len(), 0];
        self.present = [true, false];
        true
    }
}

pub struct Values {
    pub ledger: StatsState,
    pub month: StatsMonth,
    pub book: BookFacts,
    pub streak: StatsStreak,
    pub head: BookDays,
    pub archive: BookDayBlock,
    pub found_digest: StatsDigest,
    pub history: CrossInkHistory,
    pub unreadable: bool,
}

/// Runtime dispatch prevents preceding scripted writes from fixing a later
/// schema's input. Observed extents and record bytes both remain caller inputs.
#[unsafe(no_mangle)]
pub fn newt_rust_stats_store_load_probe(
    backend: &mut ProbeBackend,
    operation: u8,
    digest: &StatsDigest,
    month: u16,
    block: u16,
    name: &str,
    out: &mut Values,
) -> u8 {
    let mut store = StatsStore::new(backend);
    match operation {
        0 => store.load_ledger(&mut out.ledger) as u8,
        1 => store.load_month(month, &mut out.month) as u8,
        2 => store.load_book(digest, &mut out.book) as u8,
        3 => store.load_streak(&mut out.streak) as u8,
        4 => load_book_days(&mut store, digest, &mut out.head) as u8,
        5 => load_day_block(&mut store, digest, block, &mut out.archive) as u8,
        6 => store.load_named_book(name, &mut out.found_digest, &mut out.book) as u8,
        _ => u8::from(store.load_cross_ink(&mut out.history, Some(&mut out.unreadable))),
    }
}
#[unsafe(no_mangle)]
pub fn newt_rust_stats_store_save_probe(
    backend: &mut ProbeBackend,
    operation: u8,
    cancelled: bool,
    digest: &StatsDigest,
    input: &Values,
) -> u8 {
    let mut cancel = || cancelled;
    let Some(effects) = StatsStore::<ProbeBackend>::begin_effects(Some(&mut cancel)) else {
        return 255;
    };
    let mut store = StatsStore::new(backend);
    match operation {
        0 => input
            .ledger
            .seeded()
            .map_or(StatsSave::Unavailable, |ledger| {
                store.save_ledger(&effects, ledger)
            }) as u8,
        1 => store.save_month(&effects, &input.month) as u8,
        2 => store.save_book(&effects, digest, &input.book) as u8,
        3 => store.save_streak(&effects, &input.streak) as u8,
        4 => save_book_days(&effects, &mut store, digest, &input.head) as u8,
        _ => save_day_block(&effects, &mut store, digest, &input.archive) as u8,
    }
}
#[unsafe(no_mangle)]
pub fn newt_rust_stats_store_repair_probe(
    backend: &mut ProbeBackend,
    operation: u8,
    cancelled: bool,
    digest: &StatsDigest,
    month: u16,
    block: u16,
    facts: &mut BookFacts,
) -> u8 {
    let mut cancel = || cancelled;
    let Some(effects) = StatsStore::<ProbeBackend>::begin_effects(Some(&mut cancel)) else {
        return 255;
    };
    let mut store = StatsStore::new(backend);
    match operation {
        0 => u8::from(store.set_aside_ledger(&effects)),
        1 => u8::from(store.set_aside_month(&effects, month)),
        2 => u8::from(store.set_aside_streak(&effects)),
        3 => u8::from(set_aside_book_days(&effects, &mut store, digest)),
        4 => u8::from(set_aside_day_block(&effects, &mut store, digest, block)),
        5 => u8::from(store.repair_book(&effects, digest, facts)),
        6 => u8::from(store.book_facts(&effects, digest, facts)),
        _ => store.store_book_facts(&effects, digest, facts) as u8,
    }
}
/// Return value reports admission only; inspect caller-owned backend effects.
#[unsafe(no_mangle)]
pub fn newt_rust_stats_store_fold_probe(
    backend: &mut ProbeBackend,
    cancelled: bool,
    digest: &StatsDigest,
    before: &StatsCounters,
    fold: &StatsFold,
) -> bool {
    let mut cancel = || cancelled;
    let Some(effects) = StatsStore::<ProbeBackend>::begin_effects(Some(&mut cancel)) else {
        return false;
    };
    fold_stored_book_days(
        &mut StatsStore::new(backend),
        &effects,
        digest,
        before,
        fold,
    );
    true
}
#[unsafe(no_mangle)]
pub fn newt_rust_stats_store_names_probe(
    digest: &StatsDigest,
    month: u16,
    block: u16,
    source: &[u8],
    source_name: &str,
    record: &[u8],
    provided_sha: &StatsDigest,
    sha_ok: bool,
    names: &mut [[u8; 16]; 5],
    valid: &mut [bool; 2],
) {
    let provider = |_: &[u8], output: &mut StatsDigest| {
        if sha_ok {
            *output = *provided_sha;
        }
        sha_ok
    };
    names[0] = *stats_book_namespace(digest).c_bytes();
    names[1] = *stats_month_namespace(month).c_bytes();
    names[2] = *book_days_namespace(digest).c_bytes();
    names[3] = *book_archive_namespace(digest, block).c_bytes();
    names[4] = *cross_ink_source_namespace(source, provider).c_bytes();
    valid[0] = valid_named_source(record, source_name, provider);
    valid[1] = valid_named_book(record, source_name);
}
