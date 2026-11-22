// SPDX-License-Identifier: MIT
//! Short SDK hash contexts under the importing Home's acquired Board.
use crate::reader_hash::MbedHasher;
use newt_core::{
    crossink_import::{BookHasher, HashFactory},
    reading_stats::StatsDigest,
};
use newt_phased_reader::imported_revision::RevisionHasher;

pub(super) fn with_factory<R>(
    board: &newt_c3_board::Board<'_>,
    run: impl FnOnce(&mut Factory<'_, '_>) -> R,
) -> R {
    run(&mut Factory { _board: board })
}
pub(super) struct Factory<'op, 'sdk> {
    _board: &'op newt_c3_board::Board<'sdk>,
}
impl HashFactory for Factory<'_, '_> {
    type Hasher = MbedHasher;
    #[inline(never)]
    fn with_hasher<R>(&mut self, run: impl FnOnce(&mut MbedHasher) -> R) -> R {
        // SAFETY: this short factory borrows the live Board's pinned SDK. The
        // linked layout assertions establish its context ABI; the callback
        // closes its file and finishes before this sole context is dropped.
        let mut hasher = unsafe { MbedHasher::new(None) };
        run(&mut hasher)
    }
}
impl BookHasher for MbedHasher {
    fn begin(&mut self) -> bool {
        RevisionHasher::begin(self)
    }
    fn update(&mut self, bytes: &[u8]) -> bool {
        RevisionHasher::update(self, bytes)
    }
    fn finish(&mut self, out: &mut StatsDigest) -> bool {
        RevisionHasher::finish(self, out)
    }
}
