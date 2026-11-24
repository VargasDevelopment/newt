// SPDX-License-Identifier: MIT
//! Durable reader checkpoint and recovery policy. The caller supplies a
//! verified source digest and the mounted State backend; this owner never opens
//! a native handle.
use newt_core::{
    durable::{Admission, Backend, Commit, Load},
    external_progress_store::ExternalProgressStore,
    preferences::ViewSettings,
    progress::Progress,
    progress_store::PeerLocators,
    selected_book::SelectedBookStore,
    sleep_intent::{SleepResumeIntent, SleepResumeStore, progress_checkpoint},
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Outcome {
    #[default]
    Unchanged,
    Verified,
    Unconfirmed,
}

#[cfg(test)]
#[path = "reader_checkpoint_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "reader_sample_checkpoint_tests.rs"]
mod sample_tests;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CheckpointResult {
    pub progress: Outcome,
    pub destination: Outcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckpointRestore {
    /// An explicit ordinary book open uses the newest valid progress record.
    Latest,
    /// The cold component route resolves the checkpoint named by NWBR.
    Committed,
}

pub struct ReaderCheckpoint {
    acknowledged: Option<Progress>,
    acknowledged_digest: Option<[u8; 32]>,
    restore: CheckpointRestore,
    loaded: Load<()>,
    result: CheckpointResult,
    uncertain: u8,
}

impl ReaderCheckpoint {
    pub const fn new(restore: CheckpointRestore) -> Self {
        Self {
            acknowledged: None,
            acknowledged_digest: None,
            restore,
            loaded: Load::Failed,
            result: CheckpointResult {
                progress: Outcome::Unchanged,
                destination: Outcome::Unchanged,
            },
            uncertain: 0,
        }
    }

    pub fn acknowledged(&self) -> Option<Progress> {
        self.acknowledged
    }

    /// Statistics must not accept another digest with the same shortened key.
    pub fn acknowledged_for(&self, digest: &[u8; 32]) -> Option<Progress> {
        self.acknowledged
            .filter(|_| self.acknowledged_digest.as_ref() == Some(digest))
    }

    /// The ROM sample has its own NVS progress namespace, with no selected-book
    /// digest. Never borrow an external acknowledgement through a short key.
    pub fn acknowledged_sample(&self, identity: u64) -> Option<Progress> {
        self.acknowledged.filter(|saved|
            self.acknowledged_digest.is_none() && saved.book == identity)
    }

    pub fn page_unconfirmed(&self) -> bool {
        self.uncertain & 1 != 0
    }

    pub fn loaded(&self) -> Load<()> {
        self.loaded
    }

    pub fn result(&self) -> CheckpointResult {
        self.result
    }

    /// A result describes this operation; acknowledgement and uncertainty
    /// survive turns which perform no save.
    pub(crate) fn begin_operation(&mut self) {
        self.result = CheckpointResult {
            progress: Outcome::Unchanged,
            destination: Outcome::Unchanged,
        };
    }

    pub(crate) fn restore_uncertainty(&mut self, page: bool, destination: bool) {
        self.uncertain = u8::from(page) | (u8::from(destination) << 1);
    }

    /// A committed selection starts its digest-specific checkpoint epoch,
    /// including commits completed after verification or storage retries.
    pub fn selection_committed<B: Backend>(&mut self, store: &mut B,
        digest: &[u8; 32], page_uncertain: bool, destination_uncertain: bool,
    ) {
        *self = Self::new(CheckpointRestore::Latest);
        self.restore_uncertainty(page_uncertain, destination_uncertain);
        self.restore(store, digest);
    }

    /// Frozen ReadingSaveNotice copy; uncertainty is never called unsaved.
    pub fn notice(&self) -> Option<&'static str> {
        (self.uncertain != 0).then_some("Save unconfirmed. Restart may use old or new page.")
    }

    pub(crate) fn observe(&mut self, result: CheckpointResult) {
        if result.progress != Outcome::Unchanged {
            self.uncertain =
                (self.uncertain & !1) | u8::from(result.progress == Outcome::Unconfirmed);
        }
        if result.destination != Outcome::Unchanged {
            self.uncertain =
                (self.uncertain & !2) | (u8::from(result.destination == Outcome::Unconfirmed) << 1);
        }
        self.result = result;
    }

    /// The frozen not-ready route performs no durable work.
    pub fn refuse(&mut self) -> CheckpointResult {
        let result = CheckpointResult {
            progress: Outcome::Unconfirmed,
            destination: Outcome::Unchanged,
        };
        self.observe(result);
        result
    }

    /// Frozen checkpointReadingCore's inactive-selection arm writes only the
    /// existing embedded ProgressStore. It does not publish an SD selection or
    /// NWBR destination, even when the caller is preparing reading sleep.
    pub fn save_sample<B: Backend>(&mut self, store: &mut B, candidate: Progress,
        progress_ready: bool,
    ) -> CheckpointResult {
        let mut result = CheckpointResult {
            progress: Outcome::Unconfirmed, destination: Outcome::Unchanged,
        };
        if progress_ready {
            let mut proposed = candidate;
            if newt_core::progress_store::ProgressStore::new(store)
                .save(&mut proposed, None, false) == Commit::Saved {
                self.acknowledged = Some(proposed);
                self.acknowledged_digest = None;
                result.progress = Outcome::Verified;
            }
        }
        self.observe(result);
        result
    }

    /// Selection's shortened key is not authority: compare every digest byte.
    pub(crate) fn selection_matches<B: Backend>(store: &mut B, digest: &[u8; 32]) -> bool {
        let book = u64::from_le_bytes(digest[..8].try_into().expect("SHA prefix"));
        matches!(SelectedBookStore::new(store).load_book(book),
            Load::Loaded(selected) if selected.digest == *digest)
    }

    fn reading_intent(saved: &Progress, settings: ViewSettings) -> SleepResumeIntent {
        SleepResumeIntent {
            book: saved.book,
            checkpoint: progress_checkpoint(saved),
            settings: settings.packed() | 4,
        }
    }

    pub(crate) fn store_destination<B: Backend>(store: &mut B, intent: SleepResumeIntent) -> bool {
        SleepResumeStore::new(store).save(intent) == Commit::Saved
            || (store.readback_durable()
                && SleepResumeStore::new(store).load() == Load::Loaded(intent))
    }

    /// Exit destinations follow the statistics fold. An unacknowledged reading
    /// checkpoint cannot replace the old intent. Home is an explicit tombstone.
    pub fn publish_destination<B: Backend>(
        &mut self,
        store: &mut B,
        digest: &[u8; 32],
        settings: ViewSettings,
        reading: bool,
        source_verified: bool,
    ) -> CheckpointResult {
        if !source_verified {
            return self.result;
        }
        let intent = if reading {
            let Some(saved) = self.acknowledged_for(digest) else {
                return self.result;
            };
            if self.result.progress != Outcome::Verified || !Self::selection_matches(store, digest)
            {
                return self.result;
            }
            Self::reading_intent(&saved, settings)
        } else {
            SleepResumeIntent::default()
        };
        // checkpointSleepDestination makes one bounded retry of the same
        // intended value. Readable unsynced SD bytes are never confirmation.
        let confirmed =
            Self::store_destination(store, intent) || Self::store_destination(store, intent);
        let result = CheckpointResult {
            progress: self.result.progress,
            destination: if confirmed {
                Outcome::Verified
            } else {
                Outcome::Unconfirmed
            },
        };
        self.observe(result);
        result
    }

    /// Source verification precedes this call. A failed read never applies
    /// caller/RTC progress or replaces the previously acknowledged value.
    pub fn restore<B: Backend>(&mut self, store: &mut B, digest: &[u8; 32]) -> Progress {
        let book = u64::from_le_bytes(digest[..8].try_into().expect("SHA prefix"));
        let initial = Progress {
            book,
            ..Progress::default()
        };
        if book == 0 || !Self::selection_matches(store, digest) {
            self.loaded = Load::Failed;
            return initial;
        }
        let mut restored = initial;
        self.loaded = ExternalProgressStore::new(store, Some(digest)).load(&mut restored);
        // A validated selection with no progress has a known zero base. Failed
        // reads cannot manufacture an acknowledgement from caller/RTC data.
        if self.loaded == Load::Empty {
            self.acknowledged = Some(initial);
            self.acknowledged_digest = Some(*digest);
        }
        if self.loaded != Load::Loaded(()) {
            return initial;
        }
        if self.restore == CheckpointRestore::Committed {
            let intent = match SleepResumeStore::new(store).load() {
                Load::Loaded(intent) if intent.book == book => intent,
                Load::Loaded(_) | Load::Empty => {
                    self.loaded = Load::Empty;
                    return initial;
                }
                _ => {
                    self.loaded = Load::Failed;
                    return initial;
                }
            };
            self.loaded = ExternalProgressStore::new(store, Some(digest))
                .load_checkpoint(&mut restored, intent.checkpoint);
            if self.loaded != Load::Loaded(()) {
                return initial;
            }
        }
        self.acknowledged = Some(restored);
        self.acknowledged_digest = Some(*digest);
        restored
    }

    /// Frozen authority -> protected peer -> strict progress commit ordering.
    /// Destination publication follows verified progress and has its own result.
    pub fn save<B: Backend>(
        &mut self,
        store: &mut B,
        digest: &[u8; 32],
        candidate: Progress,
        settings: ViewSettings,
        commit_reading: bool,
        accepted_open: bool,
    ) -> CheckpointResult {
        self.save_with_peers(store, digest, candidate, settings, commit_reading, accepted_open, None)
    }

    /// The caller may capture the SAME peers resolved by this progress commit;
    /// no second transaction or load is introduced for derived identity work.
    pub fn save_with_peers<B: Backend>(
        &mut self,
        store: &mut B,
        digest: &[u8; 32],
        candidate: Progress,
        settings: ViewSettings,
        commit_reading: bool,
        accepted_open: bool,
        peers: Option<&mut PeerLocators>,
    ) -> CheckpointResult {
        self.save_admitted(store, digest, candidate, settings, commit_reading,
            accepted_open, peers, None)
    }

    /// Idle cancellation stops before the first durable mutation, then the
    /// same admitted transaction finishes atomically despite later input.
    pub fn save_admitted<B: Backend>(
        &mut self, store: &mut B, digest: &[u8; 32], candidate: Progress,
        settings: ViewSettings, commit_reading: bool, accepted_open: bool,
        peers: Option<&mut PeerLocators>, mut admission: Option<&mut Admission<'_>>,
    ) -> CheckpointResult {
        let deferred = |admission: &mut Option<&mut Admission<'_>>| {
            admission.as_deref_mut().is_some_and(|admission| {
                if admission.ready() { false } else { admission.deferred = true; true }
            })
        };
        if deferred(&mut admission) { return CheckpointResult::default(); }
        let mut result = CheckpointResult {
            progress: Outcome::Unconfirmed,
            destination: Outcome::Unchanged,
        };
        // Frozen checkpointReadingCore's progressReady guard precedes every
        // transaction. A failed restore is not a known empty checkpoint.
        if !matches!(self.loaded, Load::Loaded(()) | Load::Empty) {
            self.observe(result);
            return result;
        }
        let book = u64::from_le_bytes(digest[..8].try_into().expect("SHA prefix"));
        let protected = (|| {
            let authority = SleepResumeStore::new(store).load();
            if book == 0 || candidate.book != book
                || !matches!(authority, Load::Loaded(_) | Load::Empty)
                || !Self::selection_matches(store, digest) { return None; }
            let mut protected = match authority {
                Load::Loaded(intent) if intent.book == book => Some(intent.checkpoint),
                _ => None,
            };
            if let Some(checkpoint) = protected {
                let mut previous = candidate;
                match ExternalProgressStore::new(store, Some(digest)).load_checkpoint(&mut previous, checkpoint) {
                    Load::Loaded(()) => {}
                    Load::Empty if accepted_open => protected = None,
                    _ => return None,
                }
            }
            Some(protected)
        })();
        // A failed read is still preparation: newly arrived input wins until
        // enter, before any notice or recovery is published.
        if deferred(&mut admission) { return CheckpointResult::default(); }
        let Some(protected) = protected else { self.observe(result); return result; };
        let mut proposed = candidate;
        if ExternalProgressStore::new(store, Some(digest)).save(
            &mut proposed,
            peers,
            protected,
            true,
            admission.as_deref_mut(),
        ) != Commit::Saved
        {
            if admission.as_ref().is_some_and(|admission| admission.deferred) {
                return CheckpointResult::default();
            }
            self.observe(result);
            return result;
        }
        // The durable store may stamp sequence; runtime session time stays live.
        self.acknowledged = Some(proposed);
        self.acknowledged_digest = Some(*digest);
        result.progress = Outcome::Verified;
        if commit_reading {
            let intent = Self::reading_intent(&proposed, settings);
            let confirmed = Self::store_destination(store, intent);
            result.destination = if confirmed {
                Outcome::Verified
            } else {
                Outcome::Unconfirmed
            };
        }
        self.observe(result);
        result
    }
}
