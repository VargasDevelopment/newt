// SPDX-License-Identifier: MIT
//! Characterizes the installed-app record loader (NVS `newt_apps`, slot0 and
//! slot1) through the firmware's own `load_boot`, over a backend that answers
//! like `NvsBlobBackend` and a host SHA-256 that can be made to fail.
//! It also holds Tiny's `check` to `decode`, whose verdict the port reports.
//! tests/app_store_test.py runs it with its negative controls.
#[path = "../../firmware/app_runtime/package.rs"]
mod package;

/// Stands in for the X3's mbedTLS provider: real SHA-256, or a provider
/// failure for every input or for one exact input.
mod reader_hash {
    use std::cell::RefCell;
    pub enum Fail {
        Every,
        Input(Vec<u8>),
    }
    thread_local! {
        pub static FAIL: RefCell<Option<Fail>> = const { RefCell::new(None) };
    }
    pub(crate) fn package_hash(input: &[u8], output: &mut [u8; 32]) -> bool {
        let fails = FAIL.with_borrow(|fail| match fail {
            Some(Fail::Every) => true,
            Some(Fail::Input(bytes)) => bytes == input,
            None => false,
        });
        if fails { return false; }
        *output = crate::sha256(input);
        true
    }
}

use newt_core::durable::{Backend, BlobRead, DurableWrite, SLOT_KEYS, StorageOpen};
use newt_tiny_app::{Definition, WIRE_BYTES, check, decode, pause::BUILTIN_BYTES};

const CORRUPT: &str = "The saved app did not pass its integrity check.";
const UNSUPPORTED: &str = "The saved app needs a different Newt package version.";
const AMBIGUOUS: &str = "The saved app generations conflict. Recovery is needed.";
const FAILED: &str = "The saved app could not be read. Please preserve its storage.";
const BUNDLED_BROKEN: &str = "The bundled app did not pass its integrity check.";

#[derive(Clone)]
enum Blob {
    Missing,
    Unreadable,
    Bytes(Vec<u8>),
    /// The size probe succeeds; the exact-size read then fails.
    RereadFails(Vec<u8>),
}

struct Nvs {
    open: StorageOpen,
    slots: [Blob; 2],
    session: bool,
}
impl Nvs {
    fn with(first: Blob, second: Blob) -> Self {
        Self { open: StorageOpen::Opened, slots: [first, second], session: false }
    }
}
impl Backend for Nvs {
    fn begin(&mut self, namespace: &str, readonly: bool) -> StorageOpen {
        assert!(namespace == "newt_apps" && readonly && !self.session, "one read-only newt_apps session");
        self.session = self.open == StorageOpen::Opened;
        self.open
    }
    fn end(&mut self) {
        assert!(self.session, "end without an open session");
        self.session = false;
    }
    fn read_blob(&mut self, key: &str, output: &mut [u8]) -> BlobRead {
        assert!(self.session, "read outside the session");
        let index = SLOT_KEYS.iter().position(|slot| *slot == key).expect("only slot keys are read");
        // NvsBlobBackend reports a size other than the buffer's without copying.
        match &self.slots[index] {
            Blob::Missing => BlobRead::Missing,
            Blob::Unreadable => BlobRead::Failed,
            Blob::Bytes(bytes) | Blob::RereadFails(bytes) if bytes.len() != output.len() => {
                BlobRead::Present(bytes.len())
            }
            Blob::Bytes(bytes) => {
                output.copy_from_slice(bytes);
                BlobRead::Present(bytes.len())
            }
            Blob::RereadFails(_) => BlobRead::Failed,
        }
    }
    fn put_bytes(&mut self, _: DurableWrite, _: &str, _: &[u8]) -> usize {
        panic!("the loader must never write")
    }
    fn readback_durable(&self) -> bool { true }
}

/// The boot result: the loaded app, or the frozen explanation shown instead.
fn boot(nvs: &mut Nvs) -> Result<Definition, &'static str> {
    let mut app = Definition::default();
    let result = package::load_boot(nvs, &mut app);
    assert!(!nvs.session, "the loader closes its session");
    result.map(|()| app).map_err(|message| core::str::from_utf8(message).unwrap())
}

fn boot_slots(first: Blob, second: Blob) -> Result<Definition, &'static str> {
    boot(&mut Nvs::with(first, second))
}

fn with_failing_hash<T>(fail: reader_hash::Fail, work: impl FnOnce() -> T) -> T {
    reader_hash::FAIL.set(Some(fail));
    let result = work();
    reader_hash::FAIL.set(None);
    result
}

/// Pocket Pause's wire under another title: a distinct valid package.
fn package(title: &[u8]) -> [u8; WIRE_BYTES] {
    let mut wire = *BUILTIN_BYTES;
    wire[58..91].fill(0);
    wire[58..58 + title.len()].copy_from_slice(title);
    wire
}

fn app(title: &[u8]) -> Definition {
    let mut definition = Definition::default();
    assert!(decode(Some(&package(title)), &mut definition));
    definition
}

fn bundled() -> Definition {
    let mut definition = Definition::default();
    assert!(decode(Some(BUILTIN_BYTES), &mut definition));
    definition
}

/// NEWTGEN1 | generation u32 LE | package SHA-256 | package | record SHA-256.
fn record(generation: u32, package: &[u8]) -> Vec<u8> {
    let mut bytes = b"NEWTGEN1".to_vec();
    bytes.extend(generation.to_le_bytes());
    bytes.extend(sha256(package));
    bytes.extend(package);
    seal(bytes)
}

fn seal(mut bytes: Vec<u8>) -> Vec<u8> {
    bytes.truncate(659);
    let digest = sha256(&bytes);
    bytes.extend(digest);
    bytes
}

/// A record with one byte flipped, resealed so only that field is wrong.
fn flipped(generation: u32, title: &[u8], at: usize, mask: u8) -> Blob {
    let mut bytes = record(generation, &package(title));
    bytes[at] ^= mask;
    Blob::Bytes(seal(bytes))
}

fn valid(generation: u32, title: &[u8]) -> Blob {
    Blob::Bytes(record(generation, &package(title)))
}

fn bad_record_hash(generation: u32, title: &[u8]) -> Blob {
    let mut bytes = record(generation, &package(title));
    bytes[690] ^= 1;
    Blob::Bytes(bytes)
}

#[test]
fn absence_loads_the_bundled_app() {
    let mut missing = Nvs::with(Blob::Missing, Blob::Missing);
    missing.open = StorageOpen::Missing;
    assert_eq!(boot(&mut missing), Ok(bundled()));
    assert_eq!(boot_slots(Blob::Missing, Blob::Missing), Ok(bundled()));
}

#[test]
fn storage_that_cannot_open_is_failed() {
    for open in [StorageOpen::Failed, StorageOpen::Damaged] {
        let mut nvs = Nvs::with(valid(1, b"First"), Blob::Missing);
        nvs.open = open;
        assert_eq!(boot(&mut nvs), Err(FAILED), "{open:?}");
    }
}

#[test]
fn the_bundled_app_must_pass_its_hash() {
    let fail = reader_hash::Fail::Input(BUILTIN_BYTES.to_vec());
    let result = with_failing_hash(fail, || boot_slots(Blob::Missing, Blob::Missing));
    assert_eq!(result, Err(BUNDLED_BROKEN));
}

#[test]
fn one_valid_slot_wins_over_a_missing_or_corrupt_peer() {
    assert_eq!(boot_slots(valid(4, b"First"), Blob::Missing), Ok(app(b"First")));
    assert_eq!(boot_slots(Blob::Missing, valid(4, b"Second")), Ok(app(b"Second")));
    assert_eq!(boot_slots(valid(1, b"First"), bad_record_hash(9, b"Second")), Ok(app(b"First")));
    assert_eq!(boot_slots(flipped(9, b"First", 0, 1), valid(1, b"Second")), Ok(app(b"Second")));
    // The package digest inside the record names other bytes.
    assert_eq!(boot_slots(flipped(9, b"First", 12, 1), valid(1, b"Second")), Ok(app(b"Second")));
    assert_eq!(boot_slots(valid(1, b"First"), Blob::Bytes(b"NEWT".to_vec())), Ok(app(b"First")));
}

#[test]
fn damage_without_a_valid_slot_is_unavailable() {
    assert_eq!(boot_slots(bad_record_hash(1, b"First"), Blob::Missing), Err(CORRUPT));
    assert_eq!(boot_slots(Blob::Missing, bad_record_hash(1, b"Second")), Err(CORRUPT));
    assert_eq!(boot_slots(bad_record_hash(1, b"First"), flipped(2, b"Second", 0, 1)), Err(CORRUPT));
}

#[test]
fn a_malformed_package_loses_to_its_older_valid_peer() {
    // A card action of 9: the record hash and package digest both match, so
    // only Tiny's decoder rejects it.
    let mut malformed = package(b"Newer");
    malformed[318 + 98] = 9;
    let newer = Blob::Bytes(record(2, &malformed));
    assert_eq!(boot_slots(newer.clone(), valid(1, b"Older")), Ok(app(b"Older")));
    assert_eq!(boot_slots(newer, Blob::Missing), Err(CORRUPT));
}

#[test]
fn a_hash_provider_failure_is_failed_even_beside_a_valid_slot() {
    // Only one slot's record or package hash fails; the other slot validates.
    let records = [record(1, &package(b"First")), record(2, &package(b"Second"))];
    for failing in 0..2 {
        let inputs = [records[failing][..659].to_vec(), records[failing][44..659].to_vec()];
        for (input, part) in inputs.into_iter().zip(["record", "package"]) {
            let result = with_failing_hash(reader_hash::Fail::Input(input), || {
                boot_slots(Blob::Bytes(records[0].clone()), Blob::Bytes(records[1].clone()))
            });
            assert_eq!(result, Err(FAILED), "slot {failing} {part} hash");
        }
    }
}

#[test]
fn versions_are_read_in_order_around_the_hashes() {
    let mut newer_app = package(b"Newer");
    newer_app[7] = b'2';
    // A newer package inside a record whose own hash is wrong is damage, so a
    // valid peer still wins: the package version is read only after the record hash.
    let mut damaged = record(9, &newer_app);
    damaged[690] ^= 1;
    assert_eq!(boot_slots(Blob::Bytes(damaged.clone()), valid(1, b"Older")), Ok(app(b"Older")));
    assert_eq!(boot_slots(Blob::Bytes(damaged), Blob::Missing), Err(CORRUPT));
    // A newer record version is read before any hash runs.
    let every = || reader_hash::Fail::Every;
    let newer_record = || flipped(1, b"Newer", 7, 3);
    assert_eq!(with_failing_hash(every(), || boot_slots(newer_record(), Blob::Missing)), Err(UNSUPPORTED));
    // A newer package version is read before the package hash, after the record hash.
    let sealed = record(9, &newer_app);
    let package_only = reader_hash::Fail::Input(sealed[44..659].to_vec());
    assert_eq!(with_failing_hash(package_only, || boot_slots(Blob::Bytes(sealed.clone()), Blob::Missing)),
               Err(UNSUPPORTED));
    assert_eq!(with_failing_hash(every(), || boot_slots(Blob::Bytes(sealed), Blob::Missing)), Err(FAILED));
}

#[test]
fn generations_use_serial_order() {
    assert_eq!(boot_slots(valid(1, b"First"), valid(2, b"Second")), Ok(app(b"Second")));
    assert_eq!(boot_slots(valid(2, b"First"), valid(1, b"Second")), Ok(app(b"First")));
    assert_eq!(boot_slots(valid(u32::MAX, b"First"), valid(0, b"Second")), Ok(app(b"Second")));
    assert_eq!(boot_slots(valid(0, b"First"), valid(u32::MAX, b"Second")), Ok(app(b"First")));
    assert_eq!(boot_slots(valid(7, b"Same"), valid(7, b"Same")), Ok(app(b"Same")));
}

#[test]
fn conflicting_generations_are_ambiguous() {
    assert_eq!(boot_slots(valid(7, b"First"), valid(7, b"Second")), Err(AMBIGUOUS));
    assert_eq!(boot_slots(valid(0x8000_0000, b"First"), valid(0, b"Second")), Err(AMBIGUOUS));
    assert_eq!(boot_slots(valid(0, b"First"), valid(0x8000_0000, b"Second")), Err(AMBIGUOUS));
}

#[test]
fn failed_outranks_unsupported_which_outranks_selection() {
    // NEWTGEN2: b'1' ^ 3 is b'2'.
    let newer_record = || flipped(1, b"Newer", 7, 3);
    let mut newer_app = package(b"Newer");
    newer_app[7] = b'2';
    let newer_package = || Blob::Bytes(record(9, &newer_app));
    assert_eq!(boot_slots(Blob::Unreadable, newer_record()), Err(FAILED));
    assert_eq!(boot_slots(newer_record(), Blob::Unreadable), Err(FAILED));
    assert_eq!(boot_slots(Blob::Unreadable, valid(1, b"Second")), Err(FAILED));
    assert_eq!(boot_slots(newer_record(), valid(1, b"Second")), Err(UNSUPPORTED));
    assert_eq!(boot_slots(valid(1, b"First"), newer_record()), Err(UNSUPPORTED));
    assert_eq!(boot_slots(valid(1, b"First"), newer_package()), Err(UNSUPPORTED));
}

#[test]
fn blob_sizes_and_reads() {
    let mut long = record(1, &package(b"Long"));
    long.push(0);
    assert_eq!(boot_slots(Blob::Bytes(long), Blob::Missing), Err(FAILED));
    assert_eq!(boot_slots(Blob::Unreadable, Blob::Missing), Err(FAILED));
    assert_eq!(boot_slots(Blob::Bytes(b"NEWTGEN".to_vec()), Blob::Missing), Err(CORRUPT));
    let short = record(1, &package(b"Short"))[..600].to_vec();
    assert_eq!(boot_slots(Blob::Bytes(short.clone()), Blob::Missing), Err(CORRUPT));
    assert_eq!(boot_slots(Blob::RereadFails(short), Blob::Missing), Err(FAILED));
    // The record version is read even from a short blob.
    assert_eq!(boot_slots(Blob::Bytes(b"NEWTGEN2".to_vec()), Blob::Missing), Err(UNSUPPORTED));
}

#[test]
fn check_agrees_with_decode_over_a_wire_corpus() {
    // (what changes, the byte, its new value, whether the wire stays valid)
    let corpus: [(&str, usize, u8, bool); 12] = [
        ("nothing", 58, b'P', true),
        ("another title", 58, b'Q', true),
        ("a newer package version", 7, b'2', false),
        ("an id that starts upper case", 8, b'P', false),
        ("a version that is not x.y.z", 42, b'x', false),
        ("a title that ends before it starts", 58, 0, false),
        ("a control byte in the subtitle", 91, 7, false),
        ("no cards", 317, 0, false),
        ("four cards", 317, 4, false),
        ("a third card beyond the count", 317, 2, false),
        ("an unknown card action", 318 + 98, 4, false),
        ("no card action", 318 + 98, 0, false),
    ];
    for (change, at, value, valid) in corpus {
        let mut wire = *BUILTIN_BYTES;
        wire[at] = value;
        let mut definition = Definition::default();
        assert_eq!(check(&wire), valid, "check: {change}");
        assert_eq!(decode(Some(&wire), &mut definition), valid, "decode: {change}");
    }
    let mut unterminated = *BUILTIN_BYTES;
    unterminated[8..41].fill(b'a');
    assert!(!check(&unterminated) && !decode(Some(&unterminated), &mut Definition::default()));
    assert!(!decode(Some(&BUILTIN_BYTES[1..]), &mut Definition::default()));
}

/// FIPS 180-4 SHA-256, as in slot_vectors: each packet keeps its own inputs,
/// so a change here selects only this test's controls. The bundled app's
/// pinned digest proves it.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut state: [u32; 8] =
        [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut message = data.to_vec();
    message.push(0x80);
    message.resize((message.len() + 8).next_multiple_of(64) - 8, 0);
    message.extend_from_slice(&(data.len() as u64 * 8).to_be_bytes());
    for block in message.chunks(64) {
        let mut w = [0u32; 64];
        for (word, chunk) in w.iter_mut().zip(block.chunks(4)) {
            *word = u32::from_be_bytes(chunk.try_into().expect("4 bytes"));
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = state;
        for (k, w) in K.iter().zip(w) {
            let [a, b, c, d, e, f, g, h] = v;
            let t1 = h
                .wrapping_add(e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25))
                .wrapping_add((e & f) ^ (!e & g))
                .wrapping_add(*k)
                .wrapping_add(w);
            let t2 = (a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22))
                .wrapping_add((a & b) ^ (a & c) ^ (b & c));
            v = [t1.wrapping_add(t2), a, b, c, d.wrapping_add(t1), e, f, g];
        }
        for (word, add) in state.iter_mut().zip(v) {
            *word = word.wrapping_add(add);
        }
    }
    let mut digest = [0; 32];
    for (chunk, word) in digest.chunks_mut(4).zip(state) {
        chunk.copy_from_slice(&word.to_be_bytes());
    }
    digest
}
