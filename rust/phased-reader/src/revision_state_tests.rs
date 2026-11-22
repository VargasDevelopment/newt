// Synthetic operation/count fixtures from prepared_tests, with the same
// Source/Store/Hash/Admission bodies. They do not implement native SHA or I/O.
use super::*;
use super::super::session::{Build, Cache, Reader, Session};
use crate::zip_container::{Entry, Status};
use core::cell::Cell;

#[derive(Default)]
struct Calls {
    source: Cell<u32>,
    io: Cell<u32>,
    hash: Cell<u32>,
    abort: Cell<u32>,
    seal: Cell<u32>,
}
fn tick(count: &Cell<u32>) { count.set(count.get() + 1); }
struct Source<'a>(&'a Calls);
impl BookSource for Source<'_> {
    fn size(&self) -> u64 { tick(&self.0.source); 17 }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        tick(&self.0.source);
        let count = out.len().min(17u64.saturating_sub(at) as usize);
        out[..count].fill(0x31);
        count as i32
    }
}
struct Store<'a> {
    calls: &'a Calls,
    generation: u64,
    bytes: [u8; 160],
    sealed: bool,
    fail_after_seal: bool,
}
impl<'a> Store<'a> {
    fn new(calls: &'a Calls) -> Self {
        Self { calls, generation: 7, bytes: [0; 160], sealed: false, fail_after_seal: false }
    }
}
impl RevisionStorage for Store<'_> {
    fn load(&mut self, bytes: &mut u32) -> bool { tick(&self.calls.io); *bytes = 160; true }
    fn begin(&mut self, bytes: u32) -> bool {
        tick(&self.calls.io); self.generation += 1; bytes == 160
    }
    fn read(&mut self, offset: u32, out: &mut [u8]) -> bool {
        tick(&self.calls.io);
        let at = offset as usize;
        if at > 160 || out.len() > 160 - at { return false; }
        out.copy_from_slice(&self.bytes[at..at + out.len()]); true
    }
    fn write(&mut self, offset: u32, input: &[u8]) -> bool {
        tick(&self.calls.io);
        let at = offset as usize;
        if at > 160 || input.len() > 160 - at { return false; }
        self.bytes[at..at + input.len()].copy_from_slice(input); true
    }
    fn seal(&mut self) -> bool { tick(&self.calls.seal); self.sealed = true; true }
    fn abort(&mut self) { tick(&self.calls.abort); self.generation += 1; }
    fn generation(&self) -> u64 { self.generation }
    fn stable(&mut self, generation: u64) -> bool {
        tick(&self.calls.io);
        generation == self.generation && !(self.sealed && self.fail_after_seal)
    }
}
struct Hash<'a>(&'a Calls);
impl RevisionHasher for Hash<'_> {
    fn begin(&mut self) -> bool { tick(&self.0.hash); true }
    fn update(&mut self, _: &[u8]) -> bool { tick(&self.0.hash); true }
    fn finish(&mut self, out: &mut Digest) -> bool {
        tick(&self.0.hash); *out = [0xa7; 32]; true
    }
    fn hash(&mut self, _: &[u8], out: &mut Digest) -> bool {
        tick(&self.0.hash); *out = [0xa7; 32]; true
    }
}
struct Admission { result: Status, admitted: u32 }
impl Reader for Admission {
    fn close(&mut self) {}
    fn strict_open(&mut self, _: &mut dyn BookSource) -> Status {
        self.admitted += 1; self.result
    }
    fn spine_count(&self) -> u32 { 0 }
    fn stage(&mut self, _: &mut dyn BookSource, _: u32, _: bool) -> Result<Entry, Status> {
        Err(Status::Unsupported)
    }
    fn commit(&mut self, _: u32) { panic!("prepared-open must not select/cache-build"); }
}
struct NoCache;
impl Cache<Admission> for NoCache {
    fn build(&mut self, _: &mut dyn BookSource, _: &Digest, _: u32, _: &mut Admission) -> Build {
        panic!("prepared-open must not build a cache");
    }
    fn stored(&mut self, _: &Digest, _: u32, _: &mut Admission) -> bool {
        panic!("prepared-open must not query a cache");
    }
}

fn observed(calls: &Calls) -> [u32; 5] {
    [calls.source.get(), calls.io.get(), calls.hash.get(), calls.abort.get(), calls.seal.get()]
}

#[test]
fn prepared_full_and_compact_state_moves_without_io_or_abort() {
    for compact in [false, true] {
        let calls = Calls::default();
        let mut source = Source(&calls);
        let mut store = Store::new(&calls);
        let mut whole = Hash(&calls);
        let mut block = Hash(&calls);
        let mut scratch = [0; 4096];
        let scratch = &mut scratch[..if compact { 256 } else { 4096 }];
        let mut revision = if compact {
            ImportedRevision::compact(&mut store, &mut whole, &mut block, scratch)
        } else {
            ImportedRevision::full(&mut store, &mut whole, scratch.try_into().expect("full scratch"))
        };
        assert!(revision.begin_import(&mut source));
        let before = observed(&calls);
        let counters = revision.counters();
        let state = revision.into_state();
        assert!(state.provisional());
        assert_eq!(*state.digest(), [0xa7; 32]);
        assert_eq!(state.counters(), counters);
        assert_eq!(observed(&calls), before);
        let revision = match state.lend(RevisionPorts {
            source: Some(&mut source), store: &mut store, whole: &mut whole,
            block: if compact { Some(&mut block) } else { None }, scratch,
        }) { Ok(revision) => revision, Err(_) => panic!("same live ports") };
        assert!(revision.provisional());
        assert_eq!(revision.counters(), counters);
        assert_eq!(observed(&calls), before);
        drop(revision);
        assert_eq!(calls.abort.get(), 1);
    }
}

#[test]
fn admission_is_still_required_before_the_one_promotion() {
    for result in [Status::Ok, Status::Unsupported] {
        let calls = Calls::default();
        let mut source = Source(&calls);
        let mut store = Store::new(&calls);
        let mut hash = Hash(&calls);
        let mut scratch = [0; 4096];
        let mut revision = ImportedRevision::full(&mut store, &mut hash, &mut scratch);
        assert!(revision.begin_import(&mut source));
        let state = revision.into_state();
        assert_eq!(calls.seal.get(), 0);
        let mut revision = match state.lend(RevisionPorts {
            source: Some(&mut source), store: &mut store, whole: &mut hash,
            block: None, scratch: &mut scratch,
        }) { Ok(revision) => revision, Err(_) => panic!("same live ports") };
        let mut reader = Admission { result, admitted: 0 };
        let mut cache = NoCache;
        assert_eq!(Session { revision: &mut revision, reader: &mut reader, cache: &mut cache }
            .finish_prepared_open(), result);
        let state = revision.into_state();
        assert_eq!(reader.admitted, 1);
        assert!(!state.provisional());
        assert_eq!(calls.seal.get(), u32::from(result == Status::Ok));
        assert_eq!(calls.abort.get(), u32::from(result != Status::Ok));
        assert_eq!(state.valid, result == Status::Ok);
        assert_eq!(state.counters().full_hash_runs, 1);
        assert_eq!(state.counters().full_hash_bytes, 17);
        assert_eq!(state.counters().receipt_hash_bytes,
            if result == Status::Ok { 128 } else { 0 });
    }
}

#[test]
fn a_sealed_receipt_with_failed_final_guard_returns_failed_resulting_state() {
    let calls = Calls::default();
    let mut source = Source(&calls);
    let mut store = Store::new(&calls);
    store.fail_after_seal = true;
    let mut hash = Hash(&calls);
    let mut scratch = [0; 4096];
    let mut revision = ImportedRevision::full(&mut store, &mut hash, &mut scratch);
    assert!(revision.begin_import(&mut source));
    let state = revision.into_state();
    let mut revision = match state.lend(RevisionPorts {
        source: Some(&mut source), store: &mut store, whole: &mut hash,
        block: None, scratch: &mut scratch,
    }) { Ok(revision) => revision, Err(_) => panic!("same live ports") };
    let mut reader = Admission { result: Status::Ok, admitted: 0 };
    let mut cache = NoCache;
    assert_eq!(Session { revision: &mut revision, reader: &mut reader, cache: &mut cache }
        .finish_prepared_open(), Status::IoError);
    let state = revision.into_state();
    assert!(!state.valid && !state.provisional());
    assert_eq!((calls.seal.get(), calls.abort.get()), (1, 0));
    assert_eq!(state.counters().full_hash_runs, 1);
    assert!(store.sealed);
}

#[test]
fn replacement_generation_is_rejected_without_aborting_that_replacement() {
    let calls = Calls::default();
    let mut source = Source(&calls);
    let mut store = Store::new(&calls);
    let mut hash = Hash(&calls);
    let mut scratch = [0; 4096];
    let mut revision = ImportedRevision::full(&mut store, &mut hash, &mut scratch);
    assert!(revision.begin_import(&mut source));
    let state = revision.into_state();
    store.generation += 1;
    let before = observed(&calls);
    let (error, state, ports) = match state.lend(RevisionPorts {
        source: Some(&mut source), store: &mut store, whole: &mut hash,
        block: None, scratch: &mut scratch,
    }) { Err(rejected) => rejected, Ok(_) => panic!("replacement cannot inherit authority") };
    assert_eq!(error, StateLoanError::Generation);
    assert!(state.provisional());
    assert_eq!(observed(&calls), before);
    drop(ports);
    assert_eq!(calls.abort.get(), 0);
}

#[test]
fn wrong_scratch_returns_every_original_loan_without_consuming_state() {
    let calls = Calls::default();
    let mut source = Source(&calls);
    let mut store = Store::new(&calls);
    let mut whole = Hash(&calls);
    let mut block = Hash(&calls);
    let mut scratch = [0; 256];
    let mut revision = ImportedRevision::compact(&mut store, &mut whole, &mut block, &mut scratch);
    assert!(revision.begin_import(&mut source));
    let state = revision.into_state();
    let before = observed(&calls);
    let (error, state, ports) = match state.lend(RevisionPorts {
        source: Some(&mut source), store: &mut store, whole: &mut whole,
        block: Some(&mut block), scratch: &mut scratch[..255],
    }) { Err(rejected) => rejected, Ok(_) => panic!("wrong scratch rejected") };
    assert_eq!(error, StateLoanError::Shape);
    assert_eq!(observed(&calls), before);
    drop(ports);
    let revision = match state.lend(RevisionPorts {
        source: Some(&mut source), store: &mut store, whole: &mut whole,
        block: Some(&mut block), scratch: &mut scratch,
    }) { Ok(revision) => revision, Err(_) => panic!("original scratch accepted") };
    drop(revision);
    assert_eq!(calls.abort.get(), 1);
}
