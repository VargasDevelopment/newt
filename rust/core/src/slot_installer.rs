// SPDX-License-Identifier: MIT
//! Alternate-slot firmware installer policy, registered as a child of `update_card` to reuse its classifier. MIT. Only caller-lent sector buffers are retained.
#![forbid(unsafe_code)]
use core::ffi::CStr;
use super::{CardImage, SlotImage, classify};
use crate::{update_manifest::{UpdateManifest, UpdateKind, semver2_compare}, update_online::Hash};
#[path = "slot_installer/identity.rs"]
pub mod identity;
#[path = "slot_installer/selection.rs"]
pub mod selection;
use identity::{app_image_end, app_image_intact, ota_boot_slot};
use selection::{OtaFlash, OtaCommit, SaveOutcome, read_ota_records, commit_ota_selection};

pub const APP_SLOT_BYTES: u32 = super::APP_SLOT_BYTES;
pub const USABLE_SLOT_BYTES: u32 = crate::update_online::USABLE_SLOT_BYTES;
pub const SECTOR_BYTES: usize = super::WINDOW_BYTES;
pub const ERASE_BYTES: u32 = 65536;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum InstallIssue { None, Unreadable, Size, Digest, Image, Identity, Older, Same, TargetUnreadable, Unsettled, Consent, Write, Readback, Selection, Unconfirmed, CheckAgain }
pub fn install_changed_nothing(issue: InstallIssue) -> bool { issue == InstallIssue::CheckAgain || issue != InstallIssue::None && issue != InstallIssue::Unsettled && (issue as u8) < InstallIssue::Write as u8 }
#[derive(Clone, Copy, Debug, Default)]
pub struct InstallConsent { pub confirmed: bool, pub reinstall: bool, pub official_return: Option<crate::official_return::ConfirmedReturn> }
pub trait Source {
    fn size(&self) -> u32;
    /// False may leave a partial output prefix; the entire slice is lent only for this call.
    fn read(&mut self, at: u32, out: &mut [u8]) -> bool;
}
impl Source for CardImage<'_> {
    fn size(&self) -> u32 { CardImage::size(self) }
    fn read(&mut self, at: u32, out: &mut [u8]) -> bool { CardImage::read(self, at, out) }
}
pub trait Ports {
    /// Exactly slot0 or slot1, as in the frozen owner contract.
    fn running(&mut self) -> i32;
    fn running_pending(&mut self) -> bool;
    fn selection_unconfirmed(&mut self) -> bool;
    fn mark_selection_unconfirmed(&mut self);
    fn slot_bytes(&mut self) -> u32;
    fn read_slot(&mut self, slot: i32, at: u32, out: &mut [u8]) -> bool;
    fn erase_slot(&mut self, slot: i32, at: u32, count: u32) -> bool;
    fn write_slot(&mut self, slot: i32, at: u32, bytes: &[u8]) -> bool;
    fn ota(&mut self) -> &mut dyn OtaFlash;
    fn progress(&mut self, done: u32, total: u32);
}
pub fn install_target_slot(running: i32) -> i32 { 1 - running }
/// Expose the existing card classifier without copying its implementation.
pub fn classify_slot(head: &[u8; 512]) -> SlotImage { classify(head) }
pub fn head_matches_manifest(head: &[u8; 512], manifest: &UpdateManifest) -> bool {
    let kind = classify(head);
    if manifest.kind == UpdateKind::Firmware {
        return kind == SlotImage::Other && head[48..80] == manifest.version && head[176..208] == manifest.elf_sha256;
    }
    let mut identity = identity::NewtAppIdentity::default();
    if kind != SlotImage::Newt || !identity::read_newt_identity(head, &mut identity) { return false; }
    let Ok(version) = CStr::from_bytes_until_nul(&manifest.version) else { return false };
    let Ok(build) = CStr::from_bytes_until_nul(&manifest.build) else { return false };
    if version.to_bytes().len() >= 24 || build.to_bytes().len() >= 16 { return false; }
    let expected = identity::newt_app_identity_fields(version, build);
    identity == expected
}
/// `image_bytes` is what the install writes and `digest` covers: the file, less any erased padding.
pub fn check_update_image(source: &mut dyn Source, manifest: &UpdateManifest, hash: &mut dyn Hash, slot_bytes: u32, running: Option<&CStr>, digest: &mut [u8; 32], order: &mut i32, image_bytes: &mut u32) -> InstallIssue {
    *order = 1;
    let newt = manifest.kind == UpdateKind::Newt;
    if source.size() != manifest.size { return InstallIssue::Size; }
    if (newt && manifest.size > USABLE_SLOT_BYTES) || manifest.size > slot_bytes || manifest.size < 512 { return InstallIssue::Size; }
    let size = manifest.size;
    let Some(end) = app_image_end(&mut |at, bytes| at + bytes.len() as u64 <= u64::from(size) && source.read(at as u32, bytes), size, hash) else { return InstallIssue::Image; };
    // A Newt release pair is exact. Another firmware's file may end in erased
    // padding (0xff), dropped as the install page drops it; any other byte
    // after the image is not what its digest describes (#368).
    let end = end as u32;
    if newt && end != size { return InstallIssue::Image; }
    let mut chunk = [0; 256];
    if !hash.start() { return InstallIssue::Unreadable; }
    let mut at = 0;
    while at < size {
        let count = (size - at).min(256) as usize;
        if !source.read(at, &mut chunk[..count]) { return InstallIssue::Unreadable; }
        let image = end.saturating_sub(at).min(count as u32) as usize;
        if image != 0 && !hash.update(&chunk[..image]) { return InstallIssue::Unreadable; }
        if chunk[image..count].iter().any(|&byte| byte != 0xff) { return InstallIssue::Image; }
        at += count as u32;
    }
    if !hash.finish(digest) { return InstallIssue::Unreadable; }
    if newt && *digest != manifest.sha256 { return InstallIssue::Digest; }
    *image_bytes = end;
    let mut head = [0; 512];
    if !source.read(0, &mut head) { return InstallIssue::Unreadable; }
    if !head_matches_manifest(&head, manifest) { return InstallIssue::Identity; }
    if newt { *order = semver2_compare(CStr::from_bytes_until_nul(&manifest.version).ok(), running); }
    InstallIssue::None
}
pub struct SlotInstaller<'a> {
    ports: &'a mut dyn Ports,
    source: &'a mut dyn Source,
    hash: &'a mut dyn Hash,
    first: &'a mut [u8; SECTOR_BYTES],
    buffer: &'a mut [u8; SECTOR_BYTES],
}
impl<'a> SlotInstaller<'a> {
    pub fn new(ports: &'a mut dyn Ports, source: &'a mut dyn Source, hash: &'a mut dyn Hash, first: &'a mut [u8; SECTOR_BYTES], buffer: &'a mut [u8; SECTOR_BYTES]) -> Self {
        Self { ports, source, hash, first, buffer }
    }
    pub fn install(&mut self, manifest: &UpdateManifest, running: Option<&CStr>, consent: InstallConsent) -> InstallIssue {
        let mut order = 0;
        let mut bytes = 0;
        let mut digest = [0;32];
        let target = install_target_slot(self.ports.running());
        let issue = check_update_image(self.source, manifest, self.hash, self.ports.slot_bytes(), running, &mut digest, &mut order, &mut bytes);
        if issue != InstallIssue::None { return issue; }
        let mut head = [0;512];
        if order < 0 {
            let Some(permission) = consent.official_return else { return InstallIssue::CheckAgain; };
            let running_slot = self.ports.running();
            let mut source = identity::NewtAppIdentity::default();
            if running_slot < 0 || !self.ports.read_slot(running_slot, 0, &mut head)
                || classify(&head) != SlotImage::Newt || !identity::read_newt_identity(&head, &mut source)
                || !permission.matches(&source, manifest) { return InstallIssue::CheckAgain; }
        }
        if order == 0 && !consent.reinstall { return InstallIssue::Same; }
        if !self.ports.read_slot(target, 0, &mut head) { return InstallIssue::TargetUnreadable; }
        if (classify(&head) == SlotImage::Other || manifest.kind == UpdateKind::Firmware) && !consent.confirmed { return InstallIssue::Consent; }
        if !self.settle() { return InstallIssue::Unsettled; }
        if !self.write(bytes, target) { return self.clear(target, InstallIssue::Write); }
        if !self.readback(manifest, bytes, &digest, target) { return self.clear(target, InstallIssue::Readback); }
        match commit_ota_selection(self.ports.ota(), target, OtaCommit::Fresh) {
            SaveOutcome::Verified => InstallIssue::None,
            SaveOutcome::Unconfirmed => { self.ports.mark_selection_unconfirmed(); InstallIssue::Unconfirmed }
            SaveOutcome::Unchanged => InstallIssue::Selection,
        }
    }
    fn settle(&mut self) -> bool {
        if self.ports.running_pending() || self.ports.selection_unconfirmed() { return false; }
        let mut records = [identity::OtaRecord::default();2];
        let running = self.ports.running();
        if !read_ota_records(self.ports.ota(), &mut records) { return false; }
        if ota_boot_slot(&records) == running { return true; }
        let outcome = commit_ota_selection(self.ports.ota(), running, OtaCommit::Settle);
        if outcome == SaveOutcome::Unconfirmed { self.ports.mark_selection_unconfirmed(); }
        outcome == SaveOutcome::Verified
    }
    fn write(&mut self, size: u32, target: i32) -> bool {
        let sector = SECTOR_BYTES as u32;
        let end = size.wrapping_add(sector - 1) / sector * sector;
        let mut block = 0;
        while block < end {
            let erase = (end - block).min(ERASE_BYTES);
            if !self.ports.erase_slot(target, block, erase) { return false; }
            let mut at = block;
            while at < block + erase {
                let count = (size - at).min(sector) as usize;
                let bytes = if at != 0 { &mut self.buffer[..count] } else { &mut self.first[..count] };
                if !self.source.read(at, bytes) || (at != 0 && !self.ports.write_slot(target, at, bytes)) { return false; }
                at = at.wrapping_add(sector);
            }
            self.ports.progress((block + erase).min(size), size);
            block = block.wrapping_add(ERASE_BYTES);
        }
        self.ports.write_slot(target, 0, &self.first[..size.min(sector) as usize])
    }
    fn readback(&mut self, manifest: &UpdateManifest, bytes: u32, expected: &[u8;32], target: i32) -> bool {
        let mut digest = [0;32];
        if !self.hash.start() { return false; }
        let mut at = 0;
        while at < bytes {
            let count = (bytes - at).min(SECTOR_BYTES as u32) as usize;
            if !self.ports.read_slot(target, at, &mut self.buffer[..count]) || !self.hash.update(&self.buffer[..count]) { return false; }
            at = at.wrapping_add(SECTOR_BYTES as u32);
        }
        let mut head = [0;512];
        if !self.hash.finish(&mut digest) || digest != *expected { return false; }
        let slot_bytes = self.ports.slot_bytes();
        app_image_intact(&mut |at, bytes| self.ports.read_slot(target, at as u32, bytes), slot_bytes, self.hash) &&
            self.ports.read_slot(target, 0, &mut head) && head_matches_manifest(&head, manifest)
    }
    fn clear(&mut self, target: i32, issue: InstallIssue) -> InstallIssue {
        self.ports.erase_slot(target, 0, SECTOR_BYTES as u32);
        issue
    }
}
