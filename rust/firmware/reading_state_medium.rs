// SPDX-License-Identifier: MIT
//! The mounted-card branch of firmware/reading_state_backend.h::mounted.
use newt_native_io::UiIo;
use newt_native_storage::reading_state::{FIRMWARE_STATE, FIRMWARE_STATS, StateMedium};
use newt_native_storage::{Native, chapter_cache::ChapterCacheAccess};

/// Borrowed only for a durable operation. UiIo proves successful fixed-profile
/// VFS registration; it is not a card-presence or durable-write receipt.
/// The outer producer must suspend its writer before entering state work.
pub struct MountedStateMedium<'pages, 'op, 'route, 'ui, N: Native> {
    pages: Option<&'pages mut ChapterCacheAccess<'op, 'route, N>>,
    _mounted: &'ui UiIo,
}

impl<'pages, 'op, 'route, 'ui, N: Native> MountedStateMedium<'pages, 'op, 'route, 'ui, N> {
    pub fn new(
        mounted: &'ui UiIo,
        pages: Option<&'pages mut ChapterCacheAccess<'op, 'route, N>>,
    ) -> Self {
        Self {
            pages,
            _mounted: mounted,
        }
    }
}

impl<N: Native, Port: Native> StateMedium<Port> for MountedStateMedium<'_, '_, '_, '_, N> {
    const STATE: &'static str = FIRMWARE_STATE;
    const STATS: &'static str = FIRMWARE_STATS;

    fn release_pages_for_state(&mut self) {
        if let Some(pages) = self.pages.as_deref_mut() {
            pages.release_for_state();
        }
    }
    fn mount(&mut self, _: Option<&mut Port>) -> bool {
        // The constructor borrows UiIo, which Board publishes only after a
        // successful mount. Like SdLibrary::mount's mounted_ fast path, never
        // reinitialize SPI beneath existing source/receipt descriptors.
        true
    }
}
