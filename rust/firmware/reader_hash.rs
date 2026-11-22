// SPDX-License-Identifier: MIT
//! X3 SHA-256 provider for the pinned ESP-IDF 5.5.2 mbedTLS implementation.
//! Its native layout assertions must compile before this module is linked.
use core::{ffi::c_int, marker::PhantomData};
use newt_phased_reader::imported_revision::{Digest, RevisionHasher};
#[path = "reader_hash/package.rs"]
mod package;
pub use package::PackageHasher;
#[path = "reader_source_hash.rs"]
mod source;
pub use source::{SourceHashObserver, hash_source};
#[path = "reader_hash/loan.rs"]
mod loan;
pub use loan::MbedLoan;

#[repr(C)]
struct Context {
    total: [u32; 2],
    state: [u32; 8],
    buffer: [u8; 64],
    first_block: c_int,
    mode: c_int,
    sha_state: c_int,
}

// sha256_alt.h's ESP32-C3 DMA/resume branch contains inline scalars only.
const _: () = {
    assert!(core::mem::size_of::<Context>() == 116);
    assert!(core::mem::align_of::<Context>() == 4);
    assert!(core::mem::offset_of!(Context, total) == 0);
    assert!(core::mem::offset_of!(Context, state) == 8);
    assert!(core::mem::offset_of!(Context, buffer) == 40);
    assert!(core::mem::offset_of!(Context, first_block) == 104);
    assert!(core::mem::offset_of!(Context, mode) == 108);
    assert!(core::mem::offset_of!(Context, sha_state) == 112);
};

unsafe extern "C" {
    fn mbedtls_sha256_init(context: *mut Context);
    fn mbedtls_sha256_free(context: *mut Context);
    fn mbedtls_sha256_starts(context: *mut Context, is224: c_int) -> c_int;
    fn mbedtls_sha256_update(context: *mut Context, input: *const u8, length: usize) -> c_int;
    fn mbedtls_sha256_finish(context: *mut Context, output: *mut u8) -> c_int;
    fn mbedtls_sha256(input: *const u8, length: usize, output: *mut u8, is224: c_int) -> c_int;
}

/// One exclusive context; one-shot hashes never disturb its incremental state.
pub struct MbedHasher {
    context: Context,
    yield_hook: Option<fn()>,
    since_yield: u32,
    task: PhantomData<*mut ()>,
}

impl MbedHasher {
    /// # Safety
    /// Use only with the pinned X3 SDK and a compiled reader_hash_layout.cpp.
    /// The SDK must be initialized and available for the program's lifetime.
    /// The yield hook must not reenter this context or unwind through SDK code.
    pub unsafe fn new(yield_hook: Option<fn()>) -> Self {
        let mut owner = Self {
            context: Context {
                total: [0; 2],
                state: [0; 8],
                buffer: [0; 64],
                first_block: 0,
                mode: 0,
                sha_state: 0,
            },
            yield_hook,
            since_yield: 0,
            task: PhantomData,
        };
        // The SDK initializes an exclusive context containing no self-pointers.
        unsafe { mbedtls_sha256_init(&mut owner.context) };
        owner
    }

    /// Like Board's scalar clock, this borrows no incremental state. The same
    /// constructor established the program-lifetime SDK contract, so receipt
    /// hashing can keep its exclusive context while cache identities use SHA.
    pub fn package_provider(&self) -> PackageHasher {
        PackageHasher(PhantomData)
    }

    /// The operation's actual UI cadence/cancellation hook never enters the
    /// persistent owner. This view retains the same SDK context and credit.
    pub fn borrow_with_yield<'op>(&'op mut self, hook: &'op dyn Fn()) -> MbedLoan<'op> {
        MbedLoan { owner: self, hook }
    }

    /// SDK result and actual shared-credit transition, without choosing a hook.
    fn update_credit(&mut self, input: &[u8]) -> (bool, bool) {
        let Ok(count) = u32::try_from(input.len()) else {
            return (false, false);
        };
        // SAFETY: the constructor initialized this exclusive pinned context;
        // the SDK borrows only the current input extent synchronously.
        if unsafe { mbedtls_sha256_update(&mut self.context, input.as_ptr(), input.len()) } != 0 {
            return (false, false);
        }
        self.since_yield = self.since_yield.wrapping_add(count);
        if self.since_yield >= 65536 {
            self.since_yield = 0;
            return (true, true);
        }
        (true, false)
    }
}

impl RevisionHasher for MbedHasher {
    fn begin(&mut self) -> bool {
        // Preserve the accumulated yield credit across begin().
        unsafe { mbedtls_sha256_starts(&mut self.context, 0) == 0 }
    }

    fn update(&mut self, input: &[u8]) -> bool {
        let (success, yield_due) = self.update_credit(input);
        if yield_due {
            if let Some(yield_hook) = self.yield_hook { yield_hook(); }
        }
        success
    }

    fn finish(&mut self, output: &mut Digest) -> bool {
        unsafe { mbedtls_sha256_finish(&mut self.context, output.as_mut_ptr()) == 0 }
    }

    fn hash(&mut self, input: &[u8], output: &mut Digest) -> bool {
        package_hash(input, output)
    }
}

impl Drop for MbedHasher {
    fn drop(&mut self) {
        unsafe { mbedtls_sha256_free(&mut self.context) };
    }
}

pub(crate) fn package_hash(input: &[u8], output: &mut Digest) -> bool {
    // packageSha256 bounds input and publishes only a successful digest.
    if input.len() > 8192 { return false; }
    let mut digest = [0; 32];
    // SAFETY: the initialized SDK borrows these exact input/output extents
    // synchronously. The one-shot call retains no context or pointer.
    if unsafe { mbedtls_sha256(input.as_ptr(), input.len(), digest.as_mut_ptr(), 0) } != 0 {
        return false;
    }
    *output = digest;
    true
}
