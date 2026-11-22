//! Construction in owned storage, without a whole-state stack temporary.
use super::{BitBuffer, Phase, State};
use core::mem::{offset_of, size_of};

impl State {
    /// Write the native representation of `new(mode)` into an exact-sized
    /// byte region. The storage owner must check alignment before viewing it
    /// as State. These bytes are not a persistent format.
    pub fn initialize_storage(storage: &mut [u8], mode: BitBuffer) -> bool {
        if storage.len() != size_of::<Self>() { return false; }
        // Every field is an integer, integer array, bool or repr(u8) enum.
        // Zero is valid for all fields; new's nonzero values are written below.
        storage.fill(0);
        storage[offset_of!(Self, phase)] = Phase::Start as u8;
        storage[offset_of!(Self, mode)] = mode as u8;
        let trailer = offset_of!(Self, trailer);
        storage[trailer..trailer + size_of::<u32>()].copy_from_slice(&1u32.to_ne_bytes());
        true
    }
}
