// SPDX-License-Identifier: MIT
//! Read-only boot path from firmware/package_store.h and app_package_load.h.
//! The two envelope copies are fallible boot-only heap scratch, never held
//! beside the running app. Tiny's existing decoder alone interprets payloads.
use newt_sdk_heap::Fixed;
use newt_core::durable::{Backend, BlobRead, SLOT_KEYS, StorageOpen};
use newt_tiny_app::{Definition, WIRE_BYTES, decode, pause::BUILTIN_BYTES};

const RECORD_BYTES: usize = 691;
const HASH_OFFSET: usize = 659;
const BUILTIN_DIGEST: [u8; 32] = [
    0xa0, 0xfc, 0x8e, 0xe6, 0x51, 0xcd, 0x3e, 0x50, 0xcf, 0x96, 0x49, 0xe1,
    0x6b, 0x01, 0x4e, 0xcf, 0x3c, 0xf7, 0x6b, 0x78, 0xe7, 0xa7, 0x1a, 0x43,
    0xcb, 0x66, 0xe1, 0x3e, 0xcc, 0xa5, 0x80, 0xab,
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status { Loaded, Empty, Corrupt, Unsupported, Ambiguous, Failed }
#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot { Missing, Valid, Corrupt, Unsupported, Failed }

/// The real PackageStore allocation: borrowed backend, two private envelopes,
/// their generations and the selected slot. No unused parity backing.
#[repr(C)]
struct BootStore<'a, B> {
    store: &'a mut B,
    bytes: [[u8; RECORD_BYTES]; 2],
    generations: [u32; 2],
    selected: u32,
}
#[cfg(target_pointer_width = "32")]
const _: () = assert!(core::mem::size_of::<BootStore<'_, crate::nvs::NvsBlobBackend<'_, crate::sdk::Sdk552>>>() == 1400);

/// Confirmed absence alone selects the bundled Pocket Pause. Every error
/// leaves storage untouched and returns the frozen unavailable explanation.
pub fn load_boot<B: Backend>(store: &mut B, app: &mut Definition) -> Result<(), &'static [u8]> {
    let Some(mut owner) = Fixed::try_new_with(|| BootStore {
        store, bytes: [[0; RECORD_BYTES]; 2], generations: [0; 2], selected: 0,
    }) else {
        return Err(b"The saved app could not be read. Please preserve its storage.");
    };
    let result = owner.load(app);
    match result {
        Status::Loaded => Ok(()),
        Status::Empty => {
            if validate(BUILTIN_BYTES, &BUILTIN_DIGEST, app) == Status::Loaded { Ok(()) }
            else { Err(b"The bundled app did not pass its integrity check.") }
        }
        Status::Corrupt => Err(b"The saved app did not pass its integrity check."),
        Status::Unsupported => Err(b"The saved app needs a different Newt package version."),
        Status::Ambiguous => Err(b"The saved app generations conflict. Recovery is needed."),
        _ => Err(b"The saved app could not be read. Please preserve its storage."),
    }
}

fn validate(bytes: &[u8], expected: &[u8], out: &mut Definition) -> Status {
    if bytes.len() >= 8 && &bytes[..7] == b"NEWTAPP" && bytes[7] != b'1' {
        return Status::Unsupported;
    }
    if bytes.len() != WIRE_BYTES { return Status::Corrupt; }
    let mut digest = [0; 32];
    if !crate::reader_hash::package_hash(bytes, &mut digest) { return Status::Failed; }
    if digest != expected || !decode(Some(bytes), out) { return Status::Corrupt; }
    Status::Loaded
}

impl<B: Backend> BootStore<'_, B> {
fn read_slot(&mut self, index: usize) -> Slot {
    let bytes = &mut self.bytes[index];
    let observed = match self.store.read_blob(SLOT_KEYS[index], bytes) {
        BlobRead::Missing => return Slot::Missing,
        BlobRead::Failed => return Slot::Failed,
        BlobRead::Present(n) => n,
    };
    if observed != RECORD_BYTES {
        if observed > RECORD_BYTES { return Slot::Failed; }
        if observed >= 8 && self.store.read_blob(SLOT_KEYS[index], &mut bytes[..observed]) != BlobRead::Present(observed) {
            return Slot::Failed;
        }
        if observed < 8 { return Slot::Corrupt; }
    }
    if &bytes[..7] == b"NEWTGEN" && bytes[7] != b'1' { return Slot::Unsupported; }
    if observed != RECORD_BYTES || &bytes[..8] != b"NEWTGEN1" { return Slot::Corrupt; }
    let mut digest = [0; 32];
    if !crate::reader_hash::package_hash(&bytes[..HASH_OFFSET], &mut digest) { return Slot::Failed; }
    if digest != bytes[HASH_OFFSET..] { return Slot::Corrupt; }
    let mut app = Definition::default();
    match validate(&bytes[44..HASH_OFFSET], &bytes[12..44], &mut app) {
        Status::Unsupported => Slot::Unsupported,
        Status::Failed => Slot::Failed,
        Status::Loaded => {
            self.generations[index] = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
            Slot::Valid
        }
        _ => Slot::Corrupt,
    }
}

fn load(&mut self, app: &mut Definition) -> Status {
    match self.store.begin("newt_apps", true) {
        StorageOpen::Missing => return Status::Empty,
        StorageOpen::Opened => {}
        _ => return Status::Failed,
    }
    // Both slots are read even if the first fails, exactly as readAndSelect.
    let first = self.read_slot(0);
    let second = self.read_slot(1);
    let result = self.select(first, second);
    self.store.end();
    if result != Status::Loaded { return result; }
    // Already validated in this private store; no second SDK hash/read.
    if decode(Some(&self.bytes[self.selected as usize][44..HASH_OFFSET]), app) { Status::Loaded }
    else { Status::Corrupt }
}

fn select(&mut self, first: Slot, second: Slot) -> Status {
    if first == Slot::Failed || second == Slot::Failed { return Status::Failed; }
    if first == Slot::Unsupported || second == Slot::Unsupported { return Status::Unsupported; }
    self.selected = match (first, second) {
        (Slot::Valid, Slot::Valid) => match self.generations[0].wrapping_sub(self.generations[1]) {
            0 if self.bytes[0] != self.bytes[1] => return Status::Ambiguous,
            0 => 0,
            0x8000_0000 => return Status::Ambiguous,
            delta => u32::from(delta >= 0x8000_0000),
        },
        (Slot::Valid, _) => 0,
        (_, Slot::Valid) => 1,
        (Slot::Missing, Slot::Missing) => return Status::Empty,
        _ => return Status::Corrupt,
    };
    Status::Loaded
}
}
