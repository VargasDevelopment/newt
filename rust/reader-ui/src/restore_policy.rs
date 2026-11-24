//! Frozen reader restore modes, shared by the owner and pending recovery.
// Values are also read by the wake evidence screen.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestorePolicy {
    AllowInitialFallback = 0,
    ExactPage = 1,
    Validated = 2,
    /// A cold checkpoint uses the exact page if it still exists. After
    /// repagination, the page containing the same verified locator is valid.
    ColdCheckpoint = 3,
}

impl RestorePolicy {
    pub const fn allows_initial_fallback(self) -> bool {
        match self {
            Self::AllowInitialFallback => true,
            Self::Validated | Self::ExactPage | Self::ColdCheckpoint => false,
        }
    }

    pub const fn requires_exact_page(self) -> bool {
        match self {
            Self::ExactPage => true,
            Self::AllowInitialFallback | Self::Validated | Self::ColdCheckpoint => false,
        }
    }

    pub const fn reloads_saved_progress(self) -> bool {
        match self {
            Self::AllowInitialFallback | Self::Validated => true,
            Self::ExactPage | Self::ColdCheckpoint => false,
        }
    }
}
