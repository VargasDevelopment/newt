// SPDX-License-Identifier: MIT
//! Eligibility to return from a nightly to the official pair selected by Check.
//! The sidecar survives update-mode restart; it never stores reader consent.
#![forbid(unsafe_code)]
use core::ffi::CStr;
use crate::{update_card::slot_installer::identity::{NewtAppIdentity, exact_build_id},
    update_manifest::{UpdateKind, UpdateManifest, semver2_compare, semver2_is_nightly, semver2_is_stable},
    update_online::TestBuilds, wire::{crc32, get32, put32}};

/// Raise after any incompatible durable writer or payload-meaning change.
/// Compatibility vectors cover the records supported by this official build.
pub const MINIMUM: &CStr = c"0.5.0";
pub const RECEIPT_BYTES: usize = 84;
pub const RECEIPT: &CStr = c"/newt-sd/Newt/Update/official.return";
pub const TEMP: &CStr = c"/newt-sd/Newt/Update/official.return.tmp";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReturnIssue { CheckAgain, Incompatible }

/// Only a successful official Check or a validated sidecar can create this.
#[derive(Clone, Copy, Debug)]
pub struct ReturnAuthorization { source: NewtAppIdentity, image: [u8; 32] }

/// Created at the explicit older-version confirmation, never when Check ends.
#[derive(Clone, Copy, Debug)]
pub struct ConfirmedReturn(ReturnAuthorization);

fn compatible(source: &NewtAppIdentity, target: &UpdateManifest, channel: TestBuilds) -> Result<(), ReturnIssue> {
    let running = CStr::from_bytes_until_nul(&source.version).map_err(|_| ReturnIssue::CheckAgain)?;
    let build = CStr::from_bytes_until_nul(&source.build).map_err(|_| ReturnIssue::CheckAgain)?;
    let version = CStr::from_bytes_until_nul(&target.version).map_err(|_| ReturnIssue::CheckAgain)?;
    if channel != TestBuilds::Off || !semver2_is_nightly(Some(running)) || !exact_build_id(Some(build))
        || target.kind != UpdateKind::Newt || !semver2_is_stable(Some(version))
        || semver2_compare(Some(version), Some(running)) >= 0 {
        return Err(ReturnIssue::CheckAgain);
    }
    if semver2_compare(Some(version), Some(MINIMUM)) < 0 { return Err(ReturnIssue::Incompatible); }
    Ok(())
}
impl ReturnAuthorization {
    /// Called only after the Off-channel latest endpoint supplied this manifest.
    pub(crate) fn from_check(source: &NewtAppIdentity, target: &UpdateManifest, channel: TestBuilds) -> Result<Self, ReturnIssue> {
        compatible(source, target, channel)?;
        Ok(Self { source: *source, image: target.sha256 })
    }
    pub fn matches(&self, source: &NewtAppIdentity, target: &UpdateManifest, channel: TestBuilds) -> bool {
        self.source == *source && self.image == target.sha256 && compatible(source, target, channel).is_ok()
    }
    pub fn targets(&self, running: &CStr, target: &UpdateManifest) -> bool {
        CStr::from_bytes_until_nul(&self.source.version).ok() == Some(running)
            && self.image == target.sha256 && compatible(&self.source, target, TestBuilds::Off).is_ok()
    }
    pub fn receipt(&self) -> [u8; RECEIPT_BYTES] {
        let mut out = [0; RECEIPT_BYTES];
        out[..4].copy_from_slice(b"NWOR"); put32(&mut out, 4, 1);
        out[8..32].copy_from_slice(&self.source.version);
        out[32..48].copy_from_slice(&self.source.build);
        out[48..80].copy_from_slice(&self.image);
        let checksum = crc32(&out[..80]); put32(&mut out, 80, checksum);
        out
    }
    pub fn restore(bytes: &[u8], source: &NewtAppIdentity, target: &UpdateManifest, channel: TestBuilds) -> Result<Self, ReturnIssue> {
        if bytes.len() != RECEIPT_BYTES || &bytes[..4] != b"NWOR" || get32(bytes, 4) != 1
            || get32(bytes, 80) != crc32(&bytes[..80]) { return Err(ReturnIssue::CheckAgain); }
        let mut saved = Self { source: NewtAppIdentity::default(), image: [0; 32] };
        saved.source.version.copy_from_slice(&bytes[8..32]);
        saved.source.build.copy_from_slice(&bytes[32..48]); saved.image.copy_from_slice(&bytes[48..80]);
        if saved.source != *source || saved.image != target.sha256 { return Err(ReturnIssue::CheckAgain); }
        compatible(source, target, channel)?;
        Ok(saved)
    }
    pub fn confirm(self) -> ConfirmedReturn { ConfirmedReturn(self) }
}
impl ConfirmedReturn {
    pub fn authorization(&self) -> ReturnAuthorization { self.0 }
    /// Installer reads the running slot identity again immediately before erase.
    pub fn matches(&self, source: &NewtAppIdentity, target: &UpdateManifest) -> bool {
        self.0.matches(source, target, TestBuilds::Off)
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod online_tests;
