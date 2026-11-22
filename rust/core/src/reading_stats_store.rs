// SPDX-License-Identifier: MIT
//! Hardware-neutral durable store for reading-statistics records.
//! Backend adapters own physical durability and evidence preservation.
#![forbid(unsafe_code)]
use crate::crossink_stats as ci;
use crate::durable::{Backend, BlobRead, Commit, SLOT_KEYS, StorageOpen, commit_bytes};
use crate::reading_stats::*;
use crate::sleep_stats::*;
use crate::wire::{fnv1a, get32, put32};

pub const LEDGER_NAMESPACE: &str = "newt_stats";
pub const STREAK_NAMESPACE: &str = "newt_stats_sk";
pub const CROSS_INK_NAMESPACE: &str = "newt_stats_ci";
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatsLoad {
    Loaded,
    Absent,
    Corrupt,
    Unsupported,
    Unavailable,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum StatsSave {
    Saved,
    Corrupt,
    Unsupported,
    Unavailable,
    Unconfirmed,
}
/// No production SD/NVS implementation is supplied. Methods are outside open transactions.
pub trait StatsBackend: Backend {
    fn reading_state_unavailable(&self) -> bool;
    /// Rename to a fresh .bad; successful source becomes absent.
    fn set_aside(&mut self, name: &str) -> bool;
    /// Copy to fresh .bad and rewrite slot0 without an absent interval.
    /// True only after the replacement's durability barrier is acknowledged.
    fn repair_in_place(&mut self, name: &str, payload: &[u8]) -> bool;
}
/// Only begin_effects constructs this operation-scoped admission token.
pub struct StatsEffects(());
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Namespace([u8; 16]);
impl Namespace {
    pub fn as_str(&self) -> &str {
        let end = self.0.iter().position(|&c| c == 0).unwrap_or(16);
        core::str::from_utf8(&self.0[..end]).expect("namespace is ASCII")
    }
    pub fn c_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}
const HEX: &[u8; 16] = b"0123456789abcdef";
fn digest_namespace(digest: &StatsDigest, prefix: [u8; 2]) -> Namespace {
    let mut name = [0; 16];
    name[..2].copy_from_slice(&prefix);
    for i in 0..13 {
        name[i + 2] = HEX[((digest[i / 2] >> if i % 2 == 0 { 4 } else { 0 }) & 15) as usize];
    }
    Namespace(name)
}
pub fn stats_book_namespace(digest: &StatsDigest) -> Namespace {
    digest_namespace(digest, *b"sb")
}
/// Native SHA-256 boundary; no SHA implementation lives here. Zero-initializes
/// the digest, calls `sha256`, and ignores its return flag so a provider
/// failure leaving output untouched routes to `ci000...` and partial writes
/// stay visible. Input stops at the first NUL byte.
pub fn cross_ink_source_namespace(
    name: &[u8],
    sha256: impl FnOnce(&[u8], &mut StatsDigest) -> bool,
) -> Namespace {
    let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    let mut digest = [0; 32];
    let _ = sha256(&name[..end], &mut digest);
    digest_namespace(&digest, *b"ci")
}
pub fn stats_month_namespace(month: u16) -> Namespace {
    let year = month / 12;
    let m = month % 12 + 1;
    let mut n = [0; 16];
    n[..2].copy_from_slice(b"sm");
    for i in 0..4 {
        n[5 - i] = b'0' + ((year / 10u16.pow(i as u32)) % 10) as u8;
    }
    n[6] = b'0' + (m / 10) as u8;
    n[7] = b'0' + (m % 10) as u8;
    Namespace(n)
}

pub(crate) struct Slots<const N: usize> {
    bytes: [[u8; N]; 2],
    present: bool,
    newest: i8,
}
impl<const N: usize> Default for Slots<N> {
    fn default() -> Self {
        Self {
            bytes: [[0; N]; 2],
            present: false,
            newest: -1,
        }
    }
}
fn read<B: StatsBackend, const N: usize>(
    store: &mut B,
    magic: u32,
    valid: &impl Fn(&[u8]) -> bool,
    slots: &mut Slots<N>,
) -> StatsLoad {
    let mut unsupported = false;
    let mut good = [false; 2];
    for (i, key) in SLOT_KEYS.iter().enumerate() {
        match store.read_blob(key, &mut slots.bytes[i]) {
            BlobRead::Failed => return StatsLoad::Unavailable,
            BlobRead::Missing => continue,
            BlobRead::Present(observed) => {
                slots.present = true;
                let kind = if observed == N {
                    classify_stats_record(&slots.bytes[i], magic)
                } else {
                    StatsDecode::Unsupported
                };
                unsupported |= kind == StatsDecode::Unsupported;
                good[i] = kind == StatsDecode::Valid && valid(&slots.bytes[i]);
            }
        }
    }
    if unsupported {
        return StatsLoad::Unsupported;
    }
    slots.newest = if good[0] {
        0
    } else if good[1] {
        1
    } else {
        -1
    };
    if good[0] && good[1] {
        let delta = get32(&slots.bytes[0], 8).wrapping_sub(get32(&slots.bytes[1], 8));
        slots.newest = if delta == 0x8000_0000 || (delta == 0 && slots.bytes[0] != slots.bytes[1]) {
            -2
        } else if delta < 0x8000_0000 {
            0
        } else {
            1
        };
    }
    if slots.newest >= 0 {
        StatsLoad::Loaded
    } else {
        StatsLoad::Corrupt
    }
}
pub(crate) fn load<B: StatsBackend, const N: usize>(
    store: &mut B,
    name: &str,
    magic: u32,
    valid: impl Fn(&[u8]) -> bool,
    out: impl FnOnce(&[u8]),
) -> StatsLoad {
    match store.begin(name, true) {
        StorageOpen::Damaged => return StatsLoad::Corrupt,
        StorageOpen::Missing => {
            return if store.reading_state_unavailable() {
                StatsLoad::Unavailable
            } else {
                StatsLoad::Absent
            };
        }
        StorageOpen::Opened => {}
        _ => return StatsLoad::Unavailable,
    }
    let mut slots = Slots::<N>::default();
    let result = read(store, magic, &valid, &mut slots);
    store.end();
    if result == StatsLoad::Loaded {
        out(&slots.bytes[slots.newest as usize]);
    }
    result
}
pub(crate) fn stamp<const N: usize>(bytes: &mut [u8; N], generation: u32) {
    put32(bytes, 8, generation);
    let checksum = fnv1a(&bytes[..N - 4]);
    put32(bytes, N - 4, checksum);
}
#[inline(never)]
pub(crate) fn save<B: StatsBackend, const N: usize>(
    _effects: &StatsEffects,
    store: &mut B,
    name: &str,
    magic: u32,
    valid: impl Fn(&[u8]) -> bool,
    mut encoded: [u8; N],
) -> StatsSave {
    match store.begin(name, false) {
        StorageOpen::Damaged => return StatsSave::Corrupt,
        StorageOpen::Opened => {}
        _ => return StatsSave::Unavailable,
    }
    let mut slots = Slots::<N>::default();
    let found = read(store, magic, &valid, &mut slots);
    if found == StatsLoad::Unavailable
        || found == StatsLoad::Unsupported
        || slots.newest == -2
        || (found == StatsLoad::Corrupt && slots.present)
    {
        store.end();
        return match found {
            StatsLoad::Unavailable => StatsSave::Unavailable,
            StatsLoad::Unsupported => StatsSave::Unsupported,
            _ => StatsSave::Corrupt,
        };
    }
    let generation = if slots.newest < 0 {
        1
    } else {
        get32(&slots.bytes[slots.newest as usize], 8).wrapping_add(1)
    };
    stamp(&mut encoded, generation);
    let target = usize::from(slots.newest == 0);
    let committed = commit_bytes(store, SLOT_KEYS[target], &encoded, &mut slots.bytes[target]);
    store.end();
    match committed {
        Commit::Saved => StatsSave::Saved,
        Commit::Unconfirmed => StatsSave::Unconfirmed,
    }
}
pub fn valid_ledger(bytes: &[u8]) -> bool {
    decode_ledger(bytes, &mut StatsState::default())
}
pub fn valid_month(bytes: &[u8], month: u16) -> bool {
    decode_month(bytes).is_some_and(|m| m.month == month)
}
pub fn valid_book(bytes: &[u8], digest: &StatsDigest) -> bool {
    decode_book_facts(bytes, digest).is_some()
}
pub fn valid_streak(bytes: &[u8]) -> bool {
    decode_streak(bytes).is_some()
}
pub fn valid_cross_ink_global(bytes: &[u8]) -> bool {
    ci::decode_cross_ink_global_record(bytes).is_some()
}
pub fn valid_named_source(
    bytes: &[u8],
    name: &str,
    sha256: impl FnOnce(&[u8], &mut StatsDigest) -> bool,
) -> bool {
    ci::decode_cross_ink_source(bytes)
        .is_some_and(|source| cross_ink_source_namespace(&source.name, sha256).as_str() == name)
}
pub fn valid_named_book(bytes: &[u8], name: &str) -> bool {
    let Some(raw) = bytes.get(12..44) else {
        return false;
    };
    let mut digest = [0; 32];
    digest.copy_from_slice(raw);
    stats_book_namespace(&digest).as_str() == name && valid_book(bytes, &digest)
}
/// A borrowed view. Record buffers exist only during calls; callers cannot
/// alias this exclusive backend borrow or call into it recursively.
pub struct StatsStore<'a, B: StatsBackend> {
    store: &'a mut B,
}
impl<'a, B: StatsBackend> StatsStore<'a, B> {
    pub fn new(store: &'a mut B) -> Self {
        Self { store }
    }
    pub fn store(&mut self) -> &mut B {
        self.store
    }
    pub fn begin_effects(cancelled: Option<&mut dyn FnMut() -> bool>) -> Option<StatsEffects> {
        if cancelled.is_some_and(|c| c()) {
            None
        } else {
            Some(StatsEffects(()))
        }
    }
    pub fn load_ledger(&mut self, out: &mut StatsState) -> StatsLoad {
        load::<_, LEDGER_BYTES>(
            self.store,
            LEDGER_NAMESPACE,
            LEDGER_MAGIC,
            valid_ledger,
            |b| {
                let _ = decode_ledger(b, out);
            },
        )
    }
    pub fn load_month(&mut self, month: u16, out: &mut StatsMonth) -> StatsLoad {
        load::<_, MONTH_BYTES>(
            self.store,
            stats_month_namespace(month).as_str(),
            MONTH_MAGIC,
            |b| valid_month(b, month),
            |b| *out = decode_month(b).expect("validated month"),
        )
    }
    pub fn load_book(&mut self, digest: &StatsDigest, out: &mut BookFacts) -> StatsLoad {
        load::<_, BOOK_BYTES>(
            self.store,
            stats_book_namespace(digest).as_str(),
            BOOK_MAGIC,
            |b| valid_book(b, digest),
            |b| *out = decode_book_facts(b, digest).expect("validated book"),
        )
    }
    pub fn load_named_book(
        &mut self,
        name: &str,
        digest: &mut StatsDigest,
        out: &mut BookFacts,
    ) -> StatsLoad {
        load::<_, BOOK_BYTES>(
            self.store,
            name,
            BOOK_MAGIC,
            |b| valid_named_book(b, name),
            |b| {
                digest.copy_from_slice(&b[12..44]);
                *out = decode_book_facts(b, digest).expect("validated named book");
            },
        )
    }
    #[inline(never)]
    pub fn load_cross_ink(
        &mut self,
        history: &mut CrossInkHistory,
        unreadable: Option<&mut bool>,
    ) -> bool {
        let mut copy = ci::CrossInkGlobalCopy::default();
        let loaded = self.load_cross_ink_copy(&mut copy);
        if loaded != StatsLoad::Loaded && loaded != StatsLoad::Absent {
            if let Some(flag) = unreadable {
                *flag = true;
            }
        }
        if loaded != StatsLoad::Loaded || copy.status != ci::CrossInkStatus::Imported {
            return false;
        }
        *history = ci::cross_ink_history(&copy);
        true
    }
    /// Keep the validated imported copy for the existing graph decoders.
    pub fn load_cross_ink_copy(&mut self, out: &mut ci::CrossInkGlobalCopy) -> StatsLoad {
        load::<_, { ci::GLOBAL_RECORD_BYTES }>(
            self.store,
            CROSS_INK_NAMESPACE,
            ci::GLOBAL_MAGIC,
            valid_cross_ink_global,
            |bytes| *out = ci::decode_cross_ink_global_record(bytes).expect("validated copy"),
        )
    }
    pub fn load_streak(&mut self, out: &mut StatsStreak) -> StatsLoad {
        load::<_, STREAK_BYTES>(
            self.store,
            STREAK_NAMESPACE,
            STREAK_MAGIC,
            valid_streak,
            |b| *out = decode_streak(b).expect("validated streak"),
        )
    }
    pub fn save_streak(&mut self, effects: &StatsEffects, streak: &StatsStreak) -> StatsSave {
        save(
            effects,
            self.store,
            STREAK_NAMESPACE,
            STREAK_MAGIC,
            valid_streak,
            encode_streak(streak),
        )
    }
    pub fn save_ledger(&mut self, effects: &StatsEffects, ledger: &StatsLedger) -> StatsSave {
        save(
            effects,
            self.store,
            LEDGER_NAMESPACE,
            LEDGER_MAGIC,
            valid_ledger,
            encode_ledger(ledger, 0),
        )
    }
    pub fn save_month(&mut self, effects: &StatsEffects, month: &StatsMonth) -> StatsSave {
        save(
            effects,
            self.store,
            stats_month_namespace(month.month).as_str(),
            MONTH_MAGIC,
            |b| valid_month(b, month.month),
            encode_month(month, 0),
        )
    }
    pub fn save_book(
        &mut self,
        effects: &StatsEffects,
        digest: &StatsDigest,
        facts: &BookFacts,
    ) -> StatsSave {
        save(
            effects,
            self.store,
            stats_book_namespace(digest).as_str(),
            BOOK_MAGIC,
            |b| valid_book(b, digest),
            encode_book_facts(digest, facts, 0),
        )
    }
    pub fn repair_book(
        &mut self,
        _effects: &StatsEffects,
        digest: &StatsDigest,
        facts: &BookFacts,
    ) -> bool {
        self.store.repair_in_place(
            stats_book_namespace(digest).as_str(),
            &encode_book_facts(digest, facts, 1),
        )
    }
    pub fn set_aside_streak(&mut self, _effects: &StatsEffects) -> bool {
        self.store.set_aside(STREAK_NAMESPACE)
    }
    pub fn set_aside_ledger(&mut self, _effects: &StatsEffects) -> bool {
        self.store.set_aside(LEDGER_NAMESPACE)
    }
    pub fn set_aside_month(&mut self, _effects: &StatsEffects, month: u16) -> bool {
        self.store.set_aside(stats_month_namespace(month).as_str())
    }
    pub fn book_facts(
        &mut self,
        effects: &StatsEffects,
        digest: &StatsDigest,
        facts: &mut BookFacts,
    ) -> bool {
        *facts = BookFacts::default();
        let loaded = self.load_book(digest, facts);
        if loaded == StatsLoad::Loaded || loaded == StatsLoad::Absent {
            return true;
        }
        *facts = unknown_book_facts();
        loaded == StatsLoad::Corrupt && self.repair_book(effects, digest, facts)
    }
    pub fn store_book_facts(
        &mut self,
        effects: &StatsEffects,
        digest: &StatsDigest,
        facts: &BookFacts,
    ) -> StatsSave {
        let saved = self.save_book(effects, digest, facts);
        if saved != StatsSave::Corrupt {
            return saved;
        }
        if self.repair_book(effects, digest, facts) {
            StatsSave::Saved
        } else {
            StatsSave::Unconfirmed
        }
    }
}

// Only sibling namespace formatters use this; all bytes they emit are ASCII.
pub(crate) fn namespace_from_book_bytes(bytes: [u8; 16]) -> Namespace {
    Namespace(bytes)
}
