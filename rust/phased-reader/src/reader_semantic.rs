// MIT. Included inside reader.rs. Complete-table preparation only.
impl PhasedReader<'_, '_> {
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_semantic_index<B: ParserBackend + ?Sized, C: crate::phase_storage::ReadScope>(
        &mut self,
        source: &mut dyn BookSource,
        store: &mut C,
        digest: &newt_core::book_semantic_index::Digest,
        index: &mut newt_core::book_semantic_index::Index,
        metadata: &mut Arena<'_>,
        parse: &mut Arena<'_>,
        r: &mut canonical::Resources<'_, B>,
        slot: &mut Slot<'_>,
        lease: Option<&Lease>,
    ) -> Status {
        if !self.opened {
            return Status::NotFound;
        }
        // Guard the published borrowed frame while a parse lease is held.
        // Pending capture is also unpublished live parse storage in this API.
        if (self.has_page && lease.is_some()) || self.pending.is_some() {
            return Status::Unsupported;
        }
        let held = match lease {
            Some(l) => match l.hold() {
                Ok(h) => Some(h),
                Err(e) => return e,
            },
            None => None,
        };
        // Safe disjoint typed loans replace validSemanticOwner address checks.
        // Retained semantic owner detachment must be added here on adoption.
        crate::semantic_builder::build(
            source,
            self.source_bytes,
            self.selection().spines,
            digest,
            store,
            index,
            metadata,
            parse,
            r,
            self.on_yield,
            self.work,
            || {
                self.styles_ready = false;
                slot.reset(slot.spine, StyleState::Empty, 0);
            },
            || drop(held),
        )
    }
}
