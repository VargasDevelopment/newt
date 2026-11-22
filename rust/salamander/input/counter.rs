// SPDX-License-Identifier: MIT
//! Fixed SeqCst counter primitive; cancellation/input/clock policy stays in lib.rs.
#[cfg(not(any(target_arch = "riscv32", feature = "native-atomics")))]
pub(crate) use core::sync::atomic::AtomicU32 as Counter;

#[cfg(any(target_arch = "riscv32", feature = "native-atomics"))]
mod native {
    use core::{marker::PhantomData, ptr::NonNull, sync::atomic::Ordering};

    /// Pointer identity only: Rust never allocates, dereferences, or sizes the
    /// C++ atomic. This zero-sized declaration is NOT its native object layout.
    #[repr(C)]
    pub struct NativeAtomicU32 {
        _opaque: [u8; 0],
    }

    unsafe extern "C" {
        fn np_atomic_load(pointer: *const NativeAtomicU32) -> u32;
        fn np_atomic_or(pointer: *mut NativeAtomicU32, value: u32) -> u32;
        fn np_atomic_and(pointer: *mut NativeAtomicU32, value: u32) -> u32;
        fn np_atomic_add(pointer: *mut NativeAtomicU32, value: u32) -> u32;
        fn np_atomic_sub(pointer: *mut NativeAtomicU32, value: u32) -> u32;
        fn np_atomic_exchange(pointer: *mut NativeAtomicU32, value: u32) -> u32;
        fn np_atomic_cas_weak(
            pointer: *mut NativeAtomicU32,
            expected: *mut u32,
            desired: u32,
        ) -> u8;
    }

    // Native-owned fixed-address objects; no Rust allocation, self-reference or registry.
    // No Send/Sync is asserted: object emission is not task-safety evidence.
    pub(crate) struct Counter {
        pointer: NonNull<NativeAtomicU32>,
        _local: PhantomData<*mut ()>,
    }

    impl Counter {
        /// # Safety
        /// Sole capability to a live, immovable, initialized native
        /// atomic; no reset/plain alias/destruction during capability or loans,
        /// including forgotten ownership. Every bridge call is SeqCst, returns
        /// the previous u32, and retains no pointer; CAS returns exactly 0 or 1,
        /// updates expected on failure and permits failure with no state change.
        pub(crate) unsafe fn from_raw(pointer: *mut NativeAtomicU32) -> Option<Self> {
            Some(Self {
                pointer: NonNull::new(pointer)?,
                _local: PhantomData,
            })
        }
        // Ordering arguments retain the host policy's exact call sites. Every
        // native operation is fixed SeqCst, including both weak-CAS outcomes.
        pub(crate) fn load(&self, _order: Ordering) -> u32 {
            unsafe { np_atomic_load(self.pointer.as_ptr()) }
        }
        pub(crate) fn fetch_or(&self, value: u32, _order: Ordering) -> u32 {
            unsafe { np_atomic_or(self.pointer.as_ptr(), value) }
        }
        pub(crate) fn fetch_and(&self, value: u32, _order: Ordering) -> u32 {
            unsafe { np_atomic_and(self.pointer.as_ptr(), value) }
        }
        pub(crate) fn fetch_add(&self, value: u32, _order: Ordering) -> u32 {
            unsafe { np_atomic_add(self.pointer.as_ptr(), value) }
        }
        pub(crate) fn fetch_sub(&self, value: u32, _order: Ordering) -> u32 {
            unsafe { np_atomic_sub(self.pointer.as_ptr(), value) }
        }
        pub(crate) fn swap(&self, value: u32, _order: Ordering) -> u32 {
            unsafe { np_atomic_exchange(self.pointer.as_ptr(), value) }
        }
        pub(crate) fn compare_exchange_weak(
            &self,
            current: u32,
            desired: u32,
            _success: Ordering,
            _failure: Ordering,
        ) -> Result<u32, u32> {
            let mut expected = current;
            let success =
                unsafe { np_atomic_cas_weak(self.pointer.as_ptr(), &mut expected, desired) };
            if success != 0 {
                Ok(current)
            } else {
                Err(expected)
            }
        }
    }
}
#[cfg(any(target_arch = "riscv32", feature = "native-atomics"))]
pub(crate) use native::Counter;
#[cfg(any(target_arch = "riscv32", feature = "native-atomics"))]
pub use native::NativeAtomicU32;
