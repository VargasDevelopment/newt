// SPDX-License-Identifier: MIT
use super::*;
use crate::{BookLanguage, OperationPhased, PhasedChapterCache, phase_storage::ReadScope, producer_probe::ProducerProbe};
use super::super::super::identity;
use core::ffi::CStr;
use newt_core::{cache_names::{namespace::PageCacheNeed, segment::{self, ChapterSegment, PAGES}},
    chapter_layout::types::LineBreak, preferences::{Orientation, ViewSettings}, producer_visit::MissingProbe};
use newt_phased_sink::{FRAME_BYTES, Plan, RUNS, Run, TEXT, Target};
use std::collections::BTreeMap;

struct Japanese(usize);
impl LineBreak for Japanese {
    fn breaks(&mut self, text: &[u8], language: Option<&CStr>, out: &mut [u8]) {
        assert_eq!(language, Some(c"ja"), "producer dropped the OPF language");
        linebreak::NativeBreak.breaks(text, language, out);
        self.0 += 1;
    }
}

#[derive(Default)]
struct Files {
    files: BTreeMap<Vec<u8>, Vec<u8>>,
    writing: Option<(Vec<u8>, Vec<u8>)>,
}
impl CacheStorage for Files {
    fn exists(&mut self, name: &[u8]) -> bool { self.files.contains_key(name) }
    fn file_size(&mut self, _: &[u8]) -> i64 { -1 }
    fn read_at(&mut self, _: &[u8], _: u32, _: &mut [u8]) -> i32 { -1 }
    fn remove(&mut self, _: &[u8]) -> bool { self.writing = None; true }
    fn begin_write(&mut self, name: &[u8]) -> bool {
        self.writing = Some((name.to_vec(), Vec::new()));
        true
    }
    fn write(&mut self, bytes: &[u8]) -> bool {
        let Some((_, file)) = self.writing.as_mut() else { return false; };
        file.extend_from_slice(bytes);
        true
    }
    fn end_write(&mut self) -> bool {
        let Some((name, file)) = self.writing.take() else { return false; };
        self.files.insert(name, file);
        true
    }
}

impl CachePresence for Files {
    fn present(&mut self, name: &[u8], _: bool) -> bool { self.files.contains_key(name) }
}
impl ProducerProbe for Files {
    fn producer_probe(&mut self, name: &[u8], cancelled: &dyn Fn() -> bool) -> MissingProbe {
        if cancelled() { MissingProbe::Stopped }
        else if self.files.contains_key(name) { MissingProbe::Complete }
        else { MissingProbe::Ready }
    }
    fn release_for_admission(&mut self) -> bool { true }
}
impl ReadScope for Files {
    fn query<R>(&mut self, _: &[u8], _: bool, _: &mut [u8], _: &mut dyn FnMut() -> bool,
        _: impl FnOnce(&mut dyn CacheStorage) -> R,
    ) -> Result<(R, bool), Status> {
        unreachable!("a current segmented chapter probes names without reading a map")
    }
}

#[test]
fn current_and_staged_segments_use_the_book_language() {
    let mut source = Source { bytes: include_bytes!("../../../tests/producer_forwarding/ja.epub").to_vec(), fail: false };
    let bytes = source.size();
    let mut selections = [Selection::default(), Selection::default()];
    let mut runs = [Run::default(); RUNS];
    let mut text = [0; TEXT];
    let mut plan = Plan::new(&mut runs, &mut text);
    let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), false);
    let mut shared = [0; BANK_BYTES];
    let mut native = vec![0; FRAME_BYTES];
    let context = Context::default();
    let mut loan = Loan { bytes: &mut native, signal: &context.signal, returns: 0 };
    let mut storage = Storage::new(&mut shared);
    let mut parser = Expat::default();
    let mut breaker = Japanese(0);
    assert_eq!(reader.book_language(), None);
    assert_eq!(reader.open_phased(&mut source, &mut storage, &mut loan, &mut parser), Status::Ok);
    assert_eq!(reader.book_language(), Some(BookLanguage::Japanese));
    assert_eq!(reader.capture_phased(&mut source, 0, u32::MAX, Target::Anchor, &mut storage,
        &mut loan, &mut parser, &mut breaker), Status::Ok);
    assert_eq!(reader.publish(&mut source, &mut [0; FRAME_BYTES]), Status::Ok);
    let info = reader.info();
    assert!(info.page_count > PAGES, "fixture needs multiple cache segments");
    let settings = ViewSettings { orientation: Orientation::Portrait, ..ViewSettings::default() };
    let trace = Trace::new(0);
    let mut hash = passes::PassHash(&trace);
    let digest = [0; 32];
    let mut route = Route { settings, digest: &digest, info, hash: &mut hash };
    let mut files = Files::default();
    let cancel = Cancellation::new(Hooks { context: &context, stop: Some(stopped), yield_now: None, pause: None });

    for spine in 0..=1 {
        breaker.0 = 0;
        let status = storage.with_producer(&mut loan, &cancel, &mut files, &mut source, info.page_count,
            None, |producer| producer.with_source_guard(bytes, |producer| Ok(if spine == 0 {
                producer.produce_current(&mut reader, &mut parser, &mut breaker, &mut route)
            } else {
                producer.produce_next(&mut reader, spine, &mut parser, &mut breaker, &mut route, None)
            })));
        assert_eq!(status, Ok(Status::Ok));
        assert!(breaker.0 > 0);
        let (base, generation) = identity(settings, &digest, spine, route.hash, BookLanguage::Japanese)
            .unwrap_or_else(|error| unreachable!("fixture hash failed: {error:?}"));
        let mut name = [0; 128];
        name[..69].copy_from_slice(&base);
        assert!(segment::map_name(&mut name, &mut |bytes, out| route.hash.hash(bytes, out)));
        assert!(files.files.contains_key(&name[..69]), "Japanese segment map for spine {spine}");
        name[..69].copy_from_slice(&base);
        let first = ChapterSegment { index: 0, pages: info.page_count, chars: info.total_chars };
        assert!(segment::segment_name(first, &mut name, &mut |bytes, out| route.hash.hash(bytes, out)));
        let file = &files.files[&name[..69]];
        assert_eq!(&file[8..12], &generation.to_le_bytes());
        assert_eq!(reader.info(), info, "staged producer leaves the visible page alone");
        if spine == 0 {
            let mut need = PageCacheNeed::default();
            let mut parsed = false;
            let mut operation = OperationPhased::new(&mut reader, &mut storage, &mut loan, &mut parser, &mut breaker);
            let mut cache = PhasedChapterCache { store: &mut files, hash: route.hash, settings };
            assert_eq!(cache.producer_stored(&mut source, &digest, spine, Some(&mut need), &|| false,
                &mut parsed, &mut operation), MissingProbe::Complete, "Japanese producer admission");
            assert_eq!(need.files, 0);
            assert_eq!(cache.store.files.len(), ChapterSegment::count(info.page_count) as usize + 1);
            for name in cache.store.files.keys() {
                assert!(need.keep.may_contain(name), "protect the current Japanese segments and map");
            }
        }
    }
}

#[test]
fn bounded_layout_uses_the_book_language() {
    let epub: &[u8] = include_bytes!("../../../tests/producer_forwarding/ja-short.epub");
    use crate::{bounded::{BOOK_BYTES, Chapters, LAYOUT_BYTES, PARSE_BYTES},
        phase_storage::{bounded::MetadataState, bounded_chapters::Work}};
    struct Discard;
    impl newt_core::chapter_layout::types::Sink for Discard {
        fn page(&mut self, _: newt_core::chapter_layout::types::Page<'_>) -> bool { true }
    }

    let mut book = [0; BOOK_BYTES];
    let mut parse = [0; PARSE_BYTES];
    let mut layout = [0; LAYOUT_BYTES];
    let mut metadata = MetadataState::default();
    let mut parser = newt_target_expat::Expat::default();
    let mut source = Source { bytes: epub.to_vec(), fail: false };
    assert_eq!(metadata.load(&mut book, &mut parse, &mut source, &mut parser), Status::Ok);
    let mut breaks = Japanese(0);
    let mut chapters = Work::new(epub, &mut layout, &mut parse, &mut parser, &mut breaks);
    // SAFETY: the metadata and its original initialized book bank remain local
    // to the fixture, and only the bounded layout operation borrows them.
    let result = unsafe { metadata.with_metadata(&mut book, |view|
        chapters.layout(view, 0, ViewSettings::default(), &mut Discard)) };
    assert!(matches!(result, Ok(Ok(_))));
    assert!(breaks.0 > 0, "bounded reader must break text");
}
