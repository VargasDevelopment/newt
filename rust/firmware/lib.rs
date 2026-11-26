// SPDX-License-Identifier: MIT
//! Rust application image. Native code supplies only SDK and hardware ports.
#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]
extern crate alloc;
extern crate newt_nvs_guard;

#[global_allocator]
static HEAP: newt_sdk_heap::SdkHeap = newt_sdk_heap::SdkHeap;

#[path = "nvs/nvs.rs"]
pub mod nvs;
#[path = "nvs/sdk.rs"]
pub mod sdk;
#[path = "fonts/native.rs"]
pub mod fonts;
pub mod board_clock;
pub mod home_startup;
pub mod retained_runtime;
pub mod quick_start_owner;
pub mod quick_start_ports;
pub mod rest_app;
pub mod sample_scene;
#[cfg(feature = "embedded-sample")]
pub mod sample_epub;
pub mod device_lifecycle;
pub mod app_runtime;
pub mod reader_book;
pub mod reader_image;
mod reader_restore_policy;
pub mod reader_exit_metadata;
pub mod reader_metadata;
pub mod reader_duty;
pub mod reader_action;
mod reader_selected_source;
pub mod reader_pages;
pub mod reading_state_medium;
pub mod sd_revision;
pub mod reader_hash;
pub mod reader_linebreak;
pub mod reader_verification;
pub mod stats_clock;
pub mod no_card;
pub mod serial_console;
pub mod dev_usb;
pub mod app_dev;
pub mod app_power;
pub mod firmware_switch;
pub mod library_covers;
pub mod runtime_lows;
#[cfg(feature = "dev-usb")]
pub mod usb_link_row;

// SAFETY: IDF abort and the image-local crash-output setter use these exact ABIs.
unsafe extern "C" {
    fn abort() -> !;
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    stop()
}

pub(crate) fn stop() -> ! {
    // SAFETY: IDF abort never returns and takes no borrowed memory.
    unsafe { abort() }
}

pub mod app_image;
mod reading_storage;
pub mod wifi_secrets;
pub mod update_boot;
pub mod update_entry;
#[cfg(feature = "wifi-update")]
pub mod update_runtime;
#[cfg(feature = "wifi-update")]
pub mod update_radio;
#[cfg(feature = "wifi-update")]
pub mod update_native;
#[cfg(feature = "wifi-update")]
pub mod update_keys;
#[cfg(feature = "wifi-update")]
pub mod update_ui;
#[cfg(feature = "wifi-update")]
pub mod update_state;
#[cfg(feature = "wifi-update")]
pub mod update_install;
#[cfg(feature = "wifi-update")]
pub mod update_mode;
#[cfg(feature = "wifi-update")]
pub mod update_download;
#[cfg(feature = "wifi-update")]
pub mod update_network_clock;
#[cfg(feature = "wifi-update")]
pub mod update_https;
#[cfg(feature = "wifi-update")]
pub mod update_online_runtime;
pub mod app_startup;
pub mod start_failure;
pub mod wifi_memory;

/// # Safety
/// Called once by the fixed X3 SDK entry, before input starts. Both native
/// objects and all SDK globals remain exclusively lent until reset.
// SAFETY: the native entry declares this exact ABI and owns both SDK objects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newt_rust_app_run(display: *mut core::ffi::c_void, input: *mut core::ffi::c_void) {
    // SAFETY: same one-shot SDK/global lease; the concrete update operations
    // reborrow their final pinned secret fields and never move the filled view.
    unsafe { app_startup::run(display, input) }
}

// SAFETY: the SDK calls this exact callback before setup, without borrowed state.
#[unsafe(export_name = "verifyRollbackLater")]
pub extern "C" fn verify_rollback_later() -> bool { true }

// Actual SDK-backed transaction layout in this RV32 application graph.
#[cfg(target_pointer_width = "32")]
const _: () = {
    type Port = newt_native_io::OsNative<'static>;
    type Medium = reading_state_medium::MountedStateMedium<'static, 'static, 'static, 'static, Port>;
    type Store = newt_native_storage::reading_state::ReadingStateRouter<'static,
        nvs::NvsBlobBackend<'static, sdk::Sdk552>, Port, Medium>;
    assert!(Store::ACTIVE_BYTES == 112);
    assert!(Store::ACTIVE_ALIGN == 4);
    assert!(core::mem::size_of::<nvs::TransactionState>() == 8);
};
