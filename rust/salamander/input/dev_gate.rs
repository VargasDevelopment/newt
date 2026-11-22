// SPDX-License-Identifier: MIT
//! The same native mux and pending byte, with all takeover decisions in Rust.
use core::ffi::c_void;

// SAFETY: native calls only acquire/release the existing portMUX and lend its
// initialized pending bool. No SDK/app work or callback runs on the C++ side.
unsafe extern "C" {
    fn np_gate_enter(gate: *mut c_void) -> *mut bool;
    fn np_gate_exit(gate: *mut c_void);
}

struct Unlock(*mut c_void);
impl Drop for Unlock {
    fn drop(&mut self) {
        // SAFETY: created only after the one successful, non-fallible enter.
        // The scoped loan ended, including on host-only Rust unwinding.
        unsafe { np_gate_exit(self.0); }
    }
}

/// # Safety
/// gate is the initialized, immovable DEV gate in the still-live native input
/// owner. Both producer and sole UI use this same mux. No ISR/other writer,
/// recursive enter, gate destruction, SDK I/O, yield or task suspension inside
/// operation. The short bool loan and lock must not escape; panic=abort on X3.
pub unsafe fn with_dev_gate<R>(
    gate: *mut c_void,
    operation: impl for<'loan> FnOnce(&'loan mut bool) -> R,
) -> R {
    // SAFETY: caller establishes the same live gate and nonrecursive task use.
    let pending = unsafe { np_gate_enter(gate) };
    let _unlock = Unlock(gate);
    // SAFETY: enter owns the mux; C++ lends exactly its one initialized bool.
    // The higher-ranked callback cannot retain this exclusive loan.
    operation(unsafe { &mut *pending })
}

pub(super) fn observe(gate: *mut c_void) {
    // SAFETY: the private PollIo carrier comes only from split_initialized's
    // fixed native owner contract; its DEV gate outlives this input task.
    unsafe { with_dev_gate(gate, |pending| *pending = true); }
}
