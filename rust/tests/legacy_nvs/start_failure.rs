// SPDX-License-Identifier: MIT
//! Start failures through the shipped app_startup::run (#324): its real Board
//! acquisition, update-request take, deferred native owner and card mount run
//! over the existing host Board fake. Only modules past the mount, which these
//! failures never reach, are stand-ins. Usage: start_failure <panel|no-panel>.
use core::ffi::c_void;
use std::sync::{Mutex, atomic::{AtomicU32, Ordering}};
#[path = "../../firmware/app_startup.rs"] pub mod app_startup;
#[path = "../../firmware/start_failure.rs"] pub mod start_failure;
#[path = "../../firmware/update_boot.rs"] pub mod update_boot;
#[allow(dead_code)]
#[path = "../../firmware/reading_storage.rs"] mod reading_storage;
use newt_core::{app::{AppState, Screen}, frame::{self, FRAME_BYTES, Frame}, preferences::Orientation};
use newt_device_ui::{DeviceInfo, FIRMWARE_BUILD, SystemBar, render_device_ui};

/// The image's stop is IDF abort, whose panic handler restarts the device.
/// No start failure reaches it now; an abort regression would.
#[allow(dead_code)]
pub(crate) fn stop() -> ! { panic!("start failure aborted; the device would restart") }

pub mod sdk {
    pub struct Sdk552;
    impl Sdk552 {
        /// # Safety
        /// Stand-in: no native NVS is opened before these failures.
        pub unsafe fn from_exclusive_firmware_lease() -> Self { Self }
    }
}
pub mod retained_runtime {
    pub mod record { pub struct BootSnapshot { pub software_reset: bool } }
    /// # Safety
    /// Stand-in for a cold power-on; retained evidence is not under test.
    pub unsafe fn snapshot_before_board() -> Result<record::BootSnapshot, ()> {
        Ok(record::BootSnapshot { software_reset: false })
    }
}
pub mod serial_console {
    pub static LOG: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
    pub struct SerialConsole(());
    impl SerialConsole {
        /// # Safety
        /// Host log only.
        pub unsafe fn begin() -> Self { Self(()) }
    }
    impl core::fmt::Write for SerialConsole {
        fn write_str(&mut self, s: &str) -> core::fmt::Result { LOG.lock().unwrap().push_str(s); Ok(()) }
    }
}
pub mod reader_action { pub struct Owner; pub fn allocate() -> Owner { Owner } }
pub mod wifi_memory {
    pub struct WifiLend;
    /// # Safety
    /// Stand-in: lends nothing.
    pub unsafe fn lend() -> WifiLend { WifiLend }
}
pub mod app_runtime { pub mod home_refresh { pub struct HomeRefresh; } }
pub mod dev_usb { pub struct Bridge; pub struct PanelRing; }
pub mod app_image {
    use crate::{reader_action::Owner, reading_storage::ReadingStorage, retained_runtime::record::BootSnapshot,
        sdk::Sdk552, serial_console::SerialConsole, wifi_memory::WifiLend};
    use newt_c3_board::{Board, PendingInput, Probe};
    use newt_native_input_owner::{InputStart, StartFailure};
    #[allow(clippy::too_many_arguments)]
    pub fn run(_: Board<'_>, _: PendingInput<'_>, _: InputStart<'_>, _: Probe, _: SerialConsole,
        _: &mut newt_core::frame::Frame, _: BootSnapshot, _: Sdk552, _: WifiLend, _: ReadingStorage, _: Owner,
    ) -> StartFailure {
        panic!("a failed start reached the reading application")
    }
}
pub mod app_power { pub fn delay_ms(ms: u32) { crate::halted(ms) } }

#[unsafe(no_mangle)]
extern "C" fn nb_allow_reading_crash_output() {}
/// The native deferred scope runs its one body with an owner token.
#[unsafe(no_mangle)]
extern "C" fn np_owner_with_deferred(body: extern "C" fn(*mut c_void, *mut c_void) -> u32, argument: *mut c_void) -> u32 {
    static mut OWNER: u8 = 0;
    body((&raw mut OWNER).cast(), argument)
}
#[repr(C)]
struct Regions { sleep: *mut u32, sleep_words: usize, sd: *mut u32, sd_words: usize }
/// Both RTC regions start empty, as on a cold power-on.
#[unsafe(no_mangle)]
extern "C" fn nb_retained_words(out: *mut Regions) -> i32 {
    static mut SLEEP: [u32; 17] = [0; 17];
    static mut SD: [u32; 35] = [0; 35];
    // SAFETY: the Board wrapper passes one live output; regions outlive it.
    unsafe { *out = Regions { sleep: (&raw mut SLEEP).cast(), sleep_words: 17, sd: (&raw mut SD).cast(), sd_words: 35 } };
    0
}
unsafe extern "C" {
    fn checkpoint_cold_reset();
    fn fixture_fault(value: u32);
    fn fixture_updates() -> u32;
    fn fixture_mode() -> u32;
    fn statistics_frame(out: *mut u8, size: usize);
}

static CASE: Mutex<String> = Mutex::new(String::new());
static HALTS: AtomicU32 = AtomicU32::new(0);

/// Frozen fail()'s Diagnostics page with `panel` in its Display row.
fn diagnostics(panel: &[u8]) -> Box<Frame> {
    let state = AppState { screen: Screen::Device, row_open: true, ..AppState::default() };
    let mut info = DeviceInfo::default();
    info.panel = Some(panel);
    let mut logical = Box::new([0; FRAME_BYTES]);
    render_device_ui(&state, &info, &SystemBar::default(), Some(&mut logical), Default::default(),
        None, None, FIRMWARE_BUILD, Default::default(), Default::default());
    let mut native = Box::new([0; FRAME_BYTES]);
    frame::to_native_frame(&logical, &mut native, Orientation::Portrait);
    native
}

/// Each halt turn arrives here; a returning or aborting stop never does.
fn halted(ms: u32) {
    assert_eq!(ms, 1000, "frozen fail() halts in one-second delays");
    if HALTS.fetch_add(1, Ordering::Relaxed) < 2 { return; }
    let case = CASE.lock().unwrap().clone();
    let log = serial_console::LOG.lock().unwrap().clone();
    // SAFETY: host fixture counters; no frame loan is live during the halt.
    let (updates, mode) = unsafe { (fixture_updates(), fixture_mode()) };
    if case == "panel" {
        assert_eq!(log, "Newt stopped: storage unavailable\n", "start failure log names its reason");
        assert_eq!((updates, mode), (1, 0), "one full refresh shows the reason");
        let mut shown = Box::new([0; FRAME_BYTES]);
        // SAFETY: exact panel size; the fake asserts no live loan.
        unsafe { statistics_frame(shown.as_mut_ptr(), FRAME_BYTES) };
        assert!(*shown == *diagnostics(b"storage unavailable"), "panel shows the reason on Diagnostics");
        assert!(*shown != *diagnostics(b"X3 / unverified"), "the reason replaces the panel description");
    } else {
        assert_eq!(log, "Newt stopped: display buffer unavailable\n", "start failure log names its reason");
        assert_eq!(updates, 0, "an unusable display is never refreshed");
    }
    println!("PASS start failure ({case}): logged, {} and halted without restart",
        if updates == 0 { "nothing painted" } else { "reason on Diagnostics" });
    std::process::exit(0);
}

fn main() {
    let case = std::env::args().nth(1).expect("usage: start_failure <panel|no-panel>");
    let fault = match case.as_str() {
        "panel" => 25,    // the card mount fails after the panel has begun
        "no-panel" => 1,  // the panel never begins
        _ => panic!("unknown case {case}"),
    };
    *CASE.lock().unwrap() = case;
    let mut tokens = [0u8; 2];
    let [display, input] = &mut tokens;
    // SAFETY: the host Board fake owns these distinct tokens for the process;
    // all fixture state is reset before the one startup.
    unsafe {
        checkpoint_cold_reset();
        fixture_fault(fault);
        app_startup::run((display as *mut u8).cast(), (input as *mut u8).cast())
    }
}
