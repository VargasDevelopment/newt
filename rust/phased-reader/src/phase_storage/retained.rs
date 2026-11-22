// SPDX-License-Identifier: MIT
//! Return style ownership with the same bank between physical reader loans.
use super::{BANK_BYTES, Storage};

/// Retained style ownership without a borrow of the shared bank.
pub struct StorageState {
    pub(super) held: bool,
}
impl StorageState {
    pub fn new() -> Self {
        Self { held: false }
    }
}
impl Default for StorageState {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Storage<'a> {
    /// Resume style ownership over the same unchanged shared bank.
    ///
    /// # Safety
    /// A held state must come from `into_state` for this exact unchanged bank,
    /// whose held `Home` must remain initialized.
    pub unsafe fn from_state(shared: &'a mut [u8; BANK_BYTES], state: StorageState) -> Self {
        Self {
            shared,
            held: state.held,
        }
    }
    pub fn into_state(self) -> StorageState {
        StorageState { held: self.held }
    }
    /// Return the actual bank with its style state; a typed owner must check
    /// bank identity before retaining that proof for a later loan.
    pub fn into_parts(self) -> (&'a mut [u8; BANK_BYTES], StorageState) {
        (self.shared, StorageState { held: self.held })
    }
    /// Discard retained styles before lending bytes that may overwrite them.
    pub fn with_unpublished_backing<R>(
        &mut self,
        operation: impl for<'loan> FnOnce(&'loan mut [u8; BANK_BYTES]) -> R,
    ) -> R {
        self.held = false;
        operation(self.shared)
    }
}
