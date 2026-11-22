use newt_core::library_catalog::*;
use newt_core::library_name::valid_library_book_name;

#[test]
fn retained_names_and_openable_names_have_distinct_authority() {
    for name in [
        b":Newt:Books:a.epub\0".as_slice(),
        b":Newt:Secrets:a.epub\0",
        b":A::a.epub\0",
        b":A:.hidden:a.epub\0",
    ] {
        assert!(valid_library_book_name(name));
        assert!(!listable(name));
    }
    for name in [
        b"a.epub\0".as_slice(),
        b":A:a.epub\0",
        b":nEwT:bOoKs:sub:a.epub\0",
        b"a:b.epub\0",
    ] {
        assert!(listable(name));
    }
}
#[test]
fn lifecycle_and_notice_priority() {
    let mut c = LibraryCatalog::default();
    c.add_plain(b"b.epub\0", 0);
    assert_eq!(c.size(), 0);
    assert!(c.filename(0).is_none());
    c.omit(LibraryOmission::Folder);
    assert_eq!(c.notice(), Some("Some folders are too deep or unreadable."));
    c.add_plain(b".bad.epub\0", 0);
    assert_eq!(c.notice(), Some("Some books have names Newt can't show."));
    c.omit(LibraryOmission::Stopped);
    assert_eq!(c.notice(), Some("The card has too many files to list all."));
    c.finish();
    c.finish();
    assert_eq!(c.filename(0), Some(b"b.epub\0".as_slice()));
    assert_eq!(c.add_plain(b"a.epub\0", 0), LibraryStatus::Unavailable);
    assert_eq!(c.size(), 0);
    assert_eq!(*c.omissions(), LibraryOmissions::default());
    assert_eq!(c.notice(), None);
    c.reset(LibraryStatus::Ok);
    c.finish();
    assert_eq!(c.size(), 0);
}
#[test]
fn ascii_filename_whole_name_and_raw_byte_tie_order() {
    let mut c = LibraryCatalog::default();
    for name in [
        b":Z:A.epub\0".as_slice(),
        b"a.epub\0",
        b"A.epub\0",
        b":a:A.epub\0",
        b"z.epub\0",
        "é.epub\0".as_bytes(),
    ] {
        c.add_plain(name, 0);
    }
    c.finish();
    let actual: Vec<_> = (0..c.size()).map(|i| c.filename(i).unwrap()).collect();
    assert_eq!(
        actual,
        vec![
            b":a:A.epub\0".as_slice(),
            b":Z:A.epub\0",
            b"A.epub\0",
            b"a.epub\0",
            b"z.epub\0",
            "é.epub\0".as_bytes()
        ]
    );
}
#[test]
fn all_slots_and_replacement_preserve_size_mapping_before_sort() {
    let mut c = LibraryCatalog::default();
    let mut p = LibraryCopies::<fn(&[u8], &[u8]) -> bool>::default();
    for i in 0..BOOK_CAPACITY {
        let name = format!(":s:{i:03}.epub\0");
        c.add(name.as_bytes(), (i + 1) as u32, Some(&mut p));
    }
    c.add(b"zz.epub\0", 987, Some(&mut p));
    assert_eq!(p.bytes[127], 987);
    assert_eq!(c.omissions().full, 1);
    c.add(b":s:zzz.epub\0", 99, Some(&mut p));
    assert_eq!(c.omissions().full, 2);
    assert_eq!(p.bytes[127], 987);
    c.finish();
    assert_eq!(c.size(), 128);
    assert_eq!(c.filename(127), Some(b"zz.epub\0".as_slice()));
    // finish sorts names, never the scanner-owned map (no adds allowed afterward).
    assert_eq!(p.bytes[127], 987);
}
#[test]
fn failed_proofs_continue_in_slot_order_and_zero_size_skips_proof() {
    let mut c = LibraryCatalog::default();
    let mut calls = Vec::new();
    let mut p = LibraryCopies {
        bytes: [0; BOOK_CAPACITY],
        same: Some(|kept: &[u8], copy: &[u8]| {
            calls.push((kept.to_vec(), copy.to_vec()));
            kept == b":B:b.epub"
        }),
    };
    c.add(b":A:a.epub\0", 7, Some(&mut p));
    c.add(b":B:b.epub\0", 7, Some(&mut p));
    c.add(b"c.epub\0", 7, Some(&mut p));
    c.add(b"d.epub\0", 0, Some(&mut p));
    assert_eq!(c.copies(), 1);
    assert_eq!(p.bytes[1], 7);
    assert_eq!(
        calls.iter().map(|(k, _)| k.as_slice()).collect::<Vec<_>>(),
        vec![b":A:a.epub".as_slice(), b":A:a.epub", b":B:b.epub"]
    );
    c.finish();
    assert_eq!(c.size(), 3);
    assert_eq!(c.filename(1), Some(b"c.epub\0".as_slice()));
}
#[test]
fn identical_names_are_not_deduplicated_without_content_proof() {
    let mut c = LibraryCatalog::default();
    c.add_plain(b"a.epub\0", 3);
    c.add_plain(b"a.epub\0", 3);
    c.finish();
    assert_eq!(c.size(), 2);
    assert_eq!(c.copies(), 0);
}
#[test]
fn omission_and_copy_counts_saturate() {
    let mut c = LibraryCatalog::default();
    for _ in 0..65540 {
        c.omit(LibraryOmission::Name);
        c.omit(LibraryOmission::Folder);
    }
    assert_eq!(c.omissions().names, u16::MAX);
    assert_eq!(c.omissions().folders, u16::MAX);
    let mut p = LibraryCopies {
        bytes: [0; BOOK_CAPACITY],
        same: Some(|_: &[u8], _: &[u8]| true),
    };
    for _ in 0..300 {
        c.add(b"a.epub\0", 3, Some(&mut p));
    }
    assert_eq!(c.copies(), 255);
    c.finish();
    assert_eq!(c.size(), 1);
}
#[test]
fn truncation_preserves_unconverted_colons_and_untouched_tail() {
    let mut out = [0xa5; 40];
    assert!(!library_book_path(
        Some(b"/sd/Newt/Books\0"),
        Some(b":A:a.epub\0"),
        Some(&mut out[..8])
    ));
    assert_eq!(&out[..8], b"/sd:A:a\0");
    assert!(out[8..].iter().all(|&b| b == 0xa5));
    assert!(library_book_path(
        Some(b"/sd/Newt/Books\0"),
        Some(b":A:a.epub\0"),
        Some(&mut out)
    ));
    assert_eq!(&out[..13], b"/sd/A/a.epub\0");
    let before = out;
    assert!(!library_book_path(
        Some(b"/sd/newt/books\0"),
        Some(b":A:a.epub\0"),
        Some(&mut out)
    ));
    assert_eq!(out, before);
    assert!(!library_name_file(
        None,
        Some(b"a.epub\0"),
        Some(b".x\0"),
        Some(&mut out)
    ));
    assert_eq!(out, before);
}

#[test]
fn full_omissions_saturate_and_keep_notice_priority() {
    let mut c = LibraryCatalog::default();
    for i in 0..BOOK_CAPACITY {
        c.add_plain(format!("{i:03}.epub\0").as_bytes(), 0);
    }
    for _ in 0..65540 {
        c.add_plain(b"zzzz.epub\0", 0);
    }
    assert_eq!(c.omissions().full, u16::MAX);
    c.omit(LibraryOmission::Name);
    c.omit(LibraryOmission::Folder);
    assert_eq!(c.notice(), Some("Only the first 128 books are shown."));
    c.omit(LibraryOmission::Stopped);
    assert_eq!(c.notice(), Some("The card has too many files to list all."));
    c.finish();
    assert_eq!(c.size(), BOOK_CAPACITY);
}
