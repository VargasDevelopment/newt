// SPDX-License-Identifier: MIT
// Included in reader.rs's scope only with the style-recapture-fixture Cargo feature.
// Actual canonical metadata, style Resolve and Build; no policy substitutes.
pub fn dynamic_metadata_fixture<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    source: &mut S,
    storage: &mut PhaseStorage<'_>,
    lender: &mut impl BuildLoan,
    backend: &mut B,
    short: Option<bool>,
    after_capture: impl FnOnce(&mut S, &mut B),
    expected: Status,
) -> Option<(u32, u16, usize, usize)> {
    let mut available = 0;
    storage
        .with_loan(lender, |phase| {
            let mut selected = Selection::default();
            assert_eq!(phase.admit(source, backend, None, None)?, Status::Ok);
            let (undeclared, resolved) = phase.metadata_counted(
                source,
                backend,
                false,
                None,
                None,
                |book, sr, sources, capacity, offset, base| {
                    assert_eq!(book.package().spine_count, 80);
                    assert!(book.package().manifest_count > 64);
                    assert!(book.catalog().len() > 64);
                    for i in 0..80 {
                        let href = book
                            .spine_item(i)
                            .and_then(|m| m.href)
                            .and_then(|h| book.arena().c_string(h))
                            .unwrap();
                        let mut expected = *b"c00.xhtml";
                        expected[1] += (i / 10) as u8;
                        expected[2] += (i % 10) as u8;
                        let expected = if i < 10 {
                            expected[1] = b'c';
                            &expected[1..]
                        } else {
                            &expected[..]
                        };
                        assert_eq!(href, expected);
                    }
                    available = book.arena().capacity() - book.arena().used();
                    let expected_capacity = available.min(styles::EMBEDDED);
                    let mut backing = ObservedToc {
                        count: 0,
                        exhaust_strings: false,
                    };
                    let result = select_book_metadata(
                        book,
                        79,
                        &mut selected,
                        sr,
                        sources,
                        None,
                        false,
                        Some(capacity),
                        Some((base, offset)),
                        &mut backing,
                    );
                    assert_eq!(backing.count, 80, "actual count pass");
                    assert_eq!(
                        *capacity, expected_capacity,
                        "all TOC bytes reclaimed before Resolve"
                    );
                    if result.is_ok() {
                        let section = selected.navigation.section(0);
                        assert_eq!(section.toc, 79, "bound TOC identity above 64");
                        assert_eq!(section.spine, 79);
                        assert_eq!(section.href, b"c79.xhtml");
                        assert_eq!(section.label, b"Section 79");
                        assert!(!section.generic);
                        assert_eq!(
                            sources.events, 2,
                            "fixture must have inline then linked CSS"
                        );
                        assert_eq!(sources.external_mask, 2);
                        assert_eq!(sources.links[0].link, styles::Link::Resolved);
                        if let Some(resource) = short {
                            assert!(
                                *capacity < styles::EMBEDDED,
                                "short fixture has {available} free bytes"
                            );
                            assert_eq!(sources.issues & styles::RESOURCE != 0, resource);
                        } else {
                            assert_eq!(*capacity, styles::EMBEDDED);
                            assert_eq!(sources.issues & styles::RESOURCE, 0);
                        }
                    }
                    result
                },
            )??;
            assert!(!undeclared && resolved);
            after_capture(source, backend);
            let status = phase
                .build_styles(source, backend, &selected.entry, selected.href(), 79, None)
                .unwrap_or_else(|e| e);
            assert_eq!(status, expected);
            if status != Status::Ok {
                assert!(!phase.ready(79));
                return Ok(None);
            }
            phase.edit_slot(|slot| {
                assert_eq!(slot.issues, 0);
                assert_eq!(slot.count, 2, "both inline and external rule must survive");
                Some((slot.hash, slot.issues, slot.count, available))
            })
        })
        .unwrap()
}

// Assert reclamation at the actual backing boundary, before style resolution.
struct ObservedToc {
    count: usize,
    exhaust_strings: bool,
}
impl package::TocBacking for ObservedToc {
    fn with_toc<R>(
        &mut self,
        arena: &mut Arena<'_>,
        count: usize,
        operation: impl FnOnce(&mut Arena<'_>, &mut [TocEntry]) -> R,
    ) -> Result<R, Status> {
        self.count = count;
        let capacity = arena.capacity();
        let result = package::TocBacking::with_toc(
            &mut crate::phase_storage::ScopedToc,
            arena,
            count,
            |prefix, entries| {
                if self.exhaust_strings {
                    assert!(prefix.alloc(prefix.capacity() - prefix.used(), 1).is_some());
                }
                operation(prefix, entries)
            },
        );
        assert_eq!(
            arena.capacity(),
            capacity,
            "temporary typed tail must return"
        );
        result
    }
}

// Actual count/fill operations with host-controlled faults between passes.
pub fn toc_scope_policy_fixture<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    source: &mut S,
    backend: &mut B,
    bytes: &mut [u8],
    length: u32,
    after_count: impl FnOnce(&mut S, &mut B),
    expected: Result<usize, Status>,
) {
    let mut arena = Arena::new(bytes);
    let capacity = arena.capacity();
    let mut xml = [0; 2048];
    let entry = newt_core::member_check::Checked::raw(length);
    let mut workflow = Workflow {
                policy: None,
        source,
        backend,
        decoder: None,
        buffer: Some(&mut xml),
        filter: None,
    };
    let mark = arena.mark();
    let result = package::count_toc(&mut workflow, &entry, b"", &mut arena, TocKind::Nav).and_then(
        |count| {
            // Empty TOCs need no typed backing, even at an unaligned full end.
            if count == 0 {
                assert!(arena.alloc(capacity - arena.used(), 1).is_some());
            }
            after_count(workflow.source, workflow.backend);
            package::TocBacking::with_toc(
                &mut ObservedToc {
                    count: 0,
                    exhaust_strings: false,
                },
                &mut arena,
                count,
                |arena, toc| {
                    package::fill_toc(&mut workflow, &entry, b"", arena, TocKind::Nav, toc)
                },
            )?
        },
    );
    assert_eq!(result, expected);
    assert_eq!(arena.capacity(), capacity);
    arena.release(mark);
    assert_eq!(arena.used(), mark);
    assert!(
        arena.alloc(capacity, 1).is_some(),
        "all temporary bytes reusable"
    );
}

// Optional failed fill must discard partial declarations and retain fallback.
pub fn optional_toc_failure_fixture<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    source: &mut S,
    storage: &mut PhaseStorage<'_>,
    lender: &mut impl BuildLoan,
    backend: &mut B,
    exhaust_strings: bool,
) {
    storage
        .with_loan(lender, |phase| {
            let mut selected = Selection::default();
            phase.metadata_counted(
                source,
                backend,
                false,
                None,
                None,
                |book, sr, sources, _, _, _| {
                    let mut backing = ObservedToc {
                        count: 0,
                        exhaust_strings,
                    };
                    let result = select_book_metadata(
                        book,
                        79,
                        &mut selected,
                        sr,
                        sources,
                        None,
                        true,
                        None,
                        None,
                        &mut backing,
                    );
                    assert_eq!(result, Ok((false, false)));
                    assert_eq!(backing.count, 80);
                    let section = selected.navigation.section(0);
                    assert!(
                        section.generic,
                        "failed fill must not publish partial declarations"
                    );
                    assert_eq!(section.spine, 79);
                    assert_eq!(section.href, b"");
                    assert_eq!(section.label, b"Section 80");
                },
            )
        })
        .unwrap();
}
