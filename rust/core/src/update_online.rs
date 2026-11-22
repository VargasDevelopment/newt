// SPDX-License-Identifier: MIT
//! Online firmware release check and download policy. MIT. Only short loans over caller-supplied transient regions.
#![forbid(unsafe_code)]
use core::ffi::CStr;
use crate::{update_manifest::{UpdateManifest, UpdateKind, parse_update_manifest, semver2_compare}, update_release::ReleaseJson};
#[path = "update_online/check.rs"]
mod check;
pub use check::{fetch_online_update, fetch_online_update_with_http};
#[cfg(test)]
#[path = "update_online/loan_tests.rs"]
mod loan_tests;

pub const URL_BYTES: usize = 2048;
pub const READ_BYTES: usize = 2048;
pub const LATEST_RELEASE_URL: &CStr = c"https://api.github.com/repos/VargasDevelopment/newt/releases/latest";
pub const USABLE_SLOT_BYTES: u32 = 7016448;
pub const SLOT_HEAD_BYTES: u32 = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum OnlineCheck { Joining, Looking, Downloading, Ready, Latest, NoNetwork, NoJoin, Unreachable, NoRelease, WrongFile, Interrupted, CardFailed }
impl OnlineCheck { pub fn running(self) -> bool { (self as u8) < Self::Ready as u8 } }
#[derive(Clone, Copy, Debug)]
pub struct HttpHead { pub status: i32, pub length: i64, pub location_cut: bool, pub date: u32 }
impl Default for HttpHead { fn default() -> Self { Self { status: 0, length: -1, location_cut: false, date: 0 } } }

/// Client enforces its deadline on every open/read, including slow calls.
/// The borrowed clock replaces the client's platform clock, not policy timing.
pub trait Clock { fn millis(&mut self) -> u32; }
pub trait Http {
    fn limit(&mut self, milliseconds: u32, clock: &mut dyn Clock);
    /// Location is NUL-terminated unless location_cut; no source loan is retained.
    fn open(&mut self, url: &CStr, api: bool, head: &mut HttpHead, location: &mut [u8; URL_BYTES], clock: &mut dyn Clock) -> bool;
    /// Return a count within the lent slice, zero EOF, or negative failure.
    fn read(&mut self, bytes: &mut [u8], clock: &mut dyn Clock) -> i32;
    fn close(&mut self);
}
pub trait CardDownload {
    /// Remove the previous manifest; create/truncate the image for this attempt.
    fn begin(&mut self) -> bool;
    fn write(&mut self, bytes: &[u8]) -> bool;
    /// Sync/close image, then write/sync/close manifest last. False is a failure,
    /// not authority for the policy to erase or retry this possibly durable pair.
    fn finish(&mut self, manifest: &[u8]) -> bool;
    fn abort(&mut self);
}
pub trait Hash {
    fn start(&mut self) -> bool;
    fn update(&mut self, bytes: &[u8]) -> bool;
    fn finish(&mut self, digest: &mut [u8; 32]) -> bool;
}
pub trait Progress { fn report(&mut self, done: u32, total: u32); }

/// Descriptors only: no Default/constructor that creates an owned ~8KiB Work.
/// Owner supplies its existing regions and fresh ReleaseJson/length in place.
pub struct OnlineWork<'a> {
    pub url: &'a mut [u8; URL_BYTES],
    pub location: &'a mut [u8; URL_BYTES],
    pub release: &'a mut ReleaseJson,
    pub manifest: &'a mut [u8; crate::update_manifest::MAX_BYTES],
    pub manifest_length: &'a mut usize,
    pub buffer: &'a mut [u8; READ_BYTES],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyEnd { Whole, TooLong, Broken, Refused }
pub fn copy_url(out: &mut [u8; URL_BYTES], url: &CStr) -> bool {
    let bytes = url.to_bytes_with_nul();
    if bytes.len() > out.len() { return false; }
    out[..bytes.len()].copy_from_slice(bytes);
    true
}
pub fn open_following<H: Http + ?Sized>(http: &mut H, clock: &mut dyn Clock, url: &mut [u8; URL_BYTES], location: &mut [u8; URL_BYTES], api: bool, head: &mut HttpHead) -> bool {
    for hop in 0..=5 {
        if !url.starts_with(b"https://") { return false; }
        *head = HttpHead::default(); location[0] = 0;
        let Ok(address) = CStr::from_bytes_until_nul(url) else { return false };
        if !http.open(address, api, head, location, clock) { http.close(); return false; }
        if !matches!(head.status, 301 | 302 | 303 | 307 | 308) { return true; }
        http.close();
        if hop == 5 || head.location_cut || location[0] == 0 { return false; }
        url.copy_from_slice(location);
    }
    false
}
pub fn read_body(http: &mut dyn Http, clock: &mut dyn Clock, buffer: &mut [u8; READ_BYTES], cap: u32, take: &mut dyn FnMut(&[u8]) -> bool) -> BodyEnd {
    read_body_with_http(http, clock, buffer, cap, &mut |_, bytes| take(bytes))
}
/// The read's mutable HTTP loan ends before `take` receives it. A concrete
/// client can then lend its reclaimed frame without a separate permission.
pub fn read_body_with_http<H: Http + ?Sized>(http: &mut H, clock: &mut dyn Clock, buffer: &mut [u8; READ_BYTES], cap: u32, take: &mut dyn FnMut(&mut H, &[u8]) -> bool) -> BodyEnd {
    let mut total = 0;
    loop {
        let got = http.read(buffer, clock);
        if got < 0 { return BodyEnd::Broken; }
        if got == 0 { return BodyEnd::Whole; }
        if got as u32 > cap - total { return BodyEnd::TooLong; }
        let Some(bytes) = buffer.get(..got as usize) else { return BodyEnd::Broken };
        total += got as u32;
        if !take(http, bytes) { return BodyEnd::Refused; }
    }
}
