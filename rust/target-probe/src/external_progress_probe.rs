// SPDX-License-Identifier: MIT
//! MIT; runtime-borrowed no_std retention probe. Caller supplies backend, full digest, admission and outputs; no medium or SHA.
use newt_core::{
    durable::{Admission, Backend, Commit, Load},
    external_progress_store::{ExternalProgressStore, Status},
    progress::Progress,
    progress_store::PeerLocators,
};
#[inline(never)]
pub fn probe<B: Backend>(
    backend: &mut B,
    digest: Option<&[u8; 32]>,
    value: &mut Progress,
    peers: &mut PeerLocators,
    checkpoint: Option<u32>,
    strict: bool,
    admission: &mut Admission<'_>,
    operation: u8,
) -> (u8, Status) {
    let mut store = ExternalProgressStore::new(backend, digest);
    let code = match operation {
        0 => load_code(store.load(value)),
        1 => load_code(store.load_checkpoint(value, checkpoint.unwrap_or(0))),
        2 => load_code(store.load_peer_locators(peers)),
        3 => {
            u8::from(store.save(value, None, checkpoint, strict, Some(admission)) == Commit::Saved)
        }
        _ => u8::from(
            store.save(value, Some(peers), checkpoint, strict, Some(admission)) == Commit::Saved,
        ),
    };
    (code, store.status())
}
fn load_code(result: Load<()>) -> u8 {
    match result {
        Load::Loaded(()) => 0,
        Load::Empty => 1,
        Load::Ambiguous => 2,
        Load::Failed => 3,
    }
}

/// Synthetic backing medium, distinct from the core's transaction snapshot.
/// All storage, flags, extents and counters are borrowed from the runtime caller.
/// This is one explicitly named namespace; it is not an SD/NVS driver.
pub struct ProbeBackend<'a> {
    pub namespace: &'a str,
    pub digest: &'a mut [u8; 32],
    pub slots: &'a mut [[u8; 52]; 2],
    pub present: &'a mut [bool; 3],
    pub lengths: &'a mut [usize; 3],
    pub calls: &'a mut [u32; 4],
    pub fail_read: bool,
    pub fail_write: bool,
    pub durable: bool,
}
impl Backend for ProbeBackend<'_> {
    fn begin(&mut self, namespace: &str, readonly: bool) -> newt_core::durable::StorageOpen {
        use newt_core::durable::StorageOpen;
        self.calls[0] = self.calls[0].wrapping_add(1);
        if namespace != self.namespace {
            StorageOpen::Failed
        } else if readonly && !self.present.iter().any(|x| *x) {
            StorageOpen::Missing
        } else {
            StorageOpen::Opened
        }
    }
    fn end(&mut self) {
        self.calls[1] = self.calls[1].wrapping_add(1);
    }
    fn read_blob(&mut self, key: &str, output: &mut [u8]) -> newt_core::durable::BlobRead {
        use newt_core::durable::BlobRead;
        self.calls[2] = self.calls[2].wrapping_add(1);
        if self.fail_read {
            return BlobRead::Failed;
        }
        let index = match key {
            "digest" => 0,
            "slot0" => 1,
            "slot1" => 2,
            _ => return BlobRead::Failed,
        };
        if !self.present[index] {
            return BlobRead::Missing;
        }
        let bytes: &[u8] = if index == 0 {
            &self.digest[..]
        } else {
            &self.slots[index - 1]
        };
        let n = output.len().min(bytes.len()).min(self.lengths[index]);
        output[..n].copy_from_slice(&bytes[..n]);
        BlobRead::Present(self.lengths[index])
    }
    fn put_bytes(&mut self, _: newt_core::durable::DurableWrite, key: &str, bytes: &[u8]) -> usize {
        self.calls[3] = self.calls[3].wrapping_add(1);
        let index = match key {
            "digest" => 0,
            "slot0" => 1,
            "slot1" => 2,
            _ => return 0,
        };
        let output: &mut [u8] = if index == 0 {
            &mut self.digest[..]
        } else {
            &mut self.slots[index - 1]
        };
        if bytes.len() > output.len() {
            return 0;
        }
        output[..bytes.len()].copy_from_slice(bytes);
        self.present[index] = true;
        self.lengths[index] = bytes.len();
        if self.fail_write { 0 } else { bytes.len() }
    }
    fn readback_durable(&self) -> bool {
        self.durable
    }
}

/// Concrete instantiation retains all five operations with runtime inputs.
#[inline(never)]
pub fn runtime_probe(
    backend: &mut ProbeBackend<'_>,
    digest: Option<&[u8; 32]>,
    value: &mut Progress,
    peers: &mut PeerLocators,
    checkpoint: Option<u32>,
    strict: bool,
    admission: &mut Admission<'_>,
    operation: u8,
) -> (u8, Status) {
    probe(
        backend, digest, value, peers, checkpoint, strict, admission, operation,
    )
}
