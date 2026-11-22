// SPDX-License-Identifier: MIT
//! Separate-update-boot SDK calls. No station, task or secret owner is retained.
use newt_c3_board::{Board, MonotonicClock};
use newt_core::wifi_network_store::workflow::Cipher;
use crate::{update_radio::Ports, update_runtime::{EntropyPorts, FlashFence}};

// SAFETY: linked wifi.cpp uses the pinned SDK ABI, borrows exact initialized
// extents synchronously, never retains these byte loans, and returns raw status.
unsafe extern "C" {
    fn newt_wifi_fence(raised: u8);
    fn newt_wifi_fenced() -> u8;
    fn newt_wifi_refusals(out: *mut u8);
    fn newt_wifi_silence_log();
    fn newt_wifi_create_loop() -> i32;
    fn newt_wifi_delete_loop() -> i32;
    fn newt_wifi_init_no_nvs() -> i32;
    fn newt_wifi_usb_bbpll();
    fn newt_wifi_storage_ram() -> i32;
    fn newt_wifi_mode_station() -> i32;
    fn newt_wifi_start() -> i32;
    fn newt_wifi_scan() -> i32;
    fn newt_wifi_scan_count(count: *mut u16) -> i32;
    fn newt_wifi_scan_record(name: *mut u8, rssi: *mut i32, open: *mut u8) -> i32;
    fn newt_wifi_clear_ap_list() -> i32;
    fn newt_wifi_radio_draw(fresh: *mut u8);
    fn newt_wifi_random_word() -> u32;
    fn newt_wifi_stop() -> i32;
    fn newt_wifi_deinit() -> i32;
    fn newt_wifi_clear_config() -> i32;
    fn newt_wifi_sha256(input: *const u8, bytes: usize, output: *mut u8) -> i32;
    fn newt_wifi_gcm_seal(key: *const u8, nonce: *const u8, header: *const u8,
        plain: *const u8, cipher: *mut u8, tag: *mut u8) -> i32;
    fn newt_wifi_gcm_open(key: *const u8, nonce: *const u8, header: *const u8,
        cipher: *const u8, tag: *const u8, plain: *mut u8) -> i32;
}

pub struct Sdk { clock: MonotonicClock }
impl Sdk {
    /// # Safety
    /// Call once on the selected update boot after its skipped Wi-Fi
    /// constructors run. Wi-Fi memory must remain SDK-owned until reset;
    /// no other application may call the station SDK during this lease.
    // SAFETY: the caller supplies the unique update-boot SDK/global lease.
    pub unsafe fn for_update_boot(board: &Board<'_>) -> Self { Self { clock: board.monotonic_clock() } }
    pub fn cipher(&self) -> Gcm { Gcm { _ui: core::marker::PhantomData } }
    pub fn clear_config(&mut self) -> i32 {
        // SAFETY: synchronous SDK empty-config call; retains no Rust storage.
        unsafe { newt_wifi_clear_config() }
    }
}
macro_rules! status_call {
    ($method:ident, $native:ident) => {
        fn $method(&mut self) -> i32 {
            // SAFETY: one initialized SDK operation, no Rust pointer crosses.
            unsafe { $native() }
        }
    };
}
impl Ports for Sdk {
    fn millis(&self) -> u32 { self.clock.millis() }
    fn fence_flash_for_wifi(&mut self, raised: bool) {
        // SAFETY: existing serialized NVS guard, no reference crosses its leaf.
        unsafe { newt_wifi_fence(u8::from(raised)); }
    }
    fn silence_wifi_log(&mut self) {
        // SAFETY: SDK scalar log setting; no private text is passed or logged.
        unsafe { newt_wifi_silence_log(); }
    }
    status_call!(create_default_event_loop, newt_wifi_create_loop);
    status_call!(init_station_without_nvs, newt_wifi_init_no_nvs);
    fn enable_usb_bbpll(&mut self) {
        // SAFETY: pinned ESP32-C3 USB workaround, one scalar SDK call.
        unsafe { newt_wifi_usb_bbpll(); }
    }
    status_call!(set_storage_ram, newt_wifi_storage_ram);
    status_call!(set_station_mode, newt_wifi_mode_station);
    status_call!(start_station, newt_wifi_start);
    status_call!(scan_blocking, newt_wifi_scan);
    fn scan_count(&mut self, count: &mut u16) -> i32 {
        // SAFETY: exclusive initialized exact SDK count output.
        unsafe { newt_wifi_scan_count(count) }
    }
    fn scan_record(&mut self, name: &mut [u8; 33], rssi: &mut i32, open: &mut bool) -> i32 {
        let mut raw_open = u8::from(*open);
        // SAFETY: exact SDK-projected SSID/rssi/auth extents; complete outputs
        // only on SDK success and no references retained by this synchronous call.
        let result = unsafe { newt_wifi_scan_record(name.as_mut_ptr(), rssi, &mut raw_open) };
        if result == 0 { *open = raw_open != 0; }
        result
    }
    status_call!(clear_ap_list, newt_wifi_clear_ap_list);
    fn radio_draw(&mut self, fresh: &mut [u8; 32]) {
        // SAFETY: exact initialized 32-byte output; caller starts radio first.
        unsafe { newt_wifi_radio_draw(fresh.as_mut_ptr()); }
    }
    status_call!(stop_station, newt_wifi_stop);
    status_call!(deinit_station, newt_wifi_deinit);
    fn flash_fence_refusals(&self) -> (u32, u32) {
        let mut counts = [0; 2];
        // SAFETY: same serialized guard returns its two saturating byte counts.
        unsafe { newt_wifi_refusals(counts.as_mut_ptr()); }
        (u32::from(counts[0]), u32::from(counts[1]))
    }
    status_call!(delete_default_event_loop, newt_wifi_delete_loop);
}
impl FlashFence for Sdk {
    fn flash_fenced_for_wifi(&self) -> bool {
        // SAFETY: read-only serialized leaf of the existing NVS guard.
        unsafe { newt_wifi_fenced() != 0 }
    }
}
impl EntropyPorts for Sdk {
    fn random_word(&mut self) -> u32 {
        // SAFETY: SDK scalar RNG; seed authority is separately enforced by Core.
        unsafe { newt_wifi_random_word() }
    }
    fn sha256(&mut self, input: &[u8], output: &mut [u8; 32]) -> bool {
        // SAFETY: exact readable input and exclusive digest, one-shot SDK call.
        unsafe { newt_wifi_sha256(input.as_ptr(), input.len(), output.as_mut_ptr()) == 0 }
    }
}
pub struct Gcm { _ui: core::marker::PhantomData<*mut ()> }
impl Cipher for Gcm {
    fn seal(&mut self, key: &[u8; 32], nonce: &[u8; 12], header: &[u8; 12],
        plain: &[u8; 98], cipher: &mut [u8; 98], tag: &mut [u8; 16],
    ) -> bool {
        // SAFETY: fixed reviewed GCM extents are disjoint typed loans. Native
        // frees/zeroizes its sole SDK key context before returning, even on error.
        unsafe { newt_wifi_gcm_seal(key.as_ptr(), nonce.as_ptr(), header.as_ptr(),
            plain.as_ptr(), cipher.as_mut_ptr(), tag.as_mut_ptr()) == 0 }
    }
    fn open(&mut self, key: &[u8; 32], nonce: &[u8; 12], header: &[u8; 12],
        cipher: &[u8; 98], tag: &[u8; 16], plain: &mut [u8; 98],
    ) -> bool {
        // SAFETY: same exact authenticated SDK operation and key-context cleanup.
        unsafe { newt_wifi_gcm_open(key.as_ptr(), nonce.as_ptr(), header.as_ptr(),
            cipher.as_ptr(), tag.as_ptr(), plain.as_mut_ptr()) == 0 }
    }
}

use core::{ffi::c_void, mem::{ManuallyDrop, MaybeUninit}, ptr::NonNull};
use crate::update_radio::JoinPorts;

// SAFETY: wifi.cpp constructs the actual config/events on its stack, invokes
// the callback exactly once, then destroys them after the Rust guard closes.
// Each other function uses only this scope's opaque SDK payload synchronously.
unsafe extern "C" {
    // SAFETY: native invokes this exact callback once within its live scope.
    fn newt_wifi_with_join(context: *mut c_void, callback: unsafe extern "C" fn(*mut c_void, *mut c_void));
    fn newt_wifi_netif_init() -> i32;
    fn newt_wifi_join_station(scope: *mut c_void) -> u8;
    fn newt_wifi_join_register_wifi(scope: *mut c_void) -> i32;
    fn newt_wifi_join_register_ip(scope: *mut c_void) -> i32;
    fn newt_wifi_join_prepare(scope: *mut c_void, name: *const u8, password: *const u8, size: usize);
    fn newt_wifi_join_set_config(scope: *mut c_void) -> i32;
    fn newt_wifi_join_wipe_config(scope: *mut c_void);
    fn newt_wifi_connect() -> i32;
    fn newt_wifi_join_addressed(scope: *mut c_void) -> u8;
    fn newt_wifi_join_reason(scope: *mut c_void) -> u16;
    fn newt_wifi_join_connected(scope: *mut c_void) -> u8;
    fn newt_wifi_disconnect() -> i32;
    fn newt_wifi_join_destroy_station(scope: *mut c_void);
    fn newt_wifi_join_unregister_ip() -> i32;
    fn newt_wifi_join_unregister_wifi() -> i32;
}

/// No native owner is duplicated. The three guard bits only protect callbacks
/// and unwiped config if a caller leaves before the normal Rust teardown.
pub struct JoinScope<'scope> {
    sdk: &'scope mut Sdk,
    native: NonNull<c_void>,
    registered: u8,
    config_live: bool,
}
struct Call<'a, F, R> { sdk: &'a mut Sdk, work: ManuallyDrop<F>, output: MaybeUninit<R> }

// SAFETY: only newt_wifi_with_join calls this, once with its live payload and
// the matching Call<F,R>; neither pointer escapes and R cannot borrow the scope.
unsafe extern "C" fn invoke<F, R>(call: *mut c_void, native: *mut c_void)
where F: for<'scope> FnOnce(&mut JoinScope<'scope>) -> R {
    // SAFETY: the synchronous native call retains this exact live Call type.
    let call = unsafe { &mut *call.cast::<Call<'_, F, R>>() };
    // SAFETY: the native payload is a non-null local that outlives this callback.
    let native = unsafe { NonNull::new_unchecked(native) };
    let mut scope = JoinScope { sdk: call.sdk, native, registered: 0, config_live: false };
    // SAFETY: native invokes once; taking the FnOnce leaves no second drop/use.
    let work = unsafe { ManuallyDrop::take(&mut call.work) };
    call.output.write(work(&mut scope));
}
impl Sdk {
    pub fn with_join<R, F>(&mut self, work: F) -> R
    where F: for<'scope> FnOnce(&mut JoinScope<'scope>) -> R {
        let mut call = Call { sdk: self, work: ManuallyDrop::new(work), output: MaybeUninit::uninit() };
        // SAFETY: native calls the monomorphized callback exactly once and does
        // not retain Call or its SDK loan beyond return.
        unsafe { newt_wifi_with_join((&mut call as *mut Call<'_, F, R>).cast(), invoke::<F, R>); }
        // SAFETY: the sole native callback wrote R before returning normally.
        unsafe { call.output.assume_init() }
    }
}
macro_rules! delegate {
    ($($name:ident($($arg:ident: $ty:ty),*) $(-> $result:ty)?; )*) => { $(
        fn $name(&mut self, $($arg: $ty),*) $(-> $result)? { self.sdk.$name($($arg),*) }
    )* };
}
impl Ports for JoinScope<'_> {
    fn millis(&self) -> u32 { self.sdk.millis() }
    fn flash_fence_refusals(&self) -> (u32, u32) { self.sdk.flash_fence_refusals() }
    delegate! {
        fence_flash_for_wifi(raised: bool);
        silence_wifi_log();
        create_default_event_loop() -> i32;
        init_station_without_nvs() -> i32;
        enable_usb_bbpll();
        set_storage_ram() -> i32;
        set_station_mode() -> i32;
        start_station() -> i32;
        scan_blocking() -> i32;
        scan_count(count: &mut u16) -> i32;
        scan_record(name: &mut [u8; 33], rssi: &mut i32, open: &mut bool) -> i32;
        clear_ap_list() -> i32;
        radio_draw(fresh: &mut [u8; 32]);
        stop_station() -> i32;
        deinit_station() -> i32;
    }
    fn delete_default_event_loop(&mut self) -> i32 {
        let result = self.sdk.delete_default_event_loop();
        if result == 0 { self.registered = 0; }
        result
    }
}
macro_rules! status {
    ($name:ident, $native:ident) => {
        fn $name(&mut self) -> i32 {
            // SAFETY: one synchronous SDK step without any borrowed pointer.
            unsafe { $native() }
        }
    };
}
impl JoinPorts for JoinScope<'_> {
    status!(netif_init, newt_wifi_netif_init);
    status!(connect_station, newt_wifi_connect);
    status!(disconnect_station, newt_wifi_disconnect);
    fn create_default_station(&mut self) -> bool {
        // SAFETY: only this live native context owns the resulting SDK station.
        unsafe { newt_wifi_join_station(self.native.as_ptr()) != 0 }
    }
    fn register_wifi_observer(&mut self) -> i32 {
        // SAFETY: Drop unregisters this context's actual handler before expiry.
        let result = unsafe { newt_wifi_join_register_wifi(self.native.as_ptr()) };
        if result == 0 { self.registered |= 1; }
        result
    }
    fn register_ip_observer(&mut self) -> i32 {
        // SAFETY: same lexical registration guarantee for the IP callback.
        let result = unsafe { newt_wifi_join_register_ip(self.native.as_ptr()) };
        if result == 0 { self.registered |= 2; }
        result
    }
    fn prepare_config(&mut self, name: &[u8; 33], password: &[u8; 64], size: usize) {
        if size > password.len() || !name.contains(&0) { crate::stop(); }
        // SAFETY: validated terminated SSID and exact bounded password loan;
        // SDK payload receives a copy and exports no reference to these inputs.
        unsafe { newt_wifi_join_prepare(self.native.as_ptr(), name.as_ptr(), password.as_ptr(), size); }
        self.config_live = true;
    }
    fn set_prepared_config(&mut self) -> i32 {
        // SAFETY: the same native config remains initialized throughout scope.
        unsafe { newt_wifi_join_set_config(self.native.as_ptr()) }
    }
    fn wipe_prepared_config(&mut self) {
        // SAFETY: native volatile clearing covers only this owned SDK config.
        unsafe { newt_wifi_join_wipe_config(self.native.as_ptr()); }
        self.config_live = false;
    }
    fn addressed(&self) -> bool {
        // SAFETY: native atomic read; no Rust reference aliases its event task.
        unsafe { newt_wifi_join_addressed(self.native.as_ptr()) != 0 }
    }
    fn disconnect_reason(&self) -> u16 {
        // SAFETY: same native atomic load of the actual disconnect reason.
        unsafe { newt_wifi_join_reason(self.native.as_ptr()) }
    }
    fn connected(&self) -> bool {
        // SAFETY: same native atomic load of the actual connected observation.
        unsafe { newt_wifi_join_connected(self.native.as_ptr()) != 0 }
    }
    fn destroy_default_station(&mut self) {
        // SAFETY: consumes only this context's station; native clears its slot.
        unsafe { newt_wifi_join_destroy_station(self.native.as_ptr()); }
    }
    fn unregister_ip_observer(&mut self) -> i32 {
        // SAFETY: SDK synchronizes removal of the exact registered callback.
        let result = unsafe { newt_wifi_join_unregister_ip() };
        if result == 0 { self.registered &= !2; }
        result
    }
    fn unregister_wifi_observer(&mut self) -> i32 {
        // SAFETY: same SDK synchronization for the Wi-Fi callback.
        let result = unsafe { newt_wifi_join_unregister_wifi() };
        if result == 0 { self.registered &= !1; }
        result
    }
}
impl Drop for JoinScope<'_> {
    fn drop(&mut self) {
        if self.config_live { self.wipe_prepared_config(); }
        if self.registered & 2 != 0 { self.unregister_ip_observer(); }
        if self.registered & 1 != 0 { self.unregister_wifi_observer(); }
        // A failed removal cannot allow the SDK to access an expired stack.
        if self.registered != 0 { crate::stop(); }
    }
}
