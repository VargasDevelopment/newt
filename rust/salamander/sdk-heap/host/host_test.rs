// SPDX-License-Identifier: MIT
use core::alloc::{GlobalAlloc, Layout};
use newt_sdk_heap::SdkHeap;

unsafe extern "C" {
    fn newt_allocator_cpp_checks();
    fn newt_allocator_fail(value: i32);
}

fn main() {
    // Deliberately no global registration: failure injection cannot sabotage
    // std's test/reporting allocations. Exercise the actual GlobalAlloc impl.
    unsafe {
        newt_allocator_cpp_checks();
        for alignment in [1, 2, 4, 8, 16, 64, 256, 4096] {
            let layout = Layout::from_size_align(37, alignment).unwrap();
            let pointer = SdkHeap.alloc_zeroed(layout);
            assert!(!pointer.is_null());
            assert_eq!((pointer as usize) % alignment, 0);
            assert!(
                core::slice::from_raw_parts(pointer, 37)
                    .iter()
                    .all(|b| *b == 0)
            );
            core::ptr::write_bytes(pointer, 0xa5, 37);
            newt_allocator_fail(1);
            let failed = SdkHeap.realloc(pointer, layout, 91);
            newt_allocator_fail(0);
            assert!(failed.is_null());
            assert!(
                core::slice::from_raw_parts(pointer, 37)
                    .iter()
                    .all(|b| *b == 0xa5)
            );
            let grown = SdkHeap.realloc(pointer, layout, 91);
            assert!(!grown.is_null());
            assert_eq!((grown as usize) % alignment, 0);
            assert!(
                core::slice::from_raw_parts(grown, 37)
                    .iter()
                    .all(|b| *b == 0xa5)
            );
            SdkHeap.dealloc(grown, Layout::from_size_align(91, alignment).unwrap());
            newt_allocator_fail(1);
            let failed = SdkHeap.alloc(layout);
            let failed_zero = SdkHeap.alloc_zeroed(layout);
            newt_allocator_fail(0);
            assert!(failed.is_null() && failed_zero.is_null());
        }
    }
    // No shared Rust allocator state; the SDK, not a Rust critical section,
    // owns concurrency. Host libc thread stress is a separate host witness.
    let threads: Vec<_> = (0..2)
        .map(|_| {
            std::thread::spawn(|| {
                for _ in 0..128 {
                    unsafe {
                        let layout = Layout::from_size_align(113, 64).unwrap();
                        let pointer = SdkHeap.alloc(layout);
                        assert!(!pointer.is_null());
                        core::ptr::write_bytes(pointer, 0x71, 113);
                        assert!(
                            core::slice::from_raw_parts(pointer, 113)
                                .iter()
                                .all(|b| *b == 0x71)
                        );
                        SdkHeap.dealloc(pointer, layout);
                    }
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
}
