// SPDX-License-Identifier: MIT
//! Short revision operation over the selected session's one existing state.
use newt_phased_reader::imported_revision::{ImportedRevision, RevisionPorts,
    RevisionState, StateLoanError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RevisionLoanError {
    Absent,
    Rejected(StateLoanError),
}

/// The slot belongs inside the optional session, replacing its revision view.
/// Relend its SAME live source, receipt, hashers and scratch. This wrapper adds
/// no freshness check, import, reset, promotion or storage operation.
///
/// The resulting state is returned on success, operation failure or callback
/// unwind. As with RevisionState::lend, the enclosing native owner must close
/// its real writer if admission itself unwinds or this session is discarded.
pub(crate) fn with_revision<'ports, R>(
    state: &mut Option<RevisionState>,
    ports: RevisionPorts<'ports>,
    operation: impl for<'loan> FnOnce(&'loan mut ImportedRevision<'ports>) -> R,
) -> Result<R, RevisionLoanError> {
    let original = state.take().ok_or(RevisionLoanError::Absent)?;
    let revision = match original.lend(ports) {
        Ok(revision) => revision,
        Err((reason, resulting, _ports)) => {
            *state = Some(resulting);
            return Err(RevisionLoanError::Rejected(reason));
        }
    };
    let mut loan = RevisionLoan { revision: Some(revision), returned: state };
    let Some(revision) = loan.revision.as_mut() else {
        return Err(RevisionLoanError::Absent);
    };
    let result = operation(revision);
    drop(loan);
    Ok(result)
}

// The Option permits moving the real view out of Drop without unsafe or a
// fabricated replacement revision. No old scalar snapshot is kept here.
struct RevisionLoan<'ports, 'saved> {
    revision: Option<ImportedRevision<'ports>>,
    returned: &'saved mut Option<RevisionState>,
}

impl Drop for RevisionLoan<'_, '_> {
    fn drop(&mut self) {
        if let Some(revision) = self.revision.take() {
            *self.returned = Some(revision.into_state());
        }
    }
}
