// SPDX-License-Identifier: MIT
//! Boot UI's mounted native domain. Descriptor owners stay in the caller;
//! this permission has no pointer, backing, allocation, or native operation.
use super::{Board, Error};
use core::marker::PhantomData;
use newt_native_io::UiIo;

/// A zero-size SDK permission, not a file or a second mounted filesystem.
/// It cannot be cloned or sent to another task. All descriptor loans end
/// inside with_io, including when the application returns for power shutdown.
pub struct MountedIo<'sdk> {
    io: UiIo,
    _sdk: PhantomData<&'sdk mut ()>,
}
impl MountedIo<'_> {
    pub fn with_io<R>(&self, operation: impl for<'io> FnOnce(&'io UiIo) -> R) -> R {
        operation(&self.io)
    }
}

impl<'sdk> Board<'sdk> {
    /// Update boot's raw POSIX scope, after its real state-store mount attempt.
    /// A missing card still reaches actual libc open/mkdir errors; this must
    /// not retry SD mount or mint a successful file result for the caller.
    pub fn with_optional_sd_io<R>(&mut self,
        work: impl for<'io> FnOnce(&mut Self, &'io UiIo) -> R,
    ) -> R {
        // SAFETY: the sole Board grants the initialized UI domain. Both loans
        // remain in this synchronous scope; no frame/parser operation overlaps
        // a native call, and unsafe rail/unmount operations require IO closed.
        let io = unsafe { UiIo::acquire_optional_fat() };
        work(self, &io)
    }
    /// Keep selected-source ownership across Home without retaining a panel,
    /// frame or parser loan. Calls on all views are sequential on the same UI
    /// task; the existing unsafe rail/shutdown operations require them ended.
    pub fn mounted_io(&mut self) -> Result<MountedIo<'sdk>, Error> {
        self.ensure_sd_mounted()?;
        // SAFETY: Board acquisition grants the fixed X3 UI SDK domain. The
        // mount succeeded and no competing task/IRQ/bus hook exists. All native
        // calls remain synchronous; frame/parser loans end before UI operations
        // and every descriptor/view must end before unsafe power/profile work.
        let io = unsafe { UiIo::acquire() };
        Ok(MountedIo { io, _sdk: PhantomData })
    }
}
const _: () = assert!(core::mem::size_of::<MountedIo<'static>>() == 0);
