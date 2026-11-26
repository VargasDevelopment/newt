//! Missing files keep the parser's Contents projection and failure policy.
use super::*;
use newt_core::{book_index::{self as index, Header, Identity, Member, SpineItem},
    page_cache::{CacheStorage, ReadScope}, xml_session::{Handler, ParserBackend},
    zip_window::{arena::Arena, source::BookSource}};
use newt_phased_reader::{PhasedReader, Selection, phase_storage::{BANK_BYTES, BuildLoan, Storage}};
use newt_reader_navigation::{BookView, Declaration, Item, Navigation, Section,
    index_mapping::{self, HashRow}, index_view::IndexedBook};
struct Oracle;
impl BookView for Oracle {
    fn spine_count(&self) -> usize { 2 }
    fn spine_item(&self, row: usize) -> Option<Item<'_>> {
        Some(Item { href: match row { 0 => Some(b"exists"), 1 => Some(b"missing"), _ => return None },
            media: Some(b"application/xhtml+xml") })
    }
    fn toc_count(&self) -> usize { 2 }
    fn toc_entry(&self, row: usize) -> Option<Declaration<'_>> {
        Some(Declaration { title: match row { 0 => b"Present", 1 => b"Missing", _ => return None },
            href: self.spine_item(row)?.href?, fragment: b"", depth: 0 })
    }
    fn member(&self, href: &[u8]) -> Option<newt_reader_navigation::Member> {
        (href == b"exists").then_some(newt_reader_navigation::Member { method: 0, uncompressed: 1 })
    }
}
struct Store([u8; 448]);
impl CacheStorage for Store {
    fn file_size(&mut self, _: &[u8]) -> i64 { self.0.len() as i64 }
    fn read_at(&mut self, _: &[u8], at: u32, out: &mut [u8]) -> i32 {
        let Some(bytes) = self.0.get(at as usize..at as usize + out.len()) else { return -1; };
        out.copy_from_slice(bytes); out.len() as i32
    }
    fn remove(&mut self, _: &[u8]) -> bool { false }
    fn begin_write(&mut self, _: &[u8]) -> bool { false }
    fn write(&mut self, _: &[u8]) -> bool { false }
    fn end_write(&mut self) -> bool { false }
}
impl ReadScope for Store {
    fn query<R>(&mut self, _: &[u8], _: bool, _: &mut [u8], live: &mut dyn FnMut() -> bool,
        read: impl FnOnce(&mut dyn CacheStorage) -> R) -> Result<(R, bool), Status> {
        assert!(live()); let value = read(self); Ok((value, live()))
    }
}
struct Source;
impl BookSource for Source {
    fn size(&self) -> u64 { 1 }
    fn read_at(&mut self, _: u64, out: &mut [u8]) -> i32 { assert!(out.is_empty()); 0 }
}
struct NeverParse;
impl ParserBackend for NeverParse {
    fn create(&mut self) -> bool { panic!("missing member cannot parse XML"); }
    fn close(&mut self) { panic!("missing member cannot parse XML"); }
    fn feed(&mut self, _: &[u8], _: bool, _: &mut dyn Handler) -> bool { panic!("missing member cannot parse XML"); }
}
struct Loan([u8; BANK_BYTES + 16]);
impl BuildLoan for Loan {
    fn with_parse<R>(&mut self, operation: impl for<'a> FnOnce(&'a mut [u8]) -> R) -> Result<R, Status> {
        Ok(operation(&mut self.0))
    }
}
#[test]
fn missing_file_keeps_contents_rows_not_found_and_no_chapter_notice() {
    let identity = Identity { digest: [1; 32], policy: newt_core::admission::POLICY_VERSION };
    let mut title = [0; index::TITLE_BYTES]; title[0] = b'T';
    let header = Header { identity, members: 1, spine: 2, spine_bytes: 0, nav: index::NONE,
        ncx: index::NONE, cover: index::NONE, language: Default::default(), stylesheets: false,
        navigation_declared: false, title };
    let mut store = Store([0; 448]);
    header.encode((&mut store.0[..384]).try_into().unwrap());
    Member { hash: index::name_hash(b"exists"), contents_spine: 0, uncompressed: 1, compressed: 1,
        ..Default::default() }.encode((&mut store.0[384..416]).try_into().unwrap());
    for (row, member) in [0, index::NONE].into_iter().enumerate() {
        SpineItem { member, before: 0 }.encode((&mut store.0[416 + row * 16..432 + row * 16]).try_into().unwrap());
    }
    let reader = index::Reader::open(&mut store, &identity).unwrap();
    let mut bytes = [0; 128]; let mut arena = Arena::new(&mut bytes);
    let toc = [b"exists".as_slice(), b"missing"].map(|href| newt_core::package_parsers::TocEntry {
        title: arena.copy_nul_ref(Some(if href == b"exists" { b"Present" } else { b"Missing" })),
        href: arena.copy_nul_ref(Some(href)), ..Default::default() });
    let mut mapping = [0; 2];
    let mut book = IndexedBook { header: reader.header(), arena: &arena, toc: &toc, mapping: None,
        lookup: &|_| panic!("batch must avoid per-row lookup"), progress: Default::default() };
    index_mapping::declared_spines(&book, &reader, &mut |scan| scan(&mut store), &mut mapping, &mut [HashRow::default(); 2]).unwrap();
    book.mapping = Some(&mapping);
    let (mut actual, mut expected) = (Navigation::default(), Navigation::default());
    assert!(actual.bind_menu(&book, Section::default(), 0, &mut [0; 2]));
    assert!(expected.bind_menu(&Oracle, Section::default(), 0, &mut [0; 2]));
    assert_eq!(actual.menu_info(), expected.menu_info());
    for row in 0..actual.menu_info().count { assert_eq!(actual.menu_row(row), expected.menu_row(row)); }
    let mut selections = [Selection::default(), Selection::default()];
    let mut physical = PhasedReader::new(&mut selections, None, newt_core::preferences::PageLayout::PORTRAIT);
    let mut bank = [0; BANK_BYTES]; let mut storage = Storage::new(&mut bank);
    let mut loan = Loan([0; BANK_BYTES + 16]);
    let status = storage.with_loan(&mut loan, |phase| physical.select_indexed(&mut Source,
        &mut store, &identity.digest, 1, phase, &mut NeverParse, false)).unwrap_err();
    assert_eq!(status, Status::NotFound);
    assert!(!ReaderOwnerState::from_open(true, status, 2).chapter_notice());
}
