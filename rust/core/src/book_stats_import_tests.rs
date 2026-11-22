// SPDX-License-Identifier: MIT
//! Synthetic storage exercises the reader helper, not physical durability.
extern crate std;
use super::*;
use crate::book_days::{BOOK_DAYS_BYTES, encode_book_days};
use crate::book_days_store::book_days_namespace;
use crate::durable::{Backend, BlobRead, DurableWrite, StorageOpen};
use crate::reading_stats_store::{cross_ink_source_namespace, stamp};
use std::collections::BTreeMap;
use std::string::{String, ToString};
use std::vec::Vec;

#[derive(Default)]
struct Medium {
    records: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
    order: Vec<String>,
    current: String,
    failed: Option<String>,
    listed: usize,
    list_fail: bool,
    puts: usize,
    aside: usize,
    aside_fail: bool,
    unconfirmed: bool,
}
impl Medium {
    fn inject(&mut self, name: &str, key: &str, bytes: &[u8]) {
        if !self.records.contains_key(name) {
            self.order.push(name.to_string());
        }
        self.records.entry(name.to_string()).or_default().insert(key.to_string(), bytes.to_vec());
    }
    fn head(&mut self, digest: &StatsDigest) -> BookDays {
        let mut days = BookDays::default();
        assert_eq!(load_book_days(&mut StatsStore::new(self), digest, &mut days), StatsLoad::Loaded);
        days
    }
}
impl Backend for Medium {
    fn begin(&mut self, name: &str, readonly: bool) -> StorageOpen {
        self.current = name.to_string();
        if self.failed.as_deref() == Some(name) {
            return StorageOpen::Failed;
        }
        if readonly && !self.records.contains_key(name) {
            return StorageOpen::Missing;
        }
        self.records.entry(name.to_string()).or_default();
        StorageOpen::Opened
    }
    fn end(&mut self) {}
    fn read_blob(&mut self, key: &str, output: &mut [u8]) -> BlobRead {
        let Some(bytes) = self.records.get(&self.current).and_then(|slots| slots.get(key)) else {
            return BlobRead::Missing;
        };
        let size = bytes.len().min(output.len());
        output[..size].copy_from_slice(&bytes[..size]);
        BlobRead::Present(bytes.len())
    }
    fn put_bytes(&mut self, _: DurableWrite, key: &str, bytes: &[u8]) -> usize {
        self.puts += 1;
        self.records.get_mut(&self.current).unwrap().insert(key.to_string(), bytes.to_vec());
        if self.unconfirmed { 0 } else { bytes.len() }
    }
    fn readback_durable(&self) -> bool { !self.unconfirmed }
}
impl StatsBackend for Medium {
    fn reading_state_unavailable(&self) -> bool { false }
    fn set_aside(&mut self, name: &str) -> bool {
        self.aside += 1;
        if self.aside_fail { return false; }
        self.records.remove(name);
        true
    }
    fn repair_in_place(&mut self, _: &str, _: &[u8]) -> bool {
        panic!("reader import never repairs in place")
    }
}
impl NamespaceBackend for Medium {
    fn for_each_namespace(&mut self, prefix: &str, visitor: &mut dyn FnMut(&mut Self, &str) -> bool) -> bool {
        self.listed += 1;
        if self.list_fail { return false; }
        for name in self.order.clone() {
            if name.starts_with(prefix) && !visitor(self, &name) { return false; }
        }
        true
    }
}

// Injected provider keeps source-name validation observable without adding crypto.
fn source_hash(name: &[u8], output: &mut StatsDigest) -> bool {
    *output = [0; 32];
    output[..name.len()].copy_from_slice(name);
    true
}
fn source(name: &[u8], digest: StatsDigest, binding: ci::CrossInkBinding, seconds: u32) -> ci::CrossInkSource {
    let mut source = ci::CrossInkSource {
        binding,
        digest,
        copy: ci::CrossInkCopy {
            status: ci::CrossInkStatus::Imported,
            candidate: ci::CrossInkCandidate::Primary,
            size: 11,
            ..ci::CrossInkCopy::default()
        },
        ..ci::CrossInkSource::default()
    };
    source.name[..name.len()].copy_from_slice(name);
    source.copy.bytes[0] = 1;
    source.copy.bytes[1..3].copy_from_slice(&2u16.to_le_bytes());
    source.copy.bytes[3..7].copy_from_slice(&seconds.to_le_bytes());
    source.copy.bytes[7..11].copy_from_slice(&3u32.to_le_bytes());
    source
}
fn inject_source(store: &mut Medium, source: &ci::CrossInkSource) -> String {
    let name = cross_ink_source_namespace(&source.name, source_hash).as_str().to_string();
    store.inject(&name, "slot0", &ci::encode_cross_ink_source(source).unwrap());
    name
}
fn import(store: &mut Medium, digest: &StatsDigest, wrote: &mut bool) -> StatsResult {
    import_book_cross_ink(store, digest, &mut || false, &source_hash, wrote)
}

#[test]
fn frozen_requires_a_valid_supported_marker_and_actual_load() {
    let mut store = Medium::default();
    assert!(!cross_ink_import_frozen(&mut store));
    let mut bytes = ci::encode_import(&ci::ImportRecord::default());
    store.inject("newt_import", "slot0", &bytes);
    assert!(cross_ink_import_frozen(&mut store));
    bytes[48] = 1;
    stamp(&mut bytes, 0);
    store.inject("newt_import", "slot0", &bytes);
    assert!(!cross_ink_import_frozen(&mut store));
    bytes = ci::encode_import(&ci::ImportRecord::default());
    bytes[4] = 2;
    stamp(&mut bytes, 0);
    store.inject("newt_import", "slot0", &bytes);
    assert!(!cross_ink_import_frozen(&mut store));
    store.inject("newt_import", "slot0", &ci::encode_import(&ci::ImportRecord::default()));
    store.failed = Some("newt_import".to_string());
    assert!(!cross_ink_import_frozen(&mut store));
    assert_eq!((store.puts, store.aside, store.listed), (0, 0, 0));
}

#[test]
fn sums_every_matching_bound_source_and_preserves_native_days() {
    let digest = [7; 32];
    let mut store = Medium::default();
    let mut before = BookDays::default();
    before.since_day = crate::reading_stats::FIRST_DAY;
    before.current.block = crate::book_days::book_block(before.since_day);
    before.period_seconds[2] = 37;
    before.imported.seconds = 999;
    store.inject(book_days_namespace(&digest).as_str(), "slot0", &encode_book_days(&digest, &before, 0));
    inject_source(&mut store, &source(b"a_1", digest, ci::CrossInkBinding::Manifest, 11));
    inject_source(&mut store, &source(b"b_2", digest, ci::CrossInkBinding::Unverified, 23));
    inject_source(&mut store, &source(b"c_3", [8; 32], ci::CrossInkBinding::Manifest, 100));
    inject_source(&mut store, &source(b"d_4", [0; 32], ci::CrossInkBinding::None, 200));
    // A source stored under a different namespace must not contribute.
    store.inject("ci_wrong", "slot0", &ci::encode_cross_ink_source(&source(b"e_5", digest, ci::CrossInkBinding::Manifest, 300)).unwrap());
    let mut wrote = false;
    assert_eq!(import(&mut store, &digest, &mut wrote), StatsResult::Done);
    assert!(wrote);
    let mut expected = before;
    expected.flags = BOOK_IMPORT_CHECKED;
    expected.imported = BookImported { seconds: 34, pages: 6, sessions: 4, periods: [0; 4] };
    assert_eq!(store.head(&digest), expected);
    assert_eq!(store.puts, 1);
    store.list_fail = true;
    wrote = false;
    assert_eq!(import(&mut store, &digest, &mut wrote), StatsResult::Done);
    assert!(!wrote);
    assert_eq!((store.listed, store.puts), (1, 1));
    // The out parameter is not reset by a no-op.
    wrote = true;
    assert_eq!(import(&mut store, &digest, &mut wrote), StatsResult::Done);
    assert!(wrote);
}

#[test]
fn newer_heads_are_done_and_unavailable_heads_do_not_scan() {
    let digest = [9; 32];
    let mut store = Medium::default();
    let name = book_days_namespace(&digest);
    let mut newer = encode_book_days(&digest, &BookDays::default(), 0);
    newer[4] = 2;
    stamp(&mut newer, 0);
    store.inject(name.as_str(), "slot0", &newer);
    let mut wrote = false;
    assert_eq!(import(&mut store, &digest, &mut wrote), StatsResult::Done);
    store.failed = Some(name.as_str().to_string());
    assert_eq!(import(&mut store, &digest, &mut wrote), StatsResult::Unavailable);
    assert!(!wrote);
    assert_eq!((store.listed, store.puts), (0, 0));
}

#[test]
fn cancellation_stops_before_source_read_or_before_write() {
    let digest = [1; 32];
    for cancel_at in [1, 2] {
        let mut store = Medium::default();
        let name = inject_source(&mut store, &source(b"a_1", digest, ci::CrossInkBinding::Manifest, 3));
        let mut calls = 0;
        let mut wrote = false;
        let result = import_book_cross_ink(&mut store, &digest, &mut || { calls += 1; calls == cancel_at }, &source_hash, &mut wrote);
        assert_eq!(result, StatsResult::Cancelled);
        assert!(!wrote);
        assert_eq!((store.puts, store.aside), (0, 0));
        if cancel_at == 1 { assert_ne!(store.current, name); }
    }
}

#[test]
fn corrupt_heads_are_set_aside_only_after_admission() {
    let digest = [2; 32];
    let name = book_days_namespace(&digest);
    for aside_fail in [false, true] {
        let mut store = Medium { aside_fail, ..Medium::default() };
        store.inject(name.as_str(), "slot0", &[0; BOOK_DAYS_BYTES]);
        let mut wrote = false;
        assert_eq!(import(&mut store, &digest, &mut wrote), if aside_fail { StatsResult::Unavailable } else { StatsResult::Done });
        assert!(wrote);
        assert_eq!(store.aside, 1);
        assert_eq!(store.puts, usize::from(!aside_fail));
        if !aside_fail { assert_eq!(store.head(&digest).flags, BOOK_IMPORT_CHECKED); }
    }
}

#[test]
fn source_unavailable_or_list_failure_aborts_but_newer_source_is_skipped() {
    let digest = [3; 32];
    let mut store = Medium::default();
    let name = inject_source(&mut store, &source(b"a_1", digest, ci::CrossInkBinding::Manifest, 3));
    store.failed = Some(name.clone());
    let mut wrote = false;
    assert_eq!(import(&mut store, &digest, &mut wrote), StatsResult::Unavailable);
    store.failed = None;
    store.list_fail = true;
    assert_eq!(import(&mut store, &digest, &mut wrote), StatsResult::Unavailable);
    assert!(!wrote);
    assert_eq!(store.puts, 0);
    store.list_fail = false;
    let mut bytes = ci::encode_cross_ink_source(&source(b"a_1", digest, ci::CrossInkBinding::Manifest, 3)).unwrap();
    bytes[4] = 2;
    stamp(&mut bytes, 0);
    store.inject(&name, "slot0", &bytes);
    assert_eq!(import(&mut store, &digest, &mut wrote), StatsResult::Done);
    assert!(wrote);
    assert_eq!(store.head(&digest).imported, BookImported::default());
}

#[test]
fn unconfirmed_save_reports_attempt_without_retrying_the_service() {
    let digest = [4; 32];
    let mut store = Medium { unconfirmed: true, ..Medium::default() };
    let mut wrote = false;
    assert_eq!(import(&mut store, &digest, &mut wrote), StatsResult::Unconfirmed);
    assert!(wrote);
    assert_eq!(store.puts, 2, "only the shared durable primitive's bounded retry");
}
