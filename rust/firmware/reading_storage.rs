// SPDX-License-Identifier: MIT
//! Explicit startup loans of the frozen firmware's static reading objects.
use core::mem::MaybeUninit;
use crate::app_runtime::home_refresh::HomeRefresh;
use newt_stage_home::HomeData;
use newt_tiny_app::Definition;

/// Created once by the sole startup callback; no ambient object access exists.
pub struct ReadingStorage {
    pub(crate) definition: &'static mut MaybeUninit<Definition>,
    pub(crate) home_refresh: &'static mut MaybeUninit<HomeRefresh>,
    #[cfg(feature = "dev-usb")]
    pub(crate) bridge: &'static mut MaybeUninit<crate::dev_usb::Bridge>,
    #[cfg(feature = "dev-usb")]
    pub(crate) panels: &'static mut MaybeUninit<crate::dev_usb::PanelRing>,
    pub(crate) title: &'static mut MaybeUninit<[u8; 256]>,
    pub(crate) home: &'static mut MaybeUninit<HomeData<'static>>,
    pub(crate) picture: &'static mut MaybeUninit<newt_core::resume_picture::Mark>,
}

/// Construction scratch ends before the returned owner's boot-long loan.
#[inline(never)]
pub(crate) fn initialize<T>(slot: &'static mut MaybeUninit<T>, make: impl FnOnce() -> T) -> &'static mut T {
    slot.write(make())
}
