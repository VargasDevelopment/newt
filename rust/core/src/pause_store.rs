// SPDX-License-Identifier: MIT
//! Strict Pocket Pause A/B records from firmware/pause_preferences_store.h.
use crate::durable::{
    Backend, BlobRead, Commit, DurableWrite, SLOT_KEYS, StorageOpen, Transaction, commit_bytes,
};
use crate::pause::{DecodeError, PREFERENCES_BYTES as BYTES, Preferences};

pub const NAMESPACE: &str = "newt_pause";
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreError {
    Empty,
    Corrupt,
    Unsupported,
    Ambiguous,
    Failed,
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum Slot {
    Missing,
    Valid,
    Corrupt,
    Unsupported,
    Failed,
}

pub struct Store<'a, B: Backend> {
    backend: &'a mut B,
    bytes: [[u8; BYTES]; 2],
    values: [Preferences; 2],
}
impl<'a, B: Backend> Store<'a, B> {
    pub fn new(backend: &'a mut B) -> Self {
        Self {
            backend,
            bytes: [[0; BYTES]; 2],
            values: [Preferences::default(); 2],
        }
    }

    pub fn load(&mut self) -> Result<Preferences, StoreError> {
        match self.backend.begin(NAMESPACE, true) {
            StorageOpen::Missing => return Err(StoreError::Empty),
            StorageOpen::Opened => {}
            _ => return Err(StoreError::Failed),
        }
        let transaction = Transaction(self.backend);
        let selected = read_and_select(transaction.0, &mut self.bytes, &mut self.values)?;
        selected
            .map(|index| self.values[index])
            .ok_or(StoreError::Empty)
    }

    /// Failed acknowledgement leaves the caller's running choice unchanged.
    /// A complete old or new durable generation may still load after restart.
    pub fn save(&mut self, value: &mut Preferences) -> Result<(), StoreError> {
        let mut encoded = [0; BYTES];
        if !value.encode_into(&mut encoded) {
            return Err(StoreError::Corrupt);
        }
        if self.backend.begin(NAMESPACE, false) != StorageOpen::Opened {
            return Err(StoreError::Failed);
        }
        let decoded = {
            let transaction = Transaction(self.backend);
            let selected = read_and_select(transaction.0, &mut self.bytes, &mut self.values)?;
            if let Some(index) = selected
                && value.enabled_mask == self.values[index].enabled_mask
                && transaction.0.readback_durable()
            {
                self.values[index]
            } else {
                let proposed = Preferences {
                    sequence: selected
                        .map_or(1, |index| self.values[index].sequence.wrapping_add(1)),
                    ..*value
                };
                let target = selected.map_or(0, |index| index ^ 1);
                if !proposed.encode_into(&mut encoded) {
                    return Err(StoreError::Failed);
                }
                let verified = &mut self.bytes[target ^ 1];
                if commit_bytes(transaction.0, SLOT_KEYS[target], &encoded, verified)
                    != Commit::Saved
                {
                    return Err(StoreError::Failed);
                }
                Preferences::decode(verified).map_err(|_| StoreError::Failed)?
            }
        };
        *value = decoded;
        Ok(())
    }
}

fn read_slot<B: Backend>(
    backend: &mut B,
    bytes: &mut [u8; BYTES],
    value: &mut Preferences,
    index: usize,
) -> Slot {
    let observed = match backend.read_blob(SLOT_KEYS[index], bytes) {
        BlobRead::Missing => return Slot::Missing,
        BlobRead::Failed => return Slot::Failed,
        BlobRead::Present(size) if size > BYTES => return Slot::Failed,
        BlobRead::Present(size) => size,
    };
    if observed != BYTES
        && observed >= 8
        && backend.read_blob(SLOT_KEYS[index], &mut bytes[..observed])
            != BlobRead::Present(observed)
    {
        return Slot::Failed;
    }
    match Preferences::decode(&bytes[..observed]) {
        Ok(decoded) => {
            *value = decoded;
            Slot::Valid
        }
        Err(DecodeError::Unsupported) => Slot::Unsupported,
        Err(DecodeError::Corrupt) => Slot::Corrupt,
    }
}

fn read_and_select<B: Backend>(
    backend: &mut B,
    bytes: &mut [[u8; BYTES]; 2],
    values: &mut [Preferences; 2],
) -> Result<Option<usize>, StoreError> {
    let first = read_slot(backend, &mut bytes[0], &mut values[0], 0);
    let second = read_slot(backend, &mut bytes[1], &mut values[1], 1);
    if first == Slot::Failed || second == Slot::Failed {
        return Err(StoreError::Failed);
    }
    if first == Slot::Unsupported || second == Slot::Unsupported {
        return Err(StoreError::Unsupported);
    }
    // Pocket Pause deliberately blocks a corrupt peer, even beside a valid one.
    // Reader preferences have a different recovery policy; do not reuse it.
    if first == Slot::Corrupt || second == Slot::Corrupt {
        return Err(StoreError::Corrupt);
    }
    match (first, second) {
        (Slot::Missing, Slot::Missing) => Ok(None),
        (Slot::Valid, Slot::Valid) => {
            let delta = values[0].sequence.wrapping_sub(values[1].sequence);
            if delta == 0x8000_0000 || (delta == 0 && bytes[0] != bytes[1]) {
                Err(StoreError::Ambiguous)
            } else {
                Ok(Some(usize::from(delta >= 0x8000_0000)))
            }
        }
        (Slot::Valid, _) => Ok(Some(0)),
        (_, Slot::Valid) => Ok(Some(1)),
        _ => Err(StoreError::Failed),
    }
}

/// Restricts the existing NVS backend to the exact Pause namespace and keys.
/// It owns no native handle; the caller supplies NvsBlobBackend in firmware.
pub struct Namespace<'a, B: Backend> {
    backend: &'a mut B,
    readonly: bool,
}
impl<'a, B: Backend> Namespace<'a, B> {
    pub fn new(backend: &'a mut B) -> Self {
        Self {
            backend,
            readonly: true,
        }
    }
}
fn allowed_key(key: &str) -> bool {
    SLOT_KEYS.contains(&key)
}
impl<B: Backend> Backend for Namespace<'_, B> {
    fn begin(&mut self, namespace: &str, readonly: bool) -> StorageOpen {
        if namespace != NAMESPACE {
            return StorageOpen::Failed;
        }
        self.readonly = readonly;
        self.backend.begin(NAMESPACE, readonly)
    }
    fn end(&mut self) {
        self.backend.end();
        self.readonly = true;
    }
    fn read_blob(&mut self, key: &str, bytes: &mut [u8]) -> BlobRead {
        if !allowed_key(key) || !(8..=BYTES).contains(&bytes.len()) {
            return BlobRead::Failed;
        }
        self.backend.read_blob(key, bytes)
    }
    fn put_bytes(&mut self, permit: DurableWrite, key: &str, bytes: &[u8]) -> usize {
        if self.readonly || !allowed_key(key) || bytes.len() != BYTES {
            return 0;
        }
        self.backend.put_bytes(permit, key, bytes)
    }
    fn readback_durable(&self) -> bool {
        self.backend.readback_durable()
    }
}
