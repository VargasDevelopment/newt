// SPDX-License-Identifier: MIT
//! One source state loan, with frozen action scratch withdrawn on every exit.
use newt_reader_ui::{ReaderSource, ReaderSourceState};
use newt_phased_reader::imported_revision::{OriginalScratch, RevisionPorts, StateLoanError};

pub(crate) fn with_source<'ports, R>(slot: &mut Option<ReaderSourceState>,
    ports: RevisionPorts<'ports>,
    operation: impl FnOnce(&mut ReaderSource<'ports>) -> R,
) -> Result<R, StateLoanError> {
    with_storage(slot, ports, None, operation)
}

pub(crate) fn with_source_lent<'ports, R>(slot: &mut Option<ReaderSourceState>,
    ports: RevisionPorts<'ports>, action: Option<&'ports mut [u8]>,
    operation: impl FnOnce(&mut ReaderSource<'ports>) -> R,
) -> Result<R, StateLoanError> {
    with_storage(slot, ports, action, operation)
}

fn with_storage<'ports, R>(slot: &mut Option<ReaderSourceState>,
    ports: RevisionPorts<'ports>, action: Option<&'ports mut [u8]>,
    operation: impl FnOnce(&mut ReaderSource<'ports>) -> R,
) -> Result<R, StateLoanError> {
    let mut returned = Returned { source: None, original: None, slot };
    initialize_source(returned.slot, ports, &mut returned.source)?;
    let Some(source) = returned.source.as_mut() else { return Err(StateLoanError::Shape); };
    if let Some(action) = action {
        returned.original = Some(source.lend_action_storage(action).ok_or(StateLoanError::Shape)?);
    }
    Ok(operation(source))
}

// End construction moves before lending the live source to the operation.
#[inline(never)]
fn initialize_source<'ports>(slot: &mut Option<ReaderSourceState>,
    ports: RevisionPorts<'ports>, out: &mut Option<ReaderSource<'ports>>,
) -> Result<(), StateLoanError> {
    let Some(state) = slot.take() else { return Err(StateLoanError::Shape); };
    match state.lend(ports) {
        Ok(source) => {
            *out = Some(source);
            Ok(())
        }
        Err((error, resulting, _ports)) => {
            *slot = Some(resulting);
            Err(error)
        }
    }
}

struct Returned<'ports, 'slot> {
    source: Option<ReaderSource<'ports>>,
    original: Option<OriginalScratch<'ports>>,
    slot: &'slot mut Option<ReaderSourceState>,
}
impl Drop for Returned<'_, '_> {
    // Retire the short view after work; keep its state move out of work's frame.
    #[inline(never)]
    fn drop(&mut self) {
        if let Some(mut source) = self.source.take() {
            if let Some(original) = self.original.take() { source.withdraw_action_storage(original); }
            *self.slot = Some(source.into_state());
        }
    }
}
