// SPDX-License-Identifier: MIT
#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

use core::{ffi::c_void, marker::PhantomData, ptr::NonNull};
use newt_input_policy::native::QuiescedInput;

/// Thin USB SDK boundary; application protocol and sole-port loans stay Rust.
pub mod dev_usb;
pub mod hardware;
pub mod grayscale;
pub mod image;
#[cfg(feature = "rust-app")]
pub mod app_frame;
pub mod retained;
pub mod retained_words;
mod mounted_io;
pub use mounted_io::MountedIo;
mod startup;
mod acquisition;
mod clock_write;
pub use startup::PendingInput;
mod selection_init;
pub mod reader_memory;
#[cfg(feature = "embedded-sample")]
pub mod sample_memory;

// The panel in native scan order (salamander-device, through Core's frame).
pub const WIDTH: usize = newt_core::frame::NATIVE_WIDTH;
pub const HEIGHT: usize = newt_core::frame::NATIVE_HEIGHT;
pub const STRIDE: usize = WIDTH / 8;
pub const FRAME_BYTES: usize = STRIDE * HEIGHT;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Null,
    Frame,
    Mode,
    Io,
    Length,
    NotBegun,
    Native(i32),
}
fn status(code: i32) -> Result<(), Error> {
    match code {
        0 => Ok(()),
        1 => Err(Error::Null),
        2 => Err(Error::Frame),
        3 => Err(Error::Mode),
        4 => Err(Error::Io),
        5 => Err(Error::Length),
        other => Err(Error::Native(other)),
    }
}
#[repr(C)]
struct Frame {
    data: *mut u8,
    len: usize,
    width: u16,
    height: u16,
    stride: u16,
}
#[repr(C)]
struct BuildStorage {
    data: *mut u8,
    len: usize,
}
// Private and never given to callbacks: safe mem::forget cannot strand a loan.
struct ReturnBuild(NonNull<c_void>);
impl Drop for ReturnBuild {
    fn drop(&mut self) {
        // SAFETY: constructed only after successful acquisition; all borrowed
        // views end before this guard, including during host Rust unwinding.
        unsafe { nb_return_build(self.0.as_ptr()) }
    }
}
#[repr(C)]
#[derive(Default, Debug)]
pub struct Probe {
    pub ver: [u8; 3],
    pub verdict: u8,
    pub valid: u8,
    pub timed_out: u8,
    pub promoted: u8,
}
#[repr(C)]
#[derive(Default, Debug)]
pub struct Boot {
    pub reset: u32,
    pub wake: u32,
    pub power_on: u32,
    pub deep_sleep_reset: u32,
}
impl Boot {
    /// Mirrored pinned IDF enum values, asserted by board.cpp when linked.
    pub const POWER_ON_RESET: u32 = 1;
    pub const SOFTWARE_RESET: u32 = 3;
    pub const GPIO_WAKE: u32 = 7;
}
/// One bounded DS3231 read, undecoded. Only `complete` makes `raw` a clock
/// value; after a failed read `raw` holds the bytes that read initialized and
/// zero after them, for Home's DEV line only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClockSnapshot {
    pub raw: [u8; 16],
    pub complete: bool,
}

// UI-only callers serialize this same Wire transaction, including a producer
// pause whose parser and SD writer are suspended before housekeeping.
fn read_clock_snapshot() -> ClockSnapshot {
    let mut raw = [0; 16];
    // SAFETY: exclusive initialized sensor bus; exact private output slice.
    let complete = status(unsafe { nb_clock_snapshot(raw.as_mut_ptr(), raw.len()) }).is_ok();
    ClockSnapshot { raw, complete }
}
#[derive(Clone, Copy)]
#[repr(u32)]
pub enum Refresh {
    Full = 0,
    Half = 1,
    Fast = 2,
}

unsafe extern "C" {
    fn nb_select_x3(display: *mut c_void) -> i32;
    fn nb_release_sd();
    fn nb_probe_panel(out: *mut Probe) -> i32;
    fn nb_mount_sd() -> i32;
    fn nb_begin(display: *mut c_void) -> i32;
    fn nb_lend_build(display: *mut c_void, out: *mut BuildStorage) -> i32;
    fn nb_return_build(display: *mut c_void);
    fn nb_frame(display: *mut c_void, out: *mut Frame) -> i32;
    fn nb_update(display: *mut c_void, mode: u32, turn_off: u32) -> i32;
    fn nb_panel_sleep(display: *mut c_void) -> i32;
    fn nb_input_begin(input: *mut c_void) -> i32;
    fn nb_millis() -> u32;
    fn nb_micros() -> u32;
    fn nb_clock_read(out: *mut u8, len: usize) -> i32;
    fn nb_clock_snapshot(out: *mut u8, len: usize) -> i32;
    fn nb_boot(out: *mut Boot) -> i32;
    fn nb_wait_power_release();
    fn nb_arm_power_wake() -> u32;
    fn nb_power_down_rails();
    fn nb_try_sleep() -> i32;
    fn nb_restart();
}

#[cfg(feature = "wake-timing")]
pub mod wake_timing;

/// Read-only SDK timer, without a Board/input/bus loan or allocation.
/// The SDK timer is live from SDK startup, before Rust entry, for program lifetime.
#[derive(Clone, Copy)]
pub struct MonotonicClock {
    _local: PhantomData<*mut ()>,
}
impl MonotonicClock {
    #[cfg(feature = "wake-timing")]
    pub fn wake_stamp(self, stage: newt_core::wake_timing::Stage) { wake_timing::TIMELINE.stamp(stage, self.micros()); }
    pub fn micros(self) -> u32 {
        // SAFETY: SDK startup initialized the program-lifetime timer before Rust entry.
        unsafe { nb_micros() }
    }

    pub fn millis(self) -> u32 {
        // SAFETY: SDK startup initialized the program-lifetime esp_timer.
        // The fixed Arduino bridge only reads that timer.
        unsafe { nb_millis() }
    }
}

/// Exclusive borrowed SDK access, with no allocation, task, destructor or policy.
/// Neither Send nor Sync: the SDK bus and ACTIVE profile are global resources.
pub struct Board<'a> {
    display: NonNull<c_void>,
    display_ready: bool,
    sd_mounted: bool,
    _exclusive: PhantomData<(&'a mut (), *mut ())>,
}
impl<'a> Board<'a> {
    /// # Safety
    /// Sole immovable display owner for 'a, plus ACTIVE, panel singletons,
    /// shared SPI/card rail and the live exclusive Wire object. The input is separately
    /// lent to InitializedInput; no task can start before startup completes.
    /// No external display/SD/Wire aliases or outstanding frame/parser loans.
    /// ACTIVE stays fixed at the selected X3 profile until all owners end.
    unsafe fn borrow_raw(display: *mut c_void) -> Result<Self, Error> {
        Ok(Self {
            display: NonNull::new(display).ok_or(Error::Null)?,
            display_ready: false,
            sd_mounted: false,
            _exclusive: PhantomData,
        })
    }
    #[cfg(feature = "wake-timing")]
    pub fn wake_timeline(&self) -> &newt_core::wake_timing::Timeline { &wake_timing::TIMELINE }
    #[cfg(feature = "wake-timing")]
    pub fn wake_stamp(&self, stage: newt_core::wake_timing::Stage) { self.monotonic_clock().wake_stamp(stage); }

    /// Whether this owner registered the card VFS. This is not evidence that
    /// the card remains present or that any write reached durable storage.
    pub fn sd_mounted(&self) -> bool {
        self.sd_mounted
    }
    /// Retry a failed mount between I/O scopes. A successful mount is never
    /// reinitialized beneath open descriptors. &mut self excludes every panel,
    /// parser and native I/O view created by with_reader_io.
    pub fn ensure_sd_mounted(&mut self) -> Result<(), Error> {
        if !self.display_ready {
            return Err(Error::NotBegun);
        }
        if !self.sd_mounted {
            // SAFETY: exclusive initialized UI SPI/rail authority. No native
            // view is published until this synchronous call succeeds.
            status(unsafe { nb_mount_sd() })?;
            self.sd_mounted = true;
        }
        Ok(())
    }
    fn select_x3(&mut self) -> Result<(), Error> {
        self.display_ready = false;
        // SAFETY: exclusive SDK object borrow established at acquisition.
        status(unsafe { nb_select_x3(self.display.as_ptr()) })
    }
    fn release_sd_rail(&mut self) {
        // SAFETY: acquisition also excludes other shared bus/rail users.
        unsafe { nb_release_sd() }
    }
    fn probe_panel(&mut self) -> Result<Probe, Error> {
        let mut out = Probe::default();
        // SAFETY: private, valid output and exclusively borrowed shared bus.
        status(unsafe { nb_probe_panel(&mut out) })?;
        Ok(out)
    }
    fn begin_display(&mut self) -> Result<(), Error> {
        self.display_ready = false;
        // SAFETY: SDK object's lifetime is covered by this exclusive borrow.
        status(unsafe { nb_begin(self.display.as_ptr()) })?;
        self.display_ready = true;
        Ok(())
    }
    /// Native row-major 1bpp bytes, MSB first; 1 white, 0 black.
    /// Borrow ends before refresh, begin, sleep, or another framebuffer loan.
    pub fn display_ready(&self) -> bool { self.display_ready }
    pub fn frame(&mut self) -> Result<&mut [u8], Error> {
        if !self.display_ready {
            return Err(Error::NotBegun);
        }
        let mut out = Frame {
            data: core::ptr::null_mut(),
            len: 0,
            width: 0,
            height: 0,
            stride: 0,
        };
        // SAFETY: output is live, disjoint from the SDK object and frame.
        status(unsafe { nb_frame(self.display.as_ptr(), &mut out) })?;
        if out.data.is_null()
            || out.len != FRAME_BYTES
            || usize::from(out.width) != WIDTH
            || usize::from(out.height) != HEIGHT
            || usize::from(out.stride) != STRIDE
        {
            return Err(Error::Frame);
        }
        // SAFETY: real bridge returns SDK allocation only, validated exact size;
        // acquisition excludes all other aliases. &mut self limits this loan.
        Ok(unsafe { core::slice::from_raw_parts_mut(out.data, out.len) })
    }
    /// Lend the actual SDK primary frame synchronously. Returning clears pixels;
    /// caller must repaint before refresh. Callback results cannot borrow bytes.
    /// Production uses panic=abort: an abort never resumes or publishes a frame.
    /// Host panic=unwind runs ReturnBuild after callback locals are destroyed;
    /// no unwind crosses C, and foreign exceptions/longjmp are unsupported.
    pub fn with_build_storage<R>(
        &mut self,
        operation: impl for<'loan> FnOnce(&'loan mut [u8]) -> R,
    ) -> Result<R, Error> {
        if !self.display_ready {
            return Err(Error::NotBegun);
        }
        let mut out = BuildStorage {
            data: core::ptr::null_mut(),
            len: 0,
        };
        // SAFETY: exclusive Board resources; &mut self excludes frame loans.
        status(unsafe { nb_lend_build(self.display.as_ptr(), &mut out) })?;
        let returned = ReturnBuild(self.display);
        if out.data.is_null() || out.len != FRAME_BYTES {
            return Err(Error::Frame); // Guard returns malformed successful loan.
        }
        // SAFETY: native bridge supplied the sole SDK allocation at exact extent;
        // the callback cannot retain a safe reference, reenter Board or forget guard.
        let result = operation(unsafe { core::slice::from_raw_parts_mut(out.data, out.len) });
        drop(returned);
        Ok(result)
    }
    pub fn update(&mut self, mode: Refresh, turn_off: bool) -> Result<(), Error> {
        if !self.display_ready {
            return Err(Error::NotBegun);
        }
        #[cfg(feature = "wake-timing")]
        wake_timing::TIMELINE.display_start(self.monotonic_clock().micros(), mode as u32);
        // SAFETY: &mut self excludes live frame loans; SDK call is blocking.
        let result = status(unsafe { nb_update(self.display.as_ptr(), mode as u32, u32::from(turn_off)) });
        #[cfg(feature = "wake-timing")]
        wake_timing::TIMELINE.display_return(self.monotonic_clock().micros());
        result
    }
    pub fn panel_sleep(&mut self) -> Result<(), Error> {
        if !self.display_ready {
            return Err(Error::NotBegun);
        }
        self.display_ready = false;
        // SAFETY: &mut self excludes live frame loans.
        status(unsafe { nb_panel_sleep(self.display.as_ptr()) })
    }
    /// Mint a scalar timer capability after successful initialized acquisition.
    pub fn monotonic_clock(&self) -> MonotonicClock {
        MonotonicClock {
            _local: PhantomData,
        }
    }
    pub fn millis(&mut self) -> u32 {
        // SAFETY: scalar Arduino clock call; wraps naturally at 32 bits.
        unsafe { nb_millis() }
    }
    pub fn micros(&mut self) -> u32 {
        // SAFETY: scalar Arduino clock call; no retained pointers.
        unsafe { nb_micros() }
    }
    pub fn boot_evidence(&mut self) -> Result<Boot, Error> {
        // SAFETY: Board acquisition establishes the same live native SDK ABI.
        unsafe { Self::boot_before_acquisition() }
    }
    /// X3 DS3231 registers 0x00..0x0f, undecoded. Rust owns plausibility/OSF
    /// checks and all UTC, zone, formatting and statistics policy.
    pub fn clock_registers(&mut self) -> Result<[u8; 16], Error> {
        let mut out = [0; 16];
        // SAFETY: exclusive initialized sensor bus; exact private output slice.
        status(unsafe { nb_clock_read(out.as_mut_ptr(), out.len()) })?;
        Ok(out)
    }
    /// The same bounded transaction for Home's clock, as the pinned bool read:
    /// every non-OK status is an incomplete snapshot, keeping the partial bytes
    /// its DEV line reports. `clock_registers` alone reports typed errors.
    pub fn clock_snapshot(&mut self) -> ClockSnapshot {
        read_clock_snapshot()
    }
    /// Poll the physical power GPIO with the pinned SDK release behavior.
    /// # Safety
    /// Scoped input exclusion is live; no other GPIO user can race this call.
    pub unsafe fn wait_power_release(&mut self, _input: &QuiescedInput<'_>) {
        unsafe { nb_wait_power_release() }
    }
    /// True means SDK found a wake pin, not that IDF confirmed arming it.
    /// # Safety
    /// Scoped input exclusion is live and all other power-pin users are stopped.
    /// A false result changes no wake configuration. After true, do not resume
    /// input; a suspended task's scope remains live until sleep/rejection restart.
    pub unsafe fn arm_power_wake_pin_available(&mut self, _input: &QuiescedInput<'_>) -> bool {
        // SAFETY: exclusive board resources acquired for this lifetime.
        unsafe { nb_arm_power_wake() != 0 }
    }
    /// # Safety
    /// Scoped input exclusion is live; all other rail/I/O users and derived
    /// storage/frame loans ended. Retain the input/native owners, but never
    /// resume them after this call; finish sleep/rejection restart in the scope.
    pub unsafe fn power_down_rails(&mut self, _input: &QuiescedInput<'_>) {
        self.display_ready = false;
        // SAFETY: exclusive board resources; no live frame borrow.
        unsafe { nb_power_down_rails() }
    }
    /// Returns raw esp_err_t only if entry rejected. No automatic restart.
    /// GPIO holds/isolation may remain configured on rejection; app must recover.
    /// # Safety
    /// Scoped input exclusion is live; all other native users stopped, durable
    /// work and I/O closed, button released and rails/panel stopped. Keep input
    /// suspended and retain every owner on rejection; stamp/restart, never resume.
    pub unsafe fn try_sleep(&mut self, _input: &QuiescedInput<'_>) -> i32 {
        self.display_ready = false;
        // SAFETY: exclusive board resources; caller contract owns eligibility.
        unsafe { nb_try_sleep() }
    }
    pub fn restart(&mut self) {
        // SAFETY: IDF owns reset; no pointers passed or policy inferred.
        unsafe { nb_restart() }
    }
}

fn reader_error(error: Error) -> newt_core::zip_container::Status {
    use newt_core::zip_container::Status;
    match error {
        Error::NotBegun | Error::Mode => Status::Unsupported,
        Error::Null | Error::Frame | Error::Io | Error::Length | Error::Native(_) => {
            Status::IoError
        }
    }
}
impl newt_phased_reader::phase_storage::BuildLoan for Board<'_> {
    fn with_parse<R>(
        &mut self,
        operation: impl for<'loan> FnOnce(&'loan mut [u8]) -> R,
    ) -> Result<R, newt_core::zip_container::Status> {
        self.with_build_storage(operation)
            .map_err(|_| newt_core::zip_container::Status::OutOfMemory)
    }
}
impl newt_reader_ui::ReaderPanelLoan for Board<'_> {
    fn with_frame<R>(
        &mut self,
        paint: impl for<'frame> FnOnce(&'frame mut newt_core::frame::Frame) -> R,
    ) -> Result<R, newt_core::zip_container::Status> {
        let frame = self.frame().map_err(reader_error)?;
        let frame = frame
            .try_into()
            .map_err(|_| newt_core::zip_container::Status::IoError)?;
        Ok(paint(frame))
    }
}

/// The sole UI panel owner. Blocking refresh and clock reads run between
/// parser/frame/SD calls; rail, startup and profile changes remain unavailable.
pub struct ReaderPanel<'scope, 'sdk> {
    board: &'scope mut Board<'sdk>,
}
impl<'sdk> Board<'sdk> {
    /// Panel RAM and synchronous native ports; input remains with its task.
    /// UiControl supplies cancellation without polling or borrowing InputManager.
    /// The enclosing Board is borrowed until all ports/descriptor owners die.
    /// Blocking refresh/Wire calls remain on ReaderPanel; no rail, startup,
    /// sleep, remount or profile access is exposed with live descriptors.
    pub fn with_reader_io<R>(
        &mut self,
        operation: impl for<'scope> FnOnce(&mut ReaderPanel<'scope, 'sdk>, &newt_native_io::UiIo) -> R,
    ) -> Result<R, Error> {
        if !self.display_ready {
            return Err(Error::NotBegun);
        }
        if !self.sd_mounted {
            return Err(Error::Io);
        }
        // SAFETY: UI retains the global display/SD/Wire exclusion. The input
        // task owns only fixed-profile ADC/GPIO and synchronized queue/control.
        // SDK lend drains display work; return is RAM-only. No task touches SPI,
        // SD, Wire or display; no hook may reenter them.
        let io = unsafe { newt_native_io::UiIo::acquire() };
        Ok(operation(&mut ReaderPanel { board: self }, &io))
    }
}
impl ReaderPanel<'_, '_> {
    /// Sequential native frame loan; return it before parsing or refreshing.
    pub fn frame(&mut self) -> Result<&mut [u8], Error> {
        self.board.frame()
    }
    pub fn millis(&self) -> u32 {
        self.board.monotonic_clock().millis()
    }
    /// Synchronous inverse-overlay drive after the frame loan has ended.
    pub fn gray_cleanup_frame(&mut self) -> Result<(), Error> {
        self.board.gray_cleanup_frame()
    }

    /// Blocking SDK refresh after all frame/parse loans have returned. Native
    /// filesystem calls have no callbacks and cannot overlap this UI call.
    pub fn update(&mut self, mode: Refresh, turn_off: bool) -> Result<(), Error> {
        self.board.update(mode, turn_off)
    }
    /// Wire uses the same UI owner between synchronous SD/display operations.
    pub fn clock_snapshot(&mut self) -> ClockSnapshot {
        self.board.clock_snapshot()
    }
}
impl newt_phased_reader::phase_storage::BuildLoan for ReaderPanel<'_, '_> {
    fn with_parse<R>(
        &mut self,
        operation: impl for<'loan> FnOnce(&'loan mut [u8]) -> R,
    ) -> Result<R, newt_core::zip_container::Status> {
        newt_phased_reader::phase_storage::BuildLoan::with_parse(self.board, operation)
    }
}
impl newt_reader_ui::ReaderPanelLoan for ReaderPanel<'_, '_> {
    fn with_frame<R>(
        &mut self,
        paint: impl for<'frame> FnOnce(&'frame mut newt_core::frame::Frame) -> R,
    ) -> Result<R, newt_core::zip_container::Status> {
        newt_reader_ui::ReaderPanelLoan::with_frame(self.board, paint)
    }
}
