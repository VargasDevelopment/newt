// SPDX-License-Identifier: MIT
//! Wi-Fi-only static memory belongs either to the SDK or to the reading heap.
//! The sole boot entry selects that ownership before any SD/input allocation.
use core::ffi::c_char;

// SAFETY: pinned linker script defines one complete aligned range and the
// deferred SDK constructor list. Native only forwards the heap registration.
unsafe extern "C" {
    static mut _newt_wifi_start: c_char;
    static mut _newt_wifi_end: c_char;
    // SAFETY: pinned deferred entries have this exact SDK constructor ABI.
    static _newt_wifi_init_start: unsafe extern "C" fn();
    // SAFETY: the linker-only end marker is never itself called or read.
    static _newt_wifi_init_end: unsafe extern "C" fn();
    fn newt_wifi_lend_memory(start: usize, end: usize, bytes: *mut i32) -> i32;
}
#[derive(Clone, Copy)]
#[repr(C)]
pub struct WifiLend { pub bytes: i32, pub error: i32 }

/// # Safety
/// Call exactly once on a reading boot, before SD mount. No Wi-Fi SDK call or
/// deferred constructor may run afterward, including through another owner.
// SAFETY: the sole image entry chooses the exclusive boot-wide memory owner.
#[inline(never)]
pub unsafe fn lend() -> WifiLend {
    let start = core::ptr::addr_of_mut!(_newt_wifi_start) as usize;
    let end = core::ptr::addr_of_mut!(_newt_wifi_end) as usize;
    let mut result = WifiLend { bytes: 0, error: 0 };
    // Have the synchronous SDK leaf report the actual extent too, so linker
    // addresses cannot escape this ownership boundary through a folded return.
    // SAFETY: caller grants this aligned unused range to IDF with the frozen
    // caps/priority; result.bytes is exclusively lent until the call returns.
    result.error = unsafe { newt_wifi_lend_memory(start, end, &mut result.bytes) };
    result
}

/// # Safety
/// Call exactly once on the selected update boot, before any station/crypto/
/// TLS SDK call. The Wi-Fi range must never have been lent to the heap.
// SAFETY: the caller owns the one early-boot choice and SDK constructor scope.
#[inline(never)]
pub unsafe fn run_constructors() {
    let mut next = core::ptr::addr_of!(_newt_wifi_init_start);
    let end = core::ptr::addr_of!(_newt_wifi_init_end);
    while next < end {
        // SAFETY: pinned linker emitted complete live function pointers; this
        // constructor is called once and the range remains SDK-owned.
        unsafe { (*next)(); next = next.add(1); }
    }
}
const _: () = assert!(core::mem::size_of::<WifiLend>() == 8);

#[cfg(feature = "dev-usb")]
pub mod report {
    //! Frozen DEV Wi-Fi lend/heap rows, one per quiet housekeeping pass.
    use super::WifiLend;
    use crate::{app_runtime::trace::row, dev_usb::Bridge, serial_console::SerialConsole};
    use core::ffi::c_void;

    // SAFETY: native projects each synchronous SDK walker callback into scalars.
    // It retains neither the context nor callback, and always completes the walk.
    unsafe extern "C" {
        fn newt_wifi_memory_report_walk(
            context: *mut c_void,
            visit: extern "C" fn(*mut c_void, usize, usize, usize, usize, u8),
        );
    }

    /// The frozen report's eight scalar bytes plus the actual eight-byte lend
    /// result. Keep this one owner in the reading UI scope, not a boot print loop.
    #[repr(C)]
    pub struct WifiLendReport {
        lend: WifiLend,
        next: u32,
        armed: bool,
        lent: bool,
        blocks: bool,
        done: bool,
    }
    impl WifiLendReport {
        pub const fn new(lend: WifiLend) -> Self {
            Self { lend, next: 0, armed: false, lent: false, blocks: false, done: false }
        }

        /// The first actually published Runtime row arms the deferred report.
        pub fn arm(&mut self) { self.armed = true; }

        pub fn publish(&mut self, log: &mut SerialConsole, bridge: &Bridge) {
            if !self.armed || self.done || bridge.output_pending() || bridge.request_pending() { return; }
            if !self.lent {
                self.lent = row(log, 80, format_args!(
                    "Newt Wi-Fi memory lent: {} bytes, error {}\n", self.lend.bytes, self.lend.error));
                return;
            }
            let mut walk = Walk::new(self.blocks, self.next);
            // SAFETY: walk is exclusively lent for this synchronous call. The
            // callback only reduces scalars while SDK heap locks are held; all
            // formatting and Serial work happens after the complete walk returns.
            unsafe { newt_wifi_memory_report_walk((&raw mut walk).cast(), visit); }
            if !walk.found {
                self.done = self.blocks;
                self.blocks = true;
                self.next = 0;
                return;
            }
            let printed = if self.blocks {
                row(log, 80, format_args!(
                    "Newt heap block {}: heap={} at=0x{:08x} size={}\n",
                    self.next, walk.heap, walk.start, walk.size))
            } else {
                row(log, 96, format_args!(
                    "Newt heap {}: at=0x{:08x} size={} free={} largest={}\n",
                    self.next, walk.start, walk.end.wrapping_sub(walk.start), walk.free, walk.largest))
            };
            if printed { self.next = self.next.wrapping_add(1); }
        }
    }

    /// Attempt-local frozen Walk: no copied heap list survives this pass.
    #[repr(C)]
    struct Walk {
        blocks: bool,
        wanted: u32,
        heaps: u32,
        seen: u32,
        heap: u32,
        found: bool,
        current: usize,
        start: usize,
        end: usize,
        free: usize,
        largest: usize,
        size: usize,
    }
    impl Walk {
        fn new(blocks: bool, wanted: u32) -> Self {
            Self { blocks, wanted, heaps: 0, seen: 0, heap: 0, found: false,
                current: 0, start: 0, end: 0, free: 0, largest: 0, size: 0 }
        }

        fn add(&mut self, start: usize, end: usize, block: usize, size: usize, used: bool) {
            if self.heaps == 0 || self.current != start {
                self.current = start;
                self.heaps = self.heaps.wrapping_add(1);
            }
            if self.blocks {
                if !used || size < 8192 { return; }
                let seen = self.seen;
                self.seen = self.seen.wrapping_add(1);
                if seen != self.wanted { return; }
                self.found = true;
                self.heap = self.heaps.wrapping_sub(1);
                self.start = block;
                self.size = size;
                return;
            }
            if self.heaps != self.wanted.wrapping_add(1) { return; }
            self.found = true;
            self.start = start;
            self.end = end;
            if used { return; }
            self.free = self.free.wrapping_add(size);
            self.largest = self.largest.max(size);
        }
    }

    extern "C" fn visit(context: *mut c_void, start: usize, end: usize,
        block: usize, size: usize, used: u8,
    ) {
        // SAFETY: publish supplies its one live Walk. Native calls sequentially
        // under each heap lock and retains no pointer; add cannot allocate, log,
        // invoke SDK work or panic on integer overflow.
        let walk = unsafe { &mut *context.cast::<Walk>() };
        walk.add(start, end, block, size, used != 0);
    }

    const _: () = assert!(core::mem::size_of::<WifiLendReport>() == 16);
    #[cfg(target_pointer_width = "32")]
    const _: () = assert!(core::mem::size_of::<Walk>() == 48);

}
