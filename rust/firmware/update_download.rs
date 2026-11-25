// SPDX-License-Identifier: MIT
//! Frozen firmware/update_card.h CardDownload, over the same short native
//! domain. Core owns retry/verification policy; this owns only the real fd,
//! byte credit and DEV first failure/slowest write counterparts.
#![forbid(unsafe_code)]
use newt_core::{update_card::{DIRECTORY, NEWT_IMAGE, NEWT_MANIFEST}, update_online::CardDownload};
use newt_device_ui::update::CardStep;
#[cfg(feature = "dev-usb")]
use newt_device_ui::update::CardFailure;
use newt_native_io::{OsNative, raw_download::{EXISTS_ERRNO, MISSING_ERRNO}};

pub(crate) mod official_return;

pub struct Download<'ui> {
    native: OsNative<'ui>,
    io: &'ui newt_native_io::UiIo,
    fd: i32,
    written: u32,
    #[cfg(feature = "dev-usb")]
    pub failure: CardFailure,
    #[cfg(feature = "dev-usb")]
    pub slowest_write_ms: u32,
}
impl<'ui> Download<'ui> {
    pub fn new(io: &'ui newt_native_io::UiIo) -> Self {
        Self { native: io.port(), io, fd: -1, written: 0,
            #[cfg(feature = "dev-usb")] failure: CardFailure::default(),
            #[cfg(feature = "dev-usb")] slowest_write_ms: 0 }
    }
    fn failed(&mut self, step: CardStep, error: i32) -> bool {
        #[cfg(feature = "dev-usb")]
        if self.failure.step == CardStep::None {
            self.failure = CardFailure { step, error, at: self.written, tries: self.failure.tries };
        }
        #[cfg(not(feature = "dev-usb"))]
        let _ = (step, error);
        false
    }
    fn closed(&mut self, fd: i32, step: CardStep) -> bool {
        fd >= 0 && match self.native.close_raw(fd) { Ok(()) => true, Err(error) => self.failed(step, error) }
    }
    fn close_image(&mut self) -> bool {
        let done = self.closed(self.fd, CardStep::CloseImage);
        self.fd = -1;
        done
    }
    fn write_all(&mut self, fd: i32, mut bytes: &[u8], step: CardStep) -> bool {
        while !bytes.is_empty() {
            let put = match self.native.write_raw(fd, bytes) {
                Ok(0) => return self.failed(step, 0),
                Ok(put) => put,
                Err(error) => return self.failed(step, error),
            };
            // The synchronous POSIX leaf cannot exceed the supplied count.
            // Refuse a violated native contract before a Rust bounds panic.
            let Some(rest) = bytes.get(put..) else { return self.failed(step, 0); };
            bytes = rest;
            self.written = self.written.wrapping_add(put as u32);
        }
        true
    }
}
impl CardDownload for Download<'_> {
    fn begin(&mut self) -> bool {
        #[cfg(feature = "dev-usb")]
        { self.failure.tries = self.failure.tries.wrapping_add(1); }
        if let Err(error) = self.native.mkdir_mode_raw(DIRECTORY, 0o755) {
            if error != EXISTS_ERRNO { return self.failed(CardStep::MakeFolder, error); }
        }
        if let Err(error) = self.native.unlink_raw(NEWT_MANIFEST) {
            if error != MISSING_ERRNO { return self.failed(CardStep::RemoveManifest, error); }
        }
        if !official_return::Store::new(self.io).remove() { return false; }
        self.written = 0;
        match self.native.open_truncate_raw(NEWT_IMAGE, 0o644) {
            Ok(fd) => { self.fd = fd; fd >= 0 }
            Err(error) => { self.fd = -1; self.failed(CardStep::OpenImage, error) }
        }
    }
    fn write(&mut self, bytes: &[u8]) -> bool {
        #[cfg(feature = "dev-usb")]
        let started = self.native.monotonic_millis();
        let done = self.fd >= 0 && self.write_all(self.fd, bytes, CardStep::WriteImage);
        #[cfg(feature = "dev-usb")]
        {
            let took = self.native.monotonic_millis().wrapping_sub(started);
            if took > self.slowest_write_ms { self.slowest_write_ms = took; }
        }
        done
    }
    fn finish(&mut self, manifest: &[u8]) -> bool {
        let mut written = self.fd >= 0 && match self.native.sync_raw(self.fd) {
            Ok(()) => true, Err(error) => self.failed(CardStep::SaveImage, error),
        };
        if !self.close_image() || !written { return false; }
        let fd = match self.native.open_truncate_raw(NEWT_MANIFEST, 0o644) {
            Ok(fd) => fd,
            Err(error) => { self.failed(CardStep::OpenManifest, error); -1 }
        };
        written = fd >= 0 && self.write_all(fd, manifest, CardStep::WriteManifest)
            && match self.native.sync_raw(fd) {
                Ok(()) => true, Err(error) => self.failed(CardStep::SaveManifest, error),
            };
        if !self.closed(fd, CardStep::CloseManifest) || !written {
            let _ = self.native.unlink_raw(NEWT_MANIFEST);
            return false;
        }
        true
    }
    fn finish_return(&mut self, manifest: &[u8], authorization: &newt_core::official_return::ReturnAuthorization) -> bool {
        self.finish(manifest) && official_return::Store::new(self.io).save(authorization)
    }
    fn abort(&mut self) { self.close_image(); let _ = self.native.unlink_raw(NEWT_IMAGE); }
}
impl Drop for Download<'_> {
    fn drop(&mut self) { if self.fd >= 0 { let _ = self.native.close_raw(self.fd); } }
}
