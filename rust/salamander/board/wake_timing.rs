// SPDX-License-Identifier: MIT
//! One opt-in, awake-only buffer under the board's exclusive UI-task lease.
use newt_core::wake_timing::{Stage, Timeline};

struct Slot(Timeline);
// SAFETY: only begin/serial_ready may access before acquisition (both unsafe,
// one-shot entry contracts). Thereafter only the !Send/!Sync Board and its
// UI-local clock expose references. No ISR or input task accesses this slot.
unsafe impl Sync for Slot {}
static SLOT: Slot = Slot(Timeline::new());

// SAFETY: exact native scalar ABI; calls retain no pointers or state.
unsafe extern "C" { fn nb_wake_cpu_mhz() -> u32; }

/// # Safety
/// Once, from firmware setup with the SDK timer live, before any Board loan.
pub unsafe fn begin() {
    // SAFETY: setup runs after the SDK initialized its scalar clocks.
    let now = unsafe { super::nb_micros() };
    // SAFETY: synchronous scalar read; no CPU clock or power setting changes.
    SLOT.0.begin(now, unsafe { nb_wake_cpu_mhz() });
}
/// # Safety
/// Same entry task as begin, before acquisition creates a Board/clock loan.
pub unsafe fn serial_ready() {
    // SAFETY: the entry contract establishes the SDK clock lifetime.
    SLOT.0.stamp(Stage::Serial, unsafe { super::nb_micros() });
}
/// # Safety
/// Only the exclusive Board acquisition can create these UI-local references.
pub(super) unsafe fn lend() -> &'static Timeline { &SLOT.0 }
