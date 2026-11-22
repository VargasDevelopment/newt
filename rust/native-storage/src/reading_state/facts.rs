// SPDX-License-Identifier: MIT
use crate::Error;

/// Boot-lived facts of the one selection-owned store: the original object's
/// flags and its process-wide side-name counter. Firmware keeps exactly
/// one per boot, outside any I/O scope.
#[derive(Default)]
pub struct StateFacts {
    pub(super) unavailable: bool,
    // Until an effective modified-handle volume barrier.
    pub(super) sync_failed: bool,
    // Startup has admissible legacy progress not yet preserved on this card.
    pub(super) legacy_pending: bool,
    pub(super) nonce: u32,
}
const _: () = assert!(core::mem::size_of::<StateFacts>() == 8);

impl StateFacts {
    pub const fn new() -> Self {
        Self {
            unavailable: false,
            sync_failed: false,
            legacy_pending: false,
            nonce: 0,
        }
    }

    pub fn set_legacy_pending(&mut self, pending: bool) {
        self.legacy_pending = pending;
    }

    pub(super) fn next_nonce(&mut self) -> u32 {
        self.nonce = self.nonce.wrapping_add(1);
        self.nonce
    }

    // The last failing native call names a missing or failed medium.
    pub(super) fn note(&mut self, errno: Option<Error>) {
        self.unavailable = self.unavailable || errno == Some(Error::Unavailable);
    }
}

