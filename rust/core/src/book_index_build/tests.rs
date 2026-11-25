extern crate std;
use crate as core_api;
use super::*;
use std::{vec, vec::Vec};
#[path = "../../tests/book_index_build/support.rs"]
mod support;
#[derive(Clone, Copy)]
enum Fault { None, Crc, Short, Write, Sync }
struct Store { work: Vec<u8>, pending: Vec<u8>, published: Vec<u8>, removed: bool, fault: Fault, remove_fails: bool }
impl Store {
    fn new(fault: Fault) -> Self { Self { work: Vec::new(), pending: Vec::new(), published: Vec::new(), removed: false, fault, remove_fails: false } }
}
impl WorkFile for Store {
    fn append(&mut self, bytes: &[u8]) -> bool {
        if matches!(self.fault, Fault::Write) { return false; }
        self.work.extend_from_slice(bytes); true
    }
    fn read_at(&mut self, offset: u32, out: &mut [u8]) -> bool {
        if matches!(self.fault, Fault::Short) { return false; }
        if matches!(self.fault, Fault::Crc) { self.work[24] ^= 1; self.fault = Fault::None; }
        let Some(bytes) = self.work.get(offset as usize..offset as usize + out.len()) else { return false; };
        out.copy_from_slice(bytes); true
    }
    fn remove_work(&mut self) -> bool {
        if self.remove_fails { return false; }
        self.removed = true; self.work.clear(); true
    }
    fn sync_work(&mut self) -> bool { !matches!(self.fault, Fault::Sync) }
}
impl CacheStorage for Store {
    fn remove(&mut self, _: &[u8]) -> bool { self.published.clear(); true }
    fn file_size(&mut self, _: &[u8]) -> i64 { self.published.len() as i64 }
    fn read_at(&mut self, _: &[u8], at: u32, out: &mut [u8]) -> i32 {
        let Some(bytes) = self.published.get(at as usize..at as usize + out.len()) else { return -1; };
        out.copy_from_slice(bytes); out.len() as i32
    }
    fn begin_write(&mut self, _: &[u8]) -> bool { self.pending.clear(); true }
    fn write(&mut self, bytes: &[u8]) -> bool { self.pending.extend_from_slice(bytes); true }
    fn end_write(&mut self) -> bool { self.published = std::mem::take(&mut self.pending); true }
}
#[test]
fn spill_faults_remove_work_and_never_publish() {
    for fault in [Fault::Crc, Fault::Short, Fault::Write, Fault::Sync] {
        let mut store = Store::new(fault);
        let mut buffer = [0; 448];
        let mut join = support::JoinBacking::new();
        let mut state = join.state();
        let result = support::prepare(&mut store, &mut buffer, 64, 40, 40, 40, &mut state)
            .and_then(|prepared| prepared.publish(&mut store, &mut state, &mut |_| true, &mut || true));
        assert!(result.is_err());
        assert!(store.removed && store.work.is_empty());
        assert!(store.published.is_empty());
    }
}
#[test]
fn a_misread_spine_position_fails_the_build_instead_of_panicking() {
    let mut set = [0; POSITION_WORDS];
    let mut positions = engine::Positions(&mut set);
    positions.mark(MAX_SPINE - 1).unwrap();
    assert_eq!(positions.rank(MAX_SPINE - 1), Ok(0));
    assert_eq!(positions.mark(MAX_SPINE), Err(Error::Book(Status::IoError)));
    assert_eq!(positions.rank(u32::MAX), Err(Error::Book(Status::IoError)));
}
#[test]
fn a_published_index_survives_a_failed_work_removal() {
    let mut store = Store::new(Fault::None);
    store.remove_fails = true;
    let mut buffer = [0; 448];
    let mut join = support::JoinBacking::new();
    let mut state = join.state();
    let report = support::prepare(&mut store, &mut buffer, 64, 40, 40, 40, &mut state).unwrap()
        .publish(&mut store, &mut state, &mut |_| true, &mut || true).unwrap();
    assert!(!report.work_removed && !store.work.is_empty());
    let reader = crate::book_index::Reader::open(&mut store, &support::IDENTITY).unwrap();
    assert_eq!(reader.header().spine, 40);
}
#[test]
fn cancellation_during_merges_removes_work() {
    let mut store = Store::new(Fault::None);
    let mut buffer = [0; 448];
    let mut join = support::JoinBacking::new();
    let mut state = join.state();
    let prepared = support::prepare(&mut store, &mut buffer, 64, 40, 40, 40, &mut state).unwrap();
    let result = prepared.publish(&mut store, &mut state, &mut |p| p.phase != Phase::Merge, &mut || true);
    assert!(result.is_err());
    assert!(store.removed && store.work.is_empty() && store.published.is_empty());
}
#[test]
fn progress_never_falls_within_a_phase() {
    let mut store = Store::new(Fault::None);
    let mut buffer = [0; 448];
    let mut join = support::JoinBacking::new();
    let mut state = join.state();
    let mut seen: Vec<Progress> = Vec::new();
    let prepared = support::prepare_watched(&mut store, &mut buffer, 64, 120, 120, 120, &mut state,
        &mut |p| { seen.push(p); true }).unwrap();
    prepared.publish(&mut store, &mut state, &mut |p| { seen.push(p); true }, &mut || true).unwrap();
    for phase in [Phase::Central, Phase::Package, Phase::Join, Phase::Merge, Phase::Rank, Phase::Publish] {
        let steps: Vec<_> = seen.iter().filter(|p| p.phase == phase).collect();
        assert!(!steps.is_empty(), "{phase:?} never reported");
        assert!(steps.windows(2).all(|w| w[0].done <= w[1].done), "{phase:?} fell back");
        assert!(steps.iter().all(|p| p.total == 0 || p.done <= p.total), "{phase:?} passed its total");
    }
}
#[test]
fn index_limits_io_model() {
    for manifests in [MAX_SPINE, MAX_MEMBERS] {
    let mut store = Store::new(Fault::None);
    let mut buffer = vec![0; 32768];
    let mut join = support::JoinBacking::new();
    let mut state = join.state();
    let report = support::prepare(&mut store, &mut buffer, 32768, MAX_MEMBERS, manifests, MAX_SPINE, &mut state).unwrap()
        .publish(&mut store, &mut state, &mut |_| true, &mut || true).unwrap();
    let reader = crate::book_index::Reader::open(&mut store, &support::IDENTITY).unwrap();
    assert_eq!((reader.header().members, reader.header().spine), (MAX_MEMBERS, MAX_SPINE));
    for i in 0..MAX_SPINE {
        let s = reader.spine(&mut store, i).unwrap();
        assert_eq!(s.before, u64::from(i) * 10);
        let m = reader.member(&mut store, s.member).unwrap();
        assert_eq!(m.local_offset, i * 64);
        assert_eq!(m.first_spine, i);
    }
    let bound = storage_bound(MAX_MEMBERS, u32::MAX).unwrap();
    let payload = store.published.len() as u64;
    let envelope = payload + payload.div_ceil(4096) * 4 + 64;
    assert!(report.statistics.written + envelope <= bound);
    assert!(bound + envelope < 16 * 1024 * 1024);
    if manifests == MAX_SPINE {
        assert!(report.statistics.written <= 4_500_000);
        assert!(report.statistics.read <= 7_000_000);
    }
    std::println!("LIMITS manifests={manifests} {:?} buffer=32768 reservation={bound} envelope={envelope}", report.statistics);
    }
}
#[test]
fn normal_book_never_opens_a_work_file() {
    let mut store = Store::new(Fault::None);
    let mut buffer = [0; 32768];
    let mut join = support::JoinBacking::new();
    let mut state = join.state();
    let report = support::prepare(&mut store, &mut buffer, 32768, 104, 100, 100, &mut state).unwrap()
        .publish(&mut store, &mut state, &mut |_| true, &mut || true).unwrap();
    assert_eq!((report.statistics.written, report.statistics.read, report.statistics.read_calls), (0, 0, 0));
    assert_eq!(report.header.spine, 100);
    std::println!("STATE stack={STACK_BYTES} join_state={JOIN_STATE_BYTES}");
}
