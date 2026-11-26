// SPDX-License-Identifier: MIT
use super::*;
use crate::{BookLanguage, OperationPhased, PageInfo, PhasedChapterCache, phase_storage::ReadScope,
    producer::Segments, producer_probe::ProducerProbe, producer_store::{ProducerStore, StoreState},
    producer_writer::CappedWriter, reader::session_adapter::identity};
use core::ffi::CStr;
use newt_core::{cache_names::{namespace::PageCacheNeed, segment::{self, ChapterSegment, PAGES}},
    chapter_layout::types::LineBreak, preferences::{Orientation, ViewSettings}, producer_visit::MissingProbe};
use newt_phased_sink::{FRAME_BYTES, Plan, RUNS, Run, TEXT, Target};
use std::collections::BTreeMap;

struct Japanese(usize);
impl LineBreak for Japanese {
    fn breaks(&mut self, text: &[u8], language: Option<&CStr>, out: &mut [u8]) {
        assert_eq!(language, Some(c"ja"), "producer line-breaking language");
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
    fn with_index<R>(&mut self, _: &[u8; 32], _: impl FnOnce(&mut dyn crate::index_build::Store) -> R) -> Option<R> { None }
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
    // native.py assemble generates ja.epub for this producer case.
    let mut source = Source { bytes: include_bytes!("../../../tests/producer_forwarding/ja.epub").to_vec(), fail: false };
    let bytes = source.size();
    let mut selections = [Selection::default(), Selection::default()];
    let mut runs = [Run::default(); RUNS];
    let mut text = [0; TEXT];
    let mut plan = Plan::new(&mut runs, &mut text);
    let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), PageLayout::PORTRAIT);
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
                match producer.produce_current(&mut reader, &mut parser, &mut breaker, &mut route) {
                    newt_core::producer_visit::CurrentOutcome::Done => Status::Ok,
                    newt_core::producer_visit::CurrentOutcome::Progress => Status::IoError,
                    newt_core::producer_visit::CurrentOutcome::NoProgress(status) => status,
                }
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
    // native.py assemble generates ja-short.epub for the bounded reader.
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

#[test]
fn segment_map_requires_a_book_language() {
    let context = Context::default();
    let cancel = Cancellation::new(Hooks { context: &context, stop: Some(stopped), yield_now: None, pause: None });
    let mut files = Files::default();
    let mut store_state = StoreState::new(None);
    let mut store = ProducerStore::new(&mut files, &cancel, &mut store_state, None);
    let mut starts = [0; segment::MAX_SEGMENTS];
    let writer = CappedWriter::new(&cancel, &mut starts);
    writer.starts[1] = PAGES;
    let mut bytes = [0; segment::MAX_BYTES];
    let trace = Trace::new(0);
    let mut route = Route { settings: ViewSettings::default(), digest: &[0; 32],
        info: PageInfo { page_count: PAGES + 1, total_chars: PAGES + 1, ..PageInfo::default() },
        hash: &mut passes::PassHash(&trace) };
    let mut segments = Segments::new(route.info.page_count);
    assert_eq!(segments.write_map(&writer, &mut store, Some(&mut bytes), &mut route, 0), Status::IoError);
    segments.language = Some(BookLanguage::Japanese);
    assert_eq!(segments.write_map(&writer, &mut store, Some(&mut bytes), &mut route, 0), Status::Ok);
    assert_eq!(files.files.len(), 1);
}

#[test]
fn settle_mismatch_with_concurrent_input_is_terminal_and_incomplete_settle_stays_interruptible() {
    struct CheckpointHook {
        arm: Cell<bool>,
        breaks: Cell<usize>,
        full_breaks: usize,
        post_break_yields: Cell<u32>,
        interrupt_on_yield: Cell<u32>,
        returned_interrupted: Cell<bool>,
    }
    fn hook_stop(ctx: &CheckpointHook) -> Stop {
        if ctx.arm.get() {
            ctx.returned_interrupted.set(true);
            Stop::Interrupted
        } else {
            Stop::None
        }
    }
    fn hook_yield(ctx: &CheckpointHook) {
        if ctx.breaks.get() >= ctx.full_breaks {
            let n = ctx.post_break_yields.get() + 1;
            ctx.post_break_yields.set(n);
            if n == ctx.interrupt_on_yield.get() {
                ctx.arm.set(true);
            }
        }
    }
    struct WatchBreaker<'a> {
        count: &'a Cell<usize>,
        interrupt_on_first: Option<&'a Cell<bool>>,
    }
    impl newt_core::chapter_layout::types::LineBreak for WatchBreaker<'_> {
        fn breaks(&mut self, text: &[u8], language: Option<&core::ffi::CStr>, out: &mut [u8]) {
            let n = self.count.get() + 1;
            self.count.set(n);
            if n == 1 {
                if let Some(arm) = self.interrupt_on_first {
                    arm.set(true);
                }
            }
            let mut inner = Japanese(0);
            inner.breaks(text, language, out);
        }
    }
    /// Stops on the first cache existence probe (right after settle_current_selected
    /// returns Status::Ok) so a clean matching-totals run calibrates the exact
    /// post-layout checkpoint count independently of the mismatch path.
    struct StopAfterSettle<'a> {
        files: Files,
        arm: &'a Cell<bool>,
        reached_cache_phase: &'a Cell<bool>,
    }
    impl CachePresence for StopAfterSettle<'_> {
        fn with_index<R>(&mut self, _: &[u8; 32], _: impl FnOnce(&mut dyn crate::index_build::Store) -> R) -> Option<R> { None }
        fn present(&mut self, name: &[u8], opening: bool) -> bool { self.files.present(name, opening) }
    }
    impl CacheStorage for StopAfterSettle<'_> {
        fn exists(&mut self, name: &[u8]) -> bool {
            self.reached_cache_phase.set(true);
            self.arm.set(true);
            self.files.exists(name)
        }
        fn remove(&mut self, name: &[u8]) -> bool { self.files.remove(name) }
        fn file_size(&mut self, name: &[u8]) -> i64 { self.files.file_size(name) }
        fn read_at(&mut self, name: &[u8], offset: u32, out: &mut [u8]) -> i32 { self.files.read_at(name, offset, out) }
        fn begin_write(&mut self, name: &[u8]) -> bool { self.files.begin_write(name) }
        fn write(&mut self, bytes: &[u8]) -> bool { self.files.write(bytes) }
        fn end_write(&mut self) -> bool { self.files.end_write() }
    }

    let raw_epub = include_bytes!("../../../tests/producer_forwarding/ja.epub").to_vec();
    let mut source = Source { bytes: raw_epub.clone(), fail: false };
    let bytes = source.size();
    let mut selections = [Selection::default(), Selection::default()];
    let mut runs = [Run::default(); RUNS];
    let mut text = [0; TEXT];
    let mut plan = Plan::new(&mut runs, &mut text);
    let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), PageLayout::PORTRAIT);
    let mut shared = [0; BANK_BYTES];
    let mut native = vec![0; FRAME_BYTES];
    let context = Context::default();
    let mut loan = Loan { bytes: &mut native, signal: &context.signal, returns: 0 };
    let mut storage = Storage::new(&mut shared);
    let mut parser = Expat::default();
    let mut full_breaker = Japanese(0);
    assert_eq!(reader.open_phased(&mut source, &mut storage, &mut loan, &mut parser), Status::Ok);
    assert_eq!(reader.capture_phased(&mut source, 0, u32::MAX, Target::Anchor, &mut storage,
        &mut loan, &mut parser, &mut full_breaker), Status::Ok);
    assert_eq!(reader.publish(&mut source, &mut [0; FRAME_BYTES]), Status::Ok);
    let full_breaks = full_breaker.0;
    assert!(full_breaks > 10, "precondition: full chapter runs many line breaks");
    let real_info = reader.info();
    let settings = ViewSettings { orientation: Orientation::Portrait, ..ViewSettings::default() };
    let trace = Trace::new(0);
    let mut hash = passes::PassHash(&trace);
    let digest = [0; 32];

    // Calibrate on a clean MATCHING-totals run stopped at the very first
    // cache-phase `exists()` probe (which itself runs one checkpoint before
    // `exists`). Thus:
    //   y_settle_end = post_break_yields - 1  (producer_current.rs post-settle checkpoint)
    //   y_flush      = y_settle_end - 1       (producer_next.rs trailing empty store.flush())
    //   y_post_eof   = y_settle_end - 2       (admission.rs post-EOF finish_checked poll)
    //   y_pre_eof    = y_settle_end - 3       (admission.rs pre-EOF finish_checked poll)
    let cal_ctx = CheckpointHook {
        arm: Cell::new(false),
        breaks: Cell::new(0),
        full_breaks,
        post_break_yields: Cell::new(0),
        interrupt_on_yield: Cell::new(u32::MAX),
        returned_interrupted: Cell::new(false),
    };
    let reached_cache = Cell::new(false);
    let cal_cancel = Cancellation::new(Hooks {
        context: &cal_ctx,
        stop: Some(hook_stop),
        yield_now: Some(hook_yield),
        pause: None,
    });
    let mut fresh_storage = Storage::new(&mut shared);
    let mut cal_store = StopAfterSettle {
        files: Files::default(),
        arm: &cal_ctx.arm,
        reached_cache_phase: &reached_cache,
    };
    let mut cal_src = Source { bytes: raw_epub.clone(), fail: false };
    let mut cal_breaker = WatchBreaker { count: &cal_ctx.breaks, interrupt_on_first: None };
    let mut route = Route { settings, digest: &digest, info: real_info, hash: &mut hash };
    let _ = fresh_storage.with_producer(&mut loan, &cal_cancel, &mut cal_store, &mut cal_src,
        real_info.page_count, None, |producer| producer.with_source_guard(bytes, |producer|
            Ok(producer.produce_current(&mut reader, &mut parser, &mut cal_breaker, &mut route))));
    assert!(reached_cache.get(), "calibration must reach cache phase after clean settle");
    assert_eq!(cal_ctx.breaks.get(), full_breaks);
    let y_settle_end = cal_ctx.post_break_yields.get().checked_sub(1).expect("post-break yields");
    assert!(y_settle_end >= 4, "expected pre-EOF, post-EOF, empty-flush, and post-settle checkpoints");
    let y_flush = y_settle_end - 1;
    let y_post_eof = y_settle_end - 2;
    let y_pre_eof = y_settle_end - 3;

    let mut run_at = |wrong_totals: bool, target_yield: u32| -> (newt_core::producer_visit::CurrentOutcome, bool, bool, bool) {
        reader.info = real_info;
        if wrong_totals {
            reader.info.page_count = real_info.page_count + 1;
        }
        let ctx = CheckpointHook {
            arm: Cell::new(false),
            breaks: Cell::new(0),
            full_breaks,
            post_break_yields: Cell::new(0),
            interrupt_on_yield: Cell::new(target_yield),
            returned_interrupted: Cell::new(false),
        };
        let cancel = Cancellation::new(Hooks {
            context: &ctx,
            stop: Some(hook_stop),
            yield_now: Some(hook_yield),
            pause: None,
        });
        let mut fresh_storage = Storage::new(&mut shared);
        let mut files = Files::default();
        let mut src = Source { bytes: raw_epub.clone(), fail: false };
        let mut breaker = WatchBreaker { count: &ctx.breaks, interrupt_on_first: None };
        let mut route = Route { settings, digest: &digest, info: reader.info, hash: &mut hash };
        let outcome = fresh_storage.with_producer(&mut loan, &cancel, &mut files, &mut src,
            reader.info.page_count, None, |producer| producer.with_source_guard(bytes, |producer|
                Ok(producer.produce_current(&mut reader, &mut parser, &mut breaker, &mut route)))).unwrap();
        assert_eq!(ctx.breaks.get(), full_breaks, "layout ran through final text");
        (outcome, cancel.terminal(), cancel.interrupted(), ctx.returned_interrupted.get())
    };

    // 1. Pre-EOF checkpoint (before EOF verification): wrong totals MUST stay interruptible.
    let (out, term, intr, fired) = run_at(true, y_pre_eof);
    assert!(fired, "pre-EOF interrupt fired");
    assert!(matches!(out, newt_core::producer_visit::CurrentOutcome::NoProgress(_)));
    assert!(intr && !term, "stop before EOF verification must remain interruptible");

    // 2. Post-EOF checkpoint (`ChapterSource::finish_checked` after EOF verified):
    //    - wrong totals -> MUST be terminal (overrides concurrent input).
    //    - matching totals -> MUST stay interruptible.
    let (out, term, intr, fired) = run_at(true, y_post_eof);
    assert!(fired, "post-EOF interrupt fired on wrong totals");
    assert_eq!(out, newt_core::producer_visit::CurrentOutcome::NoProgress(Status::Stale));
    assert!(term && !intr, "wrong totals after EOF verification must be terminal even with input");

    let (out, term, intr, fired) = run_at(false, y_post_eof);
    assert!(fired, "post-EOF interrupt fired on matching totals");
    assert!(matches!(out, newt_core::producer_visit::CurrentOutcome::NoProgress(_)));
    assert!(intr && !term, "matching totals interrupted after EOF must remain interruptible");

    // 3. Empty `store.flush()` checkpoint (`producer_next.rs`):
    //    - wrong totals -> MUST be terminal.
    //    - matching totals (Deferred slot) -> MUST stay interruptible (never false denial!).
    let (out, term, intr, fired) = run_at(true, y_flush);
    assert!(fired, "empty-flush interrupt fired on wrong totals");
    assert_eq!(out, newt_core::producer_visit::CurrentOutcome::NoProgress(Status::Stale));
    assert!(term && !intr, "wrong totals at empty flush must be terminal even with input");

    let (out, term, intr, fired) = run_at(false, y_flush);
    assert!(fired, "empty-flush interrupt fired on matching totals");
    assert!(matches!(out, newt_core::producer_visit::CurrentOutcome::NoProgress(_)));
    assert!(intr && !term, "valid chapter interrupted at empty flush must stay interruptible");

    // 4. Mid-layout interruption (break #1 inside entered settle pass): MUST stay interruptible.
    reader.info = real_info;
    let mid_ctx = CheckpointHook {
        arm: Cell::new(false),
        breaks: Cell::new(0),
        full_breaks,
        post_break_yields: Cell::new(0),
        interrupt_on_yield: Cell::new(u32::MAX),
        returned_interrupted: Cell::new(false),
    };
    let mid_cancel = Cancellation::new(Hooks {
        context: &mid_ctx,
        stop: Some(hook_stop),
        yield_now: Some(hook_yield),
        pause: None,
    });
    let mut fresh_storage = Storage::new(&mut shared);
    let mut files = Files::default();
    let mut mid_src = Source { bytes: raw_epub, fail: false };
    let mut mid_breaker = WatchBreaker { count: &mid_ctx.breaks, interrupt_on_first: Some(&mid_ctx.arm) };
    let mut route = Route { settings, digest: &digest, info: reader.info, hash: &mut hash };
    let mid_out = fresh_storage.with_producer(&mut loan, &mid_cancel, &mut files, &mut mid_src,
        reader.info.page_count, None, |producer| producer.with_source_guard(bytes, |producer|
            Ok(producer.produce_current(&mut reader, &mut parser, &mut mid_breaker, &mut route)))).unwrap();
    assert!(mid_ctx.breaks.get() >= 1 && mid_ctx.breaks.get() < full_breaks,
        "settle entered layout ({}) and stopped before completion ({full_breaks})", mid_ctx.breaks.get());
    assert!(mid_ctx.returned_interrupted.get());
    assert!(matches!(mid_out, newt_core::producer_visit::CurrentOutcome::NoProgress(_)));
    assert!(mid_cancel.interrupted() && !mid_cancel.terminal());
}

#[test]
fn produce_segments_preserves_oom_status_on_begin_interrupt() {
    struct InterruptSecondBegin<'a> {
        files: Files,
        begins: u32,
        stop: &'a Cell<bool>,
    }
    impl CachePresence for InterruptSecondBegin<'_> {
        fn with_index<R>(&mut self, _: &[u8; 32], _: impl FnOnce(&mut dyn crate::index_build::Store) -> R) -> Option<R> { None }
        fn present(&mut self, name: &[u8], opening: bool) -> bool { self.files.present(name, opening) }
    }
    impl CacheStorage for InterruptSecondBegin<'_> {
        fn exists(&mut self, name: &[u8]) -> bool { self.files.exists(name) }
        fn remove(&mut self, name: &[u8]) -> bool { self.files.remove(name) }
        fn file_size(&mut self, name: &[u8]) -> i64 { self.files.file_size(name) }
        fn read_at(&mut self, name: &[u8], offset: u32, out: &mut [u8]) -> i32 { self.files.read_at(name, offset, out) }
        fn begin_write(&mut self, name: &[u8]) -> bool {
            self.begins += 1;
            if self.begins == 2 {
                // Simulate user input arriving during segment 1's begin_write
                // after segment 0 already sealed: base.begin_write succeeds,
                // then ProducerStore::begin_write's post-step checkpoint sees
                // Stop::Interrupted and returns false -> boundary() returns OutOfMemory.
                self.stop.set(true);
            }
            self.files.begin_write(name)
        }
        fn write(&mut self, bytes: &[u8]) -> bool { self.files.write(bytes) }
        fn end_write(&mut self) -> bool { self.files.end_write() }
    }

    let mut b = b"<html><body>".to_vec();
    for _ in 0..2570 { b.extend_from_slice(b"A<hr/>"); }
    b.extend_from_slice(b"</body></html>");
    let mut source = Source { bytes: b.clone(), fail: false };
    let mut selections = [Selection::default(), Selection::default()];
    selections[0].entry = newt_core::member_check::Checked::raw(b.len() as u32);
    let mut reader = PhasedReader::new(&mut selections, None, PageLayout::PORTRAIT);
    reader.source_bytes = b.len() as u64;
    reader.info.page_count = 257;
    reader.info.total_chars = 2570;
    let mut shared = [0; BANK_BYTES];
    let mut native = vec![0; FRAME_BYTES];
    let context = Context::default();
    let mut loan = Loan { bytes: &mut native, signal: &context.signal, returns: 0 };
    let mut storage = Storage::new(&mut shared);
    let mut parser = Expat::default();
    let mut breaker = linebreak::NativeBreak;
    let trace = Trace::new(0);
    let mut hash = passes::PassHash(&trace);
    let mut route = Route {
        settings: ViewSettings { orientation: Orientation::Portrait, ..ViewSettings::default() },
        digest: &[0; 32],
        info: reader.info,
        hash: &mut hash,
    };
    let cancel = Cancellation::new(Hooks { context: &context, stop: Some(stopped), yield_now: None, pause: None });
    let mut store = InterruptSecondBegin { files: Files::default(), begins: 0, stop: &context.signal.cancel };
    let status = storage.with_producer(&mut loan, &cancel, &mut store, &mut source, 257, None, |owner| {
        owner.with_buffered(|phase, mut attempt| {
            attempt.with_writer(|a| {
                phase.defer(0)?;
                a.writer.segmented = true;
                a.writer.mapping = false;
                a.state.language = Some(BookLanguage::default());
                Ok(a.produce_segments(&reader, phase, &mut parser, &mut breaker, &mut route))
            })?
        })
    }).unwrap();
    assert_eq!(store.files.files.len(), 1, "segment 0 sealed before segment 1 begin_write interrupted");
    assert!(cancel.interrupted() && !cancel.terminal());
    assert_eq!(status, Status::OutOfMemory, "legacy produce_segments wrapper must preserve exact OutOfMemory status");
}

#[test]
fn settled_write_pass_post_eof_mismatch_is_terminal_and_matching_stays_interruptible() {
    struct WriteHook {
        arm: Cell<bool>,
        breaks: Cell<usize>,
        full_breaks: usize,
        post_break_yields: Cell<u32>,
        interrupt_on_yield: Cell<u32>,
        returned_interrupted: Cell<bool>,
    }
    fn hook_stop(ctx: &WriteHook) -> Stop {
        if ctx.arm.get() {
            ctx.returned_interrupted.set(true);
            Stop::Interrupted
        } else {
            Stop::None
        }
    }
    fn hook_yield(ctx: &WriteHook) {
        if ctx.breaks.get() >= ctx.full_breaks {
            let n = ctx.post_break_yields.get() + 1;
            ctx.post_break_yields.set(n);
            if n == ctx.interrupt_on_yield.get() {
                ctx.arm.set(true);
            }
        }
    }
    struct CountBreaker<'a>(&'a Cell<usize>);
    impl newt_core::chapter_layout::types::LineBreak for CountBreaker<'_> {
        fn breaks(&mut self, text: &[u8], language: Option<&core::ffi::CStr>, out: &mut [u8]) {
            self.0.set(self.0.get() + 1);
            let mut inner = Japanese(0);
            inner.breaks(text, language, out);
        }
    }
    struct PostStreamProbeSource<'a> {
        inner: Source,
        breaks: &'a Cell<usize>,
        full_breaks: usize,
        arm: &'a Cell<bool>,
        reached_validate: &'a Cell<bool>,
    }
    impl BookSource for PostStreamProbeSource<'_> {
        fn size(&self) -> u64 { self.inner.size() }
        fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
            // In finish_current(), validate_source() issues a 0-byte probe right
            // after stream_current() returns Ok (after pre-probe checkpoint).
            if out.is_empty() && self.breaks.get() >= self.full_breaks {
                self.reached_validate.set(true);
                self.arm.set(true);
            }
            self.inner.read_at(at, out)
        }
    }
    impl newt_core::producer_policy::ProducerReceipt for PostStreamProbeSource<'_> {
        fn producer_receipt_live(&mut self) -> bool { true }
    }

    let raw_epub = include_bytes!("../../../tests/producer_forwarding/ja-short.epub").to_vec();
    let mut source = Source { bytes: raw_epub.clone(), fail: false };
    let bytes = source.size();
    let mut selections = [Selection::default(), Selection::default()];
    let mut runs = [Run::default(); RUNS];
    let mut text = [0; TEXT];
    let mut plan = Plan::new(&mut runs, &mut text);
    let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), PageLayout::PORTRAIT);
    let mut shared = [0; BANK_BYTES];
    let mut native = vec![0; FRAME_BYTES];
    let context = Context::default();
    let mut loan = Loan { bytes: &mut native, signal: &context.signal, returns: 0 };
    let mut storage = Storage::new(&mut shared);
    let mut parser = Expat::default();
    let mut full_breaker = Japanese(0);
    assert_eq!(reader.open_phased(&mut source, &mut storage, &mut loan, &mut parser), Status::Ok);
    assert_eq!(reader.capture_phased(&mut source, 0, u32::MAX, Target::Anchor, &mut storage,
        &mut loan, &mut parser, &mut full_breaker), Status::Ok);
    assert_eq!(reader.publish(&mut source, &mut [0; FRAME_BYTES]), Status::Ok);
    let real_info = reader.info();
    assert!(real_info.page_count > 0 && real_info.page_count <= PAGES, "short chapter precondition");
    let full_breaks = full_breaker.0;
    assert!(full_breaks > 0);
    let settings = ViewSettings { orientation: Orientation::Portrait, ..ViewSettings::default() };
    let trace = Trace::new(0);
    let mut hash = passes::PassHash(&trace);
    let digest = [0; 32];

    // Retain `storage` so spine 0's Settled style slot bypasses settle_current_selected
    // and enters the whole-chapter write pass directly. Calibrate on a matching run
    // stopped at finish_current()'s post-stream validate_source 0-byte probe.
    let cal_ctx = WriteHook {
        arm: Cell::new(false),
        breaks: Cell::new(0),
        full_breaks,
        post_break_yields: Cell::new(0),
        interrupt_on_yield: Cell::new(u32::MAX),
        returned_interrupted: Cell::new(false),
    };
    let reached_validate = Cell::new(false);
    let cal_cancel = Cancellation::new(Hooks {
        context: &cal_ctx,
        stop: Some(hook_stop),
        yield_now: Some(hook_yield),
        pause: None,
    });
    let mut files = Files::default();
    let mut cal_src = PostStreamProbeSource {
        inner: Source { bytes: raw_epub.clone(), fail: false },
        breaks: &cal_ctx.breaks,
        full_breaks,
        arm: &cal_ctx.arm,
        reached_validate: &reached_validate,
    };
    let mut cal_breaker = CountBreaker(&cal_ctx.breaks);
    let mut route = Route { settings, digest: &digest, info: real_info, hash: &mut hash };
    let _ = storage.with_producer(&mut loan, &cal_cancel, &mut files, &mut cal_src,
        real_info.page_count, None, |producer| producer.with_source_guard(bytes, |producer|
            Ok(producer.produce_current(&mut reader, &mut parser, &mut cal_breaker, &mut route))));
    assert!(reached_validate.get(), "calibration must reach finish_current validate_source");
    assert_eq!(cal_ctx.breaks.get(), full_breaks, "Settled slot must bypass count pass (only write pass ran)");
    // `validate_source` ran 1 checkpoint before its 0-byte probe, so `post_break_yields - 1`
    // is `ChapterSource::finish_checked`'s post-EOF checkpoint, and `- 2` is pre-EOF.
    let y_post_eof = cal_ctx.post_break_yields.get().checked_sub(1).expect("post-EOF checkpoint");
    let y_pre_eof = y_post_eof.checked_sub(1).expect("pre-EOF checkpoint");
    assert!(y_pre_eof >= 1);

    let mut run_write = |wrong_chars: bool, target_yield: u32| -> (newt_core::producer_visit::CurrentOutcome, bool, bool, bool) {
        reader.info = real_info;
        if wrong_chars {
            reader.info.total_chars = real_info.total_chars + 1;
        }
        let ctx = WriteHook {
            arm: Cell::new(false),
            breaks: Cell::new(0),
            full_breaks,
            post_break_yields: Cell::new(0),
            interrupt_on_yield: Cell::new(target_yield),
            returned_interrupted: Cell::new(false),
        };
        let cancel = Cancellation::new(Hooks {
            context: &ctx,
            stop: Some(hook_stop),
            yield_now: Some(hook_yield),
            pause: None,
        });
        let mut files = Files::default();
        let mut src = Source { bytes: raw_epub.clone(), fail: false };
        let mut breaker = CountBreaker(&ctx.breaks);
        let mut route = Route { settings, digest: &digest, info: reader.info, hash: &mut hash };
        let outcome = storage.with_producer(&mut loan, &cancel, &mut files, &mut src,
            reader.info.page_count, None, |producer| producer.with_source_guard(bytes, |producer|
                Ok(producer.produce_current(&mut reader, &mut parser, &mut breaker, &mut route)))).unwrap();
        assert_eq!(ctx.breaks.get(), full_breaks, "only settled write pass ran");
        (outcome, cancel.terminal(), cancel.interrupted(), ctx.returned_interrupted.get())
    };

    // 1. Pre-EOF checkpoint with wrong chars must remain interruptible.
    let (out, term, intr, fired) = run_write(true, y_pre_eof);
    assert!(fired, "pre-EOF interrupt fired on settled write pass");
    assert_eq!(out, newt_core::producer_visit::CurrentOutcome::NoProgress(Status::Truncated));
    assert!(intr && !term, "pre-EOF interrupt must stay interruptible");

    // 2. Post-EOF checkpoint with wrong chars must be terminal (overrides concurrent input).
    let (out, term, intr, fired) = run_write(true, y_post_eof);
    assert!(fired, "post-EOF interrupt fired on settled write pass mismatch");
    assert_eq!(out, newt_core::producer_visit::CurrentOutcome::NoProgress(Status::Unsupported));
    assert!(term && !intr, "post-EOF character mismatch on settled write pass must be terminal");

    // 3. Post-EOF checkpoint with matching chars must stay interruptible.
    let (out, term, intr, fired) = run_write(false, y_post_eof);
    assert!(fired, "post-EOF interrupt fired on settled write pass matching totals");
    assert_eq!(out, newt_core::producer_visit::CurrentOutcome::NoProgress(Status::Truncated));
    assert!(intr && !term, "post-EOF interrupt with matching totals must stay interruptible");
}
