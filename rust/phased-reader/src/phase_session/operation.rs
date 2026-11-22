// SPDX-License-Identifier: MIT
//! Construct and discard an operation loan without changing the owner.
use super::*;

impl<'op, 'sel, 'plan, 'bank, B: ParserBackend + ?Sized, L: BuildLoan>
    OperationPhased<'op, 'sel, 'plan, 'bank, B, L>
{
    pub fn new(
        reader: &'op mut PhasedReader<'sel, 'plan>,
        storage: &'op mut Storage<'bank>,
        lender: &'op mut L,
        backend: &'op mut B,
        breaker: &'op mut dyn LineBreak,
    ) -> Self {
        Self {
            reader,
            storage,
            lender,
            backend,
            breaker,
            staged: None,
        }
    }
    pub fn discard_candidate(&mut self, status: Status) -> Status {
        self.staged = None;
        self.reader.failed(status)
    }
}
