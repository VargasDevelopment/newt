//! DEV reader-open work profile spans.
//! Shared UI-only hooks mutate individual primitives, never copy a whole profile.
//! Clocks, cancellation and transport are supplied per call and never retained.
#![forbid(unsafe_code)]

use crate::admission::{WorkEvent, WorkKind, WorkPhase};
use crate::zip_container::Status;

#[path = "reader_work_profile/counters.rs"]
mod counters;
#[path = "reader_work_profile/rows.rs"]
pub(crate) mod rows;
#[path = "reader_work_profile/scalar.rs"]
mod scalar;
#[path = "reader_work_profile/trace.rs"]
mod trace;
#[path = "reader_work_profile/types.rs"]
mod types;

pub use counters::{Counters, HashCounters, IoCounters, Member, Members, Open, StreamCounters, Timing};
pub use scalar::Scalar;
pub use trace::{trace_reader_details, trace_reader_work, TraceSink};
pub use types::{ReaderHashOperation, ReaderIoOperation, ReaderOpenRoute, ReaderOpenTerminal};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[repr(u8)]
enum Operation {
    #[default]
    None,
    Inspect,
    Catalog,
    Compare,
    Stream,
    Hash,
}

/// One numeric stack value shared by lexical UI hooks. Cell makes it !Sync;
/// it has no retained pointer, second diagnostic owner, lock or heap backing.
#[repr(C)]
pub struct ReaderWorkProfile {
    index: [Scalar<u32>; 13],
    index_stage: Scalar<u8>,
    index_started: Scalar<u32>,
    phases: [Counters; 3],
    io: [IoCounters; 5],
    admission: [Timing; 3],
    hash: HashCounters,
    stream: StreamCounters,
    members: Members,
    open: Open,
    current: Member,
    maximum: Member,
    started: Scalar<u32>,
    operation_started: Scalar<u32>,
    comparisons: Scalar<u32>,
    phase: Scalar<WorkPhase>,
    operation: Scalar<Operation>,
    member_active: Scalar<bool>,
}

impl Default for ReaderWorkProfile {
    fn default() -> Self {
        Self {
            index: core::array::from_fn(|_| Scalar::new(0)),
            index_stage: Scalar::new(0),
            index_started: Scalar::new(0),
            phases: core::array::from_fn(|_| Counters::default()),
            io: core::array::from_fn(|_| IoCounters::default()),
            admission: core::array::from_fn(|_| Timing::default()),
            hash: HashCounters::default(),
            stream: StreamCounters::default(),
            members: Members::default(),
            open: Open::default(),
            current: Member::default(),
            maximum: Member::default(),
            started: Scalar::new(0),
            operation_started: Scalar::new(0),
            comparisons: Scalar::new(0),
            phase: Scalar::new(WorkPhase::Idle),
            operation: Scalar::new(Operation::None),
            member_active: Scalar::new(false),
        }
    }
}

impl ReaderWorkProfile {
    pub fn open_route(&self, route: ReaderOpenRoute) {
        self.open_route_tag(route as u32);
    }
    /// Existing typed route carried by the short WorkEvent scalar transport.
    pub fn open_route_tag(&self, route: u32) {
        self.open.flags.set(self.open.flags.get() | route);
    }

    pub fn open_status(&self, status: Status) {
        self.open.cause_status.set(status as u16);
    }

    /// Borrow the existing firmware cancellation snapshot's fields for this call.
    /// First terminal status wins; later observations may only reduce readiness.
    pub fn terminal(&self, stage: ReaderOpenTerminal, status: Option<Status>, ready: bool,
        elapsed: &u32, reason: &u8, active: &bool,
    ) {
        if self.open.terminal.get() == ReaderOpenTerminal::None {
            self.open.terminal.set(stage);
            self.open.status.set(status.map_or(u16::MAX, |status| status as u16));
            self.open.ready.set(ready);
        } else {
            self.open.ready.set(self.open.ready.get() && ready);
        }
        if *active { self.open.elapsed.set(*elapsed); }
        self.open.cancellation.set(self.open.cancellation.get() | *reason);
        self.open.active.set(self.open.active.get() || *active);
        if self.open.cancellation.get() != 0 { self.open.ready.set(false); }
    }

    pub fn opening(&self) -> &Open { &self.open }

    pub fn finish_observation(&self) {
        if self.operation.get() != Operation::None { self.open_route(ReaderOpenRoute::TimingInvalid); }
    }

    pub fn reset(&self) {
        for counters in &self.phases { counters.reset(); }
        for counters in &self.io { counters.reset(); }
        for timing in &self.admission { timing.reset(); }
        self.hash.reset(); self.stream.reset(); self.members.reset(); self.open.reset();
        self.current.reset(); self.maximum.reset();
        self.started.reset(); self.operation_started.reset(); self.comparisons.reset();
        self.phase.set(WorkPhase::Idle); self.operation.set(Operation::None); self.member_active.reset();
    }

    pub fn phase(&self, next: WorkPhase, now: u32) {
        if let Some(current) = self.counters() {
            current.milliseconds.add(now.wrapping_sub(self.started.get()));
        }
        self.phase.set(next);
        self.started.set(now);
    }

    pub fn callback(&self) {
        if let Some(current) = self.counters() { current.callbacks.add(1); }
    }

    pub fn slept(&self, elapsed: u32) {
        if let Some(current) = self.counters() {
            current.sleeps.add(1); current.wait_milliseconds.add(elapsed);
        }
    }

    pub fn event(&self, event: &WorkEvent, us: u32, ms: u32) {
        match event.kind {
            WorkKind::IndexStage => {
                let next = event.result as u8;
                if next != self.index_stage.get() {
                    let previous = usize::from(self.index_stage.get());
                    if (1..=6).contains(&previous) {
                        self.index[previous].set(self.index[previous].get().saturating_add(us.wrapping_sub(self.index_started.get())));
                    }
                    self.index_stage.set(next);
                    self.index_started.set(us);
                }
            }
            WorkKind::IndexResult => {
                let previous = usize::from(self.index_stage.get());
                if (1..=6).contains(&previous) {
                    self.index[previous].set(self.index[previous].get().saturating_add(us.wrapping_sub(self.index_started.get())));
                }
                self.index_stage.set(0);
                self.index[0].set(event.result as u32);
                self.index[7].set(event.compressed_bytes);
                self.index[10].set(u32::from(event.method));
            }
            WorkKind::IndexIo => {
                self.index[11].set(self.index[11].get().saturating_add(event.compressed_bytes));
                self.index[12].set(self.index[12].get().saturating_add(event.expanded_bytes));
            }
            WorkKind::IndexCounts => {
                self.index[8].set(event.compressed_bytes);
                self.index[9].set(event.expanded_bytes);
            }
            WorkKind::ActionBoundary | WorkKind::OpenRoute | WorkKind::CacheMiss => {},
            WorkKind::Phase => self.phase(event.phase, ms),
            WorkKind::InspectBegin => self.begin_stage(0, Operation::Inspect, us),
            WorkKind::InspectEnd => self.end_stage(0, Operation::Inspect, us),
            WorkKind::CatalogBegin => self.begin_stage(1, Operation::Catalog, us),
            WorkKind::CatalogEnd => self.end_stage(1, Operation::Catalog, us),
            WorkKind::CompareBegin => self.begin_stage(2, Operation::Compare, us),
            WorkKind::CompareEnd => {
                self.end_stage(2, Operation::Compare, us);
                if event.result > 0 { self.comparisons.add(event.result as u32); }
            }
            WorkKind::MemberBegin => self.member_begin(event),
            WorkKind::StreamBegin => { self.begin_operation(Operation::Stream, us); }
            WorkKind::StreamEnd => self.stream_end(event.result, us),
            WorkKind::MemberEnd => {
                self.current.result.set(event.result);
                self.member_active.set(false);
                if event.result == 0 { self.members.completed.add(1); } else { self.members.failed.add(1); }
                if self.members.started.get() == 1 || self.current.stream.microseconds.get() > self.maximum.stream.microseconds.get() {
                    self.maximum.copy_from(&self.current);
                }
            }
        }
    }

    pub fn io(&self, operation: ReaderIoOperation, elapsed_us: u32, bytes: u32) {
        let bucket = if self.operation.get() == Operation::Hash { 4 } else { self.phase.get() as usize };
        let bucket = &self.io[bucket];
        let index = operation as usize;
        bucket.calls[index].add(1); bucket.microseconds[index].add(elapsed_us);
        if operation == ReaderIoOperation::Read { bucket.read_bytes.add(bytes); }
        // Nested IO is a subset of stream elapsed, never an additional duration.
        if self.operation.get() == Operation::Stream {
            self.stream.io_microseconds.add(elapsed_us);
            self.current.stream.io_microseconds.add(elapsed_us);
            if operation == ReaderIoOperation::Read {
                self.stream.read_bytes.add(bytes); self.current.stream.read_bytes.add(bytes);
            }
        }
    }

    pub fn hash(&self, operation: ReaderHashOperation, us: u32, bytes: u32) {
        match operation {
            ReaderHashOperation::Begin => {
                if self.begin_operation(Operation::Hash, us) { self.hash.calls.add(1); }
            }
            ReaderHashOperation::End => {
                if self.matches_operation(Operation::Hash) {
                    self.hash.microseconds.add(us.wrapping_sub(self.operation_started.get()));
                    self.operation.set(Operation::None);
                }
            }
            ReaderHashOperation::Update => {
                self.matches_operation(Operation::Hash);
                self.hash.updates.add(1); self.hash.update_microseconds.add(us); self.hash.bytes.add(bytes);
            }
            ReaderHashOperation::Wait => {
                self.matches_operation(Operation::Hash);
                self.hash.waits.add(1); self.hash.wait_microseconds.add(us);
            }
        }
    }

    pub fn phases(&self) -> &[Counters; 3] { &self.phases }
    pub fn io_buckets(&self) -> &[IoCounters; 5] { &self.io }
    pub fn admission(&self) -> &[Timing; 3] { &self.admission }
    pub fn comparisons(&self) -> u32 { self.comparisons.get() }
    pub fn hashing(&self) -> &HashCounters { &self.hash }
    pub fn streams(&self) -> &StreamCounters { &self.stream }
    pub fn members(&self) -> &Members { &self.members }
    pub fn current_member(&self) -> &Member { &self.current }
    pub fn maximum_member(&self) -> &Member { &self.maximum }
    pub fn member_active(&self) -> bool { self.member_active.get() }

    fn begin_operation(&self, next: Operation, us: u32) -> bool {
        if self.operation.get() != Operation::None {
            self.open_route(ReaderOpenRoute::TimingInvalid);
            return false;
        }
        self.operation.set(next); self.operation_started.set(us);
        true
    }

    fn matches_operation(&self, expected: Operation) -> bool {
        if self.operation.get() == expected { return true; }
        self.open_route(ReaderOpenRoute::TimingInvalid);
        false
    }

    fn begin_stage(&self, index: usize, operation: Operation, us: u32) {
        if self.begin_operation(operation, us) { self.admission[index].calls.add(1); }
    }

    fn end_stage(&self, index: usize, operation: Operation, us: u32) {
        if self.matches_operation(operation) {
            self.admission[index].microseconds.add(us.wrapping_sub(self.operation_started.get()));
            self.operation.set(Operation::None);
        }
    }

    fn counters(&self) -> Option<&Counters> {
        if self.phase.get() == WorkPhase::Idle { None } else { Some(&self.phases[self.phase.get() as usize - 1]) }
    }

    fn member_begin(&self, event: &WorkEvent) {
        self.current.reset();
        self.current.ordinal.set(event.member); self.current.method.set(event.method);
        self.current.category.set(event.category); self.current.compressed.set(event.compressed_bytes);
        self.current.expanded.set(event.expanded_bytes); self.member_active.set(true);
        self.members.started.add(1);
        if event.method == 0 { self.members.stored.add(1); }
        if event.method == 8 { self.members.deflated.add(1); }
        if event.category == 1 { self.members.image_named.add(1); }
    }

    fn stream_end(&self, result: i32, us: u32) {
        if !self.matches_operation(Operation::Stream) { return; }
        let elapsed = us.wrapping_sub(self.operation_started.get());
        self.stream.calls.add(1); self.stream.microseconds.add(elapsed);
        self.current.stream.calls.add(1); self.current.stream.microseconds.add(elapsed);
        if result > 0 {
            let bytes = result as u32;
            self.stream.produced.add(bytes); self.current.stream.produced.add(bytes);
            if self.current.category.get() == 1 { self.members.image_produced.add(bytes); }
        }
        if self.current.category.get() == 1 { self.members.image_microseconds.add(elapsed); }
        self.operation.set(Operation::None);
    }
}

const _: () = assert!(core::mem::size_of::<ReaderWorkProfile>() == 496);
