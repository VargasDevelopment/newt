// SPDX-License-Identifier: MIT
//! Frozen aligned_storage.h's byte allocation and saved raw-pointer prefix.
//! Placement initialization belongs to the typed owner at the Board boundary.
use alloc::alloc::{alloc, dealloc};
use core::{alloc::Layout, mem::{MaybeUninit, size_of}, ptr::NonNull};

/// One exact frozen region. Only its payload pointer is held outside the
/// allocation, as in AlignedOwner; no size word, pool or second backing exists.
pub struct AlignedRegion<const BYTES: usize, const ALIGN: usize> {
    payload: NonNull<u8>,
}

impl<const BYTES: usize, const ALIGN: usize> AlignedRegion<BYTES, ALIGN> {
    pub const fn allocation_bytes() -> Option<usize> {
        if BYTES == 0 || !ALIGN.is_power_of_two() { return None; }
        let Some(overhead) = size_of::<*mut u8>().checked_add(ALIGN - 1) else { return None; };
        BYTES.checked_add(overhead)
    }

    /// Allocates the same byte count/alignment as frozen's new uint8_t[].
    /// The payload is uninitialized: safe code can access it only as such.
    pub fn try_new() -> Option<Self> {
        let bytes = Self::allocation_bytes()?;
        let layout = Layout::array::<u8>(bytes).ok()?;
        // SAFETY: checked nonzero Layout. Uses the registered allocator and
        // ordinary counters; null transfers no allocation or ownership.
        let raw = NonNull::new(unsafe { alloc(layout) })?;
        let prefix = size_of::<*mut u8>();
        // SAFETY: layout contains prefix+ALIGN-1+BYTES bytes. Starting after
        // the prefix leaves enough room for any alignment residue and payload.
        let payload = unsafe {
            let begin = raw.as_ptr().add(prefix);
            let padding = begin.align_offset(ALIGN);
            let payload = begin.add(padding);
            // memcpy in frozen writes the raw allocation immediately before
            // the aligned payload. That prefix is never lent to safe clients.
            payload.sub(prefix).cast::<*mut u8>().write_unaligned(raw.as_ptr());
            NonNull::new_unchecked(payload)
        };
        Some(Self { payload })
    }

    /// A short initialization loan. The owner cannot free the region while
    /// it is borrowed; no initialized byte or typed value is promised yet.
    pub fn uninit(&mut self) -> &mut [MaybeUninit<u8>; BYTES] {
        // SAFETY: uniquely owned live BYTES extent, aligned at least for u8.
        // MaybeUninit permits the actual uninitialized payload; prefix excluded.
        unsafe { &mut *self.payload.as_ptr().cast() }
    }

    /// Shared bytes promise no initialization; typed owners establish their
    /// own placement invariant before projecting a resident value.
    pub fn uninit_ref(&self) -> &[MaybeUninit<u8>; BYTES] {
        // SAFETY: the live owned payload has exactly BYTES bytes. Shared self
        // excludes mutation and deallocation for this loan.
        unsafe { &*self.payload.as_ptr().cast() }
    }
}

impl<const BYTES: usize, const ALIGN: usize> Drop for AlignedRegion<BYTES, ALIGN> {
    fn drop(&mut self) {
        // SAFETY: construction saved this one raw pointer in a private prefix.
        // Clients can borrow only payload bytes, so they cannot overwrite it.
        let raw = unsafe { self.payload.as_ptr().sub(size_of::<*mut u8>()).cast::<*mut u8>().read_unaligned() };
        // Construction checked this exact layout. Store no extra layout word.
        if let Some(bytes) = Self::allocation_bytes() {
            if let Ok(layout) = Layout::array::<u8>(bytes) {
                // SAFETY: exact original live pointer and registered Layout,
                // freed once after all loans; typed clients end their values first.
                unsafe { dealloc(raw, layout); }
            }
        }
    }
}

const _: () = assert!(size_of::<AlignedRegion<400, 8>>() == size_of::<*mut u8>());

#[cfg(test)]
mod tests {
    use super::*;

    fn region<const N: usize, const A: usize>() {
        let mut owner = AlignedRegion::<N, A>::try_new().expect("host allocation");
        let payload = owner.uninit();
        assert_eq!(payload.as_ptr() as usize % A, 0, "aligned payload");
        for (index, byte) in payload.iter_mut().enumerate() { byte.write(index as u8); }
        // Drop still uses the private prefix after every payload byte is written.
    }

    #[test]
    fn exact_prefix_and_payload_alignment() {
        region::<400, 8>();
        region::<49_152, 16>();
        region::<9_616, 16>();
        region::<3_552, 16>();
        assert_eq!(AlignedRegion::<400, 8>::allocation_bytes(), Some(400 + size_of::<*mut u8>() + 7));
        assert_eq!(AlignedRegion::<49_152, 16>::allocation_bytes(), Some(49_152 + size_of::<*mut u8>() + 15));
    }

    #[test]
    fn invalid_layouts_are_rejected() {
        assert!(AlignedRegion::<0, 8>::try_new().is_none());
        assert!(AlignedRegion::<4, 3>::try_new().is_none());
        assert!(AlignedRegion::<{ usize::MAX }, 16>::try_new().is_none());
    }
}
