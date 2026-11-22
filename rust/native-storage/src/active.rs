// SPDX-License-Identifier: MIT
//! Actual transaction owner: fixed SD facts and its own legacy NVS state.
use super::{FileName, UNKNOWN};

#[repr(C)]
pub(super) struct Active<T> {
    pub(super) record: Record,
    pub(super) device: T,
}

// Grouping the SD fields permits disjoint NVS/SD loans during migration.
// The frozen fd is an int with -1 invalid; Text<80> is exactly char[80].
#[repr(C)]
pub(super) struct Record {
    pub(super) legacy: bool,
    pub(super) buffered: bool,
    pub(super) stats: bool,
    pub(super) fd: i32,
    pub(super) file: FileName,
    pub(super) known: [i8; 3],
    pub(super) known_generation: [u32; 3],
}
impl Record {
    pub(super) const NEW: Self = Self {
        legacy: false, buffered: false, stats: false, fd: -1,
        file: FileName::empty(), known: [UNKNOWN; 3], known_generation: [0; 3],
    };
}
const _: () = assert!(core::mem::size_of::<Record>() == 104);
const _: () = assert!(core::mem::align_of::<Record>() == 4);
