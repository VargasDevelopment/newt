// SPDX-License-Identifier: MIT
//! Required scoped loan of the transaction-owned legacy backend state.
use newt_core::durable::Backend;

pub trait ScopedTransaction: Backend {
    type State;
    const CLOSED: Self::State;
    /// None refuses the loan, including an already-open ordinary backend.
    /// The callback cannot retain the backend or its private native handles.
    fn with_transaction<R>(
        &mut self,
        state: &mut Self::State,
        work: impl FnOnce(&mut dyn Backend) -> R,
    ) -> Option<R>;
}
