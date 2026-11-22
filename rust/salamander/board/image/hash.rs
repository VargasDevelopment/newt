// SPDX-License-Identifier: MIT
//! The frozen EspAppSlots::Sha256 lifetime over actual SDK calls.
use super::{Error, c_void};
use newt_core::update_online::Hash;

// SAFETY: SDK hash and Rust Call pointers must be live and disjoint for this
// synchronous callback; it must neither retain them nor unwind across C++.
type Callback = unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32;
// SAFETY: link matching image.cpp; context must be initialized/exclusive and
// all input/output extents valid through each call. No Rust pointer is retained.
unsafe extern "C" {
    fn nb_image_hash_scope(callback: Callback, call: *mut c_void) -> u32;
    fn nb_image_hash_start(context: *mut c_void) -> i32;
    fn nb_image_hash_update(context: *mut c_void, input: *const u8, bytes: usize) -> i32;
    fn nb_image_hash_finish(context: *mut c_void, out: *mut u8) -> i32;
}
struct Sha256(*mut c_void);
impl Hash for Sha256 {
    fn start(&mut self) -> bool {
        // SAFETY: only receive constructs this exclusive live SDK context.
        unsafe { nb_image_hash_start(self.0) == 0 }
    }
    fn update(&mut self, input: &[u8]) -> bool {
        // SAFETY: SDK borrows only this exact immutable input until return.
        unsafe { nb_image_hash_update(self.0, input.as_ptr(), input.len()) == 0 }
    }
    fn finish(&mut self, out: &mut [u8; 32]) -> bool {
        // SAFETY: exclusive output has the SHA-256 SDK's exact32-byte extent.
        unsafe { nb_image_hash_finish(self.0, out.as_mut_ptr()) == 0 }
    }
}
struct Call<F, R> { work: Option<F>, result: Option<R> }
// SAFETY: native initializes one context, invokes once synchronously and frees
// it after return. The callback/context cannot escape and must not unwind.
unsafe extern "C" fn receive<F, R>(hash: *mut c_void, call: *mut c_void) -> u32
where F: FnOnce(&mut dyn Hash) -> R {
    if hash.is_null() || call.is_null() { return 0; }
    // SAFETY: with owns this exclusive Call through the native scope.
    let call = unsafe { &mut *call.cast::<Call<F, R>>() };
    let Some(work) = call.work.take() else { return 0; };
    call.result = Some(work(&mut Sha256(hash)));
    1
}
pub(super) fn with<F, R>(work: F) -> Result<R, Error>
where F: FnOnce(&mut dyn Hash) -> R {
    let mut call = Call { work: Some(work), result: None };
    // SAFETY: caller holds the initialized Board capability. Both callback
    // and exclusive Call live until the SDK context has actually been freed.
    let complete = unsafe { nb_image_hash_scope(receive::<F, R>,
        (&mut call as *mut Call<F, R>).cast()) };
    if complete != 1 { return Err(Error::Io); }
    call.result.ok_or(Error::Io)
}
