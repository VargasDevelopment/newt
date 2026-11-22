// SPDX-License-Identifier: MIT
use core::marker::PhantomData;
use newt_phased_reader::{hash_provider::CacheHash, imported_revision::Digest};

/// Frozen packageSha256 uses the SDK one-shot function, with no retained
/// incremental context. This provider cannot be used for receipt streaming.
pub struct PackageHasher(pub(super) PhantomData<*mut ()>);
impl PackageHasher {
    /// Called only inside the image's live SDK/Board scope. This one-shot
    /// provider has no incremental context or initialization/destruction call.
    pub(crate) fn from_live_sdk() -> Self { Self(PhantomData) }
}
impl CacheHash for PackageHasher {
    fn hash(&mut self, input: &[u8], output: &mut Digest) -> bool {
        super::package_hash(input, output)
    }
}
const _: () = assert!(core::mem::size_of::<PackageHasher>() == 0);
