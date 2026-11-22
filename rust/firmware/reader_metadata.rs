// SPDX-License-Identifier: MIT
//! Frozen derived Home records over the actual Pages transaction owner.
#![forbid(unsafe_code)]
use newt_core::{page_cache::CacheStorage, progress::Locator, progress_store::PeerLocators};
use newt_native_storage::Native;
use newt_reader_navigation::Section;
use crate::reader_pages::ReaderPages;

#[path = "reader_metadata/identity.rs"]
pub mod identity;
#[path = "reader_metadata/estimate.rs"]
pub mod estimate;

/// Identity publication must distinguish a confirmed miss from uncertain IO.
pub trait IdentityStorage: CacheStorage {
    fn inspect_identity(&mut self, name: &[u8], out: &mut [u8], uncertain: &mut bool) -> bool;
    fn close_checkpoint_storage(&mut self);
}
impl<N: Native> IdentityStorage for ReaderPages<'_, N> {
    fn inspect_identity(&mut self, name: &[u8], out: &mut [u8], uncertain: &mut bool) -> bool {
        self.0.access().inspect_identity(name, out, uncertain)
    }
    fn close_checkpoint_storage(&mut self) { self.0.access().close(); }
}
impl<N: Native> IdentityStorage for newt_native_storage::chapter_cache::ChapterCacheAccess<'_, '_, N> {
    fn inspect_identity(&mut self, name: &[u8], out: &mut [u8], uncertain: &mut bool) -> bool {
        self.inspect_identity(name, out, uncertain)
    }
    fn close_checkpoint_storage(&mut self) { self.close(); }
}

/// Same transient nothrow byte owner as saveChapterIdentityCheckpoint. The
/// enclosing owner closes Pages on allocation failure. Once scratch exists,
/// the descriptor closes before that scratch is freed, as in the frozen helper.
pub fn save_checkpoint<'label, S: IdentityStorage>(store: &mut S, digest: &[u8; 32],
    at: Locator, section: Section<'_>, peers: &PeerLocators,
    section_at: impl FnMut(Locator) -> Section<'label>, stop: &dyn Fn() -> bool,
) -> bool {
    let Some(mut scratch) = newt_sdk_heap::Bytes::try_new(identity::CHECKPOINT_SCRATCH_BYTES) else { return false; };
    let saved = identity::Record::new(store, scratch.as_mut())
        .is_some_and(|mut record| record.prepare_checkpoint(digest, at, section, peers, section_at, stop));
    store.close_checkpoint_storage();
    saved
}

pub fn load_identity<S: IdentityStorage>(store: &mut S, digest: &[u8; 32], at: Locator,
    label: &mut [u8; newt_reader_navigation::TITLE_BYTES],
) -> bool {
    let mut scratch = [0; identity::LOAD_SCRATCH_BYTES];
    let found = identity::Record::new(store, &mut scratch).is_some_and(|mut record| record.load_for(digest, at, label));
    store.close_checkpoint_storage();
    found
}
