// SPDX-License-Identifier: MIT
//! First-screen boot confirmation policy for newly installed firmware.
//! Root owns the first finished frame and actual mark-valid capability.
#![forbid(unsafe_code)]
pub use crate::update_card::slot_installer::identity::ImageState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum FirstScreen { Owed, Unconfirmed, Confirmed, Noted }
const _: () = assert!(core::mem::size_of::<FirstScreen>() == 1);
const _: () = assert!(core::mem::align_of::<FirstScreen>() == 1);
const _: () = assert!(core::mem::size_of::<ImageState>() == 1);
const _: () = assert!(core::mem::align_of::<ImageState>() == 1);
const _: () = assert!(ImageState::Settled as u8 == 0 && ImageState::Pending as u8 == 1 && ImageState::Unreadable as u8 == 2);

pub const IMAGE_READBACK_ATTEMPTS: usize = 3;
pub fn image_confirmed(first: FirstScreen) -> bool { image_confirmed_raw(first as u8) }
/// Raw wire bytes may hold unnamed discriminants; retain `>=` semantics.
pub fn image_confirmed_raw(first: u8) -> bool { first >= FirstScreen::Confirmed as u8 }

/// Required short borrowed operations; no platform implementation or defaults.
pub trait Image {
    /// Existing ImageState discriminants, with unnamed raw bytes preserved.
    /// Only Unreadable is retried; every other non-Settled readback stops false.
    fn state(&mut self) -> u8;
    fn mark_valid(&mut self) -> bool;
}
pub fn confirm_image(image: &mut dyn Image) -> bool {
    if image.state() == ImageState::Settled as u8 { return true; }
    let _ = image.mark_valid();
    for _ in 0..IMAGE_READBACK_ATTEMPTS {
        let state = image.state();
        if state != ImageState::Unreadable as u8 { return state == ImageState::Settled as u8; }
    }
    false
}
