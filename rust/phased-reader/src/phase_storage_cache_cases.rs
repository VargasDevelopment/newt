// SPDX-License-Identifier: MIT
//! Native cache queries borrow only the prefix below the actual style owner.
use super::*;
extern crate std;
use newt_core::{chapter_layout::types::{Bytes, Page, Run}, page_cache::{self, CacheStorage}};
use self::std::{boxed::Box, cell::Cell, rc::Rc, vec::Vec};

#[repr(align(16))]
struct Bank([u8; BANK_BYTES]);
struct Lender { bank: Box<Bank>, returned: Rc<Cell<bool>> }
impl BuildLoan for Lender {
    fn with_parse<R>(&mut self, work: impl for<'loan> FnOnce(&'loan mut [u8]) -> R) -> Result<R, Status> {
        self.returned.set(false);
        let result = work(&mut self.bank.0);
        self.bank.0.fill(0xaa);
        self.returned.set(true);
        Ok(result)
    }
}
struct Source { probes: usize, fail_at: usize }
impl crate::zip_window::source::BookSource for Source {
    fn size(&self) -> u64 { 37 }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        assert_eq!((at, out.len()), (0, 0), "cache checks never consume raw source bytes");
        self.probes += 1;
        if self.probes == self.fail_at { -1 } else { 0 }
    }
}
struct Store { bytes: Vec<u8>, reads: usize, sizes: usize, queries: usize, exits: usize,
    finish: bool, attach: bool, read_failure: usize }
impl Default for Store {
    fn default() -> Self { Self { bytes: Vec::new(), reads: 0, sizes: 0, queries: 0, exits: 0,
        finish: true, attach: true, read_failure: 0 } }
}
impl CacheStorage for Store {
    fn file_size(&mut self, _: &[u8]) -> i64 { self.sizes += 1; self.bytes.len() as i64 }
    fn read_at(&mut self, _: &[u8], at: u32, out: &mut [u8]) -> i32 {
        self.reads += 1;
        if self.reads == self.read_failure { return -1; }
        let Some(bytes) = self.bytes.get(at as usize..) else { return -1; };
        let count = bytes.len().min(out.len());
        out[..count].copy_from_slice(&bytes[..count]); count as i32
    }
    fn remove(&mut self, _: &[u8]) -> bool { self.bytes.clear(); true }
    fn begin_write(&mut self, _: &[u8]) -> bool { self.bytes.clear(); true }
    fn write(&mut self, bytes: &[u8]) -> bool { self.bytes.extend_from_slice(bytes); true }
    fn end_write(&mut self) -> bool { true }
}
impl ReadScope for Store {
    fn query<R>(&mut self, _: &[u8], _: bool, _: &mut [u8],
        source_live: &mut dyn FnMut() -> bool,
        read: impl FnOnce(&mut dyn CacheStorage) -> R) -> Result<(R, bool), Status> {
        self.queries += 1;
        let entered = source_live();
        if !entered || !self.attach { source_live(); self.exits += 1; return Err(Status::Stale); }
        let result = read(self);
        let fresh = source_live();
        self.exits += 1;
        Ok((result, fresh && self.finish))
    }
}
fn cache() -> Store {
    let mut store = Store::default();
    let mut chunks = [page_cache::IndexChunk::default()];
    let mut anchors = [page_cache::Anchor::default(); 192];
    let mut memory = page_cache::WriterMemory { chunks: &mut chunks, anchors: &mut anchors };
    let mut writer = page_cache::PageCacheWriter::new(page_cache::Profile::Small);
    assert!(writer.begin(&mut store, b"cache", 17, &memory));
    for index in 0..55 {
        let runs = [Run { text: Bytes { start: 0, len: 1 }, x: 3, baseline: 21, size: 18, flags: 0 }];
        let page = Page { bytes: b"x", runs: &runs, images: &[], links: &[], index, char_start: index };
        assert!(writer.on_page(&mut store, &mut memory, &page));
    }
    writer.on_anchor(&mut memory, 99, 4);
    writer.set_total_chars(55);
    assert!(writer.finish(&mut store, b"cache", &memory));
    store
}
fn hold_styles(storage: &mut Storage<'_>) -> u32 {
    let (_, bytes) = top::<Home>(storage.shared).unwrap();
    let home = init_home(bytes);
    home.with_slot(|slot| {
        slot.count = 1;
        slot.rules[0].elem_hash = 0x12345678;
        slot.spine = 7;
        slot.state = styles::State::Settled;
    });
    let seal = home.seal;
    storage.held = true;
    seal
}
fn query(storage: &mut Storage<'_>, lender: &mut Lender, store: &mut Store,
    source: &mut Source, published: &Cell<bool>) -> Result<(u32, u32), Status> {
    let returned = lender.returned.clone();
    storage.cache_page(lender, store, b"cache", false, 17,
        crate::CacheRequest { spine: 7, target: 1, anchor: false, staged: false,
            build_on_miss: false, cache_only: false }, None, None, source, 37,
        &crate::reader::cache_show::Miss::default(), |_, page, pages, chars| {
            assert!(returned.get(), "native parse must return before cached publication");
            assert_eq!(page.runs[0].text.get(page.bytes), b"x");
            published.set(true); (pages, chars)
        })
}
fn run(mut store: Store, fail_at: usize, expected: Result<(u32, u32), Status>) -> (usize, usize) {
    let mut bank = Box::new(Bank([0; BANK_BYTES]));
    let mut storage = Storage::new(&mut bank.0);
    let held_seal = hold_styles(&mut storage);
    let mut lender = Lender { bank: Box::new(Bank([0; BANK_BYTES])), returned: Rc::new(Cell::new(false)) };
    let mut source = Source { probes: 0, fail_at };
    let published = Cell::new(false);
    assert_eq!(query(&mut storage, &mut lender, &mut store, &mut source, &published), expected);
    assert_eq!(published.get(), expected.is_ok());
    assert!(storage.styles_ready(7), "cache outcomes must not discard retained styles");
    let (_, bytes) = top::<Home>(storage.shared).unwrap();
    let home = initialized::<Home>(bytes);
    assert_eq!((home.seal, home.rules[0].elem_hash), (held_seal, 0x12345678));
    assert_eq!((store.queries, store.exits), (1, 1));
    (source.probes, store.reads)
}
#[test]
fn verified_query_checks_source_only_at_entry_and_exit() {
    // These are 60 logical SDK cache reads, not 60 native block reads.
    assert_eq!(run(cache(), 0, Ok((55, 55))), (2, 60));
}
#[test]
fn absent_cache_preserves_actual_style_rules_and_seal() {
    assert_eq!(run(Store::default(), 0, Err(Status::NotFound)), (2, 0));
}
#[test]
fn source_entry_failure_never_opens_or_publishes_cache() {
    assert_eq!(run(cache(), 1, Err(Status::Stale)), (2, 0));
}
#[test]
fn source_exit_failure_rejects_a_decoded_page_after_scope_close() {
    assert_eq!(run(cache(), 2, Err(Status::Stale)), (2, 60));
}
#[test]
fn cache_read_failure_still_closes_and_checks_source() {
    let mut store = cache(); store.read_failure = 4;
    assert_eq!(run(store, 0, Err(Status::IoError)), (2, 4));
}
#[test]
fn failed_attach_still_checks_source_and_preserves_styles() {
    let mut store = cache(); store.attach = false;
    assert_eq!(run(store, 0, Err(Status::Stale)), (2, 0));
}
#[test]
fn failed_native_checked_exit_withholds_decoded_page() {
    let mut store = cache(); store.finish = false;
    assert_eq!(run(store, 0, Err(Status::Stale)), (2, 60));
}
