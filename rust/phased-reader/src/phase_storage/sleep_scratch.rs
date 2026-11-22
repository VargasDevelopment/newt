// SPDX-License-Identifier: MIT
//! Frame work borrows only bytes below the retained stylesheet home.
use super::{Home, Storage, top};
use newt_core::zip_container::Status;

impl Storage<'_> {
    /// Exclusive Storage access excludes every parse phase. Keep the cached
    /// sheet and its held/seal state intact, so painting adds no style reads.
    pub fn with_frame_scratch<R>(
        &mut self,
        operation: impl for<'loan> FnOnce(&'loan mut [u8]) -> R,
    ) -> Result<R, Status> {
        let (scratch, _) = top::<Home>(&mut *self.shared)?;
        Ok(operation(scratch))
    }
}
