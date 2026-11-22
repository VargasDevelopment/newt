// SPDX-License-Identifier: MIT
//! The installed-app writer (`newt_core::app_store::install`) over a fake NVS
//! that models the pinned ESP-IDF NVS: a torn put leaves its key as it was
//! before the put, so "old or new" claims here hold under the modelled NVS
//! only. Each restart is a cold load through the firmware's own `load_boot`
//! over the durable bytes alone. The card scan's cases are in `card`.
//! tests/app_store_test.py runs it with its negative controls.
#[path = "../../firmware/app_runtime/package.rs"]
mod package;

mod card;
mod reader_hash;
mod sha256;
use sha256::sha256;

use newt_core::app_store::{self, Install, PACKAGE_BYTES, RECORD_BYTES, Refusal};
use newt_core::durable::{Backend, BlobRead, DurableWrite, SLOT_KEYS, StorageOpen};
use newt_tiny_app::{Definition, decode, pause::{BUILTIN_BYTES, is_pocket_pause}};
use package::X3Payload;
use std::collections::VecDeque;

const CORRUPT: &str = "The saved app did not pass its integrity check.";

/// What one put does to the durable bytes and what it reports.
#[derive(Clone, Copy, Debug)]
enum Put {
    Ok,
    /// Torn under the modelled NVS: the key is as it was; nothing is acknowledged.
    Torn,
    /// Acknowledges fewer bytes; nothing changes.
    Short,
    /// Not the modelled NVS: a torn put that leaves partial bytes behind.
    TornBytes,
    /// Durable, but the acknowledgement is lost.
    LostAck,
    /// Acknowledged, but one stored byte differs, and a readback shows it.
    Differs,
    /// Durable and acknowledged; the readback that follows fails.
    ReadbackFails,
    /// Acknowledged and echoed by the readback, but nothing reached storage.
    Phantom,
}

struct Nvs {
    open: StorageOpen,
    slots: [Option<Vec<u8>>; 2],
    puts: VecDeque<Put>,
    /// The slot each put attempt named.
    writes: Vec<usize>,
    durable: bool,
    session: Option<bool>,
    fail_read: Option<usize>,
    echo: Option<usize>,
}

impl Nvs {
    fn with(first: Option<Vec<u8>>, second: Option<Vec<u8>>) -> Self {
        Self {
            open: StorageOpen::Opened,
            slots: [first, second],
            puts: VecDeque::new(),
            writes: Vec::new(),
            durable: true,
            session: None,
            fail_read: None,
            echo: None,
        }
    }
    fn empty() -> Self { Self::with(None, None) }
    fn puts(mut self, puts: &[Put]) -> Self {
        self.puts.extend(puts);
        self
    }
    fn cached(mut self) -> Self {
        self.durable = false;
        self
    }
    fn failing_read(mut self, index: usize) -> Self {
        self.fail_read = Some(index);
        self
    }
}

fn slot(key: &str) -> usize {
    SLOT_KEYS.iter().position(|slot| *slot == key).expect("only slot keys")
}

impl Backend for Nvs {
    fn begin(&mut self, namespace: &str, readonly: bool) -> StorageOpen {
        assert!(namespace == "newt_apps" && self.session.is_none(), "one newt_apps session");
        if self.open == StorageOpen::Opened {
            self.session = Some(readonly);
        }
        self.open
    }
    fn end(&mut self) {
        assert!(self.session.take().is_some(), "end without a session");
    }
    fn read_blob(&mut self, key: &str, output: &mut [u8]) -> BlobRead {
        assert!(self.session.is_some(), "read outside the session");
        let index = slot(key);
        if self.fail_read.take_if(|failing| *failing == index).is_some() {
            return BlobRead::Failed;
        }
        match &self.slots[index] {
            None => BlobRead::Missing,
            Some(bytes) if bytes.len() != output.len() => BlobRead::Present(bytes.len()),
            Some(bytes) => {
                output.copy_from_slice(bytes);
                BlobRead::Present(bytes.len())
            }
        }
    }
    fn readback_bytes(&mut self, key: &str, expected: &[u8], output: &mut [u8]) -> BlobRead {
        if self.echo.take_if(|echoed| *echoed == slot(key)).is_some() {
            output.copy_from_slice(expected);
            return BlobRead::Present(expected.len());
        }
        self.read_blob(key, output)
    }
    fn put_bytes(&mut self, _: DurableWrite, key: &str, bytes: &[u8]) -> usize {
        assert_eq!(self.session, Some(false), "puts only in a writable session");
        let index = slot(key);
        self.writes.push(index);
        match self.puts.pop_front().unwrap_or(Put::Ok) {
            Put::Ok => {
                self.slots[index] = Some(bytes.to_vec());
                bytes.len()
            }
            Put::Torn => 0,
            Put::Short => bytes.len() - 1,
            Put::TornBytes => {
                self.slots[index] = Some(bytes[..bytes.len() / 2].to_vec());
                0
            }
            Put::LostAck => {
                self.slots[index] = Some(bytes.to_vec());
                0
            }
            Put::Differs => {
                let mut stored = bytes.to_vec();
                stored[100] ^= 1;
                self.slots[index] = Some(stored);
                bytes.len()
            }
            Put::ReadbackFails => {
                self.slots[index] = Some(bytes.to_vec());
                self.fail_read = Some(index);
                bytes.len()
            }
            Put::Phantom => {
                self.echo = Some(index);
                bytes.len()
            }
        }
    }
    fn readback_durable(&self) -> bool { self.durable }
}

/// A distinct valid package: Pocket Pause's wire under another id, version and title.
fn app(id: &[u8], version: &[u8], title: &[u8]) -> [u8; PACKAGE_BYTES] {
    let mut wire = *BUILTIN_BYTES;
    for (at, end, value) in [(8, 41, id), (41, 58, version), (58, 91, title)] {
        wire[at..end].fill(0);
        wire[at..at + value.len()].copy_from_slice(value);
    }
    wire
}

fn titled(title: &[u8]) -> [u8; PACKAGE_BYTES] { app(b"card-app", b"1.0.0", title) }

fn definition(wire: &[u8; PACKAGE_BYTES]) -> Definition {
    let mut definition = Definition::default();
    assert!(decode(Some(wire), &mut definition));
    definition
}

/// NEWTGEN1 | generation u32 LE | package SHA-256 | package | record SHA-256.
fn record(generation: u32, package: &[u8; PACKAGE_BYTES]) -> Vec<u8> {
    let mut bytes = b"NEWTGEN1".to_vec();
    bytes.extend(generation.to_le_bytes());
    bytes.extend(sha256(package));
    bytes.extend(package);
    bytes.extend(sha256(&bytes));
    bytes
}

fn corrupt(generation: u32, title: &[u8]) -> Vec<u8> {
    let mut bytes = record(generation, &titled(title));
    bytes[690] ^= 1;
    bytes
}

struct Outcome {
    result: Install,
    /// The package handed to the runtime, if any.
    activated: Option<[u8; PACKAGE_BYTES]>,
}

/// Installs `card` after an offer that named `offered`; the card read returns `card`.
fn install_offered(nvs: &mut Nvs, offered: &[u8; PACKAGE_BYTES], card: Result<[u8; PACKAGE_BYTES], Refusal>) -> Outcome {
    let mut slots = [[0; RECORD_BYTES]; 2];
    let mut activated = None;
    let result = app_store::install(
        nvs,
        &mut X3Payload,
        &mut slots,
        &sha256(offered),
        |wire| card.map(|bytes| *wire = bytes),
        |wire| activated = Some(*wire),
    );
    assert!(nvs.session.is_none(), "the installer closes its session");
    Outcome { result, activated }
}

fn install(nvs: &mut Nvs, card: &[u8; PACKAGE_BYTES]) -> Outcome { install_offered(nvs, card, Ok(*card)) }

/// A restart: all RAM gone, the durable bytes loaded by the firmware's boot loader.
fn restart(nvs: &Nvs) -> Result<Definition, &'static str> {
    let mut cold = Nvs::with(nvs.slots[0].clone(), nvs.slots[1].clone());
    let mut app = Definition::default();
    let result = package::load_boot(&mut cold, &mut app);
    assert!(cold.writes.is_empty() && cold.session.is_none());
    result.map(|()| app).map_err(|message| core::str::from_utf8(message).unwrap())
}

fn bundled() -> Definition { definition(BUILTIN_BYTES) }

fn installed(outcome: &Outcome, card: &[u8; PACKAGE_BYTES]) {
    assert_eq!(outcome.result, Install::Installed);
    assert_eq!(outcome.activated.as_ref(), Some(card), "the verified package is activated");
}

fn untouched(outcome: &Outcome, result: Install) {
    assert_eq!(outcome.result, result);
    assert!(outcome.activated.is_none(), "the running app must not change");
}

#[test]
fn a_first_install_writes_slot0_at_generation_1() {
    let card = titled(b"First");
    let mut nvs = Nvs::empty();
    installed(&install(&mut nvs, &card), &card);
    assert_eq!(nvs.writes, [0]);
    assert_eq!(nvs.slots, [Some(record(1, &card)), None]);
    assert_eq!(restart(&nvs), Ok(definition(&card)));
}

#[test]
fn a_replace_writes_the_peer_of_the_selected_slot_and_keeps_it() {
    let (old, card) = (titled(b"Old"), titled(b"New"));
    let mut nvs = Nvs::with(Some(record(5, &old)), None);
    installed(&install(&mut nvs, &card), &card);
    assert_eq!(nvs.slots, [Some(record(5, &old)), Some(record(6, &card))]);
    assert_eq!(restart(&nvs), Ok(definition(&card)));
    // The newer of two valid slots is selected; the older one is the target.
    let mut nvs = Nvs::with(Some(record(8, &titled(b"Older"))), Some(record(9, &old)));
    installed(&install(&mut nvs, &card), &card);
    assert_eq!(nvs.slots, [Some(record(10, &card)), Some(record(9, &old))]);
    // Serial order: the generation after u32::MAX is 0, which is newer.
    let mut nvs = Nvs::with(None, Some(record(u32::MAX, &old)));
    installed(&install(&mut nvs, &card), &card);
    assert_eq!(nvs.slots[0], Some(record(0, &card)));
    assert_eq!(restart(&nvs), Ok(definition(&card)));
}

#[test]
fn a_valid_slot_beside_damage_is_kept_and_the_damage_replaced() {
    let (old, card) = (titled(b"Old"), titled(b"New"));
    let mut nvs = Nvs::with(Some(corrupt(40, b"Damaged")), Some(record(3, &old)));
    installed(&install(&mut nvs, &card), &card);
    assert_eq!(nvs.slots, [Some(record(4, &card)), Some(record(3, &old))]);
}

#[test]
fn damage_alone_is_repaired_at_generation_1() {
    let card = titled(b"New");
    // Corrupt beside Missing writes the Missing slot and leaves the damage.
    for damaged in [0, 1] {
        let mut slots = [None, None];
        slots[damaged] = Some(corrupt(70, b"Damaged"));
        let mut nvs = Nvs::with(slots[0].clone(), slots[1].clone());
        assert_eq!(restart(&nvs), Err(CORRUPT));
        installed(&install(&mut nvs, &card), &card);
        assert_eq!(nvs.writes, [damaged ^ 1]);
        assert_eq!(nvs.slots[damaged], slots[damaged]);
        assert_eq!(nvs.slots[damaged ^ 1], Some(record(1, &card)));
        assert_eq!(restart(&nvs), Ok(definition(&card)));
    }
    let mut nvs = Nvs::with(Some(corrupt(70, b"First")), Some(b"NEWT".to_vec()));
    installed(&install(&mut nvs, &card), &card);
    assert_eq!(nvs.slots, [Some(record(1, &card)), Some(b"NEWT".to_vec())]);
    assert_eq!(restart(&nvs), Ok(definition(&card)));
}

#[test]
fn storage_that_may_hold_the_only_copy_is_refused_before_any_write() {
    let (old, card) = (titled(b"Old"), titled(b"New"));
    let mut newer_record = record(1, &old);
    newer_record[7] = b'2';
    let stores = [
        ("an unreadable slot", Nvs::with(Some(record(1, &old)), None).failing_read(1)),
        ("a newer record version", Nvs::with(Some(newer_record), Some(record(1, &old)))),
        ("conflicting generations", Nvs::with(Some(record(7, &old)), Some(record(7, &titled(b"Other"))))),
    ];
    for (what, mut nvs) in stores {
        let before = nvs.slots.clone();
        untouched(&install(&mut nvs, &card), Install::Refused(Refusal::Storage));
        assert!(nvs.writes.is_empty() && nvs.slots == before, "{what}");
    }
    for open in [StorageOpen::Failed, StorageOpen::Damaged, StorageOpen::Missing] {
        let mut nvs = Nvs::empty();
        nvs.open = open;
        untouched(&install(&mut nvs, &card), Install::Refused(Refusal::Storage));
    }
}

#[test]
fn an_interrupted_first_install_boots_the_bundled_app_or_the_new_one() {
    let card = titled(b"First");
    let mut torn = Nvs::empty().puts(&[Put::Torn]);
    untouched(&install(&mut torn, &card), Install::Unconfirmed);
    assert_eq!(restart(&torn), Ok(bundled()));
    for put in [Put::LostAck, Put::ReadbackFails] {
        let mut nvs = Nvs::empty().puts(&[put]);
        untouched(&install(&mut nvs, &card), Install::Unconfirmed);
        assert_eq!(nvs.writes, [0], "{put:?}: a durable backend writes once");
        assert_eq!(restart(&nvs), Ok(definition(&card)), "{put:?}");
    }
}

#[test]
fn an_interrupted_replace_boots_the_old_app_or_the_new_one() {
    let (old, card) = (titled(b"Old"), titled(b"New"));
    // A partial blob is not what the pinned NVS leaves; the old app loads anyway.
    for put in [Put::Torn, Put::TornBytes] {
        let mut nvs = Nvs::with(Some(record(5, &old)), None).puts(&[put]);
        untouched(&install(&mut nvs, &card), Install::Unconfirmed);
        assert_eq!(restart(&nvs), Ok(definition(&old)), "{put:?}");
    }
    let mut nvs = Nvs::with(Some(record(5, &old)), None).puts(&[Put::LostAck]);
    untouched(&install(&mut nvs, &card), Install::Unconfirmed);
    assert_eq!(restart(&nvs), Ok(definition(&card)));
}

#[test]
fn a_partial_first_record_would_leave_apps_unavailable() {
    // What the "Install not confirmed" copy would hide if the pinned NVS ever
    // left a partial blob for a new key (plan section 4.4, owner decision 3).
    let mut nvs = Nvs::empty().puts(&[Put::TornBytes]);
    untouched(&install(&mut nvs, &titled(b"First")), Install::Unconfirmed);
    assert_eq!(restart(&nvs), Err(CORRUPT));
}

#[test]
fn a_failed_put_is_retried_only_when_readback_is_not_durable() {
    let card = titled(b"First");
    for put in [Put::Torn, Put::Short] {
        let mut nvs = Nvs::empty().puts(&[put, put]);
        untouched(&install(&mut nvs, &card), Install::Unconfirmed);
        assert_eq!(nvs.writes, [0], "{put:?}");
        let mut cached = Nvs::empty().cached().puts(&[put, put]);
        untouched(&install(&mut cached, &card), Install::Unconfirmed);
        assert_eq!(cached.writes, [0, 0], "{put:?}: one bounded retry");
        let mut recovered = Nvs::empty().cached().puts(&[put]);
        installed(&install(&mut recovered, &card), &card);
        assert_eq!(recovered.writes, [0, 0], "{put:?}");
    }
}

#[test]
fn a_readback_that_differs_is_never_reported_installed() {
    let card = titled(b"First");
    let mut nvs = Nvs::empty().puts(&[Put::Differs]);
    untouched(&install(&mut nvs, &card), Install::Unconfirmed);
    assert_eq!(nvs.writes, [0]);
    // A cached backend retries once; the retry's exact bytes then verify.
    let mut cached = Nvs::empty().cached().puts(&[Put::Differs]);
    installed(&install(&mut cached, &card), &card);
    assert_eq!(cached.writes, [0, 0]);
    assert_eq!(restart(&cached), Ok(definition(&card)));
}

#[test]
fn verification_reads_the_store_not_the_acknowledgement() {
    let (old, card) = (titled(b"Old"), titled(b"New"));
    let mut nvs = Nvs::with(Some(record(5, &old)), None).puts(&[Put::Phantom]);
    untouched(&install(&mut nvs, &card), Install::Unconfirmed);
    assert_eq!(restart(&nvs), Ok(definition(&old)));
}

#[test]
fn the_same_app_already_stored_skips_only_the_write() {
    // Also the boot whose app allocation failed: the record is valid, the
    // runtime has no app, and Install must still activate it.
    let card = titled(b"Same");
    let mut nvs = Nvs::with(None, Some(record(4, &card)));
    installed(&install(&mut nvs, &card), &card);
    assert!(nvs.writes.is_empty());
    // A readback that may come from a cache proves nothing: write gen + 1.
    let mut cached = Nvs::with(None, Some(record(4, &card))).cached();
    installed(&install(&mut cached, &card), &card);
    assert_eq!(cached.slots, [Some(record(5, &card)), Some(record(4, &card))]);
}

#[test]
fn a_card_that_changed_since_the_offer_writes_nothing() {
    let (offered, other) = (titled(b"Offered"), titled(b"Other"));
    let cases = [
        (Ok(other), Refusal::Changed),
        (Err(Refusal::Damaged), Refusal::Changed),
        (Err(Refusal::Newer), Refusal::Changed),
        (Err(Refusal::Unreadable), Refusal::Unreadable),
    ];
    for (card, refusal) in cases {
        let mut nvs = Nvs::empty();
        untouched(&install_offered(&mut nvs, &offered, card), Install::Refused(refusal));
        assert!(nvs.writes.is_empty(), "{refusal:?}");
    }
}

#[test]
fn a_hash_failure_writes_nothing() {
    let card = titled(b"First");
    let mut nvs = Nvs::empty();
    let outcome = with_failing_hash(reader_hash::Fail::Every, || install(&mut nvs, &card));
    untouched(&outcome, Install::Refused(Refusal::HashFailed));
    // The new record's own hash fails after the card verified.
    let sealed = record(1, &card)[..659].to_vec();
    let outcome = with_failing_hash(reader_hash::Fail::Input(sealed), || install(&mut nvs, &card));
    untouched(&outcome, Install::Refused(Refusal::HashFailed));
    assert!(nvs.writes.is_empty());
}

fn with_failing_hash<T>(fail: reader_hash::Fail, work: impl FnOnce() -> T) -> T {
    reader_hash::FAIL.set(Some(fail));
    let result = work();
    reader_hash::FAIL.set(None);
    result
}

#[test]
fn any_app_replaces_any_other() {
    // Owner decision 4: versions are unsigned author claims.
    let old = app(b"card-app", b"9.9.9", b"Old");
    let lower = app(b"card-app", b"1.0.0", b"Lower");
    let other = app(b"another-app", b"0.0.1", b"Other");
    let mut nvs = Nvs::with(Some(record(2, &old)), None);
    installed(&install(&mut nvs, &lower), &lower);
    installed(&install(&mut nvs, &other), &other);
    assert_eq!(restart(&nvs), Ok(definition(&other)));
}

#[test]
fn reinstalling_pocket_pause_brings_back_the_bundled_owner() {
    // The explicit fallback writes a newer generation, so the older app
    // record cannot come back.
    let mut nvs = Nvs::with(Some(record(2, &titled(b"Old"))), None);
    installed(&install(&mut nvs, BUILTIN_BYTES), BUILTIN_BYTES);
    let restored = restart(&nvs).expect("the reinstalled app loads");
    assert!(restored == bundled() && is_pocket_pause(&restored));
}
