// SPDX-License-Identifier: MIT
//! One operation's UI cadence over the existing context and yield credit.
use super::{Digest, MbedHasher, RevisionHasher};

pub struct MbedLoan<'op> {
    pub(super) owner: &'op mut MbedHasher,
    pub(super) hook: &'op dyn Fn(),
}
impl RevisionHasher for MbedLoan<'_> {
    fn begin(&mut self) -> bool { self.owner.begin() }
    fn update(&mut self, input: &[u8]) -> bool {
        let (success, yield_due) = self.owner.update_credit(input);
        if yield_due { (self.hook)(); }
        success
    }
    fn finish(&mut self, output: &mut Digest) -> bool { self.owner.finish(output) }
    fn hash(&mut self, input: &[u8], output: &mut Digest) -> bool {
        self.owner.hash(input, output)
    }
}
