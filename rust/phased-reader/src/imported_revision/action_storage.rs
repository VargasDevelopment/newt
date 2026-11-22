//! Frozen compact revision scratch lending; the original slice remains exclusive.
use super::{BLOCK_BYTES, ImportedRevision};

/// One operation's original scratch loan, never an allocation or stored owner.
#[must_use = "restore the original scratch before returning the revision state"]
pub struct OriginalScratch<'a> {
    bytes: &'a mut [u8],
}

impl<'a> ImportedRevision<'a> {
    /// Match lendActionStorage without changing source or receipt authority.
    pub fn replace_action_storage(&mut self, storage: &'a mut [u8]) -> Option<OriginalScratch<'a>> {
        if self.core.block.is_none() || self.scratch.is_empty()
            || storage.len() < BLOCK_BYTES || storage.len() > u32::MAX as usize
        {
            return None;
        }
        self.core.retained_header = false;
        Some(OriginalScratch { bytes: core::mem::replace(&mut self.scratch, storage) })
    }

    /// Withdraw retained tags before the caller may reuse the lent frame.
    pub fn restore_action_storage(&mut self, original: OriginalScratch<'a>) {
        self.core.retained_header = false;
        self.scratch = original.bytes;
    }
}
