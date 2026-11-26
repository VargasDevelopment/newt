// SPDX-License-Identifier: MIT
//! Revoke a publication if the released source cannot be trusted again.
use crate::{reader_book::BookFailure, reader_hash::MbedHasher, reader_pages::ReaderPages,
    reader_verification::ReaderVerification};
use newt_native_storage::Native;
use newt_phased_reader::index_build::Indexed;
use newt_core::{zip_container::Status, zip_window::source::BookSource};

#[inline(always)]
pub(super) fn complete<N: Native>(result: Result<Indexed, Status>, pages: &mut ReaderPages<'_, N>,
    raw: &mut dyn BookSource, digest: &[u8;32], verification: Option<&mut ReaderVerification<MbedHasher>>,
    yielded: &dyn Fn(),
) -> Result<Indexed, BookFailure> {
    let indexed = match result {
        Ok(indexed) => indexed,
        Err(status) => {
            // Publication can precede a failed reopen. It is no longer a
            // confirmed cache of a source this open can read.
            pages.0.invalidate_index(digest);
            return Err(BookFailure::Open(status));
        }
    };
    if indexed.source_reopened() {
        let trusted = match verification {
            Some(verification) => restart_verification(verification, raw),
            None => verify_standalone_reopen(raw, digest, yielded),
        };
        if !trusted {
            pages.0.invalidate_index(digest);
            return Err(BookFailure::Source);
        }
    }
    Ok(indexed)
}

// These frames run only after the deep builder has returned. A reopened FD
// needs the existing background verifier, or a full check if no session fits.
#[inline(never)]
fn restart_verification(verification:&mut ReaderVerification<MbedHasher>,
    raw:&dyn newt_core::zip_window::source::BookSource)->bool {
    verification.begin(raw,|| {
        // SAFETY: the enclosing Home retains the initialized pinned SDK.
        unsafe { MbedHasher::new(None) }
    })
}
#[inline(never)]
fn verify_standalone_reopen(raw:&mut dyn newt_core::zip_window::source::BookSource,
    expected:&[u8;32],yielded:&dyn Fn())->bool {
    let mut actual=[0;32];
    // SAFETY: this is the held selected source under the enclosing Home SDK.
    (unsafe { crate::reader_hash::hash_source(raw,&mut actual,&mut || yielded(),None) }) && actual==*expected
}

