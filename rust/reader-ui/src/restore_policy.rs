//! Frozen reader restore modes, shared by the owner and pending recovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestorePolicy {
    AllowInitialFallback,
    Validated,
    ExactPage,
}
