// SPDX-License-Identifier: MIT
use newt_core::{app::Screen, frame::Frame};

/// Only facts observed from the actual reader; constructed per loop turn.
pub struct ReaderFacts {
    pub status: u32,
    pub opened: bool,
    pub progress_ready: bool,
    pub spine: u32,
    pub character: u32,
    pub book: u64,
    /// A changed source or an interrupted open: idle reopens it.
    pub reopen_owed: bool,
}
impl ReaderFacts {
    /// An owed reopen keeps the paper image: a changed-source reopen can lend
    /// logical pixels as scratch, and an interrupted open has nothing to paint.
    pub fn defers_paint(&self, screen: Screen) -> bool {
        screen == Screen::EpubReading && !self.opened && self.reopen_owed
    }
    pub fn frame<'frame>(&self, screen: Screen, logical: &'frame Frame) -> Option<&'frame Frame> {
        if self.defers_paint(screen) { None } else { Some(logical) }
    }
}
