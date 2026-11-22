// SPDX-License-Identifier: MIT
//! The existing session flags, alignment gap and producer view share four
//! bytes. Keeping them with verification avoids paying for that gap twice.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Phase { Uninitialized, Ready, Pending, Changed, Failed }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PrefetchTrace { Idle, Begin, Stored, Refused }

/// Frozen six flag bits. Verification's private initialization distinction uses
/// the old padding byte; it adds no new producer policy or source authority.
pub struct SessionFlags(u8);
impl SessionFlags {
    pub fn prepared(&self) -> bool { self.0 & 1 != 0 }
    pub fn strict(&self) -> bool { self.0 & 2 != 0 }
    pub fn title_published(&self) -> bool { self.0 & 4 != 0 }
    pub fn title_only_projection(&self) -> bool { self.0 & 8 != 0 }
    pub fn set_prepared(&mut self, value: bool) { self.set(1, value); }
    pub fn set_strict(&mut self, value: bool) { self.set(2, value); }
    pub fn set_title_published(&mut self, value: bool) { self.set(4, value); }
    pub fn set_title_only_projection(&mut self, value: bool) { self.set(8, value); }
    pub fn prefetch_trace(&self) -> PrefetchTrace {
        match self.0 >> 4 {
            0 => PrefetchTrace::Idle, 1 => PrefetchTrace::Begin,
            2 => PrefetchTrace::Stored, _ => PrefetchTrace::Refused,
        }
    }
    pub fn set_prefetch_trace(&mut self, value: PrefetchTrace) {
        self.0 = (self.0 & 15) | ((value as u8) << 4);
    }
    fn set(&mut self, bit: u8, value: bool) {
        self.0 = (self.0 & !bit) | if value { bit } else { 0 };
    }
}

#[repr(C)]
pub struct SessionControl {
    pub(super) phase: Phase,
    pub flags: SessionFlags,
    pub producer_view: u16,
}
impl SessionControl {
    pub(super) fn new() -> Self {
        Self { phase: Phase::Uninitialized, flags: SessionFlags(0), producer_view: u16::MAX }
    }
}
const _: () = {
    assert!(core::mem::size_of::<SessionFlags>() == 1);
    assert!(core::mem::size_of::<SessionControl>() == 4);
    assert!(core::mem::align_of::<SessionControl>() == 2);
};
