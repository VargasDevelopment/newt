//! Minimal FreeInk BookSource byte read/size contract; no scheduling/XML policy.
pub trait BookSource {
    fn size(&self) -> u64;
    /// Return a signed count (possibly short), zero at EOF, negative on error.
    /// Zero-length probes receive a real, live empty slice. Adapters must not
    /// dereference it. Unsupported probes may return any nonzero result.
    fn read_at(&mut self, at: u64, output: &mut [u8]) -> i32;
}

/// Frozen PhasedReader::validateSource: readable identity, not durability.
pub fn validate<S: BookSource + ?Sized>(source: &mut S, bytes: u64) -> crate::zip_container::Status {
    use crate::zip_container::Status;
    if source.size() != bytes {
        Status::Stale
    } else if source.read_at(0, &mut []) != 0 {
        Status::IoError
    } else {
        Status::Ok
    }
}
