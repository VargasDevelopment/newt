//! The frozen scalar records, including explicit unobserved status sentinels.
use super::{ReaderOpenTerminal, Scalar};

#[derive(Default)]
#[repr(C)]
pub struct Counters {
    pub milliseconds: Scalar<u32>,
    pub callbacks: Scalar<u32>,
    pub sleeps: Scalar<u32>,
    pub wait_milliseconds: Scalar<u32>,
}

#[derive(Default)]
#[repr(C)]
pub struct IoCounters {
    pub calls: [Scalar<u32>; 4],
    pub microseconds: [Scalar<u32>; 4],
    pub read_bytes: Scalar<u32>,
}

#[derive(Default)]
#[repr(C)]
pub struct Timing {
    pub microseconds: Scalar<u32>,
    pub calls: Scalar<u32>,
}

#[derive(Default)]
#[repr(C)]
pub struct HashCounters {
    pub microseconds: Scalar<u32>,
    pub calls: Scalar<u32>,
    pub update_microseconds: Scalar<u32>,
    pub updates: Scalar<u32>,
    pub bytes: Scalar<u32>,
    pub wait_microseconds: Scalar<u32>,
    pub waits: Scalar<u32>,
}

#[derive(Default)]
#[repr(C)]
pub struct StreamCounters {
    pub calls: Scalar<u32>,
    pub microseconds: Scalar<u32>,
    pub produced: Scalar<u32>,
    pub io_microseconds: Scalar<u32>,
    pub read_bytes: Scalar<u32>,
}

#[derive(Default)]
#[repr(C)]
pub struct Member {
    pub compressed: Scalar<u32>,
    pub expanded: Scalar<u32>,
    pub stream: StreamCounters,
    pub result: Scalar<i32>,
    pub ordinal: Scalar<u16>,
    pub method: Scalar<u8>,
    pub category: Scalar<u8>,
}

#[derive(Default)]
#[repr(C)]
pub struct Members {
    pub started: Scalar<u32>,
    pub completed: Scalar<u32>,
    pub stored: Scalar<u32>,
    pub deflated: Scalar<u32>,
    pub image_named: Scalar<u32>,
    pub image_microseconds: Scalar<u32>,
    pub image_produced: Scalar<u32>,
    pub failed: Scalar<u32>,
}


#[repr(C)]
pub struct Open {
    pub flags: Scalar<u32>,
    pub elapsed: Scalar<u32>,
    /// UINT16_MAX until a typed terminal status is observed, never default Ok.
    pub status: Scalar<u16>,
    pub cause_status: Scalar<u16>,
    pub terminal: Scalar<ReaderOpenTerminal>,
    pub cancellation: Scalar<u8>,
    pub active: Scalar<bool>,
    pub ready: Scalar<bool>,
}

impl Default for Open {
    fn default() -> Self {
        Self {
            flags: Scalar::new(0), elapsed: Scalar::new(0),
            status: Scalar::new(u16::MAX), cause_status: Scalar::new(u16::MAX),
            terminal: Scalar::new(ReaderOpenTerminal::None), cancellation: Scalar::new(0),
            active: Scalar::new(false), ready: Scalar::new(false),
        }
    }
}

impl Counters {
    pub(super) fn reset(&self) {
        self.milliseconds.reset(); self.callbacks.reset(); self.sleeps.reset(); self.wait_milliseconds.reset();
    }
}
impl IoCounters {
    pub(super) fn reset(&self) {
        for value in &self.calls { value.reset(); }
        for value in &self.microseconds { value.reset(); }
        self.read_bytes.reset();
    }
}
impl Timing {
    pub(super) fn reset(&self) { self.microseconds.reset(); self.calls.reset(); }
}
impl HashCounters {
    pub(super) fn reset(&self) {
        self.microseconds.reset(); self.calls.reset(); self.update_microseconds.reset(); self.updates.reset();
        self.bytes.reset(); self.wait_microseconds.reset(); self.waits.reset();
    }
}
impl StreamCounters {
    pub(super) fn reset(&self) {
        self.calls.reset(); self.microseconds.reset(); self.produced.reset(); self.io_microseconds.reset(); self.read_bytes.reset();
    }
    fn copy_from(&self, source: &Self) {
        self.calls.set(source.calls.get()); self.microseconds.set(source.microseconds.get());
        self.produced.set(source.produced.get()); self.io_microseconds.set(source.io_microseconds.get());
        self.read_bytes.set(source.read_bytes.get());
    }
}
impl Member {
    pub(super) fn reset(&self) {
        self.compressed.reset(); self.expanded.reset(); self.stream.reset(); self.result.reset();
        self.ordinal.reset(); self.method.reset(); self.category.reset();
    }
    pub(super) fn copy_from(&self, source: &Self) {
        self.compressed.set(source.compressed.get()); self.expanded.set(source.expanded.get());
        self.stream.copy_from(&source.stream); self.result.set(source.result.get());
        self.ordinal.set(source.ordinal.get()); self.method.set(source.method.get()); self.category.set(source.category.get());
    }
}
impl Members {
    pub(super) fn reset(&self) {
        self.started.reset(); self.completed.reset(); self.stored.reset(); self.deflated.reset();
        self.image_named.reset(); self.image_microseconds.reset(); self.image_produced.reset(); self.failed.reset();
    }
}
impl Open {
    pub(super) fn reset(&self) {
        self.flags.reset(); self.elapsed.reset(); self.status.set(u16::MAX); self.cause_status.set(u16::MAX);
        self.terminal.set(ReaderOpenTerminal::None); self.cancellation.reset(); self.active.reset(); self.ready.reset();
    }
}
