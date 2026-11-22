// SPDX-License-Identifier: MIT
//! UI's short pending-state loan, without a C++ Bridge or callback dispatch.
use super::DevGate;

impl DevGate<'_> {
    /// Serialize only takeover/consume decisions. Connection and timer reads
    /// happen before entry; no USB, paint, filesystem or yield belongs here.
    pub fn with_pending<R>(&mut self, operation: impl for<'loan> FnOnce(&'loan mut bool) -> R) -> R {
        // SAFETY: DevGate is minted only from this still-live native owner;
        // its pointer and gate remain immovable until the nonreturning app ends.
        let gate = unsafe { super::np_owner_gate(self.owner) };
        // SAFETY: sole UI task, paired with the physical producer's same mux.
        // The callback's short pending loan cannot escape its critical section.
        unsafe { newt_input_policy::native::with_dev_gate(gate, operation) }
    }
}
