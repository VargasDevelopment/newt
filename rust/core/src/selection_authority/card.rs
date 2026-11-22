//! Frozen cardSilent/checkSelection; physical effects are short required loans.
use super::{SelectionAuthority, flag};
use crate::{durable::{Backend, Load}, library_name::name_bytes, selected_book::SelectedBookStore};

/// Real reading-state/card/session observations, supplied per operation. No
/// fallback when allocation/mount/session is absent: caller implements the
/// frozen actual test. count_denied_if_session checks the OPTIONAL session.
pub struct CardObservation<'op, B> {
    pub reading_state_unavailable: &'op mut dyn FnMut(&B) -> bool,
    pub card_absent: &'op mut dyn FnMut(&mut B) -> bool,
    pub count_denied_if_session: &'op mut dyn FnMut(&mut B),
    pub home_cover_changed: &'op mut dyn FnMut(&mut B),
}

impl SelectionAuthority {
    fn deny_page_cache<B>(&mut self, backend: &mut B, card: &mut CardObservation<'_, B>) {
        // PageCacheAdmission::Denied == 1; Pending == 0, Spent == 2, Granted == 3.
        self.flags = (self.flags & !flag::PAGE_CACHE_MASK) | (1 << 9);
        if self.has(flag::PAGE_CACHE_VISIT) { (card.count_denied_if_session)(backend); }
    }
    /// Frozen cardAbsent: a gone card also denies page production and makes
    /// Home forget its cover facts.
    pub fn card_absent<B>(&mut self, backend: &mut B, card: &mut CardObservation<'_, B>) -> bool {
        let absent = (card.card_absent)(backend);
        if absent {
            self.deny_page_cache(backend, card);
            (card.home_cover_changed)(backend);
        }
        absent
    }
    fn card_silent<B, V>(&mut self, result: &Load<V>, backend: &mut B,
        card: &mut CardObservation<'_, B>) -> bool {
        if !matches!(result, Load::Loaded(_) | Load::Empty) { self.deny_page_cache(backend, card); }
        !matches!(result, Load::Loaded(_)) && (card.reading_state_unavailable)(backend)
            && (matches!(result, Load::Empty) || self.card_absent(backend, card))
    }
    pub fn check_selection<B: Backend>(&mut self, backend: &mut B,
        verification_pending: &mut dyn FnMut() -> bool, card: &mut CardObservation<'_, B>) -> bool {
        if verification_pending() { return true; }
        if !self.has(flag::UNCONFIRMED) { return true; }
        let result = SelectedBookStore::new(backend).load();
        let silent = self.card_silent(&result, backend, card);
        let matching = !silent && match &result {
            Load::Loaded(durable) => name_bytes(&self.identity.filename) == name_bytes(&durable.filename)
                && self.identity.digest == durable.digest,
            _ => false,
        };
        self.set(flag::SAVE_FAILED, !silent && !matching);
        self.set(flag::UNCONFIRMED, silent || self.has(flag::SAVE_FAILED));
        if !self.has(flag::UNCONFIRMED) && !backend.readback_durable() {
            self.set(flag::UNCONFIRMED | flag::COMMIT_OWED, true);
        }
        !silent
    }
    pub fn matches_durable_selection<B: Backend>(&self, backend: &mut B) -> bool {
        self.active() && matches!(SelectedBookStore::new(backend).load_book(self.book_id()),
            Load::Loaded(value) if value.digest == self.identity.digest)
    }
    pub fn reconcile_verified_selection<B: Backend>(&mut self, backend: &mut B,
        verified: &mut dyn FnMut() -> bool, pending: &mut dyn FnMut() -> bool,
        card: &mut CardObservation<'_, B>) -> bool {
        if !self.active() { return !self.selection_blocked(); }
        if !verified() { return false; }
        self.check_selection(backend, pending, card);
        self.selection_verified(verified)
    }
}
