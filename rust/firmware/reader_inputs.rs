// SPDX-License-Identifier: MIT
//! Existing caller-owned provider and memory loans, moved without added state.
use super::*;

/// The existing provider seams. Firmware has no Rust SHA-256 or line-break
/// provider yet (HANDOFF), so the caller lends them.
pub struct BookProviders<'a> {
    pub whole: &'a mut dyn RevisionHasher,
    pub block: &'a mut dyn RevisionHasher,
    pub identity: &'a mut dyn newt_phased_reader::hash_provider::CacheHash,
    pub breaker: &'a mut dyn LineBreak,
    pub stats_sha256: Sha256<'a>,
    /// Foreground and producer yield callbacks borrow the caller's SAME cadence.
    pub on_yield: newt_core::admission::Yield<'a>,
    pub producer: ProducerCallbacks<'a>,
}

// Newt MIT; the sole caller-owned book memory loan.
/// One open epoch's shared bank, placed runtime and compact receipt scratch.
pub struct BookMemory<'m, 'p> {
    pub bank: &'m mut [u8; BANK_BYTES],
    pub runtime: &'m mut BookRuntime<'m, 'p>,
    pub receipt: &'m mut [u8; 256],
    pub producer_ledger: &'m mut [u8; 256],
}



#[derive(Clone, Copy)]
pub enum BookOpen {
    /// prepareImport: hash the whole book and publish a new receipt.
    Import,
    /// prepareSelection: the expected digest, which the receipt must match.
    Receipt(Digest),
}
