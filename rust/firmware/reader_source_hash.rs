// SPDX-License-Identifier: MIT
//! Full-book SHA-256 hashing over a borrowed source. Its frame returns before
//! reader admission; neither the 1024-byte scratch nor this SDK context is held.
use core::marker::PhantomData;
use newt_core::{reader_work_profile::ReaderHashOperation, zip_window::source::BookSource};
use newt_phased_reader::imported_revision::{Digest, MAX_BOOK_BYTES};
use super::{Context, mbedtls_sha256_free, mbedtls_sha256_init, mbedtls_sha256_starts,
    mbedtls_sha256_update, mbedtls_sha256_finish};

/// One short DEV observation loan. Release supplies None and reads no clock;
/// observers retain no source, scratch or SDK pointer.
pub trait SourceHashObserver {
    fn micros(&self) -> u32;
    fn hash(&self, operation: ReaderHashOperation, microseconds: u32, bytes: u32);
}

struct Trace<'a>(Option<&'a dyn SourceHashObserver>);
impl<'a> Trace<'a> {
    fn new(observer: Option<&'a dyn SourceHashObserver>) -> Self {
        if let Some(observer) = observer {
            observer.hash(ReaderHashOperation::Begin, observer.micros(), 0);
        }
        Self(observer)
    }

    fn stamp(&self) -> Option<u32> { self.0.map(SourceHashObserver::micros) }

    fn elapsed(&self, operation: ReaderHashOperation, started: Option<u32>, bytes: u32) {
        if let (Some(observer), Some(started)) = (self.0, started) {
            observer.hash(operation, observer.micros().wrapping_sub(started), bytes);
        }
    }
}
impl Drop for Trace<'_> {
    fn drop(&mut self) {
        if let Some(observer) = self.0 {
            observer.hash(ReaderHashOperation::End, observer.micros(), 0);
        }
    }
}

struct TemporaryContext {
    context: Context,
    task: PhantomData<*mut ()>,
}
impl Drop for TemporaryContext {
    fn drop(&mut self) {
        // SAFETY: hash_source initialized this exclusive context with the same
        // pinned SDK. It has inline scalars only and is freed exactly once.
        unsafe { mbedtls_sha256_free(&mut self.context) };
    }
}

/// The separate frame also prevents this fallback scratch from accumulating
/// with parser or turn-loop frames. No zero-length source probe is added.
///
/// # Safety
/// The pinned X3 SDK must be live, with reader_hash_layout.cpp compiled under
/// the actual target flags. Calls stay on the UI task. Callbacks must not
/// reenter or invalidate the source, nor unwind through SDK code.
#[inline(never)]
pub unsafe fn hash_source<S: BookSource + ?Sized>(
    source: &mut S, output: &mut Digest, yield_one_tick: &mut dyn FnMut(),
    observer: Option<&dyn SourceHashObserver>,
) -> bool {
    let size = source.size();
    if size == 0 || size > MAX_BOOK_BYTES { return false; }
    let trace = Trace::new(observer);
    let mut chunk = [0; 1024];
    let mut context = TemporaryContext {
        context: Context {
            total: [0; 2], state: [0; 8], buffer: [0; 64],
            first_block: 0, mode: 0, sha_state: 0,
        },
        task: PhantomData,
    };
    // SAFETY: the target-layout contract above holds; the SDK initializes this
    // exclusive inline context and retains no pointer to its initial location.
    unsafe { mbedtls_sha256_init(&mut context.context) };
    // SAFETY: this context is initialized and exclusively lent synchronously.
    let mut ok = unsafe { mbedtls_sha256_starts(&mut context.context, 0) == 0 };
    let mut offset = 0u64;
    let mut since_yield = 0u32;
    while ok && offset < size {
        let want = (size - offset).min(chunk.len() as u64) as usize;
        let got = source.read_at(offset, &mut chunk[..want]);
        if got <= 0 || got as usize > want { ok = false; break; }
        let started = trace.stamp();
        // SAFETY: the initialized context and exact returned input extent are
        // exclusive live loans for this synchronous SDK call.
        ok = unsafe {
            mbedtls_sha256_update(&mut context.context, chunk.as_ptr(), got as usize) == 0
        };
        offset += got as u64;
        since_yield += got as u32;
        trace.elapsed(ReaderHashOperation::Update, started, got as u32);
        // Frozen advances/yields even when this update failed at the boundary.
        if since_yield >= 65536 {
            let started = trace.stamp();
            yield_one_tick();
            since_yield = 0;
            trace.elapsed(ReaderHashOperation::Wait, started, 0);
        }
    }
    if ok {
        // SAFETY: initialized exclusive context; output is the complete live
        // digest extent. Preserve frozen's partial output on SDK failure.
        ok = source.size() == size && unsafe {
            mbedtls_sha256_finish(&mut context.context, output.as_mut_ptr()) == 0
        };
    }
    // Context free precedes Trace::drop, matching the frozen End observation.
    drop(context);
    ok
}

const _: () = {
    assert!(core::mem::size_of::<TemporaryContext>() == 116);
    assert!(core::mem::align_of::<TemporaryContext>() == 4);
};
