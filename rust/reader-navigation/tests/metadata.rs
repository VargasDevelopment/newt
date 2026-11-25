use newt_core::{
    package_parsers::{ManifestItem, PackageResult, TocEntry},
    zip_container::{Entry, hash_path},
    zip_window::arena::Arena,
};
use newt_reader_navigation::{BookView, Declaration, Href, Item, Member, Navigation, metadata::Metadata};

// Original validated accessors are an independent fallback for the new helpers.
struct Validated<'a, 'storage>(&'a Metadata<'a, 'storage>);
impl BookView for Validated<'_, '_> {
    fn spine_count(&self) -> usize { self.0.spine_count() }
    fn spine_item(&self, index: usize) -> Option<Item<'_>> { self.0.spine_item(index) }
    fn toc_count(&self) -> usize { self.0.toc_count() }
    fn toc_entry(&self, index: usize) -> Option<Declaration<'_>> { self.0.toc_entry(index) }
    fn member(&self, href: &[u8]) -> Option<Member> { self.0.member(href) }
}

#[test]
fn field_helpers_preserve_missing_invalid_and_embedded_nul_values() {
    let mut memory = [0; 128];
    let mut arena = Arena::new(&mut memory);
    let good = arena.copy_nul_ref(Some(b"good\0unused")).unwrap();
    let broken_range = arena.copy_nul(Some(b"xy")).unwrap();
    let broken = arena.string_ref(broken_range).unwrap();
    arena.view_mut(broken_range).unwrap()[2] = b'z';
    let released = arena.copy_nul_ref(Some(b"released")).unwrap();
    arena.release(released.offset());
    let manifest: Vec<_> = [Some(good), Some(broken), Some(released), None].into_iter()
        .map(|href| ManifestItem { href, media_type: None, ..Default::default() }).collect();
    let toc: Vec<_> = manifest.iter().map(|item| TocEntry {
        title: item.href, href: item.href, fragment: item.href, depth: 0,
    }).collect();
    let spine = [0, 1, 2, 3, u16::MAX];
    let package = PackageResult { spine_count: spine.len(), ..Default::default() };
    let metadata = Metadata { arena: &arena, package: &package, manifest: &manifest,
        spine: &spine, toc: &toc, entries: &[], directory_offset: 0 };
    let validated = Validated(&metadata);
    for index in 0..=spine.len() {
        assert_eq!(metadata.toc_title(index), validated.toc_title(index));
        assert_eq!(metadata.toc_href(index), validated.toc_href(index));
        for query in [b"".as_slice(), b"good", b"good\0ignored", b"\0ignored", b"xyz", b"other"] {
            let query = Href::new(query);
            assert_eq!(metadata.toc_href_eq(index, query), validated.toc_href_eq(index, query));
            assert_eq!(metadata.spine_href_eq(index, query), validated.spine_href_eq(index, query));
        }
    }
    assert_eq!(metadata.toc_title(1), Some(b"".as_slice()));
    assert!(!arena.c_string_eq(broken, b"xyz"));
}

#[test]
fn actual_metadata_matches_validated_accessors_across_shapes_and_failures() {
    for (count, width) in [(16, 24), (64, 24), (128, 24), (64, 160)] {
        for malformed in [false, true] {
            let mut memory = vec![0; 256 * 1024];
            let mut arena = Arena::new(&mut memory);
            let mut manifest = Vec::new();
            let mut toc = Vec::new();
            let mut entries = Vec::new();
            for index in 0..count {
                let href = format!("OPS/{}chapter{index:03}.xhtml", "path/".repeat(width / 5));
                let href = if malformed && index == 3 { b"duplicate".as_slice() } else { href.as_bytes() };
                let title = if malformed && index == 4 { b"\xffinvalid".as_slice() } else { b"Shared title\0ignored" };
                let fragment = if index % 2 == 0 { b"anchor".as_slice() } else { b"" };
                let hr = arena.copy_nul_ref(Some(href)).unwrap();
                let tr = arena.copy_nul_ref(Some(title)).unwrap();
                let fr = arena.copy_nul_ref(Some(fragment)).unwrap();
                let media = arena.copy_nul_ref(Some(if malformed && index % 11 == 0 {
                    b"image/png".as_slice()
                } else { b"application/xhtml+xml\0ignored" })).unwrap();
                manifest.push(ManifestItem { href: Some(hr), media_type: Some(media), ..Default::default() });
                toc.push(TocEntry { title: Some(tr), href: Some(hr), fragment: Some(fr), depth: (index % 3) as u8 });
                entries.push(Entry { name: Some(hr), name_hash: hash_path(href),
                    method: if malformed && index % 13 == 0 { 99 } else { 8 },
                    uncompressed_size: 1000 + index as u32, ..Default::default() });
            }
            if malformed {
                manifest[5].href = manifest[3].href;
                toc[6].href = None;
                toc.swap(7, 10);
                manifest[0].href = None;
            }
            let spine: Vec<_> = (0..count as u16).collect();
            let package = PackageResult { spine_count: count, manifest_count: count, ..Default::default() };
            let metadata = Metadata { arena: &arena, package: &package, manifest: &manifest,
                spine: &spine, toc: &toc, entries: &entries, directory_offset: 0 };
            let validated = Validated(&metadata);
            for selected in [0, 8, 9, count as u32 - 1, count as u32] {
                let mut actual = Navigation::default();
                let mut expected = Navigation::default();
                assert_eq!(actual.bind(&metadata, selected), expected.bind(&validated, selected));
                assert_eq!(actual.issues(), expected.issues());
                assert_eq!(actual.progress_inputs(), expected.progress_inputs());
                actual.begin_anchors(); expected.begin_anchors();
                for start in [0, 10, 10, 20] {
                    actual.observe_anchor(hash_path(b"anchor"), start);
                    expected.observe_anchor(hash_path(b"anchor"), start);
                }
                assert_eq!(actual.issues(), expected.issues());
                assert_eq!(actual.target_count(), expected.target_count());
                for index in 0..=actual.target_count() { assert_eq!(actual.target(index), expected.target(index)); }
                for page in [0, 9, 10, 20, u32::MAX] {
                    assert_eq!(actual.section(page), expected.section(page));
                    let mut a = Navigation::default(); let mut e = Navigation::default();
                    let mut am = vec![0; count]; let mut em = vec![0; count];
                    assert_eq!(a.bind_menu(&metadata, actual.section(page), u32::MAX, &mut am),
                        e.bind_menu(&validated, expected.section(page), u32::MAX, &mut em));
                    assert_eq!(am, em); assert_eq!(a.menu_info(), e.menu_info());
                    for index in 0..a.menu_info().count { assert_eq!(a.menu_row(index), e.menu_row(index)); }
                }
            }
        }
    }
}
