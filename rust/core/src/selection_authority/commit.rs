//! Selection admission uses the existing protected store, never another codec.
use super::{SelectionAuthority, flag};
use crate::{durable::{Admission, Backend, BlobRead, Commit, DurableWrite, StorageOpen},
    selected_book::SelectedBookStore};

/// Private synchronous pass-through for SelectedBookStore ONLY. Its protected
/// read runs ungated. Both existing codec validations run before writable begin,
/// so enter occurs at the frozen point, before the real backend begin. No
/// caller can use this adapter as a substitute production storage capability.
struct Admitted<'op, 'quiet, B> {
    backend: &'op mut B,
    admission: Option<&'op mut Admission<'quiet>>,
}
impl<B: Backend> Backend for Admitted<'_, '_, B> {
    fn begin(&mut self, namespace: &str, readonly: bool) -> StorageOpen {
        if !readonly && self.admission.as_mut().is_some_and(|gate| !gate.enter()) {
            return StorageOpen::Failed;
        }
        self.backend.begin(namespace, readonly)
    }
    fn end(&mut self) { self.backend.end(); }
    fn read_blob(&mut self, key: &str, output: &mut [u8]) -> BlobRead { self.backend.read_blob(key, output) }
    fn readback_bytes(&mut self, key: &str, expected: &[u8], output: &mut [u8]) -> BlobRead {
        self.backend.readback_bytes(key, expected, output)
    }
    fn put_bytes(&mut self, permit: DurableWrite, key: &str, bytes: &[u8]) -> usize {
        self.backend.put_bytes(permit, key, bytes)
    }
    fn readback_durable(&self) -> bool { self.backend.readback_durable() }
}

impl SelectionAuthority {
    pub fn commit_selection<B: Backend>(&mut self, backend: &mut B,
        source_verified: &mut dyn FnMut() -> bool, protected_book: u64,
        mut admission: Option<&mut Admission<'_>>) -> bool {
        if !source_verified() { return false; }
        if !self.has(flag::COMMIT_OWED) { return self.selection_verified(source_verified); }
        let saved = SelectedBookStore::new(&mut Admitted {
            backend, admission: admission.as_deref_mut(),
        }).save_protected(self.identity, protected_book) == Commit::Saved;
        if let Some(gate) = admission.as_mut() {
            if !gate.ready() { gate.deferred = true; }
            if gate.deferred { return false; }
        }
        self.set(flag::SAVE_FAILED, !saved);
        self.set(flag::UNCONFIRMED, false);
        if saved { self.set(flag::UNAVAILABLE | flag::COMMIT_OWED, false); }
        saved
    }
}
