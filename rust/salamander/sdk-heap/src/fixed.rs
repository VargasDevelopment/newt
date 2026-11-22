// SPDX-License-Identifier: MIT
//! Fallible, exact-layout owners on the application's registered allocator.
//! No allocator registration, retained pool, resizing or alternate SDK path.
use alloc::alloc::{alloc, dealloc};
use core::{alloc::Layout, ops::{Deref, DerefMut}, ptr::NonNull};

/// One nonzero-sized value allocated at the caller's lifecycle boundary.
/// The application must register its reviewed allocator before construction.
pub struct Fixed<T> {
    pointer: NonNull<T>,
}

impl<T> Fixed<T> {
    pub fn try_new(value: T) -> Option<Self> {
        Self::try_new_with(|| value)
    }
    /// Allocate before evaluating the initializer. Allocation failure (or a
    /// zero-sized T) never invokes it; unwinding initialization frees storage.
    pub fn try_new_with(initialize: impl FnOnce() -> T) -> Option<Self> {
        let layout = Layout::new::<T>();
        if layout.size() == 0 { return None; }
        // SAFETY: nonzero valid Layout; null transfers no ownership. This is
        // the existing registered allocator, with its ordinary counters/faults.
        let pointer = NonNull::new(unsafe { alloc(layout) }.cast::<T>())?;
        let release = Release { pointer: pointer.cast(), layout };
        let value = initialize();
        // SAFETY: unique, correctly aligned T-sized allocation. The initializer
        // returned a valid T; no reference to uninitialized storage was exposed.
        unsafe { pointer.as_ptr().write(value); }
        // Initialized ownership now belongs to Fixed. No fallible work occurs
        // between write and transfer; Release owns an unwinding initializer.
        core::mem::forget(release);
        Some(Self { pointer })
    }
}

impl<T> Deref for Fixed<T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: construction initialized T; the owner has not been dropped.
        unsafe { self.pointer.as_ref() }
    }
}

impl<T> DerefMut for Fixed<T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: unique owner and exclusive reborrow; no raw pointer escapes.
        unsafe { self.pointer.as_mut() }
    }
}

struct Release {
    pointer: NonNull<u8>,
    layout: Layout,
}
impl Drop for Release {
    fn drop(&mut self) {
        // SAFETY: one live allocation from alloc with this exact Layout. This
        // guard also frees storage if a host-only value destructor unwinds.
        unsafe { dealloc(self.pointer.as_ptr(), self.layout); }
    }
}

impl<T> Drop for Fixed<T> {
    fn drop(&mut self) {
        let _release = Release { pointer: self.pointer.cast(), layout: Layout::new::<T>() };
        // SAFETY: initialized, uniquely owned T, destroyed exactly once before
        // Release frees its backing, including when the destructor unwinds.
        unsafe { self.pointer.as_ptr().drop_in_place(); }
    }
}

/// A transient exact-length initialized byte allocation, never resized.
pub struct Bytes {
    allocation: Release,
}
impl Bytes {
    pub fn try_new(bytes: usize) -> Option<Self> {
        if bytes == 0 {
            return None;
        }
        let layout = Layout::array::<u8>(bytes).ok()?;
        // SAFETY: checked nonzero Layout; null transfers no ownership. Same
        // registered allocator/counters as Fixed and every ordinary allocation.
        let pointer = NonNull::new(unsafe { alloc(layout) })?;
        // SAFETY: initialize the whole exclusive byte allocation in place; no
        // stack-sized temporary and no uninitialized bytes exposed to safe code.
        unsafe { pointer.as_ptr().write_bytes(0, bytes); }
        Some(Self { allocation: Release { pointer, layout } })
    }
}
impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        // SAFETY: exact initialized extent, live for this shared reborrow.
        unsafe { core::slice::from_raw_parts(self.allocation.pointer.as_ptr(), self.allocation.layout.size()) }
    }
}
impl AsMut<[u8]> for Bytes {
    fn as_mut(&mut self) -> &mut [u8] {
        // SAFETY: exact initialized extent, exclusive owner/reborrow. No other
        // mutable view survives this borrow; Drop owns the original allocation.
        unsafe { core::slice::from_raw_parts_mut(self.allocation.pointer.as_ptr(), self.allocation.layout.size()) }
    }
}
