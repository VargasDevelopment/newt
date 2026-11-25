//! Generated stored-ZIP fixtures and synthetic SAX events exercise the real index step.
extern crate std;
use super::*;
use std::{vec, vec::Vec, boxed::Box};
use core::cell::Cell;
use newt_core::{xml_session::{Attributes, Handler}, book_index_build::WorkFile};

struct Source { bytes: Vec<u8>, cancelled: std::rc::Rc<Cell<bool>> }
impl BookSource for Source {
    fn size(&self) -> u64 { self.bytes.len() as u64 }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        if self.cancelled.get() { return -1; }
        let at = (at as usize).min(self.bytes.len());
        let n = out.len().min(self.bytes.len()-at);
        out[..n].copy_from_slice(&self.bytes[at..at+n]);
        n as i32
    }
}
fn zip() -> Source {
    let mut bytes = Vec::new();
    let mut central = Vec::new();
    for (name, data) in [(b"META-INF/container.xml".as_slice(), b"C".as_slice()),
        (b"book.opf", b"P"), (b"c", b"reading unchanged")] {
        let offset = bytes.len() as u32;
        let mut local = [0u8; 30];
        local[..4].copy_from_slice(&0x04034b50u32.to_le_bytes());
        local[4..6].copy_from_slice(&20u16.to_le_bytes());
        local[18..22].copy_from_slice(&(data.len() as u32).to_le_bytes());
        local[22..26].copy_from_slice(&(data.len() as u32).to_le_bytes());
        local[26..28].copy_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&local); bytes.extend_from_slice(name); bytes.extend_from_slice(data);
        let mut header = [0u8; 46];
        header[..4].copy_from_slice(&0x02014b50u32.to_le_bytes());
        header[6..8].copy_from_slice(&20u16.to_le_bytes());
        header[20..24].copy_from_slice(&(data.len() as u32).to_le_bytes());
        header[24..28].copy_from_slice(&(data.len() as u32).to_le_bytes());
        header[28..30].copy_from_slice(&(name.len() as u16).to_le_bytes());
        header[42..46].copy_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(&header); central.extend_from_slice(name);
    }
    let offset = bytes.len() as u32;
    bytes.extend_from_slice(&central);
    let mut end = [0u8; 22];
    end[..4].copy_from_slice(&0x06054b50u32.to_le_bytes());
    end[8..10].copy_from_slice(&3u16.to_le_bytes()); end[10..12].copy_from_slice(&3u16.to_le_bytes());
    end[12..16].copy_from_slice(&(central.len() as u32).to_le_bytes());
    end[16..20].copy_from_slice(&offset.to_le_bytes()); bytes.extend_from_slice(&end);
    Source { bytes, cancelled: std::rc::Rc::new(Cell::new(false)) }
}
struct Attrs<'a>(&'a [(&'a [u8], &'a [u8])]);
impl Attributes for Attrs<'_> {
    fn next(&mut self) -> Option<(&[u8], &[u8])> { None }
    fn local(&self, key: &[u8]) -> Option<&[u8]> { self.0.iter().find(|(k,_)| *k==key).map(|(_,v)| *v) }
    fn exact(&self, key: &[u8]) -> Option<&[u8]> { self.local(key) }
}
struct Parser { items: usize }
impl ParserBackend for Parser {
    fn create(&mut self) -> bool { true }
    fn close(&mut self) {}
    fn feed(&mut self, bytes: &[u8], _: bool, h: &mut dyn Handler) -> bool {
        if bytes == b"C" {
            h.start_element(b"rootfile", &mut Attrs(&[(b"full-path", b"book.opf")]));
        } else if bytes == b"P" {
            h.start_element(b"manifest", &mut Attrs(&[]));
            for i in 0..self.items {
                let id = std::format!("id{i}");
                h.start_element(b"item", &mut Attrs(&[(b"id", id.as_bytes()), (b"href", b"c"),
                    (b"media-type", b"application/xhtml+xml")]));
            }
            h.end_element(b"manifest");
            h.start_element(b"spine", &mut Attrs(&[]));
            h.start_element(b"itemref", &mut Attrs(&[(b"idref", b"id0")]));
            h.end_element(b"spine");
        }
        true
    }
}
include!("index_build_cases/card.rs");
fn header(card: &mut Card) -> Option<book_index::Reader> {
    card.query(b"index.fibx", true, &mut [0; 8256], &mut || true, |store|
        book_index::Reader::open(store, &Identity { digest: [7;32], policy: admission::POLICY_VERSION }))
        .ok()?.0
}
struct Loan(Vec<u8>);
impl BuildLoan for Loan {
    fn with_parse<R>(&mut self, op: impl for<'a> FnOnce(&'a mut [u8]) -> R) -> Result<R, Status> { Ok(op(&mut self.0)) }
}
fn attempt(card: &mut Card, items: usize, source: &mut Source, hook: Yield<'_>) {
    let mut shared = Box::new([0; crate::phase_storage::BANK_BYTES]);
    let mut storage = Storage::new(&mut shared);
    let mut loan = Loan(vec![0; crate::phase_storage::BANK_BYTES+15]);
    open(source, &[7;32], &mut storage, &mut loan, &mut Parser { items }, card, hook);
}
#[test]
fn first_open_publishes_second_hits_without_census_or_writes() {
    std::println!("index state: builder={} parsed={} prepared={} core_stack={} join={} inflate={} filter={}",
        core::mem::size_of::<newt_core::book_index_build::Builder<'_>>(),
        core::mem::size_of::<newt_core::book_index_build::Parsed<'_>>(),
        core::mem::size_of::<newt_core::book_index_build::Prepared<'_>>(),
        newt_core::book_index_build::STACK_BYTES, newt_core::book_index_build::JOIN_STATE_BYTES,
        core::mem::size_of::<newt_core::inflate::State>(), core::mem::size_of::<newt_core::xml_input::EntityFilter>());
    let mut card = Card::default(); let mut source = zip();
    attempt(&mut card, 1, &mut source, None);
    let index = header(&mut card).expect("first open publishes");
    assert_eq!(index.header().members, 3); assert_eq!(index.header().spine, 1);
    let counts = (card.census, card.creates, card.writes);
    attempt(&mut card, 1, &mut source, None);
    assert_eq!((card.census, card.creates, card.writes), counts);
    assert_eq!(card.most, 3);
}
#[test]
fn stale_and_corrupt_indexes_rebuild() {
    for offset in [8, 40, 0] {
        let mut card = Card::default(); let mut source = zip();
        attempt(&mut card, 1, &mut source, None);
        card.file[offset] ^= 1;
        attempt(&mut card, 1, &mut source, None);
        assert_eq!(card.census, 2); assert_eq!(card.creates, 2);
        assert!(header(&mut card).is_some());
    }
}
#[test]
fn spilled_build_is_deferred_and_removes_work_without_publication() {
    let mut card = Card::default();
    attempt(&mut card, 1500, &mut zip(), None);
    assert!(card.removed && card.work.is_empty() && card.file.is_empty());
    assert_eq!(card.creates, 0); assert_eq!(card.most, 3);
}
#[test]
fn card_failure_leaves_no_work_or_published_index() {
    let mut card = Card { fail: true, ..Card::default() };
    attempt(&mut card, 1, &mut zip(), None);
    assert!(card.removed && card.work.is_empty() && card.file.is_empty());
}
#[test]
fn cancelled_build_removes_work_and_never_publishes() {
    struct Cancel { spilled: std::rc::Rc<Cell<bool>>, cancelled: std::rc::Rc<Cell<bool>> }
    impl admission::WorkYield for Cancel {
        fn yield_now(&self) {
            if self.spilled.get() { self.cancelled.set(true); }
        }
    }
    let mut source = zip();
    let mut card = Card::default();
    let cancel = Cancel { spilled: card.spilled_once.clone(), cancelled: source.cancelled.clone() };
    attempt(&mut card, 1500, &mut source, Some(&cancel));
    assert!(card.removed && card.work.is_empty() && card.file.is_empty());
    assert_eq!(card.most, 3, "cancel after work-file creation");
}
#[cfg(feature = "dev-usb")]
#[test]
fn trace_distinguishes_hit_build_rebuild_failure_and_deferred() {
    struct Trace(core::cell::RefCell<Vec<admission::WorkEvent>>);
    impl admission::WorkYield for Trace {
        fn yield_now(&self) {}
        fn observe(&self, event: admission::WorkEvent) { self.0.borrow_mut().push(event); }
    }
    let trace = Trace(core::cell::RefCell::new(Vec::new()));
    let mut card = Card::default(); let mut source = zip();
    attempt(&mut card, 1, &mut source, Some(&trace));
    attempt(&mut card, 1, &mut source, Some(&trace));
    card.file[0] ^= 1;
    attempt(&mut card, 1, &mut source, Some(&trace));
    card.file.clear(); card.fail = true;
    attempt(&mut card, 1, &mut source, Some(&trace));
    card.fail = false;
    attempt(&mut card, 1500, &mut source, Some(&trace));
    let events = trace.0.borrow();
    let outcomes: Vec<_> = events.iter().filter(|e| e.kind == admission::WorkKind::IndexResult).collect();
    assert_eq!(outcomes.iter().map(|e| e.result).collect::<Vec<_>>(), vec![1,0,2,3,4]);
    assert_eq!(outcomes[3].compressed_bytes, Status::IoError as u32);
    let counts = events.iter().rev().find(|e| e.kind == admission::WorkKind::IndexCounts).unwrap();
    assert_eq!((counts.compressed_bytes, counts.expanded_bytes), (3, 1));
    assert!(outcomes.iter().all(|e| e.method == 1));
    assert!(events.iter().any(|e| e.kind == admission::WorkKind::IndexStage && e.result == 6));
}
