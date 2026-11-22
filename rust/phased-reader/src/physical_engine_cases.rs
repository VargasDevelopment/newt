// SPDX-License-Identifier: MIT
//! Native operation fixtures. Caller supplies approved SHA/linebreak
//! ports and generated EPUB directory. Host loan return is not device evidence.
extern crate std;
use super::*;
use crate::phase_storage::{BANK_BYTES, BuildLoan, ReadScope, Storage, with_writer_index};
use crate::imported_revision::{Digest, RevisionHasher, session};
use newt_core::{chapter_layout::types::LineBreak, preferences::ViewSettings};
use newt_phased_sink::{RUNS, TEXT, Run as PlanRun};
use newt_target_expat::Expat;
use std::{path::Path, vec, vec::Vec, sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst}};

#[repr(align(16))]
struct Bank([u8; BANK_BYTES]);
#[derive(Default)]
struct Signals {
    revoked: AtomicBool,
    live: AtomicBool,
    parser_live: AtomicBool,
    final_feed: AtomicBool,
    returned: AtomicUsize,
    final_guard: AtomicBool,
    feeds: AtomicUsize,
    feeds_at_failure: AtomicUsize,
    saw_write_failure: AtomicBool,
}
struct Source<'a> { bytes: Vec<u8>, signals: &'a Signals }
impl BookSource for Source<'_> {
    fn size(&self) -> u64 { self.bytes.len() as u64 }
    fn read_at(&mut self, offset: u64, out: &mut [u8]) -> i32 {
        if self.signals.revoked.load(SeqCst) { return -1; }
        if out.is_empty() {
            if !self.signals.live.load(SeqCst) { self.signals.final_guard.store(true, SeqCst); }
            return 0;
        }
        let at = (offset as usize).min(self.bytes.len());
        let n = out.len().min(self.bytes.len() - at);
        out[..n].copy_from_slice(&self.bytes[at..at+n]);
        n as i32
    }
}
struct HostLoan<'a> { bank: &'a mut Bank, signals: &'a Signals, revoke: bool, fail: bool }
impl BuildLoan for HostLoan<'_> {
    fn with_parse<R>(&mut self, operation: impl for<'a> FnOnce(&'a mut [u8]) -> R) -> Result<R, Status> {
        assert!(!self.signals.live.swap(true, SeqCst));
        self.signals.final_guard.store(false, SeqCst);
        let result = operation(&mut self.bank.0);
        assert!(!self.signals.parser_live.load(SeqCst), "SAX destroyed before native return");
        self.bank.0.fill(0xff);
        self.signals.live.store(false, SeqCst);
        self.signals.returned.fetch_add(1, SeqCst);
        if self.revoke { self.signals.revoked.store(true, SeqCst); }
        if self.fail { Err(Status::Stale) } else { Ok(result) }
    }
}
struct Parser<'a> { xml: Expat, signals: &'a Signals }
impl ParserBackend for Parser<'_> {
    fn create(&mut self) -> bool {
        assert!(self.signals.live.load(SeqCst));
        self.signals.final_feed.store(false, SeqCst);
        let ok = self.xml.create();
        if ok { assert!(!self.signals.parser_live.swap(true, SeqCst)); }
        ok
    }
    fn feed(&mut self, bytes: &[u8], last: bool, handler: &mut dyn xml_session::Handler) -> bool {
        self.signals.feeds.fetch_add(1, SeqCst);
        if last { self.signals.final_feed.store(true, SeqCst); }
        self.xml.feed(bytes, last, handler)
    }
    fn close(&mut self) {
        self.xml.close();
        self.signals.parser_live.store(false, SeqCst);
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault { None, Page, Header, End, StoreRevokes, ReturnRevokes, ReturnFails, BeforeBegin, Published, Pending }
struct Store<'a> {
    file: Vec<u8>, name: Vec<u8>, signals: &'a Signals, fault: Fault,
    begins: usize, writes: usize, ends: usize, removes: usize, footers: usize,
    page_after_final_feed: bool,
}
impl CacheStorage for Store<'_> {
    fn file_size(&mut self, _: &[u8]) -> i64 { self.file.len() as i64 }
    fn read_at(&mut self, _: &[u8], at: u32, out: &mut [u8]) -> i32 {
        let at = (at as usize).min(self.file.len());
        let n = out.len().min(self.file.len() - at);
        out[..n].copy_from_slice(&self.file[at..at+n]); n as i32
    }
    fn begin_write(&mut self, name: &[u8]) -> bool {
        self.begins += 1; self.writes = 0; self.file.clear();
        self.name = name.to_vec(); true
    }
    fn write(&mut self, bytes: &[u8]) -> bool {
        self.writes += 1;
        if self.writes == 2 { self.page_after_final_feed = self.signals.final_feed.load(SeqCst); }
        if bytes.len() == 24 && bytes.ends_with(b"FIBX") {
            assert!(!self.signals.live.load(SeqCst), "footer after actual loan return");
            assert!(self.signals.final_guard.load(SeqCst), "guard after actual return");
            self.footers += 1;
        }
        if self.fault == Fault::StoreRevokes && self.writes == 2 {
            self.signals.revoked.store(true, SeqCst);
        }
        if (self.fault == Fault::Page && self.writes == 2) || self.fault == Fault::Header {
            self.signals.feeds_at_failure.store(self.signals.feeds.load(SeqCst), SeqCst);
            self.signals.saw_write_failure.store(true, SeqCst);
            return false;
        }
        self.file.extend_from_slice(bytes); true
    }
    fn end_write(&mut self) -> bool { self.ends += 1; self.fault != Fault::End }
    fn remove(&mut self, _: &[u8]) -> bool { self.removes += 1; self.file.clear(); true }
}
impl ReadScope for Store<'_> {
    fn query<R>(
        &mut self,
        _: &[u8],
        _: bool,
        _: &mut [u8],
        source_live: &mut dyn FnMut() -> bool,
        read: impl FnOnce(&mut dyn newt_core::page_cache::CacheStorage) -> R,
    ) -> Result<(R, bool), Status> {
        let entered = source_live();
        if !entered { source_live(); return Err(Status::Stale); }
        let result = read(self);
        Ok((result, source_live()))
    }
}
impl CachePresence for Store<'_> {
    fn present(&mut self, _: &[u8], _: bool) -> bool { !self.file.is_empty() }
}
struct Outcome {
    result: session::Build, file: Vec<u8>, name: Vec<u8>,
    begins: usize, ends: usize, removes: usize, footers: usize,
    feeds_after_failure: usize, final_feed: bool, page_after_final_feed: bool,
}
fn run(root: &Path, name: &str, fault: Fault, hash: &mut dyn crate::hash_provider::CacheHash, breaker: &mut dyn LineBreak) -> Outcome {
    let signals = Signals::default();
    let bytes = std::fs::read(root.join(std::format!("{name}.epub"))).unwrap();
    let mut digest: Digest = [0; 32];
    assert!(hash.hash(&bytes, &mut digest));
    let mut source = Source { bytes, signals: &signals };
    let mut shared = Bank([0; BANK_BYTES]);
    let mut native = Bank([0; BANK_BYTES]);
    let mut storage = Storage::new(&mut shared.0);
    let mut loan = HostLoan { bank: &mut native, signals: &signals, revoke: false, fail: false };
    let mut parser = Parser { xml: Expat::default(), signals: &signals };
    let mut selections = [Selection::default(), Selection::default()];
    let mut runs = [PlanRun::default(); RUNS];
    let mut text = [0; TEXT];
    let mut plan = Plan::new(&mut runs, &mut text);
    let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), true);
    assert_eq!(reader.open_phased(&mut source, &mut storage, &mut loan, &mut parser), Status::Ok);
    let original_active = reader.active;
    let original_info = reader.info;
    let original_entry = reader.selection().entry;
    reader.proofs.plan = true; // invalidation witness, except rejected operations
    if fault == Fault::Published { reader.has_page = true; }
    if fault == Fault::Pending { reader.pending = Some((0, 1, 6, Default::default())); }
    let original_pending = reader.pending;
    let original_has_page = reader.has_page;
    if fault == Fault::BeforeBegin { signals.revoked.store(true, SeqCst); }
    loan.revoke = fault == Fault::ReturnRevokes;
    loan.fail = fault == Fault::ReturnFails;
    signals.final_feed.store(false, SeqCst);
    let mut store = Store {
        file: vec![0x77], name: Vec::new(), signals: &signals, fault,
        begins: 0, writes: 0, ends: 0, removes: 0, footers: 0, page_after_final_feed: false,
    };
    let result = {
        let mut operation = phase_session::OperationPhased::new(&mut reader, &mut storage, &mut loan, &mut parser, breaker);
        let mut cache = phase_session::PhasedChapterCache { store: &mut store, hash, settings: ViewSettings::default() };
        session::Cache::build(&mut cache, &mut source, &digest, 0, &mut operation)
    };
    assert_eq!(reader.active, original_active);
    assert_eq!(reader.info, original_info);
    assert_eq!(reader.selection().entry, original_entry);
    assert_eq!(reader.pending, original_pending);
    assert_eq!(reader.has_page, original_has_page);
    assert!(!reader.proofs.chapter, "no chapter proof from a build/prefix");
    assert_eq!(reader.proofs.plan, matches!(fault, Fault::Published | Fault::Pending));
    if !matches!(fault, Fault::Published | Fault::Pending) {
        assert_eq!(reader.plan.as_deref().unwrap().count(), 0);
        assert_eq!(reader.plan.as_deref().unwrap().bytes(), 0);
    }
    assert!(!signals.live.load(SeqCst) && !signals.parser_live.load(SeqCst));
    let feeds_after_failure = if signals.saw_write_failure.load(SeqCst) {
        signals.feeds.load(SeqCst) - signals.feeds_at_failure.load(SeqCst)
    } else { 0 };
    Outcome { result, file: store.file, name: store.name, begins: store.begins,
        ends: store.ends, removes: store.removes, footers: store.footers,
        feeds_after_failure, final_feed: signals.final_feed.load(SeqCst),
        page_after_final_feed: store.page_after_final_feed }
}
fn u32le(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes(bytes[at..at+4].try_into().unwrap()) }
fn sealed(out: &Outcome, chars: u32, minimum_pages: u32) {
    assert_eq!(out.result, session::Build::Ready);
    assert_eq!(out.name.len(), 69);
    assert!(out.name[..64].iter().all(u8::is_ascii_hexdigit));
    assert_eq!(&out.name[64..], b".fibp");
    assert_eq!(&out.file[..8], b"FIBP\x03\0\0\0");
    // Handwritten SDK wire boundaries, independent of PageCacheReader/Writer.
    assert_eq!(u32le(&out.file, 8), 1267189643); // canonical landscape generation
    let f = out.file.len() - 24;
    assert_eq!(&out.file[f+20..], b"FIBX");
    let pages = u32le(&out.file, f+4);
    assert!(pages >= minimum_pages);
    assert_eq!(u32le(&out.file, f+16), chars);
    let index = u32le(&out.file, f) as usize;
    let anchors = u32le(&out.file, f+8) as usize;
    let count = u32le(&out.file, f+12) as usize;
    assert_eq!(anchors, index + pages as usize * 8);
    assert_eq!(f, anchors + count * 8);
    assert_eq!(u32le(&out.file, index), 12); // first page starts after header
    assert_eq!(u32le(&out.file, index+4), 0);
    assert_eq!(out.ends, 1);
    assert_eq!(out.footers, 1);
    assert_eq!(out.removes, 0);
}
/// Use a fresh caller-owned receipt and independent real streaming/identity hashes.
/// The token is created by actual admission, promotion and verified stage_owned.
pub fn actual_staged_cache_case(
    root: &Path,
    receipt: &mut dyn crate::imported_revision::RevisionStorage,
    revision_hash: &mut dyn RevisionHasher,
    identity_hash: &mut dyn crate::hash_provider::CacheHash,
    breaker: &mut dyn LineBreak,
) {
    let signals = Signals::default();
    let mut source = Source { bytes: std::fs::read(root.join("staged.epub")).unwrap(), signals: &signals };
    let mut scratch = [0; crate::imported_revision::BLOCK_BYTES];
    let mut revision = crate::imported_revision::ImportedRevision::full(receipt, revision_hash, &mut scratch);
    assert!(revision.begin_import(&mut source));
    let digest = *revision.digest();
    let mut shared = Bank([0; BANK_BYTES]);
    let mut native = Bank([0; BANK_BYTES]);
    let mut storage = Storage::new(&mut shared.0);
    let mut loan = HostLoan { bank: &mut native, signals: &signals, revoke: false, fail: false };
    let mut parser = Parser { xml: Expat::default(), signals: &signals };
    let mut selections = [Selection::default(), Selection::default()];
    let mut runs = [PlanRun::default(); RUNS];
    let mut text = [0; TEXT];
    let mut plan = Plan::new(&mut runs, &mut text);
    let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), true);
    let mut store = Store { file: vec![0x77], name: Vec::new(), signals: &signals,
        fault: Fault::None, begins: 0, writes: 0, ends: 0, removes: 0,
        footers: 0, page_after_final_feed: false };
    let mut operation = phase_session::OperationPhased::new(&mut reader, &mut storage, &mut loan, &mut parser, breaker);
    let mut cache = phase_session::PhasedChapterCache { store: &mut store, hash: identity_hash, settings: ViewSettings::default() };
    assert_eq!(session::Session { revision: &mut revision, reader: &mut operation, cache: &mut cache }.finish_prepared_open(), Status::Ok);
    assert_eq!(session::Session { revision: &mut revision, reader: &mut operation, cache: &mut cache }.stage_owned(0), Status::Ok);
    let staged = operation.reader.staged_selection().entry;
    assert_eq!(operation.reader.staged_selection().href(), b"OPS/a.xhtml");
    assert_eq!(session::Cache::build(&mut cache, &mut revision, &digest, 1, &mut operation), session::Build::Failed(Status::Unsupported));
    assert_eq!((cache.store.begins, cache.store.removes, cache.store.footers), (0, 0, 0));
    assert_eq!(cache.store.file, [0x77]);
    assert_eq!(operation.reader.staged_selection().entry, staged);
    session::Reader::commit(&mut operation, 0);
    assert_eq!(operation.reader.info().spine, 0);
    assert_eq!(operation.reader.selection().entry, staged);
    assert_eq!(operation.reader.selection().href(), b"OPS/a.xhtml");
}
/// Call from the existing approved native test driver; no toy parser/hash/breaker.
/// The exported function does not silently skip missing fixture/native support.
pub fn actual_physical_engine_cases(root: &Path, hash: &mut dyn crate::hash_provider::CacheHash, breaker: &mut dyn LineBreak) {
    index_loan_uses_original_plan_and_restores_no_metadata(breaker);
    let short = run(root, "short", Fault::None, hash, breaker);
    sealed(&short, 6, 1);
    assert_eq!(u32le(&short.file, short.file.len()-20), 1);
    assert!(short.page_after_final_feed, "the final page must use finish's source loan");
    sealed(&run(root, "unicode", Fault::None, hash, breaker), 8, 1);
    // Collapsed trailing whitespace removes the last space, hence 139999.
    sealed(&run(root, "long", Fault::None, hash, breaker), 139999, 2);
    for name in ["empty", "suppressed"] {
        let out = run(root, name, Fault::None, hash, breaker);
        assert_eq!(out.result, session::Build::Empty);
        assert!(out.final_feed && out.file.is_empty());
        assert_eq!((out.footers, out.ends, out.removes), (0, 0, 1));
    }
    for (name, chars) in [("deferred", 9), ("pending", 7)] {
        let out = run(root, name, Fault::None, hash, breaker);
        sealed(&out, chars, 1);
        assert!(out.begins >= 2, "settlement must restart/truncate writer, not seal prefix");
    }
    let revoked_retry = run(root, "deferred", Fault::ReturnRevokes, hash, breaker);
    // Frozen C++ retry-return observation: begin clears attempted before the
    // guard fails, so this uncommitted prefix is not removed. No Ready/Empty.
    assert_eq!(revoked_retry.result, session::Build::Failed(Status::OutOfMemory));
    assert_eq!((revoked_retry.begins, revoked_retry.removes, revoked_retry.footers), (1, 0, 0));
    assert_eq!(revoked_retry.file.len(), 12);
    assert_eq!(&revoked_retry.file[..4], b"FIBP");
    let failed = run(root, "long", Fault::Page, hash, breaker);
    assert_eq!(failed.result, session::Build::Failed(Status::OutOfMemory));
    assert!(failed.feeds_after_failure > 0 && failed.final_feed);
    assert!(failed.file.is_empty() && failed.footers == 0);
    let capacity = run(root, "capacity", Fault::None, hash, breaker);
    assert_eq!(capacity.result, session::Build::Failed(Status::OutOfMemory));
    assert!(capacity.final_feed && capacity.file.is_empty() && capacity.footers == 0);
    let malformed = run(root, "malformed", Fault::Page, hash, breaker);
    assert_eq!(malformed.result, session::Build::Failed(Status::ParseError));
    assert!(malformed.feeds_after_failure > 0 && malformed.file.is_empty());
    for (fault, status) in [(Fault::Header, Status::OutOfMemory), (Fault::End, Status::IoError),
        (Fault::StoreRevokes, Status::IoError), (Fault::ReturnRevokes, Status::IoError),
        (Fault::ReturnFails, Status::Stale)] {
        let out = run(root, "short", fault, hash, breaker);
        assert_eq!(out.result, session::Build::Failed(status));
        assert!(out.file.is_empty() && out.removes == 1);
        if fault != Fault::End { assert_eq!(out.footers, 0); }
    }
    for name in ["empty", "suppressed"] {
        let out = run(root, name, Fault::ReturnRevokes, hash, breaker);
        assert_eq!(out.result, session::Build::Failed(Status::IoError));
        assert_eq!(out.footers, 0);
    }
    for fault in [Fault::BeforeBegin, Fault::Published, Fault::Pending] {
        let out = run(root, "short", fault, hash, breaker);
        assert_eq!(out.result, session::Build::Failed(if fault == Fault::BeforeBegin { Status::OutOfMemory } else { Status::Unsupported }));
        assert_eq!((out.begins, out.removes), (0, 0));
        assert_eq!(out.file, [0x77]);
    }
    let malformed_style = run(root, "malformed-style", Fault::None, hash, breaker);
    assert!(matches!(malformed_style.result,
        session::Build::Failed(Status::ParseError | Status::OutOfMemory)));
    assert!(malformed_style.file.is_empty() && malformed_style.footers == 0);
    let compressed = run(root, "short-deflate", Fault::None, hash, breaker);
    assert!(matches!(compressed.result, session::Build::Failed(Status::IoError | Status::Truncated | Status::ParseError | Status::OutOfMemory)));
    assert!(compressed.file.is_empty() && compressed.footers == 0);
}

fn index_loan_uses_original_plan_and_restores_no_metadata(breaker: &mut dyn LineBreak) {
    let mut runs = [PlanRun::default(); RUNS];
    let mut text = [0x5a; TEXT];
    runs[RUNS-1].offset = 0xbeef;
    let run_address = runs.as_ptr() as usize;
    let text_address = text.as_ptr() as usize;
    let mut plan = Plan::new(&mut runs, &mut text);
    let signals = Signals::default();
    let mut shared = Bank([0; BANK_BYTES]);
    let mut native = Bank([0; BANK_BYTES]);
    let mut storage = Storage::new(&mut shared.0);
    let mut loan = HostLoan { bank: &mut native, signals: &signals, revoke: false, fail: false };
    let mut parser = Parser { xml: Expat::default(), signals: &signals };
    storage.with_loan(&mut loan, |phase| {
        phase.defer(0)?;
        phase.layout(&mut parser, breaker, |slot, lr, _| {
            let mut metrics = ReaderAtlas;
            let atlas = ReaderAtlas;
            let mut sink = SelectingSink::new(0, Target::Page, true, &mut metrics,
                &atlas, Output::Plan(&mut plan), None).unwrap();
            let mut font = ReaderAtlas;
            let mut backend = SplitBackend { font: &mut font, sink: &mut sink };
            let mut images = lr.images.for_chapter(b"chapter.xhtml");
            let mut engine = Engine::new(reader_parameters(true, slot),
                resources::storage(&mut lr.storage), &mut backend, lr.breaker,
                &mut images, None, CssStylesheetBuilder::default()).ok().unwrap();
            engine.start(b"body", None, None, None, None, None);
            engine.text(b"Alpha.");
            assert!(engine.finish());
        })?;
        Ok(())
    }).unwrap();
    assert_eq!((plan.count(), plan.bytes()), (1, 6));
    for _ in 0..2 {
        with_writer_index(&mut plan, |memory| {
            assert_eq!((memory.chunks.len(), memory.anchors.len()), (6, 192));
            assert_eq!(memory.chunks.as_ptr() as usize % 16, 0);
            assert_eq!(memory.anchors.as_ptr() as usize % 16, 0);
            assert!((text_address..text_address+6160).contains(&(memory.chunks.as_ptr() as usize)));
            assert!((run_address..run_address+2032).contains(&(memory.anchors.as_ptr() as usize)));
            memory.chunks[5].entries[127].chars = 768;
            memory.anchors[191].hash = 0x1234;
        }).unwrap();
        assert_eq!((plan.count(), plan.bytes()), (0, 0));
    }
    drop(plan);
    assert_eq!(runs[RUNS-1].offset, 0xbeef);
    assert_eq!(text[TEXT-1], 0x5a);
}
