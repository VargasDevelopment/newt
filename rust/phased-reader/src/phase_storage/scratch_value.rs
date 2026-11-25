//! A typed owner in a caller's existing scratch, without allocation.
use super::{Parts, Status, align_of, size_of};
use core::ops::{Deref, DerefMut};

/// Dropped before the backing bytes can be reused, including on unwinding.
/// The initializer's temporary exists only before the caller's work starts.
///
/// ```compile_fail
/// use newt_phased_reader::phase_storage::scratch_value;
/// let mut bytes = [0u8; 64];
/// let escaped = {
///     let mut value = scratch_value(&mut bytes, || Ok(1u32)).unwrap();
///     &mut *value
/// };
/// drop(escaped);
/// ```
#[inline(always)]
pub fn scratch_value<T>(scratch: &mut [u8], initialize: impl FnOnce() -> Result<T, Status>)
    -> Result<ScratchValue<'_, T>, Status> {
    let bytes = Parts(scratch).bytes(size_of::<T>(), align_of::<T>())?;
    let pointer = bytes.as_mut_ptr().cast::<T>();
    initialize_value(pointer, initialize)?;
    // SAFETY: Parts checked extent/alignment; initialize_value wrote a valid T.
    // ScratchValue keeps the exclusive byte loan until it drops T exactly once.
    Ok(ScratchValue(unsafe { &mut *pointer }))
}

#[inline(never)]
fn initialize_value<T>(pointer: *mut T, initialize: impl FnOnce() -> Result<T, Status>) -> Result<(), Status> {
    let value = initialize()?;
    // SAFETY: the private caller supplies the checked disjoint scratch extent.
    unsafe { pointer.write(value); }
    Ok(())
}

pub struct ScratchValue<'a, T>(&'a mut T);
impl<T> Deref for ScratchValue<'_, T> {
    type Target = T;
    fn deref(&self) -> &T { self.0 }
}
impl<T> DerefMut for ScratchValue<'_, T> {
    fn deref_mut(&mut self) -> &mut T { self.0 }
}
impl<T> Drop for ScratchValue<'_, T> {
    fn drop(&mut self) {
        // SAFETY: this guard is created only after initialization and owns the
        // only T loan. The backing bytes stay borrowed through this drop.
        unsafe { core::ptr::drop_in_place(self.0); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;
    extern crate std;
    #[repr(align(32))]
    struct Tracked<'a>(&'a Cell<u32>);
    impl Drop for Tracked<'_> {
        fn drop(&mut self) { self.0.set(self.0.get() + 1); }
    }
    #[test]
    fn aligned_owner_drops_before_scratch_returns_even_on_unwind() {
        let dropped = Cell::new(0);
        let mut bytes = [0xa5; 128];
        {
            let mut value = scratch_value(&mut bytes[1..], || Ok(Tracked(&dropped))).unwrap();
            assert_eq!(core::ptr::from_mut(&mut *value).addr() % align_of::<Tracked<'_>>(), 0);
            assert_eq!(dropped.get(), 0);
        }
        assert_eq!(dropped.get(), 1);
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _value = scratch_value(&mut bytes[1..], || Ok(Tracked(&dropped))).unwrap();
            panic!("work stopped");
        }));
        assert!(panic.is_err());
        assert_eq!(dropped.get(), 2);
        assert_eq!(bytes[0], 0xa5);
    }
    #[test]
    fn denied_extent_and_failed_initialization_never_enter_work() {
        let initialized = Cell::new(false);
        let mut bytes = [0x7d; 16];
        let result = scratch_value(&mut bytes[1..], || {
            initialized.set(true);
            Ok([0u64; 4])
        });
        assert!(matches!(result, Err(Status::OutOfMemory)));
        drop(result);
        assert!(!initialized.get());
        assert_eq!(bytes, [0x7d; 16]);
        let result = scratch_value::<u64>(&mut bytes, || Err(Status::IoError));
        assert!(matches!(result, Err(Status::IoError)));
    }
}
