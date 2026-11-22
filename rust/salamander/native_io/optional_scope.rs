// SPDX-License-Identifier: MIT
//! Stable UI-domain permission when the actual FAT VFS may be absent.
use super::{UiIo, PhantomData};
impl UiIo {
    /// # Safety
    /// The sole initialized X3 UI domain is exclusive and stable throughout
    /// the loan. FAT may be mounted or absent; no caller mounts, unmounts or
    /// powers it down during a native call. An absent VFS fails through actual
    /// POSIX calls. Keep the same task/bus/reentry exclusions as acquire.
    /// This performs no mount and fabricates no file or medium result.
    // SAFETY: the caller owns the initialized UI domain and optional FAT scope.
    pub unsafe fn acquire_optional_fat() -> Self {
        Self { _ui: PhantomData }
    }
}
