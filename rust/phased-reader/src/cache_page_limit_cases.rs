//! Cache limits apply before storage admission, never to source rendering.
extern crate std;
use super::*;
use crate::{OperationPhased, PhasedChapterCache, CachePresence, hash_provider::CacheHash,
    phase_storage::{BANK_BYTES, BuildLoan, ReadScope, Storage}, producer::Segments,
    producer_probe::ProducerProbe, producer_writer::CappedWriter};
use newt_core::{cache_names::{namespace::PageCacheNeed, segment::{MAX_PAGES, PAGES}},
    chapter_layout::types::{LineBreak, Page}, page_cache::{Anchor, CacheStorage, IndexChunk, WriterMemory},
    preferences::{Orientation, ViewSettings}, producer_policy::{Cancellation, Hooks, ProducerReceipt, Stop},
    producer_visit::MissingProbe, xml_session::ProducerBackend};
use std::{boxed::Box, vec::Vec};

#[derive(Default)]
struct Files { names: Vec<Vec<u8>>, writes: usize, probes: usize }
impl CacheStorage for Files {
    fn file_size(&mut self, _: &[u8]) -> i64 { panic!("uncacheable chapter must not read files") }
    fn read_at(&mut self, _: &[u8], _: u32, _: &mut [u8]) -> i32 { panic!("unexpected cache read") }
    fn remove(&mut self, _: &[u8]) -> bool { panic!("uncacheable chapter must not remove files") }
    fn begin_write(&mut self, name: &[u8]) -> bool { self.names.push(name.to_vec()); true }
    fn write(&mut self, _: &[u8]) -> bool { self.writes += 1; true }
    fn end_write(&mut self) -> bool { true }
}
impl CachePresence for Files {
    fn with_index<R>(&mut self, _: &[u8; 32], _: impl FnOnce(&mut dyn crate::index_build::Store) -> R) -> Option<R> { None }
    fn present(&mut self, _: &[u8], _: bool) -> bool { panic!("unexpected presence probe") }
}
impl ReadScope for Files {
    fn query<R>(&mut self, _: &[u8], _: bool, _: &mut [u8], _: &mut dyn FnMut() -> bool,
        _: impl FnOnce(&mut dyn CacheStorage) -> R,
    ) -> Result<(R, bool), Status> { panic!("uncacheable chapter must not query SD") }
}
impl ProducerProbe for Files {
    fn producer_probe(&mut self, _: &[u8], _: &dyn Fn() -> bool) -> MissingProbe {
        self.probes += 1;
        MissingProbe::Ready
    }
    fn release_for_admission(&mut self) -> bool { true }
}
struct Source;
impl BookSource for Source {
    fn size(&self) -> u64 { 0 }
    fn read_at(&mut self, _: u64, _: &mut [u8]) -> i32 { 0 }
}
impl ProducerReceipt for Source { fn producer_receipt_live(&mut self) -> bool { true } }
#[repr(align(16))]
struct Bank([u8; newt_phased_sink::FRAME_BYTES]);
impl BuildLoan for Bank {
    fn with_parse<R>(&mut self, operation: impl for<'a> FnOnce(&'a mut [u8]) -> R) -> Result<R, Status> {
        Ok(operation(&mut self.0))
    }
}
struct Parser;
impl ParserBackend for Parser {
    fn create(&mut self) -> bool { panic!("uncacheable chapter must not parse") }
    fn feed(&mut self, _: &[u8], _: bool, _: &mut dyn Handler) -> bool { panic!("unexpected parse") }
    fn close(&mut self) {}
}
impl ProducerBackend for Parser {
    fn create_producer(&mut self) -> bool { panic!("uncacheable chapter must not start producer parsing") }
    fn current_byte_index(&self) -> i64 { 0 }
}
struct Breaks;
impl LineBreak for Breaks {
    fn breaks(&mut self, _: &[u8], _: Option<&core::ffi::CStr>, _: &mut [u8]) { panic!("unexpected layout") }
}
#[derive(Default)]
struct Hash(usize);
impl CacheHash for Hash {
    fn hash(&mut self, _: &[u8], out: &mut [u8; 32]) -> bool { self.0 += 1; out.fill(17); true }
}
fn settings() -> ViewSettings { ViewSettings { orientation: Orientation::Portrait, ..Default::default() } }

#[test]
fn exact_cache_page_boundary_does_not_wrap_or_expand_segments() {
    let mut selections = [Selection::default(), Selection::default()];
    let mut reader = PhasedReader::new(&mut selections, None, PageLayout::PORTRAIT);
    reader.has_page = true;
    reader.info.page_count = MAX_PAGES;
    assert_eq!(Segments::new(MAX_PAGES).count, 32);
    assert_eq!(cache_show::segment_of(&reader, 0, MAX_PAGES - 1).unwrap().index, 31);
    for pages in [MAX_PAGES + 1, 4250, 50_000, u32::MAX] {
        reader.info.page_count = pages;
        assert_eq!(Segments::new(pages).count, 0);
        assert_eq!(cache_show::segment_of(&reader, 0, MAX_PAGES), None);
    }
    let cancel = Cancellation::new(Hooks::<()> { context: &(), stop: Some(|_| Stop::None), yield_now: None, pause: None });
    let mut starts = [0; 32];
    let mut writer = CappedWriter::new(&cancel, &mut starts);
    writer.segmented = true;
    writer.open = true;
    writer.first = MAX_PAGES - 1;
    writer.seen = MAX_PAGES - 1;
    let mut chunks = [IndexChunk::default()];
    let mut anchors = [Anchor::default(); 192];
    let mut memory = WriterMemory { chunks: &mut chunks, anchors: &mut anchors };
    let mut files = Files::default();
    assert!(writer.cache.begin(&mut files, b"last-segment", 1, &memory));
    let page = |index| Page { bytes: b"", runs: &[], images: &[], links: &[], index, char_start: index };
    assert!(writer.on_page(&mut files, &mut memory, &page(MAX_PAGES - 1)));
    let writes = files.writes;
    assert!(!writer.on_page(&mut files, &mut memory, &page(MAX_PAGES)));
    assert!(writer.refused);
    assert_eq!(files.writes, writes);
    assert_eq!(files.names.len(), 1);
    assert_eq!(writer.seen, MAX_PAGES);
    assert_eq!(PAGES, 128);
}

#[test]
fn settled_oversize_chapters_are_denied_before_cache_io_or_writes() {
    for pages in [MAX_PAGES, MAX_PAGES + 1, 4250, 50_000, u32::MAX] {
        let mut selections = [Selection::default(), Selection::default()];
        let mut runs = [newt_phased_sink::Run::default(); newt_phased_sink::RUNS];
        let mut text = [0; newt_phased_sink::TEXT];
        let mut plan = Plan::new(&mut runs, &mut text);
        let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), PageLayout::PORTRAIT);
        reader.opened = true;
        reader.has_page = true;
        reader.info.page_count = pages;
        let mut shared = Box::new([0; BANK_BYTES]);
        let mut storage = Storage::new(&mut shared);
        let mut loan = Box::new(Bank([0; newt_phased_sink::FRAME_BYTES]));
        let (mut files, mut hash, mut source, mut parser, mut breaker) =
            (Files::default(), Hash::default(), Source, Parser, Breaks);
        let mut need = PageCacheNeed::default();
        need.keep.add(b"existing-cache-entry");
        let before = need;
        let mut parsed = false;
        let found = {
            let mut operation = OperationPhased::new(&mut reader, &mut storage, &mut *loan, &mut parser, &mut breaker);
            let mut cache = PhasedChapterCache { store: &mut files, hash: &mut hash, settings: settings() };
            cache.producer_stored(&mut source, &[0; 32], u32::MAX, Some(&mut need),
                &|| false, &mut parsed, &mut operation)
        };
        if pages == MAX_PAGES {
            assert_eq!(found, MissingProbe::Ready);
            assert!(files.probes > 0);
            assert_eq!(need.files, 4);
            continue;
        }
        assert_eq!(found, MissingProbe::Denied);
        assert_eq!(need, before);
        assert!(!parsed);
        assert_eq!((hash.0, files.probes), (0, 0));
        let cancel = Cancellation::new(Hooks::<()> { context: &(), stop: Some(|_| Stop::None), yield_now: None, pause: None });
        let mut route = crate::producer::Route { settings: settings(), digest: &[0; 32], info: reader.info(), hash: &mut hash };
        let result = storage.with_producer(&mut *loan, &cancel, &mut files, &mut source, pages, None, |producer| {
            producer.with_unbuffered(|phase, _| {
                phase.defer(0)?;
                phase.edit_slot(|slot| slot.state = styles::State::Settled)?;
                assert!(phase.ready(0));
                Ok(())
            })?;
            Ok(producer.produce_current(&mut reader, &mut parser, &mut breaker, &mut route))
        });
        assert_eq!(result, Ok(Status::Unsupported));
        assert!(files.names.is_empty(), "no segment file may begin");
        assert_eq!((files.writes, files.probes, hash.0), (0, 0, 0));
    }
}

#[test]
fn known_oversize_chapter_skips_all_cache_queries() {
    let mut selections = [Selection::default(), Selection::default()];
    let mut runs = [newt_phased_sink::Run::default(); newt_phased_sink::RUNS];
    let mut text = [0; newt_phased_sink::TEXT];
    let mut plan = Plan::new(&mut runs, &mut text);
    let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), PageLayout::PORTRAIT);
    reader.opened = true;
    reader.has_page = true;
    reader.info.page_count = MAX_PAGES + 1;
    let mut shared = Box::new([0; BANK_BYTES]);
    let mut storage = Storage::new(&mut shared);
    let mut loan = Box::new(Bank([0; newt_phased_sink::FRAME_BYTES]));
    let (mut files, mut hash, mut source, mut parser, mut breaker) =
        (Files::default(), Hash::default(), Source, Parser, Breaks);
    let mut operation = OperationPhased::new(&mut reader, &mut storage, &mut *loan, &mut parser, &mut breaker);
    let result = PhasedChapterCache { store: &mut files, hash: &mut hash, settings: settings() }.capture(
        &mut source, &[0; 32], CacheRequest { spine: 0, target: MAX_PAGES, anchor: false,
            staged: false, build_on_miss: false, cache_only: true }, &mut operation);
    assert_eq!(result, ShowResult { status: Status::NotFound, hit: false });
    assert_eq!((files.probes, hash.0), (0, 0));
}
