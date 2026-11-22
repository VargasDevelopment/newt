// SPDX-License-Identifier: MIT
//! Synthetic policy evidence only. Native syscall/SHA parity is a separate gate.
extern crate std;
use super::io::read_file;
use super::*;
use crate::durable::{Backend, BlobRead, DurableWrite, StorageOpen};
use crate::reading_stats::StatsDigest;
use crate::reading_stats_store::{STREAK_NAMESPACE, stamp, valid_named_source};
use crate::sleep_stats::encode_streak;
use std::collections::{BTreeMap, VecDeque};
use std::ffi::CString;
use std::string::{String, ToString};
use std::vec;
use std::vec::Vec;

#[derive(Default)]
struct Medium {
    records: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
    current: String,
    writes: Vec<String>,
    failed: Option<String>,
    cut_after: Option<usize>,
    unconfirmed: bool,
    aside: usize,
}
impl Medium {
    fn inject(&mut self, name: &str, bytes: &[u8]) {
        self.records
            .entry(name.to_string())
            .or_default()
            .insert("slot0".to_string(), bytes.to_vec());
    }
    fn source(&mut self, name: &[u8]) -> ci::CrossInkSource {
        let ns = cross_ink_source_namespace(name, namespace_hash);
        let mut source = None;
        assert_eq!(
            load::<_, { ci::SOURCE_RECORD_BYTES }>(
                self,
                ns.as_str(),
                ci::SOURCE_MAGIC,
                |b| valid_named_source(b, ns.as_str(), namespace_hash),
                |b| source = ci::decode_cross_ink_source(b)
            ),
            StatsLoad::Loaded
        );
        source.unwrap()
    }
}
impl Backend for Medium {
    fn begin(&mut self, name: &str, readonly: bool) -> StorageOpen {
        self.current = name.to_string();
        if self.failed.as_deref() == Some(name)
            || self
                .cut_after
                .is_some_and(|limit| self.writes.len() >= limit)
        {
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
        let Some(bytes) = self
            .records
            .get(&self.current)
            .and_then(|record| record.get(key))
        else {
            return BlobRead::Missing;
        };
        let size = bytes.len().min(output.len());
        output[..size].copy_from_slice(&bytes[..size]);
        BlobRead::Present(bytes.len())
    }
    fn put_bytes(&mut self, _: DurableWrite, key: &str, bytes: &[u8]) -> usize {
        self.writes.push(self.current.clone());
        self.records
            .entry(self.current.clone())
            .or_default()
            .insert(key.to_string(), bytes.to_vec());
        if self.unconfirmed { 0 } else { bytes.len() }
    }
    fn readback_durable(&self) -> bool {
        !self.unconfirmed
    }
}
impl StatsBackend for Medium {
    fn reading_state_unavailable(&self) -> bool {
        false
    }
    fn set_aside(&mut self, name: &str) -> bool {
        self.aside += 1;
        self.records.remove(name);
        true
    }
    fn repair_in_place(&mut self, _: &str, _: &[u8]) -> bool {
        panic!("import uses set-aside, never in-place repair")
    }
}
#[derive(Clone)]
struct TestEntry {
    name: CString,
    kind: EntryKind,
}
struct Directory {
    entries: Vec<TestEntry>,
    at: usize,
}
struct File {
    bytes: Vec<u8>,
    size: u64,
    at: usize,
}
#[derive(Default)]
struct Card {
    files: BTreeMap<Vec<u8>, (Vec<u8>, u64)>,
    directories: BTreeMap<Vec<u8>, Vec<TestEntry>>,
    failed: Option<Vec<u8>>,
    read_faults: VecDeque<Result<usize, IoError>>,
    stat_fail: bool,
    next_fail: bool,
    read_limit: Option<usize>,
    reads: usize,
    opens: usize,
    closes: usize,
    directory_opens: usize,
    directory_closes: usize,
    close_fail: bool,
}
impl Card {
    fn directory(&mut self, path: &[u8]) {
        if self.directories.contains_key(path) {
            return;
        }
        self.directories.insert(path.to_vec(), Vec::new());
        if let Some(slash) = path.iter().rposition(|&b| b == b'/') {
            if slash == 0 {
                return;
            }
            self.directory(&path[..slash]);
            self.directories
                .get_mut(&path[..slash])
                .unwrap()
                .push(TestEntry {
                    name: CString::new(&path[slash + 1..]).unwrap(),
                    kind: EntryKind::Directory,
                });
        }
    }
    fn file(&mut self, path: &[u8], bytes: &[u8]) {
        let slash = path.iter().rposition(|&b| b == b'/').unwrap();
        self.directory(&path[..slash]);
        if !self.files.contains_key(path) {
            self.directories
                .get_mut(&path[..slash])
                .unwrap()
                .push(TestEntry {
                    name: CString::new(&path[slash + 1..]).unwrap(),
                    kind: EntryKind::Other,
                });
        }
        self.files
            .insert(path.to_vec(), (bytes.to_vec(), bytes.len() as u64));
    }
    fn source(&mut self, name: &[u8], file: &[u8], bytes: &[u8]) {
        let mut path = b"/card/.crosspoint/".to_vec();
        path.extend_from_slice(name);
        path.push(b'/');
        path.extend_from_slice(file);
        self.file(&path, bytes);
    }
}
impl Files for Card {
    type File = File;
    type Directory = Directory;
    fn open(&mut self, path: &CStr) -> Result<File, IoError> {
        if self.failed.as_deref() == Some(path.to_bytes()) {
            return Err(IoError::Failed);
        }
        let Some((bytes, size)) = self.files.get(path.to_bytes()) else {
            return Err(IoError::Missing);
        };
        self.opens += 1;
        Ok(File {
            bytes: bytes.clone(),
            size: *size,
            at: 0,
        })
    }
    fn size(&mut self, file: &mut File) -> Result<u64, IoError> {
        if self.stat_fail {
            Err(IoError::Failed)
        } else {
            Ok(file.size)
        }
    }
    fn read(&mut self, file: &mut File, output: &mut [u8]) -> Result<usize, IoError> {
        self.reads += 1;
        if let Some(fault) = self.read_faults.pop_front() {
            return fault;
        }
        let n = output
            .len()
            .min(file.bytes.len() - file.at)
            .min(self.read_limit.unwrap_or(usize::MAX));
        output[..n].copy_from_slice(&file.bytes[file.at..file.at + n]);
        file.at += n;
        Ok(n)
    }
    fn close(&mut self, _: File) -> Result<(), IoError> {
        self.closes += 1;
        if self.close_fail {
            Err(IoError::Failed)
        } else {
            Ok(())
        }
    }
    fn open_directory(&mut self, path: &CStr) -> Result<Directory, IoError> {
        if self.failed.as_deref() == Some(path.to_bytes()) {
            return Err(IoError::Failed);
        }
        let entries = self
            .directories
            .get(path.to_bytes())
            .ok_or(IoError::Missing)?
            .clone();
        self.directory_opens += 1;
        Ok(Directory { entries, at: 0 })
    }
    fn next<'dir>(
        &mut self,
        directory: &'dir mut Directory,
    ) -> Result<Option<Entry<'dir>>, IoError> {
        if self.next_fail {
            return Err(IoError::Failed);
        }
        let entry = directory.entries.get(directory.at);
        directory.at += 1;
        Ok(entry.map(|entry| Entry {
            name: &entry.name,
            kind: entry.kind,
        }))
    }
    fn close_directory(&mut self, _: Directory) -> Result<(), IoError> {
        self.directory_closes += 1;
        if self.close_fail {
            Err(IoError::Failed)
        } else {
            Ok(())
        }
    }
    fn is_directory(&mut self, path: &CStr) -> Result<bool, IoError> {
        if self.failed.as_deref() == Some(path.to_bytes()) {
            return Err(IoError::Failed);
        }
        if self.directories.contains_key(path.to_bytes()) {
            Ok(true)
        } else if self.files.contains_key(path.to_bytes()) {
            Ok(false)
        } else {
            Err(IoError::Missing)
        }
    }
}
#[derive(Default)]
struct Hasher {
    begins: usize,
    updates: usize,
    finishes: usize,
    failed: bool,
}
impl BookHasher for Hasher {
    fn begin(&mut self) -> bool {
        self.begins += 1;
        !self.failed
    }
    fn update(&mut self, _: &[u8]) -> bool {
        self.updates += 1;
        !self.failed
    }
    fn finish(&mut self, output: &mut StatsDigest) -> bool {
        self.finishes += 1;
        *output = [7; 32];
        !self.failed
    }
}
#[derive(Default)]
struct Factory {
    hasher: Hasher,
    contexts: usize,
}
impl HashFactory for Factory {
    type Hasher = Hasher;
    fn with_hasher<R>(&mut self, run: impl FnOnce(&mut Hasher) -> R) -> R {
        self.contexts += 1;
        run(&mut self.hasher)
    }
}
// Explicitly injected test providers make namespace and binding calls observable.
fn namespace_hash(name: &[u8], output: &mut StatsDigest) -> bool {
    let mut hash = 14695981039346656037u64;
    for &byte in name {
        hash = (hash ^ u64::from(byte)).wrapping_mul(1099511628211);
    }
    *output = [0; 32];
    output[..8].copy_from_slice(&hash.to_le_bytes());
    true
}

fn book(version: u8) -> Vec<u8> {
    let size = [0, 11, 12, 16, 69, 73][usize::from(version)];
    let mut bytes = vec![0; size];
    bytes[0] = version;
    bytes
}
fn global(version: u8) -> Vec<u8> {
    let mut bytes = vec![0; [0, 13, 17, 159][usize::from(version)]];
    bytes[0] = version;
    bytes
}
fn run(store: &mut Medium, card: &mut Card, hash: &mut Factory) -> ImportReport {
    import_cross_ink(store, card, hash, c"/card", &mut || false, &namespace_hash)
}

#[test]
fn no_statistics_writes_nothing_and_global_commit_invalidates_streak_first() {
    let mut store = Medium::default();
    let mut card = Card::default();
    let mut factory = Factory::default();
    assert_eq!(
        run(&mut store, &mut card, &mut factory).result,
        ImportResult::Nothing
    );
    assert!(store.writes.is_empty());
    card.file(b"/card/.crosspoint/global_stats.bin", &global(1));
    store.inject(
        STREAK_NAMESPACE,
        &encode_streak(&StatsStreak {
            day: crate::reading_stats::FIRST_DAY,
            ..StatsStreak::default()
        }),
    );
    assert_eq!(
        run(&mut store, &mut card, &mut factory).result,
        ImportResult::Done
    );
    assert_eq!(
        store.writes,
        [STREAK_NAMESPACE, CROSS_INK_NAMESPACE, IMPORT_NAMESPACE]
    );
    let writes = store.writes.len();
    let opens = card.opens;
    assert_eq!(
        run(&mut store, &mut card, &mut factory),
        ImportReport::default()
    );
    assert_eq!((store.writes.len(), card.opens), (writes, opens));
}

#[test]
fn heterogeneous_walk_binds_card_relative_epubs_only_and_freezes_every_source() {
    let mut store = Medium::default();
    let mut card = Card::default();
    let mut factory = Factory::default();
    let paths = [
        b"/Shelf/one.EPUB".as_slice(),
        b"/Newt/Books/two.epub".as_slice(),
        b"/Newt/Cache/skip.epub".as_slice(),
        b"/.hidden/no.epub".as_slice(),
        b"/Shelf/empty.epub".as_slice(),
    ];
    for (i, path) in paths.iter().enumerate() {
        let mut full = b"/card".to_vec();
        full.extend_from_slice(path);
        card.file(&full, if i == 4 { &[] } else { b"synthetic epub" });
        card.source(
            epub_source_name(path).as_bytes(),
            b"stats_v5.bin",
            &book((i + 1) as u8),
        );
    }
    card.source(b"text_9", b"stats.bin", &[0]);
    let report = run(&mut store, &mut card, &mut factory);
    assert_eq!(
        report,
        ImportReport {
            result: ImportResult::Done,
            sources: 6,
            bound: 2
        }
    );
    for (i, path) in paths.iter().enumerate() {
        let source = store.source(epub_source_name(path).as_bytes());
        assert_eq!(
            source.binding,
            if i < 2 {
                ci::CrossInkBinding::Unverified
            } else {
                ci::CrossInkBinding::None
            }
        );
    }
    assert_eq!(
        store.source(b"text_9").copy.status,
        ci::CrossInkStatus::Unrecoverable
    );
    assert_eq!(
        store.writes.last().map(String::as_str),
        Some(IMPORT_NAMESPACE)
    );
    assert_eq!(card.opens, card.closes);
    assert_eq!(card.directory_opens, card.directory_closes);
    let frozen = store.records.clone();
    card.failed = Some(b"/card/.crosspoint".to_vec());
    assert_eq!(
        run(&mut store, &mut card, &mut factory).result,
        ImportResult::Done
    );
    assert_eq!(store.records, frozen);
}

#[test]
fn a_cut_after_each_frozen_record_resumes_without_rewriting_it() {
    for cut in 1..=3 {
        let mut store = Medium {
            cut_after: Some(cut),
            ..Medium::default()
        };
        let mut card = Card::default();
        let mut factory = Factory::default();
        card.file(b"/card/.crosspoint/global_stats.bin", &global(3));
        card.source(b"a_1", b"stats_v5.bin", &book(5));
        card.source(b"b_2", b"stats.bin", &book(1));
        assert_eq!(
            run(&mut store, &mut card, &mut factory).result,
            ImportResult::Unavailable
        );
        let frozen = store.records.clone();
        store.cut_after = None;
        assert_eq!(
            run(&mut store, &mut card, &mut factory).result,
            ImportResult::Done
        );
        for (name, bytes) in frozen {
            assert_eq!(store.records.get(&name), Some(&bytes));
        }
        assert_eq!(
            store.writes,
            [
                CROSS_INK_NAMESPACE.to_string(),
                cross_ink_source_namespace(b"a_1", namespace_hash)
                    .as_str()
                    .to_string(),
                cross_ink_source_namespace(b"b_2", namespace_hash)
                    .as_str()
                    .to_string(),
                IMPORT_NAMESPACE.to_string()
            ]
        );
    }
}

#[test]
fn cancellation_unconfirmed_and_newer_marker_never_publish_completion() {
    let mut store = Medium::default();
    let mut card = Card::default();
    let mut factory = Factory::default();
    card.file(b"/card/.crosspoint/global_stats.bin", &global(2));
    assert_eq!(
        import_cross_ink(
            &mut store,
            &mut card,
            &mut factory,
            c"/card",
            &mut || true,
            &namespace_hash
        )
        .result,
        ImportResult::Cancelled
    );
    assert!(store.writes.is_empty());
    store.unconfirmed = true;
    assert_eq!(
        run(&mut store, &mut card, &mut factory).result,
        ImportResult::Unconfirmed
    );
    assert!(!store.records.contains_key(IMPORT_NAMESPACE));
    assert_eq!(store.writes.len(), 2);
    let mut marker = ci::encode_import(&ci::ImportRecord::default());
    marker[4] = 2;
    stamp(&mut marker, 0);
    store.inject(IMPORT_NAMESPACE, &marker);
    let opens = card.opens;
    assert_eq!(
        run(&mut store, &mut card, &mut factory).result,
        ImportResult::Unsupported
    );
    assert_eq!(card.opens, opens);
}

#[test]
fn source_namespace_collision_is_not_an_existing_record_and_corrupt_repair_is_admitted() {
    let mut store = Medium::default();
    let mut card = Card::default();
    let mut factory = Factory::default();
    let ns = cross_ink_source_namespace(b"a_1", namespace_hash);
    let mut source = ci::CrossInkSource::default();
    source.name[..3].copy_from_slice(b"b_2");
    store.inject(ns.as_str(), &ci::encode_cross_ink_source(&source).unwrap());
    card.source(b"a_1", b"stats_v5.bin", &book(5));
    assert_eq!(
        run(&mut store, &mut card, &mut factory).result,
        ImportResult::Done
    );
    assert_eq!(
        store.source(b"a_1").copy.status,
        ci::CrossInkStatus::Imported
    );
    assert_eq!(store.aside, 1);
}

#[test]
fn walk_depth_and_path_limits_leave_deeper_sources_unbound() {
    let mut store = Medium::default();
    let mut card = Card::default();
    let mut factory = Factory::default();
    let mut overlong = b"/".to_vec();
    overlong.extend_from_slice(&[b'x'; 248]);
    overlong.extend_from_slice(b".epub");
    let paths = [
        b"/a/b/c/d/e/f/within.epub".as_slice(),
        b"/a/b/c/d/e/f/g/deeper.epub".as_slice(),
        overlong.as_slice(),
    ];
    for path in paths {
        let mut full = b"/card".to_vec();
        full.extend_from_slice(path);
        card.file(&full, b"synthetic");
        card.source(epub_source_name(path).as_bytes(), b"stats.bin", &book(1));
    }
    assert_eq!(
        run(&mut store, &mut card, &mut factory),
        ImportReport {
            result: ImportResult::Done,
            sources: 3,
            bound: 1
        }
    );
    assert_eq!(
        store.source(epub_source_name(paths[0]).as_bytes()).binding,
        ci::CrossInkBinding::Unverified
    );
    for path in &paths[1..] {
        assert_eq!(
            store.source(epub_source_name(path).as_bytes()).binding,
            ci::CrossInkBinding::None
        );
    }
    assert_eq!(card.directory_opens, card.directory_closes);
}

#[test]
fn directory_errors_close_the_entire_walk_and_unknown_stat_failure_matches_cpp() {
    for failed in [b"/card/a".as_slice(), b"/card/.crosspoint".as_slice()] {
        let mut store = Medium::default();
        let mut card = Card::default();
        let mut factory = Factory::default();
        card.file(b"/card/.crosspoint/global_stats.bin", &global(1));
        card.directory(b"/card/a/b");
        card.failed = Some(failed.to_vec());
        assert_eq!(
            run(&mut store, &mut card, &mut factory).result,
            ImportResult::Unavailable
        );
        assert_eq!(card.directory_opens, card.directory_closes);
        assert!(!store.records.contains_key(IMPORT_NAMESPACE));
    }
    let mut store = Medium::default();
    let mut card = Card::default();
    let mut factory = Factory::default();
    card.file(b"/card/.crosspoint/global_stats.bin", &global(1));
    card.directory(b"/card/a/b");
    card.directories
        .get_mut(b"/card".as_slice())
        .unwrap()
        .iter_mut()
        .find(|entry| entry.name.to_bytes() == b"a")
        .unwrap()
        .kind = EntryKind::Unknown;
    card.failed = Some(b"/card/a".to_vec());
    assert_eq!(
        run(&mut store, &mut card, &mut factory).result,
        ImportResult::Done
    );
    assert_eq!(card.directory_opens, card.directory_closes);
}

#[test]
fn marker_still_checks_global_but_unsupported_streak_stops_before_first_freeze() {
    let mut store = Medium::default();
    let mut card = Card::default();
    let mut factory = Factory::default();
    card.file(b"/card/.crosspoint/global_stats.bin", &global(3));
    store.inject(
        IMPORT_NAMESPACE,
        &ci::encode_import(&ci::ImportRecord {
            sources: 19,
            ..ci::ImportRecord::default()
        }),
    );
    assert_eq!(
        run(&mut store, &mut card, &mut factory),
        ImportReport::default()
    );
    assert_eq!(store.writes, [CROSS_INK_NAMESPACE]);
    assert_eq!(card.directory_opens, 0);
    let mut store = Medium::default();
    let mut newer = encode_streak(&StatsStreak::default());
    newer[4] = 2;
    stamp(&mut newer, 0);
    store.inject(STREAK_NAMESPACE, &newer);
    assert_eq!(
        run(&mut store, &mut card, &mut factory).result,
        ImportResult::Unsupported
    );
    assert!(store.writes.is_empty());
}

#[path = "crossink_import_fault_tests.rs"]
mod faults;
