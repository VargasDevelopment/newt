//! A typed owner in a caller's existing scratch, without allocation.
use super::{Parts, Status, align_of, size_of};
use core::ops::{Deref, DerefMut};

/// Dropped before the backing bytes can be reused, including on unwinding.
/// The initializer's temporary exists only before the caller's work starts.
/// T itself stays live through that work. Its bytes cannot be lent to any
/// other phase until this guard drops; caller-owned disjoint banks may be used.
/// Drop scrubs the typed extent, including padding. `mem::forget` skips that
/// scrub: the bank must not be reread as bytes unless it is reinitialized.
///
/// ```compile_fail,E0499
/// use newt_phased_reader::phase_storage::{scratch_value, Storage, BANK_BYTES};
/// let mut bank = [0u8; BANK_BYTES];
/// let mut owner = scratch_value(&mut bank, || Ok(1u32)).unwrap();
/// let mut builder = Storage::new(&mut bank);
/// *owner = 2;
/// drop(builder);
/// ```
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
        struct Scrub(*mut u8, usize);
        impl Drop for Scrub {
            fn drop(&mut self) {
                // SAFETY: the typed extent remains exclusively borrowed until
                // this cleanup ends, even if T's destructor unwinds.
                unsafe { self.0.write_bytes(0, self.1); }
            }
        }
        let _scrub = Scrub(core::ptr::from_mut(self.0).cast(), size_of::<T>());
        // SAFETY: self.0 is the only live T in this extent and this owner drops
        // it exactly once; the scrub guard above zeroes the bytes afterwards.
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
    fn padded_values_are_scrubbed_even_when_the_destructor_unwinds() {
        #[repr(C)]
        struct Padded { small: u8, wide: u32, panic: bool }
        impl Drop for Padded {
            fn drop(&mut self) { if self.panic { panic!("destructor failure"); } }
        }
        #[repr(align(16))]
        struct Bank([u8; 64]);
        for panic in [false, true] {
            let mut bank = Bank([0xa5; 64]);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let value = scratch_value(&mut bank.0, || Ok(Padded { small: 1, wide: 2, panic })).unwrap();
                assert_eq!(value.wide, 2);
                drop(value);
            }));
            assert_eq!(result.is_err(), panic);
            let extent = size_of::<Padded>();
            assert!(bank.0[..extent].iter().all(|&byte| byte == 0));
            assert!(bank.0[extent..].iter().all(|&byte| byte == 0xa5));
        }
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
