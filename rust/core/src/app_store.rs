// SPDX-License-Identifier: MIT
//! The installed-app record: NVS `newt_apps`, slots slot0 and slot1, each
//! `NEWTGEN1 | generation u32 LE | package SHA-256 | package | record SHA-256`.
//! The app decoder sits above core, so a `Payload` port supplies the hash and
//! the decoder's verdict. Validation order and the Failed, then Unsupported,
//! then selection precedence are the shipped ones. `install` is the only
//! writer: it never writes over the selected slot, and the running app changes
//! only after a fresh selection reads the new record back.
use crate::durable::{Backend, BlobRead, Commit, SLOT_KEYS, StorageOpen, Transaction, commit_bytes};

pub const NAMESPACE: &str = "newt_apps";
pub const PACKAGE_BYTES: usize = 615;
pub const RECORD_BYTES: usize = 691;
const DIGEST_AT: usize = 12;
const PACKAGE_AT: usize = 44;
const HASH_AT: usize = 659;
const _: () = assert!(PACKAGE_AT + PACKAGE_BYTES == HASH_AT && HASH_AT + 32 == RECORD_BYTES);

/// Both slot copies, lent by the caller for one load.
pub type Slots = [[u8; RECORD_BYTES]; 2];

pub trait Payload {
    /// SHA-256 of `bytes`. False is a provider failure, never damage.
    fn hash(&mut self, bytes: &[u8], digest: &mut [u8; 32]) -> bool;
    /// True only if the app decoder accepts the complete wire.
    fn accepts(&mut self, wire: &[u8; PACKAGE_BYTES]) -> bool;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Package {
    Valid,
    Corrupt,
    Unsupported,
    Failed,
}

/// The selected slot's index, or why no record can load.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Load {
    Selected(usize),
    Empty,
    Corrupt,
    Unsupported,
    Ambiguous,
    Failed,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Slot {
    Missing,
    Valid(u32),
    Corrupt,
    Unsupported,
    Failed,
}

/// Why an install wrote nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// The card file is not a valid NEWTAPP1 package.
    Damaged,
    /// The card file is a newer package version.
    Newer,
    /// More than one app file waits in the folder.
    Many,
    /// The folder, its listing or the file could not be read.
    Unreadable,
    /// The card file no longer matches the package that was offered.
    Changed,
    /// The SHA-256 provider failed, so nothing was judged.
    HashFailed,
    /// The record could not be opened for writing, or is Failed,
    /// Unsupported or Ambiguous: its bytes may be the only copy of an app.
    Storage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum Install {
    /// Read back, selected, and handed to the activation callback.
    Installed,
    /// Nothing was written and the running app is unchanged.
    Refused(Refusal),
    /// A write was attempted but not verified. The running app is unchanged;
    /// after a restart a complete old or new record may load, never "unsaved".
    Unconfirmed,
}

/// One package against its expected digest. A newer package version is
/// Unsupported before any size or hash check.
pub fn check_package(payload: &mut impl Payload, bytes: &[u8], expected: &[u8]) -> Package {
    let mut digest = [0; 32];
    match identify(payload, bytes, &mut digest) {
        Package::Valid if digest != expected => Package::Corrupt,
        verdict => verdict,
    }
}

/// One package's own verdict and its SHA-256, which names it.
pub fn identify(payload: &mut impl Payload, bytes: &[u8], digest: &mut [u8; 32]) -> Package {
    if newer_package(bytes) {
        return Package::Unsupported;
    }
    let Ok(wire) = <&[u8; PACKAGE_BYTES]>::try_from(bytes) else {
        return Package::Corrupt;
    };
    if !payload.hash(wire, digest) {
        return Package::Failed;
    }
    if !payload.accepts(wire) {
        return Package::Corrupt;
    }
    Package::Valid
}

/// A NEWTAPP header of another version: read before size, so a newer
/// format of any length is never called damaged.
pub(crate) fn newer_package(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && &bytes[..7] == b"NEWTAPP" && bytes[7] != b'1'
}

/// The package inside a record.
pub fn package(record: &[u8; RECORD_BYTES]) -> &[u8] {
    &record[PACKAGE_AT..HASH_AT]
}

/// Reads both slots, even after one fails, then selects. Storage is never
/// written; confirmed absence alone is Empty.
pub fn load(store: &mut impl Backend, payload: &mut impl Payload, slots: &mut Slots) -> Load {
    match store.begin(NAMESPACE, true) {
        StorageOpen::Missing => return Load::Empty,
        StorageOpen::Opened => {}
        _ => return Load::Failed,
    }
    let [first, second] = read_slots(store, payload, slots);
    store.end();
    select(first, second, slots)
}

/// Installs the package `offered` names, after the user chose it. `read_card`
/// fills a wire from the card file again, so a file that changed since the
/// offer is refused. The lent slots are the only buffers. `activate` receives
/// the selected record's package only after it was committed, read back and
/// selected; on every other outcome the running app is left alone.
pub fn install(
    store: &mut impl Backend,
    payload: &mut impl Payload,
    slots: &mut Slots,
    offered: &[u8; 32],
    read_card: impl FnOnce(&mut [u8; PACKAGE_BYTES]) -> Result<(), Refusal>,
    activate: impl FnOnce(&[u8; PACKAGE_BYTES]),
) -> Install {
    if store.begin(NAMESPACE, false) != StorageOpen::Opened {
        return Install::Refused(Refusal::Storage);
    }
    let verified = {
        let session = Transaction(store);
        commit_and_verify(session.0, payload, slots, offered, read_card)
    };
    match verified {
        Ok(wire) => {
            activate(wire);
            Install::Installed
        }
        Err(outcome) => outcome,
    }
}

/// Steps before activation; Ok is the package of the slot that now selects it.
fn commit_and_verify<'s>(
    store: &mut impl Backend,
    payload: &mut impl Payload,
    slots: &'s mut Slots,
    offered: &[u8; 32],
    read_card: impl FnOnce(&mut [u8; PACKAGE_BYTES]) -> Result<(), Refusal>,
) -> Result<&'s [u8; PACKAGE_BYTES], Install> {
    let states = read_slots(store, payload, slots);
    let load = select(states[0], states[1], slots);
    let (target, generation) = target(load, states).ok_or(Install::Refused(Refusal::Storage))?;
    let [first, second] = slots;
    let (readback, record) = if target == 0 { (first, second) } else { (second, first) };
    // The card is read into the buffer that is not written, which may hold
    // the selected record: its digest field stays intact for the check below.
    let wire = <&mut [u8; PACKAGE_BYTES]>::try_from(&mut record[PACKAGE_AT..HASH_AT])
        .map_err(|_| Install::Refused(Refusal::Storage))?;
    read_card(wire).map_err(|refusal| {
        Install::Refused(if refusal == Refusal::Unreadable { refusal } else { Refusal::Changed })
    })?;
    match check_package(payload, wire, offered) {
        Package::Valid => {}
        Package::Failed => return Err(Install::Refused(Refusal::HashFailed)),
        _ => return Err(Install::Refused(Refusal::Changed)),
    }
    // Same app already stored: skip only the write, and only when a readback
    // proves durable bytes. Verification and activation still run.
    let stored = matches!(load, Load::Selected(_)) && record[DIGEST_AT..PACKAGE_AT] == *offered;
    if !(stored && store.readback_durable()) {
        record[..8].copy_from_slice(b"NEWTGEN1");
        record[8..DIGEST_AT].copy_from_slice(&generation.to_le_bytes());
        record[DIGEST_AT..PACKAGE_AT].copy_from_slice(offered);
        let mut digest = [0; 32];
        if !payload.hash(&record[..HASH_AT], &mut digest) {
            return Err(Install::Refused(Refusal::HashFailed));
        }
        record[HASH_AT..].copy_from_slice(&digest);
        if commit_bytes(store, SLOT_KEYS[target], record, readback) != Commit::Saved {
            return Err(Install::Unconfirmed);
        }
    }
    let [first, second] = read_slots(store, payload, slots);
    match select(first, second, slots) {
        Load::Selected(slot) if slots[slot][DIGEST_AT..PACKAGE_AT] == *offered => {
            <&[u8; PACKAGE_BYTES]>::try_from(package(&slots[slot])).map_err(|_| Install::Unconfirmed)
        }
        _ => Err(Install::Unconfirmed),
    }
}

/// The slot an install writes and its generation (owner decisions 1 and 2):
/// never the selected slot; a Corrupt slot's generation is untrusted, and it
/// can never validate again, so 1 outranks it. Corrupt beside Missing writes
/// the Missing slot and leaves the damaged bytes as they are.
fn target(load: Load, states: [Slot; 2]) -> Option<(usize, u32)> {
    match load {
        Load::Selected(slot) => match states[slot] {
            Slot::Valid(generation) => Some((slot ^ 1, generation.wrapping_add(1))),
            _ => None,
        },
        // Only Missing and Corrupt remain here.
        Load::Empty | Load::Corrupt => Some((usize::from(states == [Slot::Corrupt, Slot::Missing]), 1)),
        _ => None,
    }
}

fn read_slots(store: &mut impl Backend, payload: &mut impl Payload, slots: &mut Slots) -> [Slot; 2] {
    let [first, second] = slots;
    [read_slot(store, payload, SLOT_KEYS[0], first), read_slot(store, payload, SLOT_KEYS[1], second)]
}

fn read_slot(
    store: &mut impl Backend,
    payload: &mut impl Payload,
    key: &str,
    bytes: &mut [u8; RECORD_BYTES],
) -> Slot {
    let observed = match store.read_blob(key, bytes) {
        BlobRead::Missing => return Slot::Missing,
        BlobRead::Failed => return Slot::Failed,
        BlobRead::Present(n) => n,
    };
    if observed != RECORD_BYTES {
        if observed > RECORD_BYTES {
            return Slot::Failed;
        }
        // The size probe copied nothing; read a short blob at its exact size.
        if observed >= 8 && store.read_blob(key, &mut bytes[..observed]) != BlobRead::Present(observed) {
            return Slot::Failed;
        }
        if observed < 8 {
            return Slot::Corrupt;
        }
    }
    if &bytes[..7] == b"NEWTGEN" && bytes[7] != b'1' {
        return Slot::Unsupported;
    }
    if observed != RECORD_BYTES || &bytes[..8] != b"NEWTGEN1" {
        return Slot::Corrupt;
    }
    let mut digest = [0; 32];
    if !payload.hash(&bytes[..HASH_AT], &mut digest) {
        return Slot::Failed;
    }
    if digest != bytes[HASH_AT..] {
        return Slot::Corrupt;
    }
    match check_package(payload, &bytes[PACKAGE_AT..HASH_AT], &bytes[DIGEST_AT..PACKAGE_AT]) {
        Package::Valid => Slot::Valid(u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]])),
        Package::Unsupported => Slot::Unsupported,
        Package::Failed => Slot::Failed,
        Package::Corrupt => Slot::Corrupt,
    }
}

/// Generations use serial arithmetic, so a wrap from u32::MAX to 0 is newer.
fn select(first: Slot, second: Slot, slots: &Slots) -> Load {
    if first == Slot::Failed || second == Slot::Failed {
        return Load::Failed;
    }
    if first == Slot::Unsupported || second == Slot::Unsupported {
        return Load::Unsupported;
    }
    match (first, second) {
        (Slot::Valid(a), Slot::Valid(b)) => match a.wrapping_sub(b) {
            0 if slots[0] != slots[1] => Load::Ambiguous,
            0 => Load::Selected(0),
            0x8000_0000 => Load::Ambiguous,
            delta => Load::Selected(usize::from(delta >= 0x8000_0000)),
        },
        (Slot::Valid(_), _) => Load::Selected(0),
        (_, Slot::Valid(_)) => Load::Selected(1),
        (Slot::Missing, Slot::Missing) => Load::Empty,
        _ => Load::Corrupt,
    }
}
