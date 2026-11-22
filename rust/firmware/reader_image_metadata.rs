// SPDX-License-Identifier: MIT
//! Live optional checkpoint records, before statistics or page-cache work.
use super::*;
use crate::{app_image::{Application, Events, Home}, home_startup::CardPort,
    reader_pages::ReaderPages, reading_state_medium::MountedStateMedium};
use newt_core::{durable::Load, external_progress_store::ExternalProgressStore,
    progress_store::PeerLocators, selection_authority::flag};
use newt_native_storage::{chapter_cache::ChapterCacheStorage, reading_state::ReadingStateRouter};

impl Runtime {
    // Metadata's temporary reader/store views must end before the driver runs.
    #[inline(never)]
    pub(super) fn service_metadata_image(&mut self, home: &mut Home<'_>,
        ui: &Application<'_>, events: &Events<'_, '_>,
    ) -> bool {
        if self.selection.flags() & flag::DERIVED_OWED == 0 { return false; }
        let verified = self.session.as_ref().is_none_or(|session|
            session.verification.state() == crate::reader_verification::Verification::Verified);
        if !self.selection.active() || !self.selection.selection_verified(&mut || verified) { return false; }
        let Some(saved) = self.checkpoint.acknowledged_for(self.selection.digest())
            .filter(|saved| saved.book == self.selection.book_id()) else { return false; };
        let Some(memory) = self.memory.external_mut() else { return false; };
        let Self { selection, session, idle, .. } = self;
        let title_deferred = selection.flags() & flag::TITLE_DEFERRED != 0;
        let filename = selection.identity().filename;
        let Some(filename) = newt_core::library_name::name_bytes(&filename)
            .and_then(|name| core::str::from_utf8(name).ok()) else { return false; };
        let digest = *selection.digest();
        let result = memory.lend(|memory| home.with_reader_io(|scope| {
            let stopped = || events.queue.pending() || {
                #[cfg(feature = "dev-usb")]
                { scope.bus.log.available() > 0 || !ui.bridge.background_work_ready() }
                #[cfg(not(feature = "dev-usb"))]
                { let _ = ui; false }
            };
            if stopped() { return false; }
            let mut work = |pages: &mut ReaderPages<'_, newt_native_io::OsNative<'_>>, has_session| {
                // A cached displayed page remains pinned until a normal State
                // duty releases it. Optional metadata cannot make it cold.
                if pages.0.access().pinned_page().is_some() { return false; }
                selection.retire_checkpoint_metadata();
                let mut peers = PeerLocators::default();
                let loaded = {
                    let mut access = pages.0.access();
                    let mut medium = MountedStateMedium::new(scope.io, Some(&mut access));
                    let mut native = scope.card.port(scope.io);
                    let mut store = ReadingStateRouter::new(crate::nvs::NvsBlobBackend::new(scope.nvs),
                        scope.facts, &mut medium, &mut native);
                    ExternalProgressStore::new(&mut store, Some(&digest)).load_peer_locators(&mut peers)
                };
                let retry = if loaded != Load::Loaded(()) { stopped() } else {
                    crate::reader_book::metadata::publish(pages, &memory.runtime.reader, &digest,
                        &saved, &peers, verified, title_deferred, has_session, true, false, &stopped)
                };
                if retry { selection.defer_checkpoint_metadata(); }
                true
            };
            if let Some(session) = session.as_mut() {
                let session::Session { route, pages, .. } = &mut **session;
                let route = route.lend(&boot::NoCancel, millis);
                let mut native = scope.io.port();
                let Ok(pages) = ChapterCacheStorage::lend(pages, &mut native, &route) else { return false; };
                work(&mut ReaderPages(pages), true)
            } else {
                super::checkpoint::with_ownerless_pages(scope.io, filename, |pages| work(pages, false))
                    .unwrap_or(false)
            }
        })).unwrap_or(false);
        if result { idle.note_input(millis()); }
        result
    }

    /// Frozen publishMetadata (#317): after a verified page, the card keeps
    /// this book's title for the next boot's Home. Once per session; a
    /// complete record or the same title is never rewritten. Like the other
    /// optional records, it never makes the displayed cached page cold.
    #[inline(never)]
    pub(super) fn publish_title_image(&mut self, home: &mut Home<'_>) {
        use newt_phased_reader::semantic_metadata::{PositionProjection, ReaderSemanticMetadata};
        if self.selection.flags() & flag::TITLE_DEFERRED != 0 || self.title[0] == 0 { return; }
        let Self { selection, session, title, .. } = self;
        let Some(session) = session.as_mut() else { return; };
        let session::Session { route, pages, verification, .. } = &mut **session;
        if verification.state() != crate::reader_verification::Verification::Verified
            || verification.control().flags.title_published() { return; }
        let digest = *selection.digest();
        let published = home.with_reader_io(|scope| {
            let route = route.lend(&boot::NoCancel, millis);
            let mut native = scope.io.port();
            let Ok(pages) = ChapterCacheStorage::lend(pages, &mut native, &route) else { return false; };
            // The boot load's storage type, so one copy of the record code serves both.
            let mut pages = ReaderPages(pages);
            if pages.0.access().pinned_page().is_some() { return false; }
            let mut stored = [0; 256];
            let mut position = PositionProjection::default();
            // One digest has one spine count, so any complete record is this
            // book's; a title-only record ignores the count.
            let current = ReaderSemanticMetadata::load_position(&mut pages, &digest, 0, 0,
                &mut stored, &mut position) && (position.canonical_spines != 0 || stored == **title);
            let published = current
                || ReaderSemanticMetadata::save(&mut pages, &digest, 0, Some(&title[..]), None);
            pages.0.access().close();
            published
        }).unwrap_or(false);
        if published { verification.control().flags.set_title_published(true); }
    }
}
