// SPDX-License-Identifier: MIT

//! The State medium while Board has no mounted card. Board mints no native I/O
//! then, so nothing here can mount, and no reader Pages lease exists to
//! withdraw. Retrying the mount is Board::ensure_sd_mounted's job; after it
//! succeeds, startup builds the mounted router from the same StateFacts.
use newt_native_storage::reading_state::{FIRMWARE_STATE, FIRMWARE_STATS, StateMedium};

pub struct NoCard;

impl<N: newt_native_storage::Native> StateMedium<N> for NoCard {
    const STATE: &'static str = FIRMWARE_STATE;
    const STATS: &'static str = FIRMWARE_STATS;
    fn release_pages_for_state(&mut self) {}
    fn mount(&mut self, _: Option<&mut N>) -> bool {
        false
    }
}
