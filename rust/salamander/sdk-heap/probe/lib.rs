// SPDX-License-Identifier: MIT
//! Borrowed, fallible Vec operation, not a board entrypoint or panic owner.
#![no_std]
extern crate alloc;
use alloc::vec::Vec;

// This registration exists ONLY in this optional final probe, never lib.rs.
#[global_allocator]
static HEAP: newt_sdk_heap::SdkHeap = newt_sdk_heap::SdkHeap;

/// # Safety
/// Input addresses count readable bytes and output addresses capacity writable
/// bytes; both remain exclusively borrowed for this call and do not overlap.
/// Invoke only in an initialized SDK task; no pointers are retained on return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newt_allocator_borrowed_probe(
    input: *const u8,
    count: usize,
    output: *mut u8,
    capacity: usize,
) -> i32 {
    if count > capacity || count > isize::MAX as usize {
        return -1;
    }
    if count == 0 {
        return 0;
    }
    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(count).is_err() {
        return -2;
    }
    let source = unsafe { core::slice::from_raw_parts(input, count) };
    bytes.extend_from_slice(source);
    unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), output, count) };
    0
}
