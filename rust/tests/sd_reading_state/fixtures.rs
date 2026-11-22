// SPDX-License-Identifier: MIT

//! Focused fixtures for reading_state.rs on the in-memory FAT model. The
//! language-independent protocol (cases.json) carries C++ parity and the
//! independently encoded expectations; these pin the policy's key edges.
use super::model::{Effect, Fault, Model, Op};
use super::*;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use newt_core::durable::{Commit, SleepScreenCodec, commit_bytes, commit_publish};

const STATE: &str = "data/newt-sd/Newt/State";
const SELECTED: &str = "data/newt-sd/Newt/State/newt-selected.nrs";

struct Card(bool);

impl<N: crate::Native> StateMedium<N> for Card {
    const STATE: &'static str = STATE;
    const STATS: &'static str = "data/newt-sd/Newt/Stats";
    fn release_pages_for_state(&mut self) {}
    fn mount(&mut self, _: Option<&mut N>) -> bool {
        self.0
    }
}

/// The plain NVS backend's exact two-step read (nvs_blob_backend.h) over an
/// in-memory store, for fixtures only; the protocol driver runs the
/// production NvsBlobBackend instead. Every write is counted.
#[derive(Default)]
pub struct NvsStore {
    pub spaces: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
    pub writes: u32,
}

pub struct Legacy<'a> {
    pub store: &'a mut NvsStore,
    open: Option<String>,
}

impl<'a> Legacy<'a> {
    pub fn new(store: &'a mut NvsStore) -> Self {
        Self { store, open: None }
    }
}

impl ScopedTransaction for Legacy<'_> {
    type State = Option<String>;
    const CLOSED: Self::State = None;

    fn with_transaction<R>(
        &mut self,
        state: &mut Self::State,
        work: impl FnOnce(&mut dyn newt_core::durable::Backend) -> R,
    ) -> Option<R> {
        if self.open.is_some() {
            return None;
        }
        struct Loan<'store, 'state> {
            backend: Legacy<'store>,
            state: &'state mut Option<String>,
        }
        impl Drop for Loan<'_, '_> {
            fn drop(&mut self) {
                *self.state = self.backend.open.take();
            }
        }
        // Keep the same namespace state in the transaction across short loans.
        let mut loan = Loan {
            backend: Legacy { store: &mut *self.store, open: state.take() },
            state,
        };
        Some(work(&mut loan.backend))
    }
}

impl newt_core::durable::Backend for Legacy<'_> {
    fn begin(&mut self, namespace: &str, readonly: bool) -> newt_core::durable::StorageOpen {
        use newt_core::durable::StorageOpen;
        self.open = None;
        if readonly && !self.store.spaces.contains_key(namespace) {
            return StorageOpen::Missing;
        }
        self.store.spaces.entry(namespace.to_string()).or_default();
        self.open = Some(namespace.to_string());
        StorageOpen::Opened
    }

    fn end(&mut self) {
        self.open = None;
    }

    fn read_blob(&mut self, key: &str, output: &mut [u8]) -> newt_core::durable::BlobRead {
        use newt_core::durable::BlobRead;
        let Some(space) = self
            .open
            .as_ref()
            .and_then(|name| self.store.spaces.get(name))
        else {
            return BlobRead::Failed;
        };
        let Some(bytes) = space.get(key) else {
            return BlobRead::Missing;
        };
        if bytes.len() == output.len() {
            output.copy_from_slice(bytes);
        }
        BlobRead::Present(bytes.len())
    }

    fn put_bytes(
        &mut self,
        _permit: newt_core::durable::DurableWrite,
        key: &str,
        bytes: &[u8],
    ) -> usize {
        self.store.writes += 1;
        let Some(name) = self.open.clone() else {
            return 0;
        };
        self.store
            .spaces
            .entry(name)
            .or_default()
            .insert(key.to_string(), bytes.to_vec());
        bytes.len()
    }

    fn readback_durable(&self) -> bool {
        true
    }
}

struct Bench {
    facts: StateFacts,
    card: Card,
    model: Model,
    nvs: NvsStore,
}

type Router<'a> = ReadingStateRouter<'a, Legacy<'a>, Model, Card>;

impl Bench {
    fn new() -> Self {
        let mut model = Model::default();
        model
            .dirs
            .extend(["data".to_string(), "data/newt-sd".to_string()]);
        Self {
            facts: StateFacts::new(),
            card: Card(true),
            model,
            nvs: NvsStore::default(),
        }
    }

    fn router(&mut self) -> Router<'_> {
        ReadingStateRouter::new(
            Legacy::new(&mut self.nvs),
            &mut self.facts,
            &mut self.card,
            &mut self.model,
        )
    }

    fn no_card_router(&mut self) -> Router<'_> {
        ReadingStateRouter::new_unmounted(
            Legacy::new(&mut self.nvs),
            &mut self.facts,
            &mut self.card,
        )
    }

    fn fault(&mut self, op: Op, first: u32, count: u32, effect: Effect, min: usize) {
        self.model
            .faults
            .push(Fault::new(op, first, count, effect, min));
    }

    fn durable(&self, path: &str) -> &[u8] {
        &self.model.files[path].durable
    }
}

fn record(key: &str, generation: u32, payload: &[u8]) -> [u8; RECORD_BYTES] {
    let mut record = [0; RECORD_BYTES];
    record[HEADER_BYTES..HEADER_BYTES + payload.len()].copy_from_slice(payload);
    assert!(seal(
        &mut record,
        b"NRSK",
        key.as_bytes(),
        payload.len(),
        generation
    ));
    record
}

/// A State file: header plus (key index, copy, record) entries.
fn file(namespace: &str, records: &[(usize, usize, [u8; RECORD_BYTES])]) -> alloc::vec::Vec<u8> {
    let mut bytes = vec![0; FILE_BYTES];
    let mut first = [0; RECORD_BYTES];
    assert!(header(&mut first, namespace.as_bytes()));
    bytes[..RECORD_BYTES].copy_from_slice(&first);
    for (key, copy, record) in records {
        let at = copy_offset(*key, *copy) as usize;
        bytes[at..at + RECORD_BYTES].copy_from_slice(record);
    }
    bytes
}

fn plant(model: &mut Model, path: &str, bytes: Vec<u8>) {
    model.dirs.extend(
        path.match_indices('/')
            .map(|(at, _)| path[..at].to_string()),
    );
    model.files.insert(
        path.to_string(),
        model::File {
            durable: bytes.clone(),
            readable: bytes,
        },
    );
}

fn read(router: &mut Router<'_>, key: &str, capacity: usize) -> (BlobRead, Vec<u8>) {
    let mut output = vec![0xa5; capacity];
    let result = router.read_blob(key, &mut output);
    (result, output)
}

fn commit(router: &mut Router<'_>, key: &str, bytes: &[u8]) -> Commit {
    let mut readback = vec![0; bytes.len()];
    commit_bytes(router, key, bytes, &mut readback)
}

#[test]
fn fresh_namespace_is_created_and_read_back_without_nvs_writes() {
    let mut bench = Bench::new();
    {
        let mut router = bench.router();
        assert_eq!(router.begin("newt_selected", false), StorageOpen::Opened);
        assert_eq!(commit(&mut router, "slot0", b"book"), Commit::Saved);
        assert_eq!(
            read(&mut router, "slot0", 4),
            (BlobRead::Present(4), b"book".to_vec())
        );
        assert!(router.readback_durable());
        router.end();
    }
    assert_eq!(
        bench.durable(SELECTED),
        file("newt_selected", &[(1, 0, record("slot0", 1, b"book"))])
    );
    // The O_EXCL temp was renamed into place; nothing else was left behind.
    assert_eq!(bench.model.files.len(), 1);
    assert_eq!(bench.nvs.writes, 0);
}

#[test]
fn legacy_nvs_is_read_only_until_a_file_exists_and_migrates_in_two_reads() {
    let mut bench = Bench::new();
    let mut space = BTreeMap::new();
    space.insert("digest".to_string(), vec![7; PAYLOAD_BYTES]);
    space.insert("slot0".to_string(), b"seven b".to_vec());
    bench.nvs.spaces.insert("newt_resume".to_string(), space);
    {
        let mut router = bench.router();
        assert_eq!(router.begin("newt_resume", true), StorageOpen::Opened);
        assert_eq!(read(&mut router, "slot0", 7).0, BlobRead::Present(7));
        assert_eq!(router.begin("newt_resume", false), StorageOpen::Opened);
        assert_eq!(
            read(&mut router, "digest", PAYLOAD_BYTES),
            (BlobRead::Present(PAYLOAD_BYTES), vec![7; PAYLOAD_BYTES])
        );
        assert_eq!(
            read(&mut router, "slot0", 7),
            (BlobRead::Present(7), b"seven b".to_vec())
        );
        assert_eq!(read(&mut router, "slot1", 1).0, BlobRead::Missing);
    }
    assert_eq!(bench.nvs.writes, 0);
}

#[test]
fn oversize_legacy_blob_fails_creation_and_removes_its_own_temp() {
    let mut bench = Bench::new();
    let mut space = BTreeMap::new();
    space.insert("slot1".to_string(), vec![1; PAYLOAD_BYTES + 1]);
    bench.nvs.spaces.insert("newt_zone".to_string(), space);
    assert_eq!(
        bench.router().begin("newt_zone", false),
        StorageOpen::Failed
    );
    assert!(bench.model.files.is_empty());
    assert!(
        bench
            .model
            .trace
            .iter()
            .any(|line| line.starts_with("unlink ") && line.ends_with(".tmp = 0"))
    );
}

#[test]
fn lost_sync_never_overwrites_the_only_durable_copy() {
    let mut bench = Bench::new();
    plant(
        &mut bench.model,
        SELECTED,
        file("newt_selected", &[(1, 0, record("slot0", 1, b"AAAA"))]),
    );
    // B's sync fails with its bytes readable; the retry's record write tears.
    bench.fault(Op::Sync, 1, 1, Effect::Errno("EIO"), 0);
    bench.fault(Op::Write, 2, 1, Effect::Tear(40), RECORD_BYTES);
    {
        let mut router = bench.router();
        assert_eq!(router.begin("newt_selected", false), StorageOpen::Opened);
        assert_eq!(commit(&mut router, "slot0", b"BBBB"), Commit::Unconfirmed);
        assert!(!router.readback_durable());
        // Power is lost mid-transaction: no close may drain buffered bytes.
        router.state.io.native.as_deref_mut().unwrap().power();
    }
    bench.facts = StateFacts::new();
    let mut router = bench.router();
    assert_eq!(router.begin("newt_selected", true), StorageOpen::Opened);
    // The barrier made B durable before the retry targeted A's copy.
    assert_eq!(
        read(&mut router, "slot0", 4),
        (BlobRead::Present(4), b"BBBB".to_vec())
    );
}

#[test]
fn signed_wrapping_generations_pick_the_newest_copy() {
    let mut bench = Bench::new();
    let wrapped = [
        (1, 0, record("slot0", u32::MAX, b"old")),
        (1, 1, record("slot0", 0, b"new")),
    ];
    // A half-range gap is not newer: the tie-breaking first copy wins.
    let half = [
        (2, 0, record("slot1", 0, b"one")),
        (2, 1, record("slot1", 1 << 31, b"two")),
    ];
    let mut records = wrapped.to_vec();
    records.extend(half);
    plant(&mut bench.model, SELECTED, file("newt_selected", &records));
    let mut router = bench.router();
    assert_eq!(router.begin("newt_selected", true), StorageOpen::Opened);
    assert_eq!(read(&mut router, "slot0", 3).1, b"new");
    assert_eq!(read(&mut router, "slot1", 3).1, b"one");
}

#[test]
fn foreign_records_fail_closed_and_damaged_copies_are_ignored() {
    let mut bench = Bench::new();
    let mut torn = record("slot0", 9, b"torn");
    torn[RECORD_BYTES - 1] ^= 1;
    let records = [
        (1, 0, record("slot0", 2, b"good")),
        (1, 1, torn),
        (2, 1, record("slot0", 1, b"misfiled")),
    ];
    plant(&mut bench.model, SELECTED, file("newt_selected", &records));
    let mut router = bench.router();
    assert_eq!(router.begin("newt_selected", false), StorageOpen::Opened);
    assert_eq!(read(&mut router, "slot0", 4).1, b"good");
    assert_eq!(read(&mut router, "slot1", 8).0, BlobRead::Failed);
    assert_eq!(commit(&mut router, "slot1", b"x"), Commit::Unconfirmed);
}

#[test]
fn interrupted_reads_retry_at_most_four_calls() {
    for (interruptions, expected) in [(3, BlobRead::Present(4)), (4, BlobRead::Failed)] {
        let mut bench = Bench::new();
        plant(
            &mut bench.model,
            SELECTED,
            file("newt_selected", &[(1, 0, record("slot0", 1, b"page"))]),
        );
        let mut router = bench.router();
        assert_eq!(router.begin("newt_selected", true), StorageOpen::Opened);
        let model = router.state.io.native.as_deref_mut().unwrap();
        model.faults.push(Fault::new(
            Op::Read,
            1,
            interruptions,
            Effect::Errno("EINTR"),
            0,
        ));
        assert_eq!(read(&mut router, "slot0", 4).0, expected);
        assert!(!router.reading_state_unavailable());
    }
}

#[test]
fn only_medium_errors_mark_the_card_unavailable() {
    for (errno, unavailable) in [("EIO", true), ("ENODEV", true), ("ENFILE", false)] {
        let mut bench = Bench::new();
        plant(&mut bench.model, SELECTED, file("newt_selected", &[]));
        bench.fault(Op::Open, 1, 1, Effect::Errno(errno), 0);
        let mut router = bench.router();
        assert_eq!(router.begin("newt_selected", false), StorageOpen::Failed);
        assert_eq!(router.reading_state_unavailable(), unavailable, "{errno}");
    }
    let mut bench = Bench::new();
    bench.card.0 = false;
    let mut router = bench.router();
    assert_eq!(router.begin("newt_clock", true), StorageOpen::Missing);
    assert!(router.reading_state_unavailable());
}

#[test]
fn no_card_fails_state_closed_keeps_nvs_and_hands_its_facts_to_the_mounted_router() {
    let mut bench = Bench::new();
    bench.card.0 = false;
    let mut published = 0;
    {
        let mut router = bench.no_card_router();
        assert_eq!(router.begin("newt_selected", true), StorageOpen::Missing);
        // The failed commit leaves the published Sleep screen choice alone.
        assert_eq!(
            commit_publish::<_, SleepScreenCodec, 20>(&mut router, 7, &mut published),
            Commit::Unconfirmed
        );
        assert_eq!(router.begin("newt_stats", false), StorageOpen::Failed);
        assert!(!router.repair_in_place("newt_stats", b"ledger"));
        assert!(!router.for_each_namespace("nb", &mut |_, _| true));
        assert_eq!(router.state_folder(), StorageOpen::Failed);
        assert_eq!(router.begin("newt_reader", false), StorageOpen::Opened);
        assert_eq!(commit(&mut router, "slot0", b"nvs"), Commit::Saved);
        assert!(router.reading_state_unavailable());
    }
    assert_eq!(published, 0);
    assert_eq!(bench.nvs.writes, 1);
    assert!(bench.nvs.spaces.keys().eq(["newt_reader"]));
    // Board mounted the card: the mounted router keeps the boot's facts and
    // does no I/O until asked.
    bench.card.0 = true;
    let mut router = bench.router();
    assert!(router.reading_state_unavailable());
    assert!(
        router
            .state
            .io
            .native
            .as_deref()
            .is_some_and(|model| model.trace.is_empty())
    );
    assert_eq!(router.begin("newt_sleep", true), StorageOpen::Missing);
    assert!(!router.reading_state_unavailable());
}

#[test]
fn missing_native_io_is_an_unavailable_medium_even_if_a_mount_is_claimed() {
    let mut bench = Bench::new();
    let mut space = BTreeMap::new();
    space.insert("slot0".to_string(), b"old".to_vec());
    bench.nvs.spaces.insert("newt_zone".to_string(), space);
    {
        // Card(true) without native I/O: a medium that misreports a mount.
        let mut router = bench.no_card_router();
        assert_eq!(router.begin("newt_zone", true), StorageOpen::Failed);
        assert!(router.reading_state_unavailable());
        assert_eq!(router.begin("newt_zone", false), StorageOpen::Failed);
        assert_eq!(router.state_folder(), StorageOpen::Failed);
        assert!(!router.for_each_namespace("nb", &mut |_, _| true));
    }
    assert_eq!(bench.nvs.writes, 0);
    assert!(bench.model.trace.is_empty());
}

#[test]
fn damaged_statistics_wait_for_an_explicit_repair_with_evidence() {
    let mut bench = Bench::new();
    let stats = "data/newt-sd/Newt/Stats/newt-stats.nrs";
    plant(&mut bench.model, stats, vec![3; 100]);
    bench.model.dirs.insert(STATE.to_string());
    {
        let mut router = bench.router();
        assert_eq!(router.begin("newt_stats", false), StorageOpen::Damaged);
        assert!(router.repair_in_place("newt_stats", b"ledger"));
    }
    assert_eq!(
        bench.durable(stats),
        file("newt_stats", &[(1, 0, record("slot0", 1, b"ledger"))])
    );
    let evidence = "data/newt-sd/Newt/Stats/newt-stats.nrs.00000001.bad";
    assert_eq!(bench.durable(evidence), vec![3; 100]);
}

#[test]
fn listing_visits_only_numbered_records_and_stops_on_request() {
    let mut bench = Bench::new();
    for name in ["nb0000000000001", "nb0000000000002", "sb0000000000003"] {
        plant(
            &mut bench.model,
            &alloc::format!("{STATE}/{name}.nrs"),
            vec![],
        );
    }
    for other in [
        "nb0000000000004.nrs.0000000a.tmp",
        "nb000000000000X.nrs",
        "newt-selected.nrs",
    ] {
        plant(&mut bench.model, &alloc::format!("{STATE}/{other}"), vec![]);
    }
    let mut router = bench.router();
    let mut seen = Vec::new();
    assert!(router.for_each_namespace("nb", &mut |_, name| {
        seen.push(name.to_string());
        true
    }));
    assert_eq!(seen, ["nb0000000000001", "nb0000000000002"]);
    assert!(!router.for_each_namespace("nb", &mut |_, _| false));
    assert!(!router.for_each_namespace("sm", &mut |_, _| true));
    // No Stats folder yet: an empty listing.
    assert!(router.for_each_namespace("sb", &mut |_, _| false));
}
