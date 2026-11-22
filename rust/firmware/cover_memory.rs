// SPDX-License-Identifier: MIT
//! Frozen CoverMemory intrusive allocations on the actual cover native heap.
use core::{cell::Cell, mem::size_of, ptr::{self, NonNull}};
use newt_native_io::cover as heap;
use super::heap::Heap;
pub const LIMIT: usize = 64 * 1024;
pub const RESERVE: usize = 64 * 1024;
pub const ALLOWANCE: usize = 128;
#[repr(C, align(16))]
struct Header { next: *mut Header, previous: *mut Header, bytes: usize, charge: usize }
pub struct Memory<'a> {
    heap: &'a Heap<'a>,
    head: Cell<*mut Header>, live: Cell<usize>, peak: Cell<usize>,
    rejected: Cell<usize>, reservation: usize,
}
impl<'a> Memory<'a> {
    pub fn new(reservation: usize,heap:&'a Heap<'a>) -> Self {
        let reservation = reservation.min(LIMIT);
        Self { heap, head: Cell::new(ptr::null_mut()), live: Cell::new(reservation),
            peak: Cell::new(reservation), rejected: Cell::new(0), reservation }
    }
    pub const fn charge(bytes: usize) -> usize { size_of::<Header>() + bytes + ALLOWANCE }
    pub fn live(&self) -> usize { self.live.get() }
    pub fn peak(&self) -> usize { self.peak.get() }
    pub fn rejected(&self) -> usize { self.rejected.get() }
    pub fn reservation(&self) -> usize { self.reservation }
    fn reject(&self) -> *mut u8 { self.rejected.set(self.rejected.get() + 1); ptr::null_mut() }
    pub fn allocate(&self, bytes: usize) -> *mut u8 {
        if bytes == 0 || bytes > LIMIT - size_of::<Header>() - ALLOWANCE || !heap::supported() {
            return self.reject();
        }
        let charge = Self::charge(bytes);
        if charge > LIMIT - self.live.get() || self.heap.free_bytes() < charge + RESERVE
            || self.heap.largest() < charge { return self.reject(); }
        // SAFETY: native SDK is live throughout this UI-task cover scope. The
        // original max-aligned allocation is owned by this intrusive list.
        let raw = unsafe { heap::allocate(size_of::<Header>() + bytes) }.cast::<Header>();
        if raw.is_null() { return self.reject(); }
        // SAFETY: raw is the live original SDK allocation, not its payload.
        let actual = unsafe { heap::charged(raw.cast()) };
        if actual < size_of::<Header>() + bytes || actual > charge || heap::free_bytes() < RESERVE {
            // SAFETY: failed admission owns raw and has published no payload.
            unsafe { heap::release(raw.cast()); } return self.reject();
        }
        // SAFETY: max alignment covers Header. Every linked header belongs to
        // this non-Send UI-only Memory. No user bytes overlap these links.
        unsafe {
            raw.write(Header { next: self.head.get(), previous: ptr::null_mut(), bytes, charge });
            if !self.head.get().is_null() { (*self.head.get()).previous = raw; }
            self.head.set(raw);
            // Safe byte loans expose initialized memory, never raw SDK garbage.
            raw.add(1).cast::<u8>().write_bytes(0, bytes);
        }
        self.live.set(self.live.get() + charge); self.peak.set(self.peak.get().max(self.live.get()));
        // SAFETY: one Header is inside the allocation, followed by bytes payload.
        unsafe { raw.add(1).cast() }
    }
    /// # Safety
    /// pointer is null or a uniquely owned live payload from THIS Memory.
    pub unsafe fn release(&self, pointer: *mut u8) {
        if pointer.is_null() { return; }
        // SAFETY: caller proves exact payload identity and exclusive release.
        unsafe {
            let block = pointer.cast::<Header>().sub(1);
            if (*block).previous.is_null() { self.head.set((*block).next); }
            else { (*(*block).previous).next = (*block).next; }
            if !(*block).next.is_null() { (*(*block).next).previous = (*block).previous; }
            self.live.set(self.live.get() - (*block).charge); heap::release(block.cast());
        }
    }
    /// # Safety
    /// pointer is null or an original live payload from THIS Memory, with no
    /// outstanding byte/type loans. Failed growth preserves that allocation.
    pub unsafe fn resize(&self, pointer: *mut u8, bytes: usize) -> *mut u8 {
        if pointer.is_null() { return self.allocate(bytes); }
        if bytes == 0 {
            // SAFETY: caller transfers its exact exclusive live payload.
            unsafe { self.release(pointer); } return ptr::null_mut();
        }
        let next = self.allocate(bytes);
        if next.is_null() { return next; }
        // SAFETY: distinct live allocations; growth counts both until this copy
        // finishes, then releases only the original input, as frozen resize.
        unsafe {
            let old = pointer.cast::<Header>().sub(1);
            ptr::copy_nonoverlapping(pointer, next, bytes.min((*old).bytes));
            self.release(pointer);
        }
        next
    }
    pub fn block(&self, bytes: usize) -> Option<Block<'_>> {
        Some(Block { pointer: NonNull::new(self.allocate(bytes))?, bytes, memory: self })
    }
}
impl Drop for Memory<'_> {
    fn drop(&mut self) {
        while !self.head.get().is_null() {
            // SAFETY: the list owns every remaining payload, including Expat
            // allocation failure cleanup. All consumers ended before Memory.
            unsafe { self.release(self.head.get().add(1).cast()); }
        }
    }
}
pub struct Block<'a> { pointer: NonNull<u8>, bytes: usize, memory: &'a Memory<'a> }
impl Block<'_> {
    pub fn as_mut(&mut self) -> &mut [u8] {
        // SAFETY: exclusive block borrow, initialized exact payload extent.
        unsafe { core::slice::from_raw_parts_mut(self.pointer.as_ptr(), self.bytes) }
    }
    pub fn pointer(&mut self) -> *mut u8 { self.pointer.as_ptr() }
}
impl Drop for Block<'_> {
    fn drop(&mut self) {
        // SAFETY: this RAII owner transfers its exact original live payload.
        unsafe { self.memory.release(self.pointer.as_ptr()); }
    }
}
