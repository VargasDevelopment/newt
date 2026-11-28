// SPDX-License-Identifier: MIT
//! One call's disjoint reader loans; all source modes share the existing drive.
use super::*;
use crate::{home_startup::FirmwareCard, sdk::Sdk552,
    reader_book::{self, BookExit, BookFailure, BookRequest, BookStep, MountedCheckpoint,
        ProducerCallbacks, ProducerPauseFn, Resources}, reader_linebreak::NativeBreak,
    reader_pages::ReaderPages};
use newt_core::admission::WorkEvent;
use newt_native_io::{OsNative, UiIo};
use newt_reader_ui::{PhysicalOpen, SourceOperations, ViewSettings};

pub(super) struct Opening {
    pub(super) accepted_save: bool,
    pub(super) at: u32,
    pub(super) indexed: newt_phased_reader::index_build::Indexed,
}

pub(super) struct RunLoan<'op, 'm, 'p> {
    pub(super) memory: &'op mut MemoryLoan<'m, 'p>,
    pub(super) ui: &'op mut newt_reader_ui::ReaderUiState,
    pub(super) checkpoint: &'op mut ReaderCheckpoint,
    pub(super) statistics: &'op mut ReadingStatsRuntime,
    pub(super) recovery: &'op mut Option<reader_book::PendingRecovery>,
    pub(super) accepted: &'op mut bool,
    pub(super) metadata: &'op mut Option<Fixed<crate::reader_exit_metadata::ExitMetadata>>,
    pub(super) flags: Option<&'op mut u16>,
    pub(super) settings: &'op mut ViewSettings,
}
#[derive(Clone, Copy)]
pub(super) struct Turn<'a> {
    pub(super) indexed: Option<&'a newt_phased_reader::index_build::Indexed>,
    pub(super) paint_first: bool,
    pub(super) accepted_save: bool,
    pub(super) step: Option<BookStep>,
    pub(super) open: PhysicalOpen,
    pub(super) cache_only: bool,
    pub(super) settings: ViewSettings,
}
/// Deliberately excludes the console and clock. The producer's real input
/// probe may share the console while these disjoint physical loans run.
pub(super) struct Ports<'op, 'io, 'scope, 'sdk> {
    pub(super) io: &'io UiIo,
    pub(super) panel: &'op mut newt_c3_board::ReaderPanel<'scope, 'sdk>,
    pub(super) card: &'op mut FirmwareCard,
    pub(super) preferences: &'op mut crate::home_startup::preferences::ReaderPreferences,
    pub(super) nvs: &'op mut Sdk552,
    pub(super) facts: &'op mut newt_native_storage::reading_state::StateFacts,
}
impl RunLoan<'_, '_, '_> {
    pub(super) fn finish<'io>(self, scope: Ports<'_, 'io, '_, '_>,
        pages: &mut ReaderPages<'_, OsNative<'io>>, source: &impl SourceOperations,
        digest: [u8; 32], zone: newt_core::time_zone::TimeZone, now: u32, sleeping: bool,
        allow_cancelled: bool, trace: crate::reader_action::Trace<'_>,
    ) -> Progress {
        let book = &mut *self.memory.runtime;
        let mut breaker = NativeBreak;
        let mut hash = crate::reader_hash::PackageHasher::from_live_sdk();
        let mut resources = Resources { panel: scope.panel, pages,
            storage: &mut self.memory.storage, parser: newt_target_expat::Expat::default(),
            breaker: &mut breaker, hash: &mut hash, settings: *self.settings,
            open: PhysicalOpen::Selected { digest, spine: book.owner.tracker.snapshot(now).locator.spine },
            cache_only: false, recovery: *self.recovery };
        let cancel = crate::reader_action::ObservedAuthority { original: &boot::NoCancel, trace };
        let request = reader_book::ExitRequest { zone, cancel: &cancel };
        let mut mounted = MountedCheckpoint::new(self.checkpoint, scope.nvs, scope.facts);
        mounted.finish_exit_captured(scope.io, scope.card, &mut resources, &book.reader, source,
            &mut book.owner, self.ui, digest, &request, &crate::reader_hash::package_hash,
            self.statistics, now, self.accepted, sleeping, allow_cancelled,
            if sleeping { None } else { self.metadata.as_deref_mut() });
        *self.recovery = resources.recovery;
        *book.owner.tracker.snapshot(now)
    }

    // Keep one operation's workspace out of sibling source-mode callers.
    #[inline(never)]
    pub(super) fn run<'io>(self, scope: Ports<'_, 'io, '_, '_>,
        pages: &mut ReaderPages<'_, OsNative<'io>>, source: &mut impl SourceOperations,
        request: &BookRequest<'_>, callbacks: &ProducerCallbacks<'_>,
        pause: &mut ProducerPauseFn<'_, 'io, Sdk552, FirmwareCard>,
        visit: Option<(&mut [u8; 256], &mut u16)>, turn: Turn<'_>,
        yielded: &dyn Fn(), observed: &dyn Fn(WorkEvent),
        saves: &mut newt_core::save_notice::ReadingSaveNotice, title_deferred: bool,
        index_trim_owed: &mut bool,
    ) -> Result<BookExit, BookFailure> {
        let mut breaker = NativeBreak;
        let mut hash = crate::reader_hash::PackageHasher::from_live_sdk();
        let mut resources = Resources { panel: scope.panel, pages,
            storage: &mut self.memory.storage, parser: newt_target_expat::Expat::default(),
            breaker: &mut breaker, hash: &mut hash, settings: turn.settings,
            open: turn.open, cache_only: turn.cache_only, recovery: *self.recovery };
        let book = &mut *self.memory.runtime;
        let mut mounted = MountedCheckpoint::new(self.checkpoint, scope.nvs, scope.facts);
        let mut next = turn.step;
        let mut paint_failure = None;
        let mut paint = None;
        let (ledger, parts) = match visit {
            Some((ledger, view)) => {
                let Some(flags) = self.flags else { return Err(BookFailure::Revision); };
                (Some(ledger), Some(reader_book::VisitParts { view, flags }))
            }
            None => (None, None),
        };
        let digest = match request.open {
            reader_book::BookOpen::Receipt(digest) => digest,
            // A closed source keeps the zero digest it had before, which matches no book.
            reader_book::BookOpen::Import => source.digest().unwrap_or_default(),
        };
        let result = book.reader.with_scoped_work(yielded, Some(observed), |reader| {
            reader_book::run::drive(scope.io, scope.card, request, reader,
                &mut book.owner, self.ui, source, &mut resources,
                scope.preferences, &mut mounted, &crate::reader_hash::package_hash,
                self.statistics, self.accepted, self.recovery, request.identity, request.saved, digest,
                callbacks, ledger, turn.indexed, turn.paint_first, reader_book::run::Route::Short, turn.accepted_save,
                parts, Some(self.metadata), title_deferred, Some(saves), index_trim_owed, |between, _, _| {
                    match between.composed {
                        Some(Err(status)) => paint_failure = Some(status),
                        Some(Ok(composed)) => paint = Some(composed),
                        None => {}
                    }
                    next.take()
                }, pause)
        });
        // This is SampleReader.settings_, including the resulting geometry of
        // a failed reflow. Home's acknowledged durable preference stays separate.
        *self.settings = resources.settings;
        match paint_failure {
            Some(status) => Err(BookFailure::Open(status)),
            None => result.map(|exit| BookExit { paint, ..exit }),
        }
    }
}
