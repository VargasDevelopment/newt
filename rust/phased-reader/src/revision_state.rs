//! Move the actual revision's scalar state between short loans of its SAME
//! persistent owners. No reset, source probe, hashing, promotion or abort.
use super::{Core, Counters, Digest, ImportedRevision, RevisionHasher, RevisionStorage};
use crate::zip_window::source::BookSource;

/// Not a receipt, wire record, freshness proof or independently forgeable
/// snapshot. Only consuming a real ImportedRevision creates this value.
#[must_use = "relend this state or close its real source/storage owner"]
pub struct RevisionState {
    digest: Digest,
    bytes: u64,
    generation: u64,
    epoch: u64,
    blocks: u32,
    receipt_bytes: u32,
    cached_block: u32,
    valid: bool,
    provisional: bool,
    retained_header: bool,
    retain_source_for_reopen: bool,
    source_present: bool,
    block_present: bool,
    scratch_bytes: usize,
    counters: Counters,
}

/// Trusted owner contract, like RevisionStorage: relend the ORIGINAL source,
/// store, hashers and scratch, without replacement or an intervening operation
/// on them. No pointer getter or identity comparison substitutes for ownership.
/// The enclosing owner closes/aborts its real writer on every exit, including
/// an unwind while the scalar value is outside this operation's loan.
pub struct RevisionPorts<'op> {
    pub source: Option<&'op mut dyn BookSource>,
    pub store: &'op mut dyn RevisionStorage,
    pub whole: &'op mut dyn RevisionHasher,
    pub block: Option<&'op mut dyn RevisionHasher>,
    pub scratch: &'op mut [u8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateLoanError { Shape, Generation }

impl ImportedRevision<'_> {
    /// Consume after every typed read/action loan ends. Move RESULTING state,
    /// including failures, so a failed seal cannot reinstate old authority.
    pub fn into_state(mut self) -> RevisionState {
        let state = RevisionState {
            digest: self.core.digest,
            bytes: self.core.bytes,
            generation: self.core.generation,
            epoch: self.core.epoch,
            blocks: self.core.blocks,
            receipt_bytes: self.core.receipt_bytes,
            cached_block: self.core.cached_block,
            valid: self.core.valid,
            provisional: self.core.provisional,
            retained_header: self.core.retained_header,
            retain_source_for_reopen: self.core.retain_source_for_reopen,
            source_present: self.core.source.is_some(),
            block_present: self.core.block.is_some(),
            scratch_bytes: self.scratch.len(),
            counters: self.core.counters,
        };
        // Only the obsolete view is disarmed. The one real writer stays live;
        // its persistent owner now holds the sole non-Copy state obligation.
        self.core.provisional = false;
        state
    }
}

impl RevisionState {
    pub fn digest(&self) -> &Digest { &self.digest }
    pub fn provisional(&self) -> bool { self.provisional }
    pub fn counters(&self) -> Counters { self.counters }

    /// No source/storage I/O here. The generation is the existing store's
    /// scalar lease, never a replacement for good()/the ordinary source gate.
    /// Rejection returns BOTH the resulting state and every original loan.
    pub fn lend<'op>(self, ports: RevisionPorts<'op>)
        -> Result<ImportedRevision<'op>, (StateLoanError, Self, RevisionPorts<'op>)> {
        if self.retained_header || ports.source.is_some() != self.source_present
            || ports.block.is_some() != self.block_present
            || ports.scratch.len() != self.scratch_bytes {
            return Err((StateLoanError::Shape, self, ports));
        }
        if (self.valid || self.provisional) && ports.store.generation() != self.generation {
            return Err((StateLoanError::Generation, self, ports));
        }
        Ok(ImportedRevision {
            core: Core {
                store: ports.store,
                whole: ports.whole,
                block: ports.block,
                source: ports.source,
                digest: self.digest,
                bytes: self.bytes,
                generation: self.generation,
                epoch: self.epoch,
                blocks: self.blocks,
                receipt_bytes: self.receipt_bytes,
                cached_block: self.cached_block,
                valid: self.valid,
                provisional: self.provisional,
                retained_header: self.retained_header,
                retain_source_for_reopen: self.retain_source_for_reopen,
                counters: self.counters,
            },
            scratch: ports.scratch,
        })
    }
}

#[cfg(test)]
#[path = "revision_state_tests.rs"]
mod tests;
