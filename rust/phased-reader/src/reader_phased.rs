include!("reader_metadata_entry.rs");

// Both entry points use this selection/navigation/style policy on the same Book.
pub(crate) fn select_book_metadata<
    'a,
    'storage,
    S: BookSource + ?Sized,
    B: ParserBackend + ?Sized,
>(
    mut book: canonical::Book<'a, 'storage, S, B>,
    spine: u32,
    selected: &mut Selection,
    sr: &mut StyleResources<'_, '_>,
    sources: &mut Sources<'_>,
    on_yield: newt_core::admission::Yield<'_>,
    plain: bool,
    suffix_capacity: Option<&mut usize>,
    capture_offset: Option<(usize, &mut usize)>,
    toc_backing: &mut (impl package::TocBacking + ?Sized),
) -> Result<(bool, bool), Status> {
    let (package, entry_count, navigation) = {
        let item = book.spine_item(spine as usize).ok_or(Status::NotFound)?;
        let href = item
            .href
            .and_then(|h| book.arena().c_string(h))
            .ok_or(Status::NotFound)?;
        if !newt_core::phased_metadata::admissible_href(href, selected.href.len()) {
            return Err(Status::Unsupported);
        }
        let (_, entry) = book.catalog().find(href).ok_or(Status::NotFound)?;
        selected.href[..href.len()].copy_from_slice(href);
        selected.href[href.len()] = 0;
        // The chapter's local header is checked as it opens. A damaged chapter
        // fails here and gets the chapter notice; the rest of the book reads.
        selected.entry = book.check(&entry)?;
        selected.spines =
            u32::try_from(book.package().spine_count).map_err(|_| Status::Unsupported)?;
        newt_core::phased_metadata::copy_display_title(
            &mut selected.title,
            book.package()
                .metadata
                .title
                .and_then(|h| book.arena().c_string(h)),
        );
        selected.language = BookLanguage::from_opf(book.package().metadata.language
            .and_then(|h| book.arena().c_string(h)));
        let nav = book
            .navigation_item()
            .and_then(|m| m.href)
            .and_then(|h| book.arena().c_string(h))
            .and_then(|h| book.catalog().find(h))
            .map(|(_, e)| (e, book.navigation_is_epub3()));
        (*book.package(), book.catalog().len(), nav)
    };
    let canonical::BookParts {
        mut workflow,
        arena,
        entries,
        manifest,
        spine: spines,
        result: _,
        chunk: _,
        directory_offset,
    } = book.into_parts();
    let mark = arena.mark();
    let bytes = workflow.source.size();
    let mut guarded = NavigationSource {
        source: &mut *workflow.source,
        bytes,
        failed: false,
    };
    let live = guarded.guard();
    if live != Status::Ok {
        return Err(live);
    }
    let mut bind = |arena: &Arena<'_>, toc: &[TocEntry]| {
        selected.navigation.bind(
            &Metadata {
                arena,
                package: &package,
                manifest: &manifest[..package.manifest_count],
                spine: &spines[..package.spine_count],
                toc,
                entries: &entries[..entry_count],
            },
            spine,
        )
    };
    let optional = if let Some((entry, epub3)) = navigation {
        let href = entry
            .name
            .and_then(|n| arena.c_string(n))
            .map(crate::xml_input::c_bytes);
        // A damaged optional TOC degrades to no TOC, like an unparsable one.
        match canonical::CatalogView::new(&entries[..entry_count], arena, directory_offset)
            .check(&mut guarded, &entry)
        {
            Err(status) => Err(status),
            Ok(_) if !href.is_some_and(|h| crate::xml_input::dir_name(h, sr.directory)) => {
                Err(Status::Unsupported)
            }
            Ok(entry) => {
                let mut navigation_workflow = Workflow {
                    policy: workflow.policy,
                    source: &mut guarded,
                    backend: &mut *workflow.backend,
                    decoder: resources::decoder(&mut workflow.decoder),
                    buffer: workflow.buffer.as_deref_mut(),
                    filter: workflow.filter.as_deref_mut(),
                };
                let dir = crate::xml_input::c_bytes(sr.directory);
                let kind = if epub3 { TocKind::Nav } else { TocKind::Ncx };
                package::count_toc(&mut navigation_workflow, &entry, dir, arena, kind).and_then(
                    |count| {
                        toc_backing.with_toc(arena, count, |arena, toc| {
                            package::fill_toc(&mut navigation_workflow, &entry, dir, arena, kind, toc)
                                .map(|used| bind(arena, &toc[..used]))
                        })?
                    },
                )
            }
        }
    } else {
        Err(Status::Unsupported)
    };
    let (bound, optional) = match optional {
        Ok(bound) => (bound, Status::Ok),
        Err(status) => (bind(arena, &[]), status),
    };
    // The copied projection survives both typed tail reclamation and strings.
    arena.release(mark);
    let live = guarded.guard();
    drop(guarded);
    if live != Status::Ok {
        return Err(live);
    }
    if crate::reader::optional_toc_ends_open(optional) {
        return Err(optional);
    }
    if !bound {
        return Err(Status::Unsupported);
    }
    // Cache-hit binding preserves canonical navigation and omits style I/O.
    if plain {
        return Ok((false, false));
    }
    let mut resolve = |arena: &Arena<'_>, sources: &mut Sources<'_>| {
        let book = styles::Book {
            manifest: &manifest[..package.manifest_count],
            arena,
            catalog: Some(canonical::CatalogView::new(&entries[..entry_count], arena, directory_offset)),
        };
        if !book.declares_stylesheet() {
            return (true, Status::Ok);
        }
        (
            false,
            styles::resolve(
                &book,
                &mut *workflow.source,
                &selected.entry,
                Some(selected.href()),
                &mut *workflow.backend,
                styles::ResolveScratch {
                    directory: sr.directory,
                    paths: sr.paths,
                    decoder: resources::decoder(&mut workflow.decoder),
                    xml: xml_session::Resources {
                        buffer: workflow.buffer.as_deref_mut(),
                        filter: workflow.filter.as_deref_mut(),
                    },
                },
                on_yield,
                sources,
                    workflow.policy,
                ),
        )
    };
    let (undeclared, status) = if let Some(capacity) = suffix_capacity {
        arena.with_unused_suffix(|frozen, suffix| {
            *capacity = suffix.len().min(styles::EMBEDDED);
            let at = suffix.len() - *capacity;
            if let Some((bank_base, offset)) = capture_offset {
                *offset = (suffix.as_ptr() as usize)
                    .checked_sub(bank_base)
                    .and_then(|n| n.checked_add(at))
                    .ok_or(Status::OutOfMemory)?;
            }
            let mut captured = Sources {
                links: sources.links,
                start: sources.start,
                length: sources.length,
                issues: sources.issues,
                used: sources.used,
                events: sources.events,
                external_mask: sources.external_mask,
                text: &mut suffix[at..],
            };
            let result = resolve(frozen, &mut captured);
            sources.links = captured.links;
            sources.start = captured.start;
            sources.length = captured.length;
            sources.issues = captured.issues;
            sources.used = captured.used;
            sources.events = captured.events;
            sources.external_mask = captured.external_mask;
            Ok(result)
        })?
    } else {
        resolve(arena, sources)
    };
    if !undeclared {
        if status == Status::OutOfMemory {
            return Ok((undeclared, false));
        }
        if status != Status::Ok {
            return Err(status);
        }
    }
    Ok((undeclared, true))
}

use crate::phase_storage::{BuildLoan, Phase, Storage as PhaseStorage};

impl PhasedReader<'_, '_> {
    fn select_physical<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
        &mut self,
        source: &mut S,
        spine: u32,
        phase: &mut Phase<'_>,
        backend: &mut B,
        plain: bool,
    ) -> Result<bool, Status> {
        let _work = WorkScope::new_scoped(self.on_yield, self.work, WorkPhase::Metadata);
        let selected = &mut self.selections[self.active ^ 1];
        let (undeclared, resolved) = {
            phase.metadata_counted(
                source,
                backend,
                false,
                self.on_yield,
                self.work,
                |book, sr, sources, capacity, offset, bank_base| {
                    select_book_metadata(
                        book,
                        spine,
                        selected,
                        sr,
                        sources,
                        self.on_yield,
                        plain,
                        Some(capacity),
                        Some((bank_base, offset)),
                        &mut crate::phase_storage::ScopedToc,
                    )
                },
            )??
        };
        if !plain && resolved {
            if undeclared {
                phase.defer(spine)?;
            } else {
                let status = phase
                    .build_styles(
                        source,
                        backend,
                        &selected.entry,
                        selected.href(),
                        spine,
                        self.on_yield,
                    )
                    .unwrap_or_else(|e| e);
                if !matches!(status, Status::Ok | Status::OutOfMemory) {
                    return Err(status);
                }
            }
        }
        Ok(undeclared)
    }
    /// Actual physical acquisition contains both admission and selection.
    /// No typed metadata/parse reference exists before acquisition or after return.
    pub fn open_phased<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
        &mut self,
        source: &mut S,
        storage: &mut PhaseStorage<'_>,
        lender: &mut impl BuildLoan,
        backend: &mut B,
    ) -> Status {
        self.close();
        storage.discard_styles();
        let bytes = source.size();
        if bytes == 0 {
            return Status::NotZip;
        }
        if bytes > admission::EXTERNAL_MAX_BYTES {
            return Status::Unsupported;
        }
        let result = storage.with_loan(lender, |phase| {
            let status = {
                let _work = WorkScope::new_scoped(self.on_yield, self.work, WorkPhase::Admission);
                phase.admit(source, backend, self.on_yield, self.work)?
            };
            if status != Status::Ok {
                return Err(status);
            }
            self.select_physical(source, 0, phase, backend, false)
        });
        // Actual SDK return has already cleared parse and invalidated shadow.
        let undeclared = match result {
            Ok(u) => u,
            Err(e) => return e,
        };
        let status = validate(source, bytes);
        if status != Status::Ok {
            return status;
        }
        self.active ^= 1;
        self.source_bytes = bytes;
        self.opened = true;
        self.styles_ready = storage.styles_ready(0);
        self.undeclared_styles = undeclared;
        self.proofs.selection = true;
        Status::Ok
    }
    fn layout_phased<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
        &mut self,
        source: &mut S,
        spine: u32,
        target: u32,
        mode: Target,
        storage: &mut PhaseStorage<'_>,
        lender: &mut impl BuildLoan,
        backend: &mut B,
        breaker: &mut dyn newt_core::chapter_layout::types::LineBreak,
    ) -> Status {
        self.pending = None;
        self.pending_cached = false;
        self.pending_undeclared = None;
        let resolving = mode == Target::TocResolve;
        let required = if resolving {
            target as u16
        } else if mode == Target::TocPage {
            self.selections[self.active ^ 1]
                .navigation
                .staged_required_toc()
        } else {
            NO_TOC
        };
        if !resolving {
            self.proofs.plan = false;
        }
        self.malformed_utf8 = false;
        self.empty_chapter = false;
        if !self.opened {
            return self.layout_failed(resolving, Status::NotFound);
        }
        let status = self.validate(source);
        if status != Status::Ok {
            return self.layout_failed(resolving, status);
        }
        let reuse = self.proofs.chapter && self.has_page && self.info.spine == spine;
        if reuse && mode == Target::Page && target >= self.info.page_count {
            return self.layout_failed(resolving, Status::NotFound);
        }
        let outcome = storage.with_loan(lender, |phase| {
            let inactive = self.active ^ 1;
            if !matches!(mode, Target::TocResolve | Target::TocPage)
                && self.proofs.selection
                && self.info.spine == spine
                && (self.styles_ready && phase.ready(spine) || self.undeclared_styles)
            {
                let (left, right) = self.selections.split_at_mut(1);
                if self.active == 0 {
                    right[0].copy_from(&left[0]);
                } else {
                    left[0].copy_from(&right[0]);
                }
                if !(self.styles_ready && phase.ready(spine)) {
                    phase.defer(spine)?;
                    self.styles_ready = true;
                }
            } else {
                self.styles_ready = false;
                self.selections[inactive]
                    .navigation
                    .stage_required_toc(required);
                let result = self.select_physical(source, spine, phase, backend, false);
                match result {
                    Ok(u) => {
                        self.styles_ready = true;
                        self.undeclared_styles = u;
                    }
                    Err(e) => return Err(e),
                }
            }
            let mut passes = 0;
            let outcome = loop {
                passes += 1;
                if passes > 3 || !self.styles_ready || !phase.ready(spine) {
                    break Err(Status::OutOfMemory);
                }
                let selected = &mut self.selections[inactive];
                if !reuse || matches!(mode, Target::TocResolve | Target::TocPage) {
                    selected.navigation.begin_anchors();
                }
                let _work = WorkScope::new_scoped(self.on_yield, self.work, WorkPhase::Layout);
                let result = phase.layout(backend, breaker, |slot, lr, io| {
                    pass_stream(
                        source,
                        selected,
                        target,
                        mode,
                        self.page_layout,
                        None,
                        self.plan.as_deref_mut(),
                        slot,
                        lr,
                        io,
                        reuse,
                        self.on_yield,
                    )
                })?;
                // XML, decoder, Engine, sink and layout views have ended. Style
                // payload has moved from parse top back into shared top and sealed.
                let (mut status, mut pages, mut chars, complete, state, verdict) = match result {
                    Ok(v) => v,
                    Err(e) => break Err(e),
                };
                if verdict.stopped {
                    status =
                        phase.settle(source, backend, selected, spine, verdict, self.on_yield)?;
                    if status != Status::Ok {
                        self.styles_ready = false;
                        break Err(status);
                    }
                    continue;
                }
                if status == Status::Ok
                    && phase.edit_slot(|slot| slot.issues & styles::MALFORMED != 0)?
                {
                    self.styles_ready = false;
                    break Err(Status::OutOfMemory);
                }
                if status == Status::Ok && verdict.cleared {
                    phase.edit_slot(|slot| slot.state = StyleState::Settled)?;
                }
                if status != Status::Ok {
                    break Err(status);
                }
                if reuse {
                    if pages > self.info.page_count
                        || complete
                            && (pages != self.info.page_count || chars != self.info.total_chars)
                    {
                        break Err(Status::Stale);
                    }
                    pages = self.info.page_count;
                    chars = self.info.total_chars;
                }
                if !complete && !reuse {
                    break Err(Status::Truncated);
                }
                if pages == 0 {
                    self.empty_chapter = true;
                    break Err(Status::NotFound);
                }
                if !state.found {
                    break Err(Status::NotFound);
                }
                if resolving {
                    let section = selected.navigation.target_by_toc(required);
                    if !section.target_known || !section.anchor_known {
                        break Err(Status::NotFound);
                    }
                    selected.navigation.stage_resolved_page(state.page_index);
                } else {
                    if mode == Target::TocPage {
                        let section = selected.navigation.target_by_toc(required);
                        if !section.target_known || !section.anchor_known {
                            break Err(Status::NotFound);
                        }
                    }
                    self.malformed_utf8 = state.malformed;
                    if state.malformed {
                        break Err(Status::ParseError);
                    }
                }
                break Ok((pages, chars, state));
            };
            outcome
        }); // real SDK return precedes source guard and pending publication
        let (pages, chars, state) = match outcome {
            Ok(v) => v,
            Err(e) => return self.layout_failed(resolving, e),
        };
        if resolving {
            return Status::Ok;
        }
        let status = self.validate(source);
        if status != Status::Ok {
            return self.layout_failed(resolving, status);
        }
        self.pending = Some((spine, pages, chars, state));
        Status::Ok
    }
    pub fn capture_phased<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
        &mut self,
        source: &mut S,
        spine: u32,
        target: u32,
        mode: Target,
        storage: &mut PhaseStorage<'_>,
        lender: &mut impl BuildLoan,
        backend: &mut B,
        breaker: &mut dyn newt_core::chapter_layout::types::LineBreak,
    ) -> Status {
        if self.plan.is_none() {
            return Status::Unsupported;
        }
        if matches!(mode, Target::TocResolve | Target::TocPage) {
            return self.failed(Status::Unsupported);
        }
        self.layout_phased(
            source, spine, target, mode, storage, lender, backend, breaker,
        )
    }
}

impl PhasedReader<'_, '_> {
    pub fn capture_chapter_phased<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
        &mut self,
        source: &mut S,
        spine: u32,
        toc_index: u16,
        storage: &mut PhaseStorage<'_>,
        lender: &mut impl BuildLoan,
        backend: &mut B,
        breaker: &mut dyn newt_core::chapter_layout::types::LineBreak,
    ) -> Status {
        if toc_index == NO_TOC {
            return self.capture_phased(
                source,
                spine,
                0,
                Target::Anchor,
                storage,
                lender,
                backend,
                breaker,
            );
        }
        if self.plan.is_none() {
            return Status::Unsupported;
        }
        let status = self.layout_phased(
            source,
            spine,
            u32::from(toc_index),
            Target::TocResolve,
            storage,
            lender,
            backend,
            breaker,
        );
        if status != Status::Ok {
            return status;
        }
        let result = storage.with_loan(lender, |phase| {
            let status = self.validate(source);
            if status != Status::Ok {
                return Err(status);
            }
            let selected = &self.selections[self.active ^ 1];
            let page = selected.navigation.staged_resolved_page();
            let target = selected.navigation.target_by_toc(toc_index);
            let status = phase.stream(backend, |io| {
                validate_exact_anchor_stream(
                    source,
                    &selected.entry,
                    target.fragment,
                    io,
                    self.on_yield,
                )
            })?;
            if status != Status::Ok {
                return Err(status);
            }
            Ok(page)
        });
        let page = match result {
            Ok(page) => page,
            Err(e) => return e,
        };
        let status = self.validate(source);
        if status != Status::Ok {
            return status;
        }
        self.selections[self.active ^ 1]
            .navigation
            .stage_required_toc(toc_index);
        self.layout_phased(
            source,
            spine,
            page,
            Target::TocPage,
            storage,
            lender,
            backend,
            breaker,
        )
    }
}

#[cfg(feature = "style-recapture-fixture")]
include!("dynamic_cases.rs");

impl PhasedReader<'_, '_> {
    /// Optional menu work destroys style scratch, never the retained plan.
    pub fn prepare_chapter_menu_phased<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
        &mut self, source: &mut S, focus: u32, storage: &mut PhaseStorage<'_>,
        lender: &mut impl BuildLoan, backend: &mut B,
    ) -> Status {
        if !self.opened || !self.has_page || self.plan.is_none() || self.pending.is_some() {
            return Status::Unsupported;
        }
        let (left, right) = self.selections.split_at_mut(1);
        let (selected, staged) = if self.active == 0 { (&left[0], &mut right[0]) }
            else { (&right[0], &mut left[0]) };
        let current = selected.navigation.section(self.info.char_start);
        let navigation = &mut staged.navigation;
        if navigation.reuse_menu(current, focus) { return Status::Ok; }
        navigation.invalidate_menu();
        self.styles_ready = false;
        let result = storage.with_loan(lender, |phase| {
            let status = validate(source, self.source_bytes);
            if status != Status::Ok { return Err(status); }
            let _work = WorkScope::new_scoped(self.on_yield, self.work, WorkPhase::Metadata);
            phase.metadata_counted(source, backend, false, self.on_yield, self.work,
                |book, sr, _, _, _, _| {
                    bind_menu_book(book, selected.spines, navigation,
                        sr.directory, &mut crate::phase_storage::ScopedToc, |book, navigation| {
                            // Mapping uses the dead admission-hash/path region.
                            // Denial uses the same generic projection as optional
                            // Contents parse/space failure; it cannot publish empty success.
                            crate::phase_storage::with_menu_mapping(sr.paths, book.toc.len(), |mapping| {
                                bind_menu_projection(book, navigation, current, focus, mapping)
                            }).unwrap_or(false)
                        })
                })?
        });
        let mut status = result.map_or_else(|e| e, |_| Status::Ok);
        // Return native storage before the final live-source check.
        if status == Status::Ok { status = validate(source, self.source_bytes); }
        if status != Status::Ok { navigation.invalidate_menu(); }
        status
    }
}
