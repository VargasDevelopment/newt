// SPDX-License-Identifier: MIT
//! The frozen post-job optional 32-byte resume allocation, one native pointer.
use core::{ops::{Deref, DerefMut}, ptr::NonNull};
use super::cover_idle::CoverResume;
use newt_native_io::cover as heap;
pub struct CoverResumeOwner { pointer: NonNull<CoverResume> }
impl CoverResumeOwner {
    pub fn try_new() -> Option<Self> {
        // SAFETY: invoked only at noteInterruption under the live UI SDK; native
        // cover allocator checks its exact alignment, reserve and actual charge.
        let pointer = NonNull::new(unsafe { heap::allocate(32) }.cast::<CoverResume>())?;
        // SAFETY: the max-aligned 32-byte allocation uniquely owns this record.
        unsafe { pointer.as_ptr().write(CoverResume::default()); }
        Some(Self { pointer })
    }
}
impl Deref for CoverResumeOwner {
    type Target = CoverResume;
    fn deref(&self) -> &CoverResume {
        // SAFETY: initialized actual record, owned until this pointer's Drop.
        unsafe { self.pointer.as_ref() }
    }
}
impl DerefMut for CoverResumeOwner {
    fn deref_mut(&mut self) -> &mut CoverResume {
        // SAFETY: exclusive one-pointer owner, no second record or alias.
        unsafe { self.pointer.as_mut() }
    }
}
impl Drop for CoverResumeOwner {
    fn drop(&mut self) {
        // SAFETY: same original native allocation, scalar-only record needs no
        // subsidiary destruction; release once using the original heap path.
        unsafe { heap::release(self.pointer.as_ptr().cast()); }
    }
}
#[cfg(target_pointer_width = "32")]
const _: () = assert!(core::mem::size_of::<Option<CoverResumeOwner>>() == 4);
