//! Frozen reader_revision_cache_policy.h:103-172, checked producer admission.
#![forbid(unsafe_code)]
use newt_core::producer_visit::MissingProbe;

/// Same Pages owner and its attached policy; no default admits an unchecked port.
pub trait ProducerProbe: crate::phase_storage::ReadScope {
    fn producer_probe(&mut self, name: &[u8], cancelled: &dyn Fn() -> bool) -> MissingProbe;
    fn release_for_admission(&mut self) -> bool;
}
