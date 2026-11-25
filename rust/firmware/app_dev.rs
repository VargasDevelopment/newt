// SPDX-License-Identifier: MIT
//! Short DEV views over the application's completed publication and real queue.
#![forbid(unsafe_code)]
use crate::dev_usb::{self, Bridge, ButtonEvent, Channel, Device, Diagnostics, PanelRing, Status};
use newt_c3_board::image::running_image_version;
use newt_core::dev_put::{self, Busy, Session};
use newt_input_policy::native::UiQueue;

/// Whether this view may grant a card-write lease, and the one it granted.
#[derive(Clone, Copy)]
pub enum Put { Unavailable, Admits(dev_put::Facts), Granted(Session) }

/// All facts belong to the caller's current completed publication. The view
/// ends before dispatch, paint, parser work or a physical-input takeover.
pub struct Publication<'a, 'queue> {
    pub status: Status,
    pub frame: Option<&'a [u8; dev_usb::FRAME_BYTES]>,
    pub queue: &'a mut UiQueue<'queue>,
    pub panels: &'a PanelRing,
    pub diagnostics: Diagnostics,
    pub put: Put,
    #[cfg(feature = "wake-timing")]
    pub wake: &'a newt_core::wake_timing::Timeline,
}
impl Device for Publication<'_, '_> {
    fn status(&mut self) -> Status { self.status }
    fn frame(&mut self) -> Option<&[u8; dev_usb::FRAME_BYTES]> { self.frame }
    fn enqueue(&mut self, event: ButtonEvent) -> bool {
        self.queue.enqueue_ticket(event.ticket, event.button)
    }
    fn panel_ring(&mut self) -> Option<&PanelRing> { Some(self.panels) }
    fn diagnostics(&mut self) -> Diagnostics { self.diagnostics }
    #[cfg(feature = "wake-timing")]
    fn wake_timing(&self) -> Option<[u32; newt_core::wake_timing::WORDS]> { self.wake.snapshot() }
    fn put(&mut self, session: Session) -> Result<(), Busy> {
        let Put::Admits(facts) = self.put else { return Err(Busy::NotHome) };
        dev_put::admit(facts)?;
        self.put = Put::Granted(session);
        Ok(())
    }
}
pub fn poll(bridge: &mut Bridge, channel: &mut dyn Channel,
            publication: &mut Publication<'_, '_>, build: &str, answer: bool) {
    bridge.poll(channel, publication, build, answer);
}

/// Same private install-receipt identity as frozen device_fingerprint.h. Only
/// the eight-byte digest prefix enters DEV; the MAC text stays on this stack.
pub fn fingerprint(hardware: &mut newt_c3_board::hardware::HardwareReader<'_>) -> u64 {
    let Ok(mac) = hardware.factory_mac() else { return 0; };
    const HEX: &[u8;16] = b"0123456789ABCDEF";
    let mut identity = [0;17];
    for (index, byte) in mac.iter().copied().enumerate() {
        identity[index*3] = HEX[usize::from(byte >> 4)];
        identity[index*3+1] = HEX[usize::from(byte & 15)];
        if index < 5 { identity[index*3+2] = b':'; }
    }
    let mut digest = [0;32];
    if !crate::reader_hash::package_hash(&identity, &mut digest) { return 0; }
    digest[..8].iter().fold(0, |value, byte| (value << 8) | u64::from(*byte))
}
pub const BUILD_ID: &str = env!("NEWT_DEV_BUILD_ID");
pub const BUILD_DATE: &str = env!("NEWT_DEV_BUILD_DATE");

pub(crate) fn build_identity() -> newt_device_ui::BuildIdentity<'static> {
    newt_device_ui::BuildIdentity {
        version: running_image_version().to_bytes(),
        id: Some(BUILD_ID.as_bytes()),
        date: Some(BUILD_DATE.as_bytes()),
    }
}

const fn fixed_text(bytes: &'static [u8]) -> &'static core::ffi::CStr {
    let parsed = core::ffi::CStr::from_bytes_with_nul(bytes);
    assert!(parsed.is_ok(), "invalid fixed image identity");
    match parsed { Ok(text) => text, Err(_) => c"" }
}

/// The version comes from the mapped image descriptor. Dirty IDs
/// are rejected by the existing exact-twelve rule, leaving its build empty.
pub fn image_identity() -> newt_core::update_card::slot_installer::identity::NewtAppIdentity {
    const BUILD: &core::ffi::CStr = fixed_text(concat!(env!("NEWT_DEV_BUILD_ID"), "\0").as_bytes());
    newt_core::update_card::slot_installer::identity::newt_app_identity(running_image_version(), Some(BUILD))
}
