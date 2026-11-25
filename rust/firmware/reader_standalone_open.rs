// SPDX-License-Identifier: MIT
//! The no-session prepare_image hash, over its held selected descriptor.
use crate::{reader_book::BookFailure, reader_hash::{self, SourceHashObserver}};
use newt_core::{zip_container::Status, zip_window::source::BookSource};

pub(crate) fn verify(raw: &mut dyn BookSource, expected: Option<[u8; 32]>,
    yielded: &dyn Fn(), allowed: &dyn Fn() -> bool, observer: Option<&dyn SourceHashObserver>,
) -> Result<[u8; 32], BookFailure> {
    let mut digest = [0; 32];
    // SAFETY: prepare_image holds this descriptor under Home's pinned SDK.
    let hashed = unsafe { reader_hash::hash_source(raw, &mut digest, &mut || yielded(), observer) };
    if !hashed || !allowed() { return Err(BookFailure::Source); }
    if expected.is_some_and(|expected| digest != expected) {
        return Err(BookFailure::Open(Status::Stale));
    }
    Ok(digest)
}
