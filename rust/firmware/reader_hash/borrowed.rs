// SPDX-License-Identifier: MIT
//! The operation loans its UI hook, not a replacement hash context.
use super::{Digest, MbedHasher, RevisionHasher};

pub struct BorrowedHasher<'op> {
    pub(super) owner: &'op mut MbedHasher,
    pub(super) yield_hook: &'op dyn Fn(),
}
impl RevisionHasher for BorrowedHasher<'_> {
    fn begin(&mut self) -> bool { self.owner.begin() }
    fn update(&mut self, input: &[u8]) -> bool {
        self.owner.update_with_yield(input, Some(self.yield_hook))
    }
    fn finish(&mut self, output: &mut Digest) -> bool { self.owner.finish(output) }
    fn hash(&mut self, input: &[u8], output: &mut Digest) -> bool {
        self.owner.hash(input, output)
    }
}
