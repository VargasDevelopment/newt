// SPDX-License-Identifier: MIT
//! Thin synchronous SDK grayscale operations. Rendering and policy stay Rust.
use super::{Board, Error, FRAME_BYTES, Refresh, status};
use core::ffi::c_void;
#[repr(u32)]
#[derive(Clone, Copy)]
pub enum GrayMode { Overlay = 0, Absolute = 1, Direct = 2 }
#[repr(u32)]
#[derive(Clone, Copy)]
pub enum GrayPlane { Lsb = 0, Msb = 1 }
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GrayEncoding { Unsupported, OverlayMasks, AbsolutePlanes }
// SAFETY: native grayscale.cpp forwards existing synchronous SDK calls; the
// exclusive Board loan excludes every outstanding SDK frame/parser loan.
unsafe extern "C" {
    fn nb_gray_encoding(display: *mut c_void, mode: u32, out: *mut u32) -> i32;
    fn nb_gray_base(display: *mut c_void, mode: u32, binary: u32, refresh: u32, off: u32, accepted: *mut u32) -> i32;
    fn nb_gray_copy(display: *mut c_void, plane: u32, bytes: *const u8, len: usize) -> i32;
    fn nb_gray_display(display: *mut c_void, off: u32) -> i32;
    fn nb_gray_binary_focus(display: *mut c_void, mask: u32, off: u32, accepted: *mut u32) -> i32;
    fn nb_gray_cleanup_frame(display: *mut c_void) -> i32;
    fn nb_gray_resync(display: *mut c_void) -> i32;
}
impl Board<'_> {
    pub fn gray_encoding(&mut self, mode: GrayMode) -> Result<GrayEncoding, Error> {
        let mut value = 0;
        // SAFETY: initialized exclusive display and live scalar output.
        status(unsafe { nb_gray_encoding(self.display.as_ptr(), mode as u32, &mut value) })?;
        match value { 0 => Ok(GrayEncoding::Unsupported), 1 => Ok(GrayEncoding::OverlayMasks),
            2 => Ok(GrayEncoding::AbsolutePlanes), _ => Err(Error::Mode) }
    }
    pub fn gray_base(&mut self, mode: GrayMode, binary: bool, refresh: Refresh, off: bool) -> Result<bool, Error> {
        let mut accepted = 0;
        // SAFETY: all SDK/frame loans ended; enums and booleans match the ABI.
        status(unsafe { nb_gray_base(self.display.as_ptr(), mode as u32, u32::from(binary), refresh as u32, u32::from(off), &mut accepted) })?;
        Ok(accepted != 0)
    }
    pub fn gray_copy_frame(&mut self, plane: GrayPlane) -> Result<(), Error> {
        // SAFETY: null selects the SDK's existing primary frame, with no live loan.
        status(unsafe { nb_gray_copy(self.display.as_ptr(), plane as u32, core::ptr::null(), 0) })
    }
    pub fn gray_copy_selectors(&mut self, plane: GrayPlane, selectors: &[u8; FRAME_BYTES]) -> Result<(), Error> {
        // SAFETY: exact caller-owned selectors remain immutable through the call.
        status(unsafe { nb_gray_copy(self.display.as_ptr(), plane as u32, selectors.as_ptr(), selectors.len()) })
    }
    /// Show the uploaded gray planes, then power the panel off: a gray image
    /// may sit for minutes, and one left on a powered panel greys (#318). The
    /// next paint pays one power-on (about 40 ms on the bench X3).
    pub fn gray_display(&mut self) -> Result<(), Error> {
        // SAFETY: exclusive display, completed selector uploads, no live frame loan.
        status(unsafe { nb_gray_display(self.display.as_ptr(), 1) })
    }
    pub fn gray_binary_focus(&mut self, mask: u32, off: bool) -> Result<bool, Error> {
        let mut accepted = 0;
        // SAFETY: SDK consumes its existing frame synchronously after its loan ends.
        status(unsafe { nb_gray_binary_focus(self.display.as_ptr(), mask, u32::from(off), &mut accepted) })?;
        Ok(accepted != 0)
    }
    pub fn gray_cleanup_frame(&mut self) -> Result<(), Error> {
        // SAFETY: the SDK primary frame loan ended before this synchronous call.
        status(unsafe { nb_gray_cleanup_frame(self.display.as_ptr()) })
    }
    pub fn gray_resync(&mut self) -> Result<(), Error> {
        // SAFETY: synchronous SDK flag change under the sole display owner.
        status(unsafe { nb_gray_resync(self.display.as_ptr()) })
    }
}
