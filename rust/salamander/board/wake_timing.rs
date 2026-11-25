// SPDX-License-Identifier: MIT
//! Opt-in atomic observations; no mutable static, allocation or retained state.
use newt_core::wake_timing::{Stage, Timeline};
use super::MonotonicClock;

pub(super) static TIMELINE: Timeline = Timeline::new();

// SAFETY: exact native scalar ABI; calls retain no pointers or state.
unsafe extern "C" { fn nb_wake_cpu_mhz() -> u32; }

fn now() -> u32 {
    // Arduino setup has initialized the SDK timer before firmware entry.
    MonotonicClock { _local: core::marker::PhantomData }.micros()
}
/// Called once from firmware setup before acquisition. No clock setting changes.
pub fn begin() {
    let started = now();
    // SAFETY: synchronous SDK CPU-frequency query, no pointers or mutation.
    TIMELINE.begin(started, unsafe { nb_wake_cpu_mhz() });
}
pub fn serial_ready() { TIMELINE.stamp(Stage::Serial, now()); }
