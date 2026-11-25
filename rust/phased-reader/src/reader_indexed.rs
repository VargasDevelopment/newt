// Included in reader: selected strings stay in the existing Selection owner.
use newt_core::book_index::{self, Identity as IndexIdentity, Reader as BookIndex};
use crate::phase_storage::ReadScope;

// Keep header opening workspace out of the query caller and its sibling phases.
#[inline(never)]
pub(crate) fn query_book_index<C: ReadScope, R>(
    source: &mut dyn BookSource, store: &mut C, digest: &[u8; 32],
    scratch: &mut [u8], read: impl FnOnce(&BookIndex, &mut dyn newt_core::page_cache::CacheStorage) -> Result<R, Status>,
) -> Result<R, Status> {
    query_index_store(source, store, digest, scratch, |store| {
        let identity = IndexIdentity { digest: *digest, policy: admission::POLICY_VERSION };
        let index = BookIndex::open(store, &identity).ok_or(Status::Damaged)?;
        read(&index, store)
    })
}

// Reuse the pass's checked header while retaining each storage scope's checked exit.
#[inline(never)]
pub(crate) fn query_index_store<C: ReadScope, R>(
    source: &mut dyn BookSource, store: &mut C, digest: &[u8; 32], scratch: &mut [u8],
    read: impl FnOnce(&mut dyn newt_core::page_cache::CacheStorage) -> Result<R, Status>,
) -> Result<R, Status> {
    let name = book_index::FileName::new(digest, book_index::FileKind::Index);
    let bytes = source.size();
    let scoped = store.query_index(name.as_bytes(), digest, false, scratch,
        &mut || validate(source, bytes) == Status::Ok, |store| read_index_store(store, read));
    let result = match scoped {
        Ok((result, true)) => result,
        Ok((_, false)) => Err(Status::Stale),
        Err(error) => Err(error),
    };
    let damaged = matches!(result, Err(Status::Damaged | Status::IoError))
        || matches!(result, Err(Status::Stale)) && validate(source, bytes) == Status::Ok;
    if damaged { store.invalidate_index(digest); }
    result
}

// The callback's header and result temporaries are needed only after attach,
// when the native descriptor is already open.
#[inline(never)]
fn read_index_store<R>(store: &mut dyn newt_core::page_cache::CacheStorage,
    read: impl FnOnce(&mut dyn newt_core::page_cache::CacheStorage) -> Result<R, Status>,
) -> Result<R, Status> {
    read(store)
}

// Selection workspace must return before navigation and style parsing begin.
#[inline(never)]
fn select_indexed_entry<C: ReadScope>(
    source: &mut dyn BookSource, store: &mut C, digest: &[u8; 32], spine: u32,
    scratch: &mut [u8], scan: &mut [u8; 4099], selected: &mut Selection,
    opened: &mut Option<BookIndex>,
) -> Result<newt_reader_navigation::ProgressInputs, Status> {
    let (member, before, end) = query_book_index(source, store, digest, scratch, |index, store| {
        if spine >= index.header().spine { return Err(Status::NotFound); }
        let item = index.spine(store, spine).ok_or(Status::Damaged)?;
        if item.member == book_index::NONE { return Err(Status::NotFound); }
        let member = index.member(store, item.member).ok_or(Status::Damaged)?;
        let end = if spine + 1 < index.header().spine {
            index.spine(store, spine + 1).ok_or(Status::Damaged)?.before
        } else { index.header().spine_bytes };
        if end < item.before { return Err(Status::Damaged); }
        *opened = Some(*index);
        Ok((member, item.before, end))
    })?;
    let mut directory = crate::zip_container::Directory::default();
    let status = crate::zip_container::locate(source, scan, &mut directory);
    if status != Status::Ok { return Err(status); }
    selected.entry = newt_core::member_check::check_indexed(source, &member, None,
        &mut selected.href, directory.offset)?;
    if !newt_core::phased_metadata::admissible_href(selected.href(), selected.href.len()) {
        return Err(Status::Unsupported);
    }
    let index = opened.as_ref().ok_or(Status::Damaged)?;
    selected.title.copy_from_slice(&index.header().title);
    selected.language = index.header().language;
    selected.spines = index.header().spine;
    let progress = newt_reader_navigation::ProgressInputs { before, current: end - before,
        total: index.header().spine_bytes, index: spine, count: index.header().spine,
        valid: index.header().spine_bytes != 0 };
    Ok(progress)
}

impl PhasedReader<'_, '_> {
    /// Select from the durable index; no OPF/catalog parse owns this path.
    pub fn select_indexed<C: ReadScope, B: ParserBackend + ?Sized>(
        &mut self, source: &mut dyn BookSource, store: &mut C, digest: &[u8; 32],
        spine: u32, phase: &mut Phase<'_>, backend: &mut B, plain: bool,
    ) -> Result<bool, Status> {
        let _work = WorkScope::new_scoped(self.on_yield, self.work, WorkPhase::Metadata);
        let selected = &mut self.selections[self.active ^ 1];
        let mut opened = None;
        let progress = phase.with_index_open(|query, scan| {
            select_indexed_entry(source, store, digest, spine, query, scan, selected, &mut opened)
        })?;
        let index = opened.as_ref().ok_or(Status::Damaged)?;
        phase.navigation_indexed(source, backend, store, digest, index,
            progress, &mut selected.navigation, None, self.on_yield)?;
        let undeclared = !index.header().stylesheets;
        if plain { return Ok(false); }
        if undeclared { phase.defer(spine)?; }
        else {
            let status = phase.styles_indexed(source, backend, store, digest, index, selected, self.on_yield)?;
            if status == Status::OutOfMemory { return Ok(false); }
            if status != Status::Ok { return Err(status); }
            let status = phase.build_styles(source, backend, &selected.entry, selected.href(), spine, self.on_yield)?;
            if !matches!(status, Status::Ok | Status::OutOfMemory) { return Err(status); }
        }
        Ok(undeclared)
    }
}

impl PhasedReader<'_, '_> {
    pub fn open_indexed<C: ReadScope, B: ParserBackend + ?Sized>(
        &mut self, source: &mut dyn BookSource, store: &mut C, digest: &[u8; 32],
        storage: &mut PhaseStorage<'_>, lender: &mut impl BuildLoan, backend: &mut B,
    ) -> Status {
        self.close();
        storage.discard_styles();
        let bytes = source.size();
        if bytes == 0 { return Status::NotZip; }
        if bytes > admission::EXTERNAL_MAX_BYTES { return Status::Unsupported; }
        let result = storage.with_loan(lender, |phase| {
            self.select_indexed(source, store, digest, 0, phase, backend, false)
        });
        let undeclared = match result { Ok(undeclared) => undeclared, Err(error) => return error };
        let status = validate(source, bytes);
        if status != Status::Ok { return status; }
        self.active ^= 1;
        self.source_bytes = bytes;
        self.opened = true;
        self.styles_ready = storage.styles_ready(0);
        self.undeclared_styles = undeclared;
        self.proofs.selection = true;
        Status::Ok
    }

    #[inline(never)]
    pub fn prepare_chapter_menu_indexed<C: ReadScope, B: ParserBackend + ?Sized>(
        &mut self, source: &mut dyn BookSource, store: &mut C, digest: &[u8; 32], focus: u32,
        storage: &mut PhaseStorage<'_>, lender: &mut impl BuildLoan, backend: &mut B,
    ) -> Status {
        if !self.opened || !self.has_page || self.plan.is_none() || self.pending.is_some() {
            return Status::Unsupported;
        }
        let (left, right) = self.selections.split_at_mut(1);
        let (selected, staged) = if self.active == 0 { (&left[0], &mut right[0]) } else { (&right[0], &mut left[0]) };
        let current = selected.navigation.section(self.info.char_start);
        if staged.navigation.reuse_menu(current, focus) { return Status::Ok; }
        staged.navigation.invalidate_menu();
        self.styles_ready = false;
        let result = storage.with_loan(lender, |phase| {
            let index = phase.with_index_open(|query, _| query_book_index(source, store, digest, query,
                |index, _| Ok(*index)))?;
            if index.header().spine != selected.spines { return Err(Status::Stale); }
            phase.navigation_indexed(source, backend, store, digest, &index, Default::default(),
                &mut staged.navigation, Some((current, focus)), self.on_yield)
        });
        let status = result.map_or_else(|error| error, |_| validate(source, self.source_bytes));
        if status != Status::Ok { staged.navigation.invalidate_menu(); }
        status
    }
}

impl PhasedReader<'_, '_> {
    pub fn capture_indexed<C: ReadScope, B: ParserBackend + ?Sized>(
        &mut self, source: &mut dyn BookSource, store: &mut C, digest: &[u8; 32],
        spine: u32, target: u32, mode: Target, storage: &mut PhaseStorage<'_>,
        lender: &mut impl BuildLoan, backend: &mut B,
        breaker: &mut dyn newt_core::chapter_layout::types::LineBreak,
    ) -> Status {
        if self.plan.is_none() { return Status::Unsupported; }
        if matches!(mode, Target::TocResolve | Target::TocPage) { return self.failed(Status::Unsupported); }
        self.layout_phased_selected(source, spine, target, mode, storage, lender, backend, breaker,
            |reader, source, spine, phase, backend| reader.select_indexed(source, store, digest, spine, phase, backend, false))
    }
}

impl PhasedReader<'_, '_> {
    pub fn capture_chapter_indexed<C: ReadScope, B: ParserBackend + ?Sized>(
        &mut self, source: &mut dyn BookSource, store: &mut C, digest: &[u8; 32],
        spine: u32, toc: u16, storage: &mut PhaseStorage<'_>, lender: &mut impl BuildLoan,
        backend: &mut B, breaker: &mut dyn newt_core::chapter_layout::types::LineBreak,
    ) -> Status {
        self.capture_chapter_selected(source, spine, toc, storage, lender, backend, breaker,
            |reader, source, spine, phase, backend| reader.select_indexed(source, store, digest, spine, phase, backend, false))
    }
}
