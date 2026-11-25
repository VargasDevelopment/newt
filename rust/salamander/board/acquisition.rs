// SPDX-License-Identifier: MIT
//! Panel acquisition precedes the optional update-mode branch and card mount.
use super::{Board, Error, Probe, startup::PendingInput};
use core::{ffi::c_void, ptr::NonNull};

impl<'a> Board<'a> {
    /// Begin the panel before any card mount or input initialization. The
    /// caller may enter update mode, or later mount before lending native I/O.
    ///
    /// # Safety
    /// The complete ownership and SDK startup contract of acquire_x3 applies.
    /// The SDK input remains unused until the sole PendingInput is consumed;
    /// no input queue/task or other SDK owner may be started independently.
    pub unsafe fn acquire_x3_panel_deferred_input(
        display: *mut c_void,
        input: *mut c_void,
    ) -> Result<(Self, PendingInput<'a>, Probe), Error> {
        let input = NonNull::new(input).ok_or(Error::Null)?;
        // SAFETY: caller grants the exclusive display and fixed X3 resources.
        let mut board = unsafe { Self::borrow_raw(display) }?;
        board.select_x3()?;
        board.release_sd_rail();
        let probe = board.probe_panel()?;
        #[cfg(feature = "wake-timing")]
        board.wake_stamp(newt_core::wake_timing::Stage::Probe);
        board.begin_display()?;
        #[cfg(feature = "wake-timing")]
        board.wake_stamp(newt_core::wake_timing::Stage::DisplayReady);
        Ok((board, PendingInput::from_exclusive(input), probe))
    }

    // SAFETY: the public acquisition wrappers require these same exclusive
    // SDK/display/input owners for the complete returned capability lifetime.
    pub(super) unsafe fn acquire_x3_mode(
        display: *mut c_void,
        input: *mut c_void,
        allow_missing_card: bool,
    ) -> Result<(Self, PendingInput<'a>, Probe), Error> {
        // SAFETY: the enclosing acquisition API requires the same sole owners.
        let (mut board, input, probe) = unsafe { Self::acquire_x3_panel_deferred_input(display, input) }?;
        // Only a missing/failed medium may leave startup usable. ABI/profile
        // errors still abort; no filesystem view exists before a mount succeeds.
        match board.ensure_sd_mounted() {
            Ok(()) => {}
            Err(Error::Io) if allow_missing_card => {}
            Err(error) => return Err(error),
        }
        Ok((board, input, probe))
    }
}
