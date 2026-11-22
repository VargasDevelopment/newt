// SPDX-License-Identifier: MIT
//! Move the actual reader source authority across short physical-port loans.
use crate::physical_source::{ReaderSource, ReaderSourceMode};
use newt_phased_reader::imported_revision::{Digest, RevisionPorts, RevisionState, StateLoanError};

/// Not a source snapshot or a fresh reader. Only consuming the real source
/// creates this obligation, preserving Strict and Closed authority unchanged.
#[must_use = "relend the same physical owners or close the actual session"]
pub struct ReaderSourceState {
    revision: RevisionState,
    mode: ReaderSourceMode,
    digest: Digest,
}
impl ReaderSource<'_> {
    pub fn into_state(self) -> ReaderSourceState {
        ReaderSourceState {
            revision: self.revision.into_state(),
            mode: self.mode,
            digest: self.digest,
        }
    }
}
impl ReaderSourceState {
    /// Relend the original source, receipt, hash contexts and scratch. No
    /// reset, import, source probe or replacement authority runs on this path.
    pub fn lend<'op>(self, ports: RevisionPorts<'op>)
        -> Result<ReaderSource<'op>, (StateLoanError, Self, RevisionPorts<'op>)> {
        let Self { revision, mode, digest } = self;
        match revision.lend(ports) {
            Ok(revision) => Ok(ReaderSource { revision, mode, digest }),
            Err((reason, revision, ports)) => Err((reason, Self { revision, mode, digest }, ports)),
        }
    }
}
