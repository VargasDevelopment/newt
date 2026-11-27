//! Persistent state for the private port module and its operation children.
use super::{FilePort, Native, Route, Path};
use alloc::vec::Vec;

/// Persistent native ownership; no backend, route, authority or policy borrow.
/// The enclosing source owner MUST explicitly close before dropping this state.
/// Dropping an operation view never closes the descriptor or frees its names.
#[must_use = "explicitly close native state at the enclosing source boundary"]
pub struct FilePortState {
    pub(super) path: Path,
    pub(super) final_bytes: usize,
    pub(super) suffix_bytes: usize,
    pub(super) names: Option<Vec<Path>>,
    pub(super) fd: Option<i32>,
    pub(super) bytes: u32,
    pub(super) stamp: (i64, u32),
    pub(super) generation: u64,
    pub(super) serial: u64,
    pub(super) token: u64,
    pub(super) writing: bool,
    pub(super) owns: bool,
    pub(super) failed: bool,
    pub(super) synced: bool,
    pub(super) absent: bool,
    pub(super) invalid: bool,
    pub(super) deferred_fresh: bool,
    pub(super) deferred_failed: bool,
    pub(super) promoted: bool,
    pub(super) defer_depth: u8,
    pub(super) immutable_depth: u8,
}
impl FilePortState {
    pub fn new() -> Self {
        Self {
            path: Path::empty(),
            final_bytes: 0,
            suffix_bytes: 0,
            names: None,
            fd: None,
            bytes: 0,
            stamp: (0, 0),
            generation: 0,
            serial: 0,
            token: 0,
            writing: false,
            owns: false,
            failed: false,
            synced: false,
            absent: false,
            invalid: false,
            deferred_fresh: false,
            deferred_failed: false,
            promoted: false,
            defer_depth: 0,
            immutable_depth: 0,
        }
    }
    /// Name a freshly created state through the one existing operation body.
    /// No existing state enters here, so no fallible reloan validation is needed.
    pub fn new_named<S: Native>(
        native: &mut S, route: &Route<'_>, directory: &str,
        suffix: &str, receipt: bool,
    ) -> (Self, bool) {
        let mut state = Self::new();
        let named = {
            let mut view = FilePort { native, route, directory, state: &mut state };
            view.operation().name(suffix, receipt, None)
        };
        (state, named)
    }
    pub fn generation(&self) -> u64 { self.generation }
    /// One final release through the same operation body, retaining actual
    /// cleanup failure flags. No snapshot is restored on any failure.
    pub fn close<S: Native>(
        &mut self, native: &mut S, route: &Route<'_>, directory: &str,
    ) -> core::result::Result<CloseOutcome, LoanError> {
        let mut view = FilePort::lend(self, native, route, directory)?;
        let mut operation = view.operation();
        let generation = operation.release();
        Ok(CloseOutcome { generation, clean: operation.released_cleanly(generation) })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoanError { SourceDomain }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloseOutcome { pub generation: u64, pub clean: bool }
