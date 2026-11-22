//! The reading-progress store, including protected checkpoints and peer locators.
//! Source: firmware/progress_store.h. Opening may borrow the sole snapshot
//! synchronously; adapters return metadata, never retain pointers or slot copies.
use crate::durable::{
    Backend, BlobRead, Commit, Load, SLOT_KEYS, StorageOpen, Transaction, commit_bytes,
};
use crate::progress::{self, BYTES, Locator, Progress, Selection};
use crate::wire::get32;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PeerLocators {
    pub slots: [Locator; 2],
    pub present: u8,
}

struct Slots {
    bytes: [[u8; BYTES]; 2],
    sizes: [usize; 2],
    snapshot: Option<[BlobRead; 2]>,
}

impl Default for Slots {
    fn default() -> Self {
        Self {
            bytes: [[0; BYTES]; 2],
            sizes: [0; 2],
            snapshot: None,
        }
    }
}

impl Slots {
    fn read<B: Backend>(&mut self, backend: &mut B, strict: bool) -> Option<bool> {
        let snapshot = self.snapshot.take();
        let mut present = false;
        for (index, key) in SLOT_KEYS.iter().enumerate() {
            self.sizes[index] = 0;
            match snapshot.map_or_else(
                || backend.read_blob(key, &mut self.bytes[index]),
                |reads| reads[index],
            ) {
                BlobRead::Failed => return None,
                BlobRead::Missing => {}
                BlobRead::Present(observed) => {
                    present = true;
                    if observed == BYTES {
                        self.sizes[index] = BYTES;
                    }
                    if strict
                        && (observed < 8
                            || observed > BYTES
                            || &self.bytes[index][..4] != b"NWTR"
                            || get32(&self.bytes[index], 4) != 1
                            || (observed >= 48 && get32(&self.bytes[index], 44) != 0))
                    {
                        return None;
                    }
                }
            }
        }
        Some(present)
    }

    fn slot(&self, index: usize) -> &[u8] {
        &self.bytes[index][..self.sizes[index]]
    }
    fn select(&self) -> Selection {
        progress::select(self.slot(0), self.slot(1))
    }
    fn checkpoint(&self, checkpoint: u32) -> Result<Option<usize>, ()> {
        let mut found = None;
        for index in 0..2 {
            if Progress::decode(self.slot(index)).is_none()
                || get32(&self.bytes[index], 48) != checkpoint
            {
                continue;
            }
            if found.is_some_and(|previous| self.bytes[previous] != self.bytes[index]) {
                return Err(());
            }
            found = Some(index);
        }
        Ok(found)
    }
    fn peers(&self) -> PeerLocators {
        let mut peers = PeerLocators::default();
        for index in 0..2 {
            if let Some(value) = Progress::decode(self.slot(index)) {
                peers.slots[index] = value.locator;
                peers.present |= 1 << index;
            }
        }
        peers
    }
}

pub struct ProgressStore<'a, B: Backend> {
    backend: &'a mut B,
    namespace: &'a str,
    slots: Slots,
}

impl<'a, B: Backend> ProgressStore<'a, B> {
    pub fn new(backend: &'a mut B) -> Self {
        Self::in_namespace(backend, progress::NAMESPACE)
    }
    pub fn in_namespace(backend: &'a mut B, namespace: &'a str) -> Self {
        Self {
            backend,
            namespace,
            slots: Slots::default(),
        }
    }

    fn begin(&mut self, readonly: bool) -> StorageOpen {
        let (opened, snapshot) =
            self.backend
                .begin_progress(self.namespace, readonly, &mut self.slots.bytes);
        self.slots.snapshot = snapshot;
        opened
    }

    pub fn load(&mut self) -> Load<Progress> {
        match self.begin(true) {
            StorageOpen::Missing => return Load::Empty,
            StorageOpen::Opened => {}
            _ => return Load::Failed,
        }
        let transaction = Transaction(self.backend);
        let Some(present) = self.slots.read(transaction.0, false) else {
            return Load::Failed;
        };
        match self.slots.select() {
            Selection::Ambiguous => Load::Ambiguous,
            Selection::First(value) | Selection::Second(value) => Load::Loaded(value),
            Selection::None => {
                if present {
                    Load::Failed
                } else {
                    Load::Empty
                }
            }
        }
    }

    pub fn load_checkpoint(&mut self, checkpoint: u32) -> Load<Progress> {
        match self.begin(true) {
            StorageOpen::Missing => return Load::Empty,
            StorageOpen::Opened => {}
            _ => return Load::Failed,
        }
        let transaction = Transaction(self.backend);
        let Some(present) = self.slots.read(transaction.0, true) else {
            return Load::Failed;
        };
        match self.slots.select() {
            Selection::Ambiguous => return Load::Ambiguous,
            Selection::None => return if present { Load::Failed } else { Load::Empty },
            _ => {}
        }
        match self.slots.checkpoint(checkpoint) {
            Err(()) => Load::Ambiguous,
            Ok(None) => Load::Empty,
            Ok(Some(index)) => {
                Progress::decode(self.slots.slot(index)).map_or(Load::Failed, Load::Loaded)
            }
        }
    }

    pub fn load_peer_locators(&mut self, out: &mut PeerLocators) -> Load<()> {
        match self.begin(true) {
            StorageOpen::Missing => {
                *out = PeerLocators::default();
                return Load::Empty;
            }
            StorageOpen::Opened => {}
            _ => return Load::Failed,
        }
        let transaction = Transaction(self.backend);
        let Some(present) = self.slots.read(transaction.0, false) else {
            return Load::Failed;
        };
        if self.slots.select() == Selection::Ambiguous {
            return Load::Ambiguous;
        }
        let peers = self.slots.peers();
        if peers.present == 0 {
            return if present { Load::Failed } else { Load::Empty };
        }
        *out = peers;
        Load::Loaded(())
    }

    pub fn save(&mut self, value: &mut Progress, protected: Option<u32>, strict: bool) -> Commit {
        self.save_inner(value, protected, strict, None)
    }

    pub fn save_with_peers(
        &mut self,
        value: &mut Progress,
        peers: &mut PeerLocators,
        protected: Option<u32>,
        strict: bool,
    ) -> Commit {
        self.save_inner(value, protected, strict, Some(peers))
    }

    fn save_inner(
        &mut self,
        value: &mut Progress,
        protected: Option<u32>,
        strict: bool,
        peers: Option<&mut PeerLocators>,
    ) -> Commit {
        if self.begin(false) != StorageOpen::Opened {
            return Commit::Unconfirmed;
        }
        let session = value.session_milliseconds;
        let mut encoded = [0; BYTES];
        let mut decoded = None;
        let mut saved;
        {
            let transaction = Transaction(self.backend);
            let slots = &mut self.slots;
            saved = (|| {
                let Some(present) = slots.read(transaction.0, strict || protected.is_some()) else {
                    return false;
                };
                let selected = slots.select();
                if selected == Selection::None && present {
                    return false;
                }
                let protected_slot = if let Some(checkpoint) = protected {
                    match slots.checkpoint(checkpoint) {
                        Ok(Some(index)) => Some(index),
                        _ => return false,
                    }
                } else {
                    None
                };
                let active = match selected {
                    Selection::First(value) => Some((0, value)),
                    Selection::Second(value) => Some((1, value)),
                    _ => None,
                };
                if let Some((index, active)) = active {
                    let proposed = Progress {
                        sequence: active.sequence,
                        ..*value
                    };
                    if proposed
                        .encode()
                        .is_some_and(|current| current == slots.bytes[index])
                        && transaction.0.readback_durable()
                    {
                        decoded = Some(active);
                        return true;
                    }
                }
                let Some(mut plan) = progress::plan(slots.slot(0), slots.slot(1), *value) else {
                    return false;
                };
                if let Some(kept) = protected_slot {
                    plan.target_slot = 1 - kept;
                }
                let Some(bytes) = plan.value.encode() else {
                    return false;
                };
                encoded = bytes;
                let mut verified = [0; BYTES];
                if commit_bytes(
                    transaction.0,
                    SLOT_KEYS[plan.target_slot],
                    &encoded,
                    &mut verified,
                ) != Commit::Saved
                {
                    return false;
                }
                decoded = Progress::decode(&verified);
                if decoded.is_none() {
                    return false;
                }
                slots.bytes[plan.target_slot] = verified;
                slots.sizes[plan.target_slot] = BYTES;
                true
            })();
        }
        // Reconcile through the same namespace boundary, only from durable
        // readback of the complete planned bytes, never from cached visibility.
        if !saved
            && self.backend.readback_durable()
            && Progress::decode(&encoded).is_some()
            && let Load::Loaded(observed) = self.load()
            && observed.encode() == Some(encoded)
        {
            decoded = Some(observed);
            saved = true;
        }
        if saved {
            *value = Progress {
                session_milliseconds: session,
                ..decoded.expect("verified progress")
            };
            if let Some(peers) = peers {
                *peers = self.slots.peers();
            }
            Commit::Saved
        } else {
            Commit::Unconfirmed
        }
    }
}
