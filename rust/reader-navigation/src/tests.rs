use super::*;
use std::{vec, vec::Vec};

struct Book<'a> {
    spines: Vec<Option<Item<'a>>>,
    toc: Vec<Declaration<'a>>,
    members: Vec<(&'a [u8], Member)>,
}
impl BookView for Book<'_> {
    fn spine_count(&self) -> usize {
        self.spines.len()
    }
    fn spine_item(&self, index: usize) -> Option<Item<'_>> {
        self.spines.get(index).copied().flatten()
    }
    fn toc_count(&self) -> usize {
        self.toc.len()
    }
    fn toc_entry(&self, index: usize) -> Option<Declaration<'_>> {
        self.toc.get(index).copied()
    }
    fn member(&self, href: &[u8]) -> Option<Member> {
        self.members
            .iter()
            .find(|(h, _)| c_bytes(h) == c_bytes(href))
            .map(|(_, m)| *m)
    }
}
fn chapter(href: &[u8]) -> Item<'_> {
    Item {
        href: Some(href),
        media: Some(b"application/xhtml+xml"),
    }
}
fn declaration<'a>(
    title: &'a [u8],
    href: &'a [u8],
    fragment: &'a [u8],
    depth: u8,
) -> Declaration<'a> {
    Declaration {
        title,
        href,
        fragment,
        depth,
    }
}
fn fixture<'a>() -> Book<'a> {
    Book {
        spines: vec![
            Some(chapter(b"front")),
            Some(chapter(b"one")),
            Some(chapter(b"two")),
        ],
        toc: vec![
            declaration(b"Part", b"one", b"", 0),
            declaration(b"First", b"one", b"a", 1),
            declaration(b"Next", b"one", b"b", 1),
        ],
        members: vec![
            (
                b"front",
                Member {
                    method: 0,
                    uncompressed: 10,
                },
            ),
            (
                b"one",
                Member {
                    method: 8,
                    uncompressed: 30,
                },
            ),
            (
                b"two",
                Member {
                    method: 0,
                    uncompressed: 60,
                },
            ),
        ],
    }
}
#[test]
fn labels_write_only_the_snprintf_extent_and_copy_clears_on_failure() {
    let mut out = [0x55; 24];
    assert!(generic_label(4, &mut out));
    assert_eq!(&out[..10], b"Section 5\0");
    assert_eq!(out[10], 0x55);
    assert!(!generic_label(4, &mut out[..4]));
    assert_eq!(&out[..4], b"Sec\0");
    assert!(!copy_label(Some(b"too long"), &mut out[..2]));
    assert_eq!(out[0], 0);
    assert!(!copy_label(None, &mut out));
    assert!(copy_label(Some(b"A\0ignored"), &mut out));
    assert_eq!(&out[..2], b"A\0");
}
#[test]
fn progress_counts_each_occurrence_but_ambiguous_href_does_not_map() {
    let mut book = fixture();
    book.spines.push(Some(chapter(b"one")));
    let mut nav = Navigation::default();
    assert!(nav.bind(&book, 1));
    assert_eq!(
        nav.progress_inputs(),
        ProgressInputs {
            before: 10,
            current: 30,
            total: 130,
            index: 1,
            count: 4,
            valid: true
        }
    );
    assert_eq!(declared_spine(&book, 0), NO_SPINE);
    assert_eq!(nav.target_count(), 0);
    assert_eq!(nav.issues(), UNMAPPED_TARGET);
    assert_eq!(nav.section(0).label, b"Section 2");
}
#[test]
fn weights_skip_resources_but_missing_text_disables_every_weight() {
    let mut book = fixture();
    book.spines.push(Some(Item {
        href: Some(b"art"),
        media: Some(b"image/png"),
    }));
    let mut nav = Navigation::default();
    assert!(nav.bind(&book, 2));
    assert_eq!(nav.progress_inputs().total, 100);
    book.members.pop();
    assert!(nav.bind(&book, 1));
    let progress = nav.progress_inputs();
    assert!(!progress.valid);
    assert_eq!(
        (progress.before, progress.current, progress.total),
        (0, 0, 0)
    );
    assert!(nav.issues() & INVALID_WEIGHTS != 0);
}
#[test]
fn estimate_clamps_character_and_checks_empty_and_overflow_domains() {
    let p = Estimate {
        before: 10,
        current: 30,
        total: 100,
        spine: 1,
        chapter_chars: 10,
    };
    assert_eq!(p.percent(0), Some(10));
    assert_eq!(p.percent(5), Some(25));
    assert_eq!(p.percent(u32::MAX), Some(40));
    assert_eq!(
        Estimate {
            current: u64::MAX,
            ..p
        }
        .percent(1),
        None
    );
    assert_eq!(
        Estimate {
            chapter_chars: 0,
            ..p
        }
        .percent(1),
        None
    );
    assert_eq!(
        ProgressInputs {
            before: 10,
            current: 30,
            total: 100,
            index: 1,
            count: 3,
            valid: true
        }
        .estimate(0, 10),
        Estimate::default()
    );
}
#[test]
fn unresolved_anchor_blocks_guessing_and_repeated_observations_are_idempotent() {
    let book = fixture();
    let mut nav = Navigation::default();
    assert!(nav.bind(&book, 1));
    assert_eq!(nav.section(0).label, b"Section 2");
    assert_eq!(nav.issues(), UNRESOLVED_ANCHOR);
    nav.observe_anchor(hash_path(b"a"), 0);
    nav.observe_anchor(hash_path(b"b"), 10);
    assert_eq!(nav.section(0).label, b"First");
    assert_eq!(nav.section(9).label, b"First");
    assert_eq!(nav.section(10).label, b"Next");
    assert_eq!(nav.issues(), 0);
    nav.observe_anchor(hash_path(b"b"), 10);
    assert_eq!(nav.issues(), 0);
    nav.observe_anchor(hash_path(b"b"), 11);
    assert_eq!(nav.section(99).label, b"Section 2");
    assert_eq!(nav.issues(), AMBIGUOUS_ANCHOR);
    nav.begin_anchors();
    assert_eq!(nav.issues(), UNRESOLVED_ANCHOR);
    nav.observe_anchor(hash_path(b"a"), 0);
    nav.observe_anchor(hash_path(b"b"), 10);
    assert_eq!(nav.section(10).label, b"Next");
}
#[test]
fn inherited_label_survives_but_unordered_declarations_disable_it() {
    let mut book = fixture();
    let mut nav = Navigation::default();
    assert!(nav.bind(&book, 2));
    let section = nav.section(0);
    assert_eq!(section.label, b"Next");
    assert!(section.inherited && section.target_known && !section.anchor_known);
    assert_eq!(section.spine, 1);
    book.toc.push(declaration(b"Earlier", b"front", b"", 0));
    assert!(nav.bind(&book, 2));
    assert_eq!(nav.issues(), UNORDERED);
    assert_eq!(nav.section(0).label, b"Section 3");
}
#[test]
fn conflicting_full_declarations_remain_ambiguous_after_anchor_reset() {
    let mut book = fixture();
    book.toc.push(declaration(b"Conflict", b"one", b"b", 1));
    let mut nav = Navigation::default();
    assert!(nav.bind(&book, 1));
    nav.begin_anchors();
    nav.observe_anchor(hash_path(b"a"), 0);
    nav.observe_anchor(hash_path(b"b"), 20);
    assert!(nav.issues() & AMBIGUOUS_ANCHOR != 0);
    assert_eq!(nav.section(0).label, b"Section 2");
    nav.begin_anchors();
    assert!(nav.issues() & AMBIGUOUS_ANCHOR != 0);
}
#[test]
fn same_position_and_depth_with_different_titles_cannot_publish_identity() {
    let book = fixture();
    let mut nav = Navigation::default();
    assert!(nav.bind(&book, 1));
    nav.observe_anchor(hash_path(b"a"), 0);
    nav.observe_anchor(hash_path(b"b"), 0);
    assert!(nav.issues() & AMBIGUOUS_ANCHOR != 0);
    assert!(nav.section(100).generic);
}
#[test]
fn required_toc_is_reserved_in_authored_order_when_records_overflow() {
    let mut book = fixture();
    book.toc = (0..13)
        .map(|_| declaration(b"same", b"one", b"a", 0))
        .collect();
    let mut nav = Navigation::default();
    nav.stage_required_toc(12);
    assert_eq!(nav.staged_required_toc(), 12);
    assert!(nav.bind(&book, 1));
    assert_eq!(nav.target_count(), 12);
    assert_eq!(nav.staged_required_toc(), NO_TOC);
    assert_eq!(nav.target(11).toc, 12);
    assert_eq!(nav.target(10).toc, 10);
    assert!(nav.issues() & RECORD_OVERFLOW != 0);
    assert_eq!(nav.section(0).label, b"Section 2");
}
#[test]
fn string_overflow_retains_only_complete_owned_targets() {
    let labels: Vec<Vec<u8>> = (0..8).map(|i| vec![b'A' + i; 90]).collect();
    let mut book = fixture();
    book.toc = labels
        .iter()
        .map(|s| declaration(s, b"one", b"", 0))
        .collect();
    let mut nav = Navigation::default();
    assert!(nav.bind(&book, 1));
    assert!(nav.target_count() < 8);
    assert!(nav.issues() & STRING_OVERFLOW != 0);
    assert!(nav.section(99).generic);
    for i in 0..nav.target_count() {
        assert_eq!(nav.target(i).label.len(), 90);
    }
}
#[test]
fn title_projection_keeps_complete_utf8_prefix_and_marks_short_invalid_suffix() {
    let title = [b'x'; 100];
    let mut book = fixture();
    book.toc = vec![declaration(&title, b"one", b"", 0)];
    let mut nav = Navigation::default();
    assert!(nav.bind(&book, 1));
    assert_eq!(nav.target(0).label.len(), 95);
    assert_eq!(nav.issues(), TITLE_TRUNCATED);
    book.toc = vec![declaration(b"a\xe7\x81", b"one", b"", 0)];
    assert!(nav.bind(&book, 1));
    assert_eq!(nav.section(0).label, b"a");
    assert_eq!(nav.issues(), TITLE_TRUNCATED);
    book.toc = vec![declaration(b"\xe7\x81", b"one", b"", 0)];
    assert!(nav.bind(&book, 1));
    assert_eq!(nav.target_count(), 0);
    assert_eq!(nav.section(0).label, b"Section 2");
}
#[test]
fn copied_projection_owns_its_strings_and_invalid_selection_resets_it() {
    let mut nav = Navigation::default();
    {
        let book = fixture();
        assert!(nav.bind(&book, 2));
    }
    let copy = nav.clone();
    nav.reset();
    assert_eq!(copy.section(0).label, b"Next");
    assert!(!nav.bind(&fixture(), 3));
    assert_eq!(nav.issues(), INVALID_SPINE);
    assert_eq!(nav.target_count(), 0);
    assert_eq!(nav.progress_inputs(), ProgressInputs::default());
    nav.stage_resolved_page(17);
    assert_eq!(nav.staged_resolved_page(), 17);
    assert_eq!(nav.issues(), 0);
    assert_eq!(nav.progress_inputs(), ProgressInputs::default());
}
#[test]
fn chapters_rows_focus_and_reuse_match_owned_footer_identity() {
    let book = fixture();
    let mut footer = Navigation::default();
    assert!(footer.bind(&book, 1));
    footer.observe_anchor(hash_path(b"a"), 0);
    footer.observe_anchor(hash_path(b"b"), 10);
    let current = footer.section(10);
    let mut nav = Navigation::default();
    let mut mapping = [NO_SPINE; 3];
    assert!(nav.bind_menu(&book, current, FOCUS_CURRENT, &mut mapping));
    let info = nav.menu_info();
    assert_eq!(
        (info.count, info.focus, info.first, info.visible),
        (4, 3, 0, 4)
    );
    assert!(info.authored && info.current_exact && info.valid);
    assert_eq!(nav.menu_row(0).label, b"Section 1");
    assert_eq!(nav.menu_row(3).label, b"Next");
    assert!(nav.menu_row(3).current);
    assert!(!nav.menu_row(4).valid);
    assert!(nav.reuse_menu(current, 0));
    assert_eq!(nav.menu_info().focus, 0);
    assert!(nav.reuse_menu(current, FOCUS_CURRENT));
    assert_eq!(nav.menu_info().focus, 3);
    assert_eq!(nav.section(10), Section::default());
    assert_eq!(nav.target_by_toc(2), Section::default());
    nav.invalidate_menu();
    assert!(!nav.menu_info().valid);
}
#[test]
fn generic_current_row_is_inserted_and_bad_mapping_rejects_cleanly() {
    let book = fixture();
    let current = Section {
        spine: 2,
        ..Section::default()
    };
    let mut nav = Navigation::default();
    assert!(!nav.bind_menu(&book, current, 0, &mut []));
    assert!(!nav.menu_info().valid);
    assert!(nav.bind_menu(&book, current, FOCUS_CURRENT, &mut [0; 3]));
    assert_eq!(nav.menu_info().count, 5);
    assert_eq!(nav.menu_info().focus, 4);
    assert_eq!(nav.menu_row(4).label, b"Section 3");
    assert!(nav.menu_row(4).current && nav.menu_row(4).generic);
}
#[test]
fn builtin_identity_uses_depth_and_rejects_unresolved_or_tied_distinct_titles() {
    fn make(label: &[u8], key: u16, depth: u8) -> Identity<'_> {
        Identity {
            label,
            projection_spine: 1,
            char_start: 10,
            title_key: key,
            depth,
            inherited: false,
            target_known: true,
            anchor_known: true,
        }
    }
    let mut out = [0; 48];
    let mut entries = [
        make(b"Part".as_slice(), 1, 0),
        make(b"Deep".as_slice(), 2, 1),
    ];
    assert!(resolve_identity(&entries, 1, 10, &mut out));
    assert_eq!(c_bytes(&out), b"Deep");
    entries[1].depth = 0;
    assert!(resolve_identity(&entries, 1, 10, &mut out));
    assert_eq!(c_bytes(&out), b"Section 2");
    entries[1].anchor_known = false;
    assert!(resolve_identity(&entries, 1, 0, &mut out));
    assert_eq!(c_bytes(&out), b"Section 2");
}

struct MappingIndex {
    bytes: Vec<u8>,
    reads: Vec<u32>,
    fail: bool,
}
impl newt_core::page_cache::CacheStorage for MappingIndex {
    fn remove(&mut self, _: &[u8]) -> bool { false }
    fn file_size(&mut self, _: &[u8]) -> i64 { self.bytes.len() as i64 }
    fn read_at(&mut self, _: &[u8], offset: u32, dst: &mut [u8]) -> i32 {
        self.reads.push(offset);
        if self.fail { return -1; }
        let Some(bytes) = self.bytes.get(offset as usize..offset as usize + dst.len())
            else { return -1; };
        dst.copy_from_slice(bytes);
        dst.len() as i32
    }
    fn begin_write(&mut self, _: &[u8]) -> bool { false }
    fn write(&mut self, _: &[u8]) -> bool { false }
    fn end_write(&mut self) -> bool { false }
}
fn mapping_index(book: &Book<'_>) -> (MappingIndex, newt_core::book_index::Reader) {
    use newt_core::book_index::{self as idx, Header, Identity};
    let identity = Identity { digest: [1; 32], policy: 1 };
    let mut title = [0; idx::TITLE_BYTES];
    title[..4].copy_from_slice(b"Test");
    let header = Header { identity, members: book.members.len() as u32,
        spine: book.spine_count() as u32, spine_bytes: 0, nav: idx::NONE,
        ncx: idx::NONE, cover: idx::NONE, language: Default::default(), title };
    let mut bytes = vec![0; header.payload_bytes().unwrap() as usize];
    header.encode((&mut bytes[..idx::HEADER_BYTES as usize]).try_into().unwrap());
    let mut members: Vec<_> = book.members.iter().map(|(href, member)| idx::Member {
        hash: idx::name_hash(href), contents_spine: readable_spine(book, href),
        method: member.method, uncompressed: member.uncompressed, ..Default::default()
    }).collect();
    members.sort_unstable_by_key(|member| member.hash);
    for (i, member) in members.iter().enumerate() {
        let at = header.member_offset(i as u32).unwrap() as usize;
        member.encode((&mut bytes[at..at + idx::MEMBER_BYTES as usize]).try_into().unwrap());
    }
    let mut storage = MappingIndex { bytes, reads: Vec::new(), fail: false };
    let index = idx::Reader::open(&mut storage, &identity).unwrap();
    storage.reads.clear();
    (storage, index)
}

#[test]
fn batch_contents_matches_oracle_and_reads_each_member_at_most_once() {
    use crate::index_mapping::{self, HashRow, MappingRoute};
    let mut book = fixture();
    book.spines.push(Some(chapter(b"one"))); // Repeated spine is ambiguous.
    book.spines.push(Some(Item { href: Some(b"front"), media: Some(b"image/png") }));
    book.members[2].1.method = 99;
    book.toc.extend([
        declaration(b"Missing", b"missing", b"", 0),
        declaration(b"Unsupported", b"two", b"", 0),
        declaration(b"Image", b"front", b"", 0),
        declaration(b"", b"front", b"", 0),
    ]);
    let (mut storage, index) = mapping_index(&book);
    let mut expected = vec![NO_SPINE; book.toc_count()];
    book.declared_spines(&mut expected);
    let mut actual = vec![0; book.toc_count()];
    let mut scratch = vec![HashRow::default(); actual.len()];
    assert_eq!(index_mapping::declared_spines(&book, &index, &mut storage,
        &mut actual, &mut scratch), Ok(MappingRoute::Batch));
    assert_eq!(actual, expected);
    assert!(storage.reads.len() <= book.members.len());
    assert!(storage.reads.windows(2).all(|pair| pair[1] == pair[0] + 32));
}

#[test]
fn batch_contents_preserves_duplicate_declarations_and_nul_terminated_hrefs() {
    use crate::index_mapping::{self, HashRow};
    let mut book = fixture();
    book.toc.push(declaration(b"Nul", b"one\0ignored", b"", 0));
    let (mut storage, index) = mapping_index(&book);
    let mut mapping = vec![0; book.toc_count()];
    let mut scratch = vec![HashRow::default(); mapping.len()];
    index_mapping::declared_spines(&book, &index, &mut storage, &mut mapping, &mut scratch).unwrap();
    assert_eq!(mapping, vec![1; book.toc_count()]);
}

#[test]
fn short_batch_scratch_uses_the_per_row_rule_without_index_reads() {
    use crate::index_mapping::{self, HashRow, MappingRoute};
    let book = fixture();
    let (mut storage, index) = mapping_index(&book);
    let mut expected = vec![0; book.toc_count()];
    book.declared_spines(&mut expected);
    let mut mapping = vec![0; book.toc_count()];
    let mut scratch = vec![HashRow::default(); mapping.len() - 1];
    storage.fail = true;
    assert_eq!(index_mapping::declared_spines(&book, &index, &mut storage,
        &mut mapping, &mut scratch), Ok(MappingRoute::PerRow));
    assert_eq!(mapping, expected);
    assert!(storage.reads.is_empty());
    assert_eq!(core::mem::size_of::<HashRow>(), 12);
}

#[test]
fn batch_contents_reports_storage_failure_and_short_mapping() {
    use crate::index_mapping::{self, HashRow};
    let book = fixture();
    let (mut storage, index) = mapping_index(&book);
    let mut mapping = vec![0; book.toc_count()];
    let mut scratch = vec![HashRow::default(); mapping.len()];
    assert!(index_mapping::declared_spines(&book, &index, &mut storage,
        &mut [], &mut scratch).is_err());
    storage.fail = true;
    assert!(index_mapping::declared_spines(&book, &index, &mut storage,
        &mut mapping, &mut scratch).is_err());
}

#[test]
fn contents_menu_uses_the_batch_hook() {
    struct BatchBook(Book<'static>);
    impl BookView for BatchBook {
        fn spine_count(&self) -> usize { self.0.spine_count() }
        fn spine_item(&self, _: usize) -> Option<Item<'_>> { panic!("per-row lookup"); }
        fn toc_count(&self) -> usize { self.0.toc_count() }
        fn toc_entry(&self, row: usize) -> Option<Declaration<'_>> { self.0.toc_entry(row) }
        fn member(&self, _: &[u8]) -> Option<Member> { panic!("per-row lookup"); }
        fn declared_spines(&self, mapping: &mut [u32]) { mapping.fill(1); }
    }
    let book = BatchBook(fixture());
    let mut navigation = Navigation::default();
    let mut mapping = vec![NO_SPINE; book.toc_count()];
    assert!(navigation.bind_menu(&book, Section::default(), 0, &mut mapping));
    assert_eq!(mapping, vec![1; book.toc_count()]);
    assert!(navigation.menu_info().authored);
}

#[test]
fn indexed_view_matches_parser_navigation_with_batch_and_per_row_mapping() {
    use crate::{index_mapping::{self, HashRow}, index_view::IndexedBook};
    use newt_core::{package_parsers::TocEntry, zip_window::arena::Arena};
    let oracle = fixture();
    let (mut storage, index) = mapping_index(&oracle);
    let mut bytes = [0; 512];
    let mut arena = Arena::new(&mut bytes);
    let toc: Vec<_> = oracle.toc.iter().map(|entry| TocEntry {
        title: arena.copy_nul_ref(Some(entry.title)), href: arena.copy_nul_ref(Some(entry.href)),
        fragment: arena.copy_nul_ref(Some(entry.fragment)), depth: entry.depth,
    }).collect();
    let lookup = |href: &[u8]| readable_spine(&oracle, href);
    let mut book = IndexedBook { header: index.header(), arena: &arena, toc: &toc,
        mapping: None, lookup: &lookup, progress: oracle.progress_inputs(1) };
    let mut mapping = vec![0; toc.len()];
    index_mapping::declared_spines(&book, &index, &mut storage, &mut mapping,
        &mut vec![HashRow::default(); toc.len()]).unwrap();
    let mut expected = Navigation::default();
    for batch in [false, true] {
        assert!(expected.bind(&oracle, 1));
        book.mapping = batch.then_some(&mapping);
        let mut actual = Navigation::default();
        assert!(actual.bind(&book, 1));
        assert_eq!(actual.progress_inputs(), expected.progress_inputs());
        assert_eq!(actual.section(0), expected.section(0));
        assert_eq!(actual.issues(), expected.issues());
        let mut actual_mapping = vec![0; toc.len()];
        let mut expected_mapping = vec![0; toc.len()];
        assert_eq!(actual.bind_menu(&book, Section::default(), 0, &mut actual_mapping),
            expected.bind_menu(&oracle, Section::default(), 0, &mut expected_mapping));
        assert_eq!(actual.menu_info(), expected.menu_info());
        for row in 0..actual.menu_info().count { assert_eq!(actual.menu_row(row), expected.menu_row(row)); }
    }
}
