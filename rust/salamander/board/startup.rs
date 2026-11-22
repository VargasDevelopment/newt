// SPDX-License-Identifier: MIT
//! Board startup and the single input token, before the poll task exists.
use super::{Board, Boot, Error, Probe, nb_boot, nb_input_begin, status};
use core::{ffi::c_void, marker::PhantomData, ptr::NonNull};
use newt_input_policy::native::InitializedInput;

/// The existing SDK InputManager before begin, not a second owner or task.
/// Consuming it establishes the initialized-input capability exactly once.
pub struct PendingInput<'sdk> {
    input: NonNull<c_void>,
    _exclusive: PhantomData<(&'sdk mut (), *mut ())>,
}
impl<'sdk> PendingInput<'sdk> {
    pub(super) fn from_exclusive(input: NonNull<c_void>) -> Self {
        Self { input, _exclusive: PhantomData }
    }
    pub fn begin(self) -> Result<InitializedInput<'sdk>, Error> {
        // SAFETY: acquisition grants the sole live SDK input; no input task or
        // aliases exist, and ACTIVE was finalized by the display probe.
        status(unsafe { nb_input_begin(self.input.as_ptr()) })?;
        // SAFETY: begin succeeded under the same fixed-X3/no-hook contract.
        unsafe { InitializedInput::from_initialized_x3(self.input.as_ptr()) }.ok_or(Error::Null)
    }
}

impl<'a> Board<'a> {
    /// Snapshot reset/wake evidence before display/input startup, as frozen does.
    ///
    /// # Safety
    /// The native SDK is initialized for this boot and supplies the pinned
    /// synchronous nb_boot ABI. It retains no pointer and cannot unwind.
    pub unsafe fn boot_before_acquisition() -> Result<Boot, Error> {
        let mut out = Boot::default();
        // SAFETY: private initialized output; the caller grants the native ABI.
        status(unsafe { nb_boot(&mut out) })?;
        Ok(out)
    }

    /// Acquire separate UI and initialized input capabilities after X3 startup.
    /// # Safety
    /// Distinct live aligned FreeInkDisplay/InputManager objects remain immovable
    /// for 'a. Sole acquisition of both objects, ACTIVE, singletons, SPI/card rail,
    /// buffers and initialized Wire; no mount users, hooks/reentry or async SDK
    /// input/display work. SD is unmounted on entry. No foreign unwind.
    /// InputManager's hook is its SDK default null and is never installed/mutated
    /// while either owner exists. Compile board.cpp, input_bridge.cpp AND SDK
    /// InputManager.cpp with the same X3-only, FREEINK_CAP_TOUCH=0 flags.
    /// After publication only InitializedInput/TaskPorts accesses InputManager,
    /// ADC/GPIO input configuration; UI owns display/SD/Wire. No other ADC reads
    /// or ADC reconfiguration until input task death; X3 gauge uses Wire instead.
    /// SDK-global esp_timer is initialized before acquisition and stays valid
    /// for program lifetime, including after Board and its scalar clock separate.
    /// The one Y SDK BatteryMonitor also lives until reset. HardwareReader UI
    /// views call synchronously: no competing task/IRQ/reentrant sensor user,
    /// and all views retire before committed power shutdown.
    /// The two native retained regions also belong exclusively to this Board:
    /// no other task, interrupt or native RTC alias may coexist.
    /// No native alias remains usable even if either Rust owner is forgotten.
    /// Recovery of partial startup, task lifetime and shutdown remain caller
    /// obligations. This split grants no cross-task Send/Sync or task creation.
    pub unsafe fn acquire_x3(
        display: *mut c_void,
        input: *mut c_void,
    ) -> Result<(Self, newt_input_policy::native::InitializedInput<'a>, Probe), Error> {
        // SAFETY: the caller supplies the acquisition contract above.
        let (board, input, probe) = unsafe { Self::acquire_x3_mode(display, input, false) }?;
        Ok((board, input.begin()?, probe))
    }
    /// Initialize Home and input even when no card mounts. This does not lend
    /// filesystem authority: with_reader_io rejects until ensure_sd_mounted
    /// succeeds. A successful mount is cached; failed attempts may be retried.
    /// # Safety
    /// The complete ownership and startup contract of acquire_x3 applies.
    pub unsafe fn acquire_x3_optional_sd(
        display: *mut c_void,
        input: *mut c_void,
    ) -> Result<(Self, newt_input_policy::native::InitializedInput<'a>, Probe), Error> {
        // SAFETY: same caller contract; only an SD IO failure is recoverable.
        let (board, input, probe) = unsafe { Self::acquire_x3_mode(display, input, true) }?;
        Ok((board, input.begin()?, probe))
    }
    /// Y restores durable app/reader choices before the frozen input-begin point.
    /// The pending token holds one pointer, like InitializedInput, and allocates nothing.
    ///
    /// # Safety
    /// The acquire_x3 ownership contract applies. Wire initialization alone may
    /// wait for the frozen battery read, before the first RTC operation; display
    /// and card startup do not touch it. The SDK input must remain unused
    /// until PendingInput::begin consumes this sole token; no task starts earlier.
    pub unsafe fn acquire_x3_optional_sd_deferred_input(
        display: *mut c_void,
        input: *mut c_void,
    ) -> Result<(Self, PendingInput<'a>, Probe), Error> {
        // SAFETY: identical acquisition contract; input begin alone is deferred.
        unsafe { Self::acquire_x3_mode(display, input, true) }
    }
}
