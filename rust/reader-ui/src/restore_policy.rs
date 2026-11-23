//! Frozen reader restore modes, shared by the owner and pending recovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestorePolicy {
    AllowInitialFallback,
    Validated,
    ExactPage,
    /// An acknowledged cold-boot checkpoint can cross a firmware update.
    /// If its exact page start moved, keep the same validated content locator.
    ColdCheckpoint,
}
