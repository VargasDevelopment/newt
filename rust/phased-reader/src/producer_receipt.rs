// Newt MIT; reader_revision_session.h:156-159, imported-revision route.
use newt_core::producer_policy::ProducerReceipt;
impl Core<'_> {
    fn producer_receipt_live(&mut self, strict: bool) -> bool {
        let fresh = self.store.finish_immutable_read();
        self.store.begin_immutable_read();
        fresh && if strict { self.source.as_mut().is_some_and(|source| live(&mut **source)) } else { self.good() }
    }
}
impl ImportedRevision<'_> {
    /// Only the ReaderSource owner's full-digest-authorized strict mode uses
    /// this validation; it confers no raw read or receipt certification.
    pub fn strict_producer_receipt_live(&mut self) -> bool {
        self.core.producer_receipt_live(true)
    }
}
impl ProducerReceipt for ImportedRevision<'_> {
    fn producer_receipt_live(&mut self) -> bool { self.core.producer_receipt_live(false) }
}
impl ProducerReceipt for Action<'_, '_> {
    fn producer_receipt_live(&mut self) -> bool { self.core.producer_receipt_live(false) }
}
impl Drop for ImportedRevision<'_> {
    fn drop(&mut self) {
        self.core.reset();
    }
}
