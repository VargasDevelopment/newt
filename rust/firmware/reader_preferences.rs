// SPDX-License-Identifier: MIT

//! Durable reader preferences store over NVS.
//! NVS is the fixed device-settings authority; State is consulted only for
//! the first orientation choice. A failed save never publishes its draft.
use crate::nvs::{self, NvsBlobBackend};
use newt_core::durable::{Backend, BlobRead, DurableWrite, StorageOpen};
use newt_core::preferences::{BYTES, Orientation, Preferences, ViewSettings};
use newt_core::preferences_store::{NAMESPACE, PreferencesStore, StoreError};
use newt_core::reading_stats_store::StatsBackend;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CardHistory {
    Unknown,
    New,
    RanNewt,
}

/// The real State folder and legacy history namespaces, including failures.
pub fn card_history<B: StatsBackend>(store: &mut B, folder: StorageOpen) -> CardHistory {
    match folder {
        StorageOpen::Opened => return CardHistory::RanNewt,
        StorageOpen::Missing => {}
        _ => return CardHistory::Unknown,
    }
    for name in ["newt_seen", "newt_selected", "newt_resume"] {
        let opened = store.begin(name, true);
        store.end();
        if opened == StorageOpen::Opened {
            return CardHistory::RanNewt;
        }
        if opened != StorageOpen::Missing || store.reading_state_unavailable() {
            return CardHistory::Unknown;
        }
    }
    CardHistory::New
}

/// The boot's published preference record and its frozen write gate.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReaderPreferences {
    value: Preferences,
    writable: bool,
    orientation_pending: bool,
    unavailable: bool,
}

impl ReaderPreferences {
    /// Called by the real Home startup before its clock boot read. Resetting
    /// defaults here prevents stale component state becoming boot authority.
    pub fn restore<N: nvs::Native>(&mut self, native: &mut N) -> Option<StoreError> {
        *self = Self::default();
        let mut backend = PreferencesBackend::new(native);
        match PreferencesStore::new(&mut backend).load() {
            Ok(value) => {
                self.value = value;
                self.writable = true;
                None
            }
            Err(StoreError::Empty) => {
                self.writable = true;
                self.orientation_pending = true;
                Some(StoreError::Empty)
            }
            Err(error) => {
                self.unavailable = true;
                Some(error)
            }
        }
    }

    pub fn settings(&self) -> ViewSettings {
        self.value.settings
    }
    pub fn writable(&self) -> bool {
        self.writable
    }
    pub fn orientation_pending(&self) -> bool {
        self.orientation_pending
    }
    pub fn unavailable(&self) -> bool {
        self.unavailable
    }

    /// Unknown history defers without a write. Otherwise the pending choice
    /// retires even when its commit is unconfirmed, retaining runtime defaults.
    pub fn choose_orientation<N: nvs::Native>(
        &mut self,
        native: &mut N,
        history: CardHistory,
    ) -> Option<bool> {
        if !self.orientation_pending || history == CardHistory::Unknown {
            return None;
        }
        let mut settings = self.value.settings;
        settings.orientation = if history == CardHistory::New {
            Orientation::Portrait
        } else {
            Orientation::LandscapeCcw
        };
        let saved = self.save(native, settings);
        self.orientation_pending = false;
        Some(saved)
    }

    /// Save an explicit Quick Start orientation choice; retire the default
    /// only after a verified commit.
    pub fn save_explicit_orientation<N: nvs::Native>(
        &mut self,
        native: &mut N,
        orientation: Orientation,
    ) -> bool {
        let mut settings = self.value.settings;
        settings.orientation = orientation;
        let saved = self.save(native, settings);
        if saved {
            self.orientation_pending = false;
        }
        saved
    }

    /// Store::save updates the candidate only after verified publication.
    /// False is unconfirmed, never a statement that durable bytes stayed old.
    pub fn save<N: nvs::Native>(&mut self, native: &mut N, settings: ViewSettings) -> bool {
        if !self.writable {
            self.unavailable = true;
            return false;
        }
        let mut proposed = Preferences {
            settings,
            ..self.value
        };
        let mut backend = PreferencesBackend::new(native);
        if PreferencesStore::new(&mut backend)
            .save(&mut proposed)
            .is_err()
        {
            self.unavailable = true;
            return false;
        }
        self.value = proposed;
        self.unavailable = false;
        true
    }
}

/// Same namespace, keys, readonly and extent checks as the frozen NVS port.
struct PreferencesBackend<'a, N: nvs::Native> {
    inner: NvsBlobBackend<'a, N>,
    readonly: bool,
}
impl<'a, N: nvs::Native> PreferencesBackend<'a, N> {
    fn new(native: &'a mut N) -> Self {
        Self {
            inner: NvsBlobBackend::new(native),
            readonly: true,
        }
    }
}
fn allowed(key: &str) -> bool {
    matches!(key, "slot0" | "slot1")
}
impl<N: nvs::Native> Backend for PreferencesBackend<'_, N> {
    fn begin(&mut self, name: &str, readonly: bool) -> StorageOpen {
        if name != NAMESPACE {
            return StorageOpen::Failed;
        }
        self.readonly = readonly;
        self.inner.begin(NAMESPACE, readonly)
    }
    fn end(&mut self) {
        self.inner.end();
        self.readonly = true;
    }
    fn read_blob(&mut self, key: &str, out: &mut [u8]) -> BlobRead {
        if !allowed(key) || !(8..=BYTES).contains(&out.len()) {
            return BlobRead::Failed;
        }
        self.inner.read_blob(key, out)
    }
    fn put_bytes(&mut self, permit: DurableWrite, key: &str, bytes: &[u8]) -> usize {
        if self.readonly || !allowed(key) || bytes.len() != BYTES {
            return 0;
        }
        self.inner.put_bytes(permit, key, bytes)
    }
    fn readback_durable(&self) -> bool {
        self.inner.readback_durable()
    }
}
