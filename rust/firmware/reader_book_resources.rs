use super::*;

/// Owned per book; PhysicalOwner is built per owner call, so nothing holds
/// the panel or Pages between calls.
pub(crate) struct Resources<'r, 'scope, 'sdk, 'route, 'bank, N: Native> {
    pub(crate) panel: &'r mut ReaderPanel<'scope, 'sdk>,
    pub(crate) pages: &'r mut ReaderPages<'route, N>,
    pub(crate) storage: &'r mut Storage<'bank>,
    pub(crate) parser: Expat,
    pub(crate) breaker: &'r mut dyn LineBreak,
    pub(crate) hash: &'r mut dyn newt_phased_reader::hash_provider::CacheHash,
    pub(crate) settings: ViewSettings,
    pub(crate) open: PhysicalOpen,
    pub(crate) recovery: Option<PendingRecovery>,
}

impl<'scope, 'sdk, 'route, 'bank, N: Native> Resources<'_, 'scope, 'sdk, 'route, 'bank, N> {
    pub(super) fn owner(
        &mut self,
    ) -> PhysicalOwner<'_, 'bank, '_, Expat, ReaderPanel<'scope, 'sdk>, ReaderPages<'route, N>>
    {
        PhysicalOwner {
            storage: &mut self.storage,
            panel: &mut *self.panel,
            backend: &mut self.parser,
            breaker: &mut *self.breaker,
            cache: PhasedChapterCache {
                store: &mut *self.pages,
                hash: &mut *self.hash,
                settings: self.settings,
            },
            open: self.open,
            cache_only: false,
        }
    }

    #[cfg(feature = "checkpoint-fixture")]
    pub(super) fn reopen(
        &mut self,
        reader: &mut PhasedReader<'_, '_>,
        source: &mut impl newt_reader_ui::SourceOperations,
        owner: &mut ExternalOwner,
        digest: &Digest,
        identity: u64,
        saved: Progress,
        settings: ViewSettings,
        policy: RestorePolicy,
        now: u32,
        cancel: &dyn Authority,
    ) -> bool {
        self.recovery = Some(PendingRecovery {
            progress: saved,
            attempted_sequence: saved.sequence,
            settings,
            policy,
        });
        owner.settings_committed(reader, settings);
        self.settings = settings;
        let selected = self
            .panel
            .with_parse(|scratch| source.prepare_reopen(*digest, scratch, &|| !cancel.cancelled()));
        let Ok(Some(imported)) = selected else {
            return false;
        };
        self.open = if imported {
            PhysicalOpen::Prepared
        } else {
            PhysicalOpen::Selected {
                digest: *digest,
                spine: saved.locator.spine,
            }
        };
        owner.open_physical(
            reader,
            source,
            identity,
            saved,
            policy,
            now,
            &mut self.owner(),
        )
    }
}
