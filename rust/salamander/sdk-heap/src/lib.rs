// SPDX-License-Identifier: MIT
//! Explicit `GlobalAlloc` over the SDK libc heap; the firmware binary registers it.
#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;
mod fixed;
pub use fixed::{Bytes, Fixed};
mod aligned;
pub use aligned::AlignedRegion;

use core::alloc::{GlobalAlloc, Layout};

/// Stateless bridge to the application's existing, synchronized libc heap.
/// Link exactly the matching native_alloc.c, with its reviewed SDK policy.
/// Use only after SDK heap initialization, in ordinary task context. Allocator
/// callbacks must not reenter Rust allocation, unwind, or change that policy.
/// Registration, panic/OOM handling and scheduler lifetime belong to the binary.
pub struct SdkHeap;

unsafe extern "C" {
    fn newt_rust_heap_alloc(size: usize, alignment: usize) -> *mut u8;
    fn newt_rust_heap_zeroed(size: usize, alignment: usize) -> *mut u8;
    fn newt_rust_heap_free(pointer: *mut u8, size: usize, alignment: usize);
    fn newt_rust_heap_resize(
        pointer: *mut u8,
        old_size: usize,
        alignment: usize,
        new_size: usize,
    ) -> *mut u8;
}

// SAFETY: GlobalAlloc's caller supplies valid nonzero Layouts and exclusively
// owned live pointers. C preserves alignment, zeroing and the realloc prefix;
// null never transfers ownership. C retains only the raw malloc pointer in
// over-aligned allocations. It uses no Rust callback, global state or locks;
// the existing libc/vendor heap owns synchronization across tasks. No unwind
// may cross FFI. These guarantees depend on linking the matching C bridge and
// keeping SDK heap hooks nonallocating/nonunwinding in Rust.
unsafe impl GlobalAlloc for SdkHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { newt_rust_heap_alloc(layout.size(), layout.align()) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe { newt_rust_heap_zeroed(layout.size(), layout.align()) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { newt_rust_heap_free(pointer, layout.size(), layout.align()) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe { newt_rust_heap_resize(pointer, layout.size(), layout.align(), new_size) }
    }
}
