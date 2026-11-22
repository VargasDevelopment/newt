use crate::durable::{Backend, BlobRead, DurableWrite, StorageOpen};
use super::PLAIN_BYTES;

/// Required Root boundary: guaranteed native wipe, including bytes seen to die.
pub trait NativeWipe { fn wipe(&self, bytes: &mut [u8]); }
/// Root's existing Secret boundary, borrowed only; no worker implementation.
/// bytes lends full capacity; size<=N. fill clears before the callback, clears
/// again on refusal/oversize, and publishes size only on complete success.
pub trait Secret<const N: usize> {
    fn size(&self) -> usize;
    fn bytes(&self) -> &[u8;N];
    fn clear(&mut self, wipe: &dyn NativeWipe);
    fn fill(&mut self, size: usize, wipe: &dyn NativeWipe, source: &mut dyn FnMut(&mut [u8]) -> bool) -> bool;
}
pub struct KeyRegion<'a> { pub bytes: &'a mut [u8;32], pub length: &'a mut usize }
impl KeyRegion<'_> { pub fn borrow(&mut self) -> KeyRegion<'_> { KeyRegion { bytes: self.bytes, length: self.length } } }
/// Root constructs one fresh zeroed Secret over this exact loan, invokes body
/// exactly once, then destroys/wipes it before returning (also on unwind).
/// It retains no loan and adds no backing. Nested scopes use disjoint regions.
pub trait ScopedSecrets {
    fn with_key(&self, region: KeyRegion<'_>, wipe: &dyn NativeWipe, body: &mut dyn FnMut(&mut dyn Secret<32>));
}
pub struct Work<'a> {
    pub key: KeyRegion<'a>,
    pub observed: KeyRegion<'a>,
    pub readback: &'a mut [u8;32],
    pub plain: &'a mut [u8;PLAIN_BYTES],
}
pub trait KeyBackend {
    /// Short loan of the same existing backend/session on every call.
    fn backend(&mut self) -> &mut dyn Backend;
    fn erase_key(&mut self) -> bool;
}
pub trait Random { fn fill(&mut self, bytes: &mut [u8]) -> bool; }
pub trait Cipher {
    fn seal(&mut self, key: &[u8;32], nonce: &[u8;12], header: &[u8;12], plain: &[u8;98], cipher: &mut [u8;98], tag: &mut [u8;16]) -> bool;
    fn open(&mut self, key: &[u8;32], nonce: &[u8;12], header: &[u8;12], cipher: &[u8;98], tag: &[u8;16], plain: &mut [u8;98]) -> bool;
}

// Sized view for the existing generic durable algorithm, forwarding its permit
// and specialized readback unchanged. The optional flag is the frozen witness.
pub(super) struct BackendLoan<'a> { backend: &'a mut dyn Backend, wrote: Option<&'a mut bool> }
impl<'a> BackendLoan<'a> { pub(super) fn new(backend: &'a mut dyn Backend, wrote: Option<&'a mut bool>) -> Self { Self { backend, wrote } } }
impl Backend for BackendLoan<'_> {
    fn begin(&mut self, namespace: &str, readonly: bool) -> StorageOpen { self.backend.begin(namespace, readonly) }
    fn end(&mut self) { self.backend.end(); }
    fn read_blob(&mut self, key: &str, output: &mut [u8]) -> BlobRead { self.backend.read_blob(key, output) }
    fn readback_bytes(&mut self, key: &str, expected: &[u8], output: &mut [u8]) -> BlobRead { self.backend.readback_bytes(key, expected, output) }
    fn put_bytes(&mut self, permit: DurableWrite, key: &str, bytes: &[u8]) -> usize {
        if let Some(wrote) = self.wrote.as_deref_mut() { *wrote = true; }
        self.backend.put_bytes(permit, key, bytes)
    }
    fn readback_durable(&self) -> bool { self.backend.readback_durable() }
}
