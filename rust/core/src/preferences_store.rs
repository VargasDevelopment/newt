//! Reader preference persistence from firmware/reader_preferences_store.h.
//! Failed writes are unconfirmed: a complete old or new value may survive.
use crate::durable::{
    Backend, BlobRead, Commit, SLOT_KEYS, StorageOpen, Transaction, commit_bytes,
};
use crate::preferences::{BYTES, DecodeError, Preferences};

pub const NAMESPACE: &str = "newt_prefs";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreError {
    Empty,
    Corrupt,
    Unsupported,
    Ambiguous,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Slot {
    Missing,
    Valid,
    Corrupt,
    Unsupported,
    Failed,
}

struct Slots {
    bytes: [[u8; BYTES]; 2],
    values: [Preferences; 2],
}

impl Default for Slots {
    fn default() -> Self {
        Self {
            bytes: [[0; BYTES]; 2],
            values: [Preferences::default(); 2],
        }
    }
}

impl Slots {
    fn read_slot<B: Backend>(&mut self, backend: &mut B, index: usize) -> Slot {
        let bytes = &mut self.bytes[index];
        let observed = match backend.read_blob(SLOT_KEYS[index], bytes) {
            BlobRead::Missing => return Slot::Missing,
            BlobRead::Failed => return Slot::Failed,
            BlobRead::Present(size) if size > BYTES => return Slot::Failed,
            BlobRead::Present(size) => size,
        };
        // Native NVS may not fill a size-mismatched request. Inspect a shorter
        // header only after a second, bounded read confirms its exact extent.
        if observed != BYTES
            && observed >= 8
            && backend.read_blob(SLOT_KEYS[index], &mut bytes[..observed])
                != BlobRead::Present(observed)
        {
            return Slot::Failed;
        }
        match Preferences::decode(&bytes[..observed]) {
            Ok(value) => {
                self.values[index] = value;
                Slot::Valid
            }
            Err(DecodeError::Unsupported) => Slot::Unsupported,
            Err(DecodeError::Corrupt) => Slot::Corrupt,
        }
    }

    fn read_and_select<B: Backend>(
        &mut self,
        backend: &mut B,
    ) -> Result<Option<usize>, StoreError> {
        // Both peers are observed even if the first read failed. Failure wins
        // over unsupported versions, which win over ordinary corruption.
        let first = self.read_slot(backend, 0);
        let second = self.read_slot(backend, 1);
        if first == Slot::Failed || second == Slot::Failed {
            return Err(StoreError::Failed);
        }
        if first == Slot::Unsupported || second == Slot::Unsupported {
            return Err(StoreError::Unsupported);
        }
        match (first, second) {
            (Slot::Valid, Slot::Valid) => {
                let delta = self.values[0]
                    .sequence
                    .wrapping_sub(self.values[1].sequence);
                if delta == 0x8000_0000 || (delta == 0 && self.bytes[0] != self.bytes[1]) {
                    Err(StoreError::Ambiguous)
                } else {
                    Ok(Some(usize::from(delta >= 0x8000_0000)))
                }
            }
            (Slot::Valid, _) => Ok(Some(0)),
            (_, Slot::Valid) => Ok(Some(1)),
            (Slot::Missing, Slot::Missing) => Ok(None),
            _ => Err(StoreError::Corrupt),
        }
    }
}

pub struct PreferencesStore<'a, B: Backend> {
    backend: &'a mut B,
    slots: Slots,
}

impl<'a, B: Backend> PreferencesStore<'a, B> {
    pub fn new(backend: &'a mut B) -> Self {
        Self {
            backend,
            slots: Slots::default(),
        }
    }

    pub fn load(&mut self) -> Result<Preferences, StoreError> {
        match self.backend.begin(NAMESPACE, true) {
            StorageOpen::Missing => return Err(StoreError::Empty),
            StorageOpen::Opened => {}
            _ => return Err(StoreError::Failed),
        }
        let selected = {
            let transaction = Transaction(self.backend);
            self.slots.read_and_select(transaction.0)
        }?;
        selected
            .map(|index| self.slots.values[index])
            .ok_or(StoreError::Empty)
    }

    /// The caller's value changes only after a verified save or an identical
    /// already-durable record. Failed is unconfirmed, never definitely unsaved.
    pub fn save(&mut self, value: &mut Preferences) -> Result<(), StoreError> {
        if value.encode().is_none() {
            return Err(StoreError::Corrupt);
        }
        if self.backend.begin(NAMESPACE, false) != StorageOpen::Opened {
            return Err(StoreError::Failed);
        }
        let decoded = {
            let transaction = Transaction(self.backend);
            let selected = self.slots.read_and_select(transaction.0)?;
            if let Some(index) = selected
                && value.settings == self.slots.values[index].settings
                && transaction.0.readback_durable()
            {
                // Legacy v1 with identical decoded settings stays v1, matching
                // the reference's no-write path rather than forcing migration.
                self.slots.values[index]
            } else {
                let sequence =
                    selected.map_or(1, |index| self.slots.values[index].sequence.wrapping_add(1));
                let target = selected.map_or(0, |index| index ^ 1);
                let proposed = Preferences { sequence, ..*value };
                let encoded = proposed.encode().ok_or(StoreError::Failed)?;
                // Reuse the opposite peer's RAM scratch; its durable backend
                // bytes are untouched while the inactive slot is written.
                let verified = &mut self.slots.bytes[target ^ 1];
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
