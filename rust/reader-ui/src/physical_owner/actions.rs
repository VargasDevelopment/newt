// SPDX-License-Identifier: MIT
//! Public action entry points over the existing physical owner.
use super::*;
use crate::physical_source::SourceOperations;

impl ExternalOwner {
    /// Reopening requires the preceding index pass, including after settings changes.
    ///
    /// ```compile_fail
    /// use newt_core::{progress::Progress, xml_session::ParserBackend};
    /// use newt_phased_reader::{PhasedReader, CachePresence, phase_storage::ReadScope};
    /// use newt_reader_ui::{ExternalOwner, PhysicalOwner, PhysicalSource, ReaderPanelLoan, RestorePolicy};
    /// fn reopen<B: ParserBackend, L: ReaderPanelLoan, C: ReadScope + CachePresence, S: PhysicalSource>(
    ///     owner: &mut ExternalOwner, reader: &mut PhasedReader<'_, '_>, source: &mut S,
    ///     ports: &mut PhysicalOwner<'_, '_, '_, B, L, C>,
    /// ) {
    ///     owner.open_physical(reader, source, 0, Progress::default(), RestorePolicy::Validated, 0, ports);
    /// }
    /// ```
    pub fn open_physical<
        B: ParserBackend + ?Sized,
        L: ReaderPanelLoan,
        C: ReadScope + CachePresence,
        S: PhysicalSource,
    >(
        &mut self,
        indexed: &newt_phased_reader::index_build::Indexed,
        reader: &mut PhasedReader<'_, '_>,
        revision: &mut S,
        identity: u64,
        saved: Progress,
        policy: RestorePolicy,
        now: u32,
        resources: &mut PhysicalOwner<'_, '_, '_, B, L, C>,
    ) -> bool {
        let matches = revision.with_physical_source(|source| match source {
            crate::physical_source::PhysicalSourceRef::Revision(source) => source.digest() == indexed.digest(),
            crate::physical_source::PhysicalSourceRef::Reader(source) => source.digest() == *indexed.digest(),
            crate::physical_source::PhysicalSourceRef::Strict(source) => source.digest() == *indexed.digest(),
            crate::physical_source::PhysicalSourceRef::Pending(source) => source.digest() == *indexed.digest(),
        });
        if !matches { return false; }
        self.open_with(reader, revision, identity, saved, policy, now, resources)
    }
    pub fn turn_physical<
        B: ParserBackend + ?Sized,
        L: ReaderPanelLoan,
        C: ReadScope + CachePresence,
        S: PhysicalSource,
    >(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        revision: &mut S,
        action: Action,
        now: u32,
        resources: &mut PhysicalOwner<'_, '_, '_, B, L, C>,
    ) -> bool {
        self.turn_with(reader, revision, action, now, resources)
    }
    pub fn jump_physical<
        B: ParserBackend + ?Sized,
        L: ReaderPanelLoan,
        C: ReadScope + CachePresence,
        S: PhysicalSource,
    >(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        revision: &mut S,
        chapter: u32,
        now: u32,
        resources: &mut PhysicalOwner<'_, '_, '_, B, L, C>,
    ) -> ChapterJumpOutcome {
        self.jump_with(reader, revision, chapter, now, resources)
    }
    pub fn paint_physical<
        B: ParserBackend + ?Sized,
        L: ReaderPanelLoan,
        C: ReadScope + CachePresence,
        S: PhysicalSource,
    >(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        revision: &mut S,
        now: u32,
        resources: &mut PhysicalOwner<'_, '_, '_, B, L, C>,
    ) -> bool {
        self.paint_with(reader, revision, now, resources)
    }
    pub fn complete_ui_physical<
        B: ParserBackend + ?Sized,
        L: ReaderPanelLoan,
        C: ReadScope + CachePresence,
        S: PhysicalSource,
    >(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        revision: &mut S,
        ui: &mut ReaderUiState,
        result: ReaderUiResult,
        now: u32,
        resources: &mut PhysicalOwner<'_, '_, '_, B, L, C>,
    ) -> ReaderUiResult {
        self.complete_ui_with(reader, revision, ui, result, now, resources)
    }
    pub fn handle_ui_physical<
        B: ParserBackend + ?Sized,
        L: ReaderPanelLoan,
        C: ReadScope + CachePresence,
        S: PhysicalSource,
    >(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        revision: &mut S,
        ui: &mut ReaderUiState,
        settings: ViewSettings,
        action: Action,
        now: u32,
        resources: &mut PhysicalOwner<'_, '_, '_, B, L, C>,
    ) -> ReaderUiResult {
        let failure = self.chapter_failure().copied();
        handle_reader_ui_with(
            &mut PhysicalMenu {
                reader,
                revision,
                resources,
            },
            &mut self.state,
            ui,
            settings,
            action,
            now,
            &mut self.tracker,
            failure.as_ref(),
        )
    }
}
