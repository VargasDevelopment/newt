//! Per-book data uses the existing ExternalProgressStore and Progress values.
use super::{SelectionAuthority, flag};
use crate::{durable::{Admission, Backend, Commit, Load}, external_progress_store::ExternalProgressStore,
    progress::Progress, progress_store::PeerLocators};

impl SelectionAuthority {
    pub fn load_progress<B: Backend>(&mut self, backend: &mut B, saved: &mut Progress) -> Load<()> {
        let mut store = ExternalProgressStore::new(backend, Some(&self.digest));
        let result = if self.selection_blocked() { Load::Failed } else { store.load(saved) };
        self.set(flag::SAVED_PRESENT, result == Load::Loaded(()));
        if result == Load::Empty { *saved = Progress { book: store.book_id(), ..Progress::default() }; }
        result
    }
    pub fn load_checkpoint<B: Backend>(&mut self, backend: &mut B, saved: &mut Progress,
        checkpoint: u32) -> Load<()> {
        let result = if self.selection_blocked() { Load::Failed }
            else { ExternalProgressStore::new(backend, Some(&self.digest)).load_checkpoint(saved, checkpoint) };
        if result == Load::Loaded(()) { self.set(flag::SAVED_PRESENT, true); }
        result
    }
    pub fn save_progress<B: Backend>(&mut self, backend: &mut B, saved: &mut Progress,
        source_verified: &mut dyn FnMut() -> bool, protected_checkpoint: Option<u32>,
        strict: bool, peers: Option<&mut PeerLocators>, admission: Option<&mut Admission<'_>>) -> bool {
        if !source_verified() { return false; }
        let saved_ok = ExternalProgressStore::new(backend, Some(&self.digest))
            .save(saved, peers, protected_checkpoint, strict, admission) == Commit::Saved;
        if saved_ok { self.set(flag::SAVED_PRESENT, true); }
        saved_ok
    }
}
