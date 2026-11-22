// SPDX-License-Identifier: MIT
//! Cache identities use frozen packageSha256, not an incremental context.
use crate::imported_revision::{Digest, RevisionHasher};

/// Required one-shot capability. Receipt hashing keeps RevisionHasher's
/// separate begin/update/finish contract and its two exclusive owners.
pub trait CacheHash {
    fn hash(&mut self, input: &[u8], output: &mut Digest) -> bool;
}

// Existing host and streaming providers retain exactly their old hash body.
impl<H: RevisionHasher + ?Sized> CacheHash for H {
    fn hash(&mut self, input: &[u8], output: &mut Digest) -> bool {
        RevisionHasher::hash(self, input, output)
    }
}
