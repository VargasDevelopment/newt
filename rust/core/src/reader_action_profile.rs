// SPDX-License-Identifier: MIT
//! Reader action timing and stage profiling. Clocks and short observation loans are
//! supplied by the UI; this numeric owner never acquires or retains either.
#![forbid(unsafe_code)]

use core::cell::Cell;
use crate::reader_work_profile::ReaderOpenRoute;

mod trace;
pub use trace::trace_reader_action;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ActionStage {
    Idle, ReceiptLoad, ChapterVerify, OrdinaryFallback, Projection,
    MetadataPublish, ProgressPublish, CacheQuery, Composition, BlockingDisplay,
    SourceSelect, ProgressLoad, SourcePrepare, SessionSetup, SelectionCommit,
    DeviceInfo, Push = 16, Restore = 32, MarkClose = 64, MarkOpen = 128,
}
impl ActionStage {
    pub const HOME_PAINT: Self = Self::SourceSelect;
    pub const READER_RELEASE: Self = Self::SourcePrepare;
    pub const READER_EXIT: Self = Self::SessionSetup;
    pub const DESTINATION: Self = Self::SelectionCommit;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CacheMissReason {
    None, Lease, Attach, Open, Index, Segment, Range, Read, CharStart, Image,
    Finish, Capture, Map, Edge,
}

/// A read-only view of one duration; hooks mutate only their own primitive.
#[repr(transparent)]
pub struct Elapsed(Cell<u32>);
impl Elapsed {
    pub fn get(&self) -> u32 { self.0.get() }
}

/// Fifteen exclusive durations and the frozen packed phase/depth/route bytes.
/// Cell permits synchronous borrowed hooks without copying the 76-byte owner.
#[repr(C)]
pub struct ReaderActionProfile {
    elapsed: [Elapsed; 15],
    started: Cell<u32>,
    total: Cell<u32>,
    parents: Cell<u32>,
    actions: Cell<u8>,
    faults: Cell<u8>,
    phase_depth: Cell<u8>,
    route: Cell<u8>,
}

impl ReaderActionProfile {
    pub const STAGES: usize = 15;
    pub const SCHEMA: u8 = 4;
    pub const BAD_ORDER: u16 = 1;
    pub const COUNTER_OVERFLOW: u16 = 2;
    pub const MULTIPLE_ACTIONS: u16 = 4;
    pub const UNCLOSED_STAGE: u16 = 8;
    pub const BAD_ACCOUNTING: u16 = 16;
    const ACTIVE: u8 = 128;
    const CLOSING: u8 = 64;

    pub const fn new() -> Self {
        Self {
            elapsed: [const { Elapsed(Cell::new(0)) }; 15],
            started: Cell::new(0), total: Cell::new(0), parents: Cell::new(0),
            actions: Cell::new(0), faults: Cell::new(0), phase_depth: Cell::new(0),
            route: Cell::new(0),
        }
    }

    pub fn begin(&self, now: u32) {
        if self.running() {
            self.fault(Self::MULTIPLE_ACTIONS);
            if self.actions.get() == u8::MAX { self.fault(Self::COUNTER_OVERFLOW); }
            else { self.actions.set(self.actions.get() + 1); }
            return;
        }
        for elapsed in &self.elapsed { elapsed.0.set(0); }
        self.parents.set(0); self.phase_depth.set(0); self.route.set(0);
        self.faults.set(Self::ACTIVE); self.actions.set(1);
        self.total.set(now); self.started.set(now);
    }

    /// Raw packed boundary retains the frozen rejection of malformed tags.
    pub fn boundary(&self, mut next: u8, now: u32) {
        if !self.running() { return; }
        if next == ActionStage::MarkClose as u8 {
            self.faults.set(self.faults.get() | Self::CLOSING); return;
        }
        if next == ActionStage::MarkOpen as u8 { return; }
        let mut value = next;
        let scoped = value >= ActionStage::Push as u8;
        let mut depth = self.depth();
        if next == ActionStage::Restore as u8 {
            if depth == 0 { self.fault(Self::BAD_ORDER); return; }
            next = (self.parents.get() & 15) as u8;
            self.parents.set(self.parents.get() >> 4); depth -= 1;
        } else if value & 16 != 0 && value < 32 {
            value &= 15;
            if value == 0 || depth == 8 { self.fault(Self::BAD_ORDER); return; }
            self.parents.set((self.parents.get() << 4) | u32::from(self.stage()));
            depth += 1; next = value;
        } else if usize::from(value) > Self::STAGES {
            self.fault(Self::BAD_ORDER); return;
        }
        if self.stage() == 0 && next == 0 || !scoped && self.stage() != 0 && next != 0 {
            self.fault(Self::BAD_ORDER); return;
        }
        self.charge(now);
        self.phase_depth.set(next | depth << 4); self.started.set(now);
    }

    pub fn route(&self, route: ReaderOpenRoute) {
        if !self.running() { return; }
        let bit = match route {
            ReaderOpenRoute::CacheProbe => 1,
            ReaderOpenRoute::CacheHit => 2,
            ReaderOpenRoute::OrdinaryPage => 4,
            ReaderOpenRoute::ContentsPerRow => 128,
            _ => return,
        };
        self.route.set(self.route.get() | bit);
    }

    /// Missing whole-file Open/Attach gives way to the first specific refusal.
    pub fn cache_miss(&self, reason: CacheMissReason) {
        let kept = self.miss_reason();
        if !self.running() || kept != CacheMissReason::None as u8
            && kept != CacheMissReason::Open as u8 && kept != CacheMissReason::Attach as u8 {
            return;
        }
        self.route.set((self.route.get() & 135) | (reason as u8) << 3);
    }

    pub fn finish(&self, now: u32) -> bool {
        if !self.running() { return false; }
        if self.stage() != 0 || self.depth() != 0 { self.fault(Self::UNCLOSED_STAGE); }
        self.charge(now); self.total.set(now.wrapping_sub(self.total.get()));
        self.faults.set(self.faults.get() & !Self::ACTIVE);
        if self.subtotal() > self.total.get() { self.fault(Self::BAD_ACCOUNTING); }
        true
    }

    pub fn elapsed(&self) -> &[Elapsed; 15] { &self.elapsed }
    pub fn total(&self) -> u32 { self.total.get() }
    pub fn since(&self, now: u32) -> u32 {
        if self.running() { now.wrapping_sub(self.total.get()) } else { 0 }
    }
    pub fn residual(&self) -> u32 {
        let subtotal = self.subtotal();
        if subtotal <= self.total.get() { self.total.get() - subtotal } else { 0 }
    }
    pub fn faults(&self) -> u16 { u16::from(self.faults.get() & !(Self::ACTIVE | Self::CLOSING)) }
    pub fn closing(&self) -> bool { self.faults.get() & Self::CLOSING != 0 }
    pub fn running(&self) -> bool { self.faults.get() & Self::ACTIVE != 0 }
    pub fn stage(&self) -> u8 { self.phase_depth.get() & 15 }
    pub fn actions(&self) -> u16 { u16::from(self.actions.get()) }
    pub fn routes(&self) -> u8 { self.route.get() & 135 }
    pub fn miss_reason(&self) -> u8 { (self.route.get() >> 3) & 15 }

    fn fault(&self, fault: u16) { self.faults.set(self.faults.get() | fault as u8); }
    fn depth(&self) -> u8 { self.phase_depth.get() >> 4 }
    fn subtotal(&self) -> u32 {
        self.elapsed.iter().fold(0u32, |total, elapsed| total.wrapping_add(elapsed.get()))
    }
    fn charge(&self, now: u32) {
        if self.stage() != 0 {
            let elapsed = &self.elapsed[usize::from(self.stage()) - 1];
            elapsed.0.set(elapsed.get().wrapping_add(now.wrapping_sub(self.started.get())));
        }
    }
}

impl Default for ReaderActionProfile {
    fn default() -> Self { Self::new() }
}
const _: () = assert!(core::mem::size_of::<ReaderActionProfile>() == 76);
const _: () = assert!(core::mem::align_of::<ReaderActionProfile>() == 4);

#[cfg(test)]
mod contents_trace_tests {
    use super::*;
    #[test]
    fn contents_per_row_trace_survives_cache_route_and_miss_updates() {
        let profile = ReaderActionProfile::new();
        profile.begin(0);
        profile.route(ReaderOpenRoute::ContentsPerRow);
        profile.route(ReaderOpenRoute::OrdinaryPage);
        profile.cache_miss(CacheMissReason::Map);
        assert_eq!(profile.routes(), 128 | 4);
        assert_eq!(profile.miss_reason(), CacheMissReason::Map as u8);
        assert_eq!(core::mem::size_of::<ReaderActionProfile>(), 76);
    }
}
