// SPDX-License-Identifier: MIT
use super::{FRAME_BYTES, PanelRing};

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ButtonEvent {
    pub ticket: u32,
    pub button: u8,
}
const _: () = assert!(core::mem::size_of::<ButtonEvent>() == 8);

#[derive(Clone, Copy, Default)]
pub struct Status {
    pub generation: u32,
    pub refresh_ms: u32,
    pub free_heap: u32,
    pub dropped: u32,
    pub screen: u32,
    pub width: u32,
    pub height: u32,
    pub ready: bool,
    pub landscape: bool,
    pub monochrome: bool,
}

#[derive(Clone, Copy)]
pub struct Diagnostics {
    pub available: bool,
    pub reset: u32,
    pub cause: u32,
    pub record: u32,
    pub requested: u32,
    pub restored: bool,
    pub checkpoint_saved: bool,
    pub book: u64,
    pub spine: u32,
    pub character: u32,
    pub reader_status: u32,
    pub reader_open: bool,
    pub progress_ready: bool,
    pub actual_spine: u32,
    pub actual_character: u32,
    pub first_input: u32,
    pub current_book: u64,
    pub free_heap: u32,
    pub largest_block: u32,
    pub opening: bool,
    pub library_count: u32,
    pub library_selected: u32,
    pub device_fingerprint: u64,
}
impl Default for Diagnostics {
    fn default() -> Self {
        Self {
            available: false, reset: 0, cause: 0, record: 0, requested: 0,
            restored: false, checkpoint_saved: false, book: 0, spine: 0,
            character: 0, reader_status: 0, reader_open: false,
            progress_ready: false, actual_spine: 0, actual_character: 0,
            first_input: 255, current_book: 0, free_heap: 0, largest_block: 0,
            opening: false, library_count: 0, library_selected: 0,
            device_fingerprint: 0,
        }
    }
}

pub trait Channel {
    fn connected(&mut self) -> bool;
    fn read_byte(&mut self) -> i32;
    fn write(&mut self, bytes: &[u8]) -> usize;
    fn now(&mut self) -> u32;
}

/// All observations are side-effect free. During a poll, frame() is stable and
/// borrows only a completed publication; it never exposes a parser/paint loan.
/// Unavailable observations are explicit, not a successful default provider.
pub trait Device {
    fn status(&mut self) -> Status;
    fn frame(&mut self) -> Option<&[u8; FRAME_BYTES]>;
    fn enqueue(&mut self, event: ButtonEvent) -> bool;
    fn panel_ring(&mut self) -> Option<&PanelRing>;
    fn diagnostics(&mut self) -> Diagnostics;
    /// Grants one card-write lease to this PUT, or says why not. The lease
    /// itself runs after the poll, outside every observation.
    fn put(&mut self, session: newt_core::dev_put::Session) -> Result<(), newt_core::dev_put::Busy>;
}
