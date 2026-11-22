// SPDX-License-Identifier: MIT
//! The existing empty SD-resume words carry update-mode requests, not a
//! second RTC owner or codec. Call take before any reading-boot allocation.
#![forbid(unsafe_code)]
use newt_c3_board::{Board, retained::RetainedError, retained_words::SD_RESUME_WORDS};
pub use newt_core::update_mode_request::UpdateModeRequest;
use newt_core::update_mode_request;

/// Native layout is primitive words; Core owns the request's exact policy.
/// A valid match is consumed on every reset, honored only on software reset.
pub fn take(board: &mut Board<'_>, software_reset: bool) -> Result<UpdateModeRequest, RetainedError> {
    board.with_retained(|mut words| words.write_sd_resume(|raw| {
        let (prefix, checksum) = raw.split_at_mut(SD_RESUME_WORDS - 1);
        let (magic, version) = prefix.split_at_mut(1);
        update_mode_request::take(&mut magic[0], &mut version[0], &mut checksum[0], software_reset)
    }))
}
/// Only the entry's verified reader-exit guard may call this immediately
/// before its restart. It changes no identity/digest/filename word.
pub fn request(board: &mut Board<'_>, page: UpdateModeRequest) -> Result<(), RetainedError> {
    board.with_retained(|mut words| words.write_sd_resume(|raw| {
        let (prefix, checksum) = raw.split_at_mut(SD_RESUME_WORDS - 1);
        let (magic, version) = prefix.split_at_mut(1);
        update_mode_request::request(&mut magic[0], &mut version[0], &mut checksum[0], page);
    }))
}
