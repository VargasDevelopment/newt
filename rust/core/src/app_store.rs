// SPDX-License-Identifier: MIT
//! The installed-app record, read only: NVS `newt_apps`, slots slot0 and
//! slot1, each `NEWTGEN1 | generation u32 LE | package SHA-256 | package |
//! record SHA-256`. The app decoder sits above core, so a `Payload` port
//! supplies the hash and the decoder's verdict. Validation order and the
//! Failed, then Unsupported, then selection precedence are the shipped ones.
use crate::durable::{Backend, BlobRead, SLOT_KEYS, StorageOpen};

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

/// One package against its expected digest. A newer package version is
/// Unsupported before any size or hash check.
pub fn check_package(payload: &mut impl Payload, bytes: &[u8], expected: &[u8]) -> Package {
    if bytes.len() >= 8 && &bytes[..7] == b"NEWTAPP" && bytes[7] != b'1' {
        return Package::Unsupported;
    }
    let Ok(wire) = <&[u8; PACKAGE_BYTES]>::try_from(bytes) else {
        return Package::Corrupt;
    };
    let mut digest = [0; 32];
    if !payload.hash(wire, &mut digest) {
        return Package::Failed;
    }
    if digest != expected || !payload.accepts(wire) {
        return Package::Corrupt;
    }
    Package::Valid
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
    let [first_bytes, second_bytes] = slots;
    let first = read_slot(store, payload, SLOT_KEYS[0], first_bytes);
    let second = read_slot(store, payload, SLOT_KEYS[1], second_bytes);
    store.end();
    select(first, second, slots)
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
