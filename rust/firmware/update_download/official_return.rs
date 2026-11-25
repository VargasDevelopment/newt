// SPDX-License-Identifier: MIT
//! One disposable, target-bound sidecar; it grants no confirmation or flash loan.
#![forbid(unsafe_code)]
use core::ffi::CStr;
use newt_core::{official_return::{self, ReturnAuthorization, ReturnIssue, RECEIPT_BYTES},
    update_card::slot_installer::identity::NewtAppIdentity, update_manifest::UpdateManifest, update_online::TestBuilds};
use newt_native_io::{OsNative, raw_download::MISSING_ERRNO};
use newt_native_storage::{Kind, Native, Open};

pub struct Store<'ui>(OsNative<'ui>);
impl<'ui> Store<'ui> {
    pub fn new(io: &'ui newt_native_io::UiIo) -> Self { Self(io.port()) }
    fn remove_path(&mut self, path: &CStr) -> bool {
        match self.0.unlink_raw(path) { Ok(()) => true, Err(error) => error == MISSING_ERRNO }
    }
    pub fn remove(&mut self) -> bool { self.remove_path(official_return::RECEIPT) }
    fn read_path(&mut self, path: &CStr) -> Option<[u8; RECEIPT_BYTES]> {
        let fd = self.0.open(path.to_str().ok()?, Open::SourceReadOnly).ok()?;
        let mut bytes = [0; RECEIPT_BYTES];
        let read = self.0.fstat(fd).is_ok_and(|info| info.kind == Kind::Regular && info.size == RECEIPT_BYTES as i64)
            && (|| {
                let mut at = 0;
                while at < bytes.len() {
                    let got = self.0.read(fd, &mut bytes[at..]).ok()?;
                    if got == 0 || got > bytes.len() - at { return None; }
                    at += got;
                }
                Some(())
            })().is_some();
        let closed = self.0.close(fd).is_ok();
        (read && closed).then_some(bytes)
    }
    pub fn save(&mut self, authorization: &ReturnAuthorization) -> bool {
        let bytes = authorization.receipt();
        // Invalidate the previous offer before any partial replacement exists.
        if !self.remove() || !self.remove_path(official_return::TEMP) { return false; }
        let Ok(fd) = self.0.open_truncate_raw(official_return::TEMP, 0o644) else { return false; };
        let written = (|| {
            let mut at = 0;
            while at < bytes.len() {
                let put = self.0.write_raw(fd, &bytes[at..]).ok()?;
                if put == 0 || put > bytes.len() - at { return None; }
                at += put;
            }
            self.0.sync_raw(fd).ok()
        })().is_some();
        let closed = self.0.close_raw(fd).is_ok();
        let saved = written && closed && self.read_path(official_return::TEMP) == Some(bytes)
            && self.0.rename_cstr(official_return::TEMP, official_return::RECEIPT).is_ok()
            && self.read_path(official_return::RECEIPT) == Some(bytes);
        if !saved { let _ = self.remove(); }
        let _ = self.remove_path(official_return::TEMP);
        saved
    }
    pub fn restore(&mut self, source: &NewtAppIdentity, target: &UpdateManifest, channel: TestBuilds) -> Result<ReturnAuthorization, ReturnIssue> {
        let result = self.read_path(official_return::RECEIPT).ok_or(ReturnIssue::CheckAgain)
            .and_then(|bytes| ReturnAuthorization::restore(&bytes, source, target, channel));
        if result.is_err() { let _ = self.remove(); }
        result
    }
}
