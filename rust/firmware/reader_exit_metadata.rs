// SPDX-License-Identifier: MIT
//! Frozen reader_exit_metadata.h capture; no storage, allocation or reader loan.
use newt_core::{progress::Locator, progress_store::PeerLocators};
use newt_reader_navigation::{Estimate, Section, TITLE_BYTES};

#[repr(C)]
pub(crate) struct ExitMetadata {
    pub(crate) digest: [u8; 32],
    pub(crate) locator: Locator,
    pub(crate) peers: PeerLocators,
    pub(crate) estimate: Estimate,
    pub(crate) identified: bool,
    pub(crate) estimated: bool,
    labels: [[u8; TITLE_BYTES + 1]; 3],
}
const _: () = assert!(TITLE_BYTES == 96);
const _: () = {
    assert!(core::mem::size_of::<ExitMetadata>() == 392);
    assert!(core::mem::align_of::<ExitMetadata>() == 8);
};
impl ExitMetadata {
    /// Caller allocates only at frozen deferExitMetadata, before capture.
    pub(crate) fn new() -> Self {
        Self { digest: [0; 32], locator: Locator::default(), peers: PeerLocators::default(),
            estimate: Estimate::default(), identified: false, estimated: false,
            labels: [[0; TITLE_BYTES + 1]; 3] }
    }

    #[inline(never)]
    pub(crate) fn keep_identity<'label>(
        &mut self, digest: &[u8; 32], locator: Locator, label: Option<&[u8]>,
        peers: &PeerLocators, mut section_at: impl FnMut(Locator) -> Section<'label>,
    ) {
        self.digest = *digest;
        self.locator = locator;
        self.peers = *peers;
        copy(&mut self.labels[0], label);
        for slot in 0..self.peers.slots.len() {
            if self.peers.present & (1 << slot) != 0 {
                copy(&mut self.labels[slot + 1], Some(section_at(self.peers.slots[slot]).label));
            } else {
                copy(&mut self.labels[slot + 1], None);
            }
        }
        self.identified = true;
    }

    pub(crate) fn keep_estimate(&mut self, estimate: Estimate) {
        self.estimate = estimate;
        self.estimated = true;
    }

    /// Only the captured locator/peers can answer; all other labels are misses.
    pub(crate) fn section_at(&self, locator: Locator) -> Section<'_> {
        let label = if locator == self.locator {
            Some(&self.labels[0])
        } else {
            self.peers.slots.iter().enumerate().find_map(|(slot, peer)| {
                (self.peers.present & (1 << slot) != 0 && *peer == locator)
                    .then_some(&self.labels[slot + 1])
            })
        };
        Section { label: label.map_or(b"".as_slice(), |label| {
            &label[..label.iter().position(|&byte| byte == 0).unwrap_or(label.len())]
        }), ..Section::default() }
    }
}

fn copy(to: &mut [u8; TITLE_BYTES + 1], label: Option<&[u8]>) {
    to.fill(0);
    if let Some(label) = label {
        let bytes = label.iter().position(|&byte| byte == 0).unwrap_or(label.len()).min(TITLE_BYTES);
        // Keep96 bytes plus NUL: an oversized label remains oversized for the
        // record's strict <96-byte check instead of becoming a shorter alias.
        to[..bytes].copy_from_slice(&label[..bytes]);
    }
}
