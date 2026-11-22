// SPDX-License-Identifier: MIT
//! Frozen update_mode_runtime.h's single in-frame TLS heap and allocator pair.
use core::{ffi::c_void, ptr};
use newt_core::frame::{Frame, FRAME_BYTES};
use super::native;

/// No acquisition: callbacks run only inside Https's exclusive BSS-frame epoch.
fn frame() -> *mut u8 {
    // SAFETY: Https validates the same fixed BSS owner before installing these
    // callbacks and never returns its exclusive loan until they are removed.
    unsafe { newt_c3_board::app_frame::logical_address().cast::<u8>() }
}
fn header(frame: *mut u8) -> *mut c_void {
    let offset = frame.align_offset(16);
    // SAFETY: the frozen 52,272-byte frame has room for its first aligned header;
    // native asserts its 16-byte alignment and no second header is constructed.
    unsafe { frame.add(offset).cast() }
}

pub(super) fn matches(frame: *mut Frame) -> bool { frame.cast::<u8>() == self::frame() }

pub(super) fn lend(frame: *mut Frame) -> bool {
    let bytes = frame.cast::<u8>();
    let loan = header(bytes);
    // SAFETY: the exclusive frame epoch has no UI/native references. Native
    // constructs exactly the frozen opaque heap/owner/lock in this frame.
    unsafe {
        native::nh_tls_construct(loan);
        let start = loan.cast::<u8>().add(native::nh_tls_header_bytes());
        let length = FRAME_BYTES - start.offset_from(bytes) as usize;
        if !native::nh_tls_register(loan, start, length) { return false; }
        native::nh_tls_bind_task_lock(loan);
        let _ = native::nh_tls_install(calloc, free);
    }
    true
}

pub(super) fn reclaim(frame: *mut Frame, lowest: &mut usize) -> bool {
    let mut allocated = 0;
    let mut minimum = 0;
    // SAFETY: a successful lend initialized this same header/heap, whose lock
    // synchronizes SDK frees. No frame reference exists while the heap is live.
    unsafe { native::nh_tls_info(header(frame.cast()), &mut allocated, &mut minimum); }
    *lowest = (*lowest).min(minimum);
    if allocated != 0 { return false; }
    // SAFETY: all frame blocks are gone. Restore the original SDK allocator
    // before wiping its entire backing; callbacks cannot access it afterward.
    unsafe {
        let _ = native::nh_tls_restore();
        ptr::write_bytes(frame.cast::<u8>(), 0, FRAME_BYTES);
    }
    true
}

// SAFETY: installed only under the exclusive frame epoch. SDK callers use C
// calloc extents; other tasks/overflow/zero inputs use the original allocator.
unsafe extern "C" fn calloc(count: usize, size: usize) -> *mut c_void {
    let loan = header(frame());
    // SAFETY: callback installation follows header initialization; the native
    // task comparison reads only its actual stored SDK task handle.
    if unsafe { native::nh_tls_owner_is_current(loan) } && count != 0 && size != 0 {
        if let Some(bytes) = count.checked_mul(size) {
            // SAFETY: the initialized locked heap receives this checked extent.
            let result = unsafe { native::nh_tls_malloc(loan, bytes) };
            if !result.is_null() {
                // SAFETY: successful allocation uniquely owns all bytes; calloc
                // zeroes exactly that requested extent before publishing it.
                unsafe { ptr::write_bytes(result.cast::<u8>(), 0, bytes); }
                return result;
            }
        }
    }
    // SAFETY: preserve the SDK's own zero/overflow/failure behavior on fallback.
    unsafe { native::nh_tls_default_calloc(count, size) }
}

// SAFETY: mbedTLS returns its original pointer once to this installed pair;
// address membership chooses the same allocator without a second owner table.
unsafe extern "C" fn free(bytes: *mut c_void) {
    let first = frame();
    let address = bytes as usize;
    if address >= first as usize && address - (first as usize) < FRAME_BYTES {
        // SAFETY: membership identifies the original locked frame heap block.
        unsafe { native::nh_tls_free(header(first), bytes); }
    } else {
        // SAFETY: every other pointer, including null, goes to SDK default free.
        unsafe { native::nh_tls_default_free(bytes); }
    }
}
