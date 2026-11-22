// SPDX-License-Identifier: MIT
//! Read-only SD-card firmware update offer classifier. MIT. Caller lends transient scratch and window regions; no filesystem owner or installer.
#![forbid(unsafe_code)]
use core::ffi::CStr;
use crate::update_manifest::{MAX_BYTES, UpdateKind, UpdateManifest, parse_update_manifest};
#[path = "update_card/image.rs"]
mod image;
pub use image::CardImage;

pub const DIRECTORY: &CStr = c"/newt-sd/Newt/Update";
pub const NEWT_IMAGE: &CStr = c"/newt-sd/Newt/Update/newt.bin";
pub const NEWT_MANIFEST: &CStr = c"/newt-sd/Newt/Update/newt.manifest";
pub const FILE_BYTES: usize = 48;
pub const FIRMWARE_FILES: usize = 3;
pub const OFFERS: usize = 4;
pub const PATH_BYTES: usize = DIRECTORY.to_bytes_with_nul().len() + FILE_BYTES;
pub const WINDOW_BYTES: usize = 4096;
pub const APP_SLOT_BYTES: u32 = 0x770000;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum SlotImage { Empty = 0, #[default] Invalid = 1, Other = 2, Newt = 3 }
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum FirmwareName { #[default] Unknown, CrossInk, CrossPoint }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UpdateOffer {
    pub present: bool,
    pub readable: bool,
    pub manifest: UpdateManifest,
    pub file: [u8; FILE_BYTES],
    pub image: SlotImage,
    pub name: FirmwareName,
}
impl Default for UpdateOffer {
    fn default() -> Self {
        Self { present: false, readable: false, manifest: UpdateManifest::default(), file: [0; FILE_BYTES], image: SlotImage::Invalid, name: FirmwareName::Unknown }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FileInfo { pub regular: bool, pub size: i64 }
/// Required short transport calls; a successful read never exceeds its lent slice.
/// Failed opens are negative descriptors; read returns positive bytes, zero EOF or negative failure.
pub trait Files {
    fn open(&mut self, path: &CStr) -> i32;
    fn stat(&mut self, fd: i32) -> Option<FileInfo>;
    fn read(&mut self, fd: i32, out: &mut [u8]) -> i32;
    fn seek(&mut self, fd: i32, at: u32) -> i64;
    fn close(&mut self, fd: i32);
}
pub trait Directory {
    fn open(&mut self, path: &CStr) -> bool;
    /// One entry loan, ending at the next mutable directory call; None also covers read failure.
    fn next(&mut self) -> Option<&CStr>;
    fn close(&mut self);
}
pub trait Yield { fn yield_now(&mut self); }

pub fn firmware_file_name(name: &CStr) -> bool {
    let name = name.to_bytes();
    name.first() != Some(&b'.') && name.len() > 4 && name.len() < FILE_BYTES &&
        name[name.len() - 4..].eq_ignore_ascii_case(b".bin") && !name.eq_ignore_ascii_case(b"newt.bin")
}
pub fn firmware_path(file: &[u8; FILE_BYTES]) -> [u8; PATH_BYTES] {
    let mut path = [0; PATH_BYTES];
    let directory = DIRECTORY.to_bytes();
    path[..directory.len()].copy_from_slice(directory);
    path[directory.len()] = b'/';
    let length = file[..FILE_BYTES - 1].iter().position(|&v| v == 0).unwrap_or(FILE_BYTES - 1);
    path[directory.len() + 1..directory.len() + 1 + length].copy_from_slice(&file[..length]);
    path
}
pub fn read_whole(files: &mut dyn Files, fd: i32, mut out: &mut [u8]) -> bool {
    while !out.is_empty() {
        let got = files.read(fd, out);
        if got <= 0 || got as usize > out.len() { return false; }
        out = &mut out[got as usize..];
    }
    true
}

/// The only identity operations the frozen offer needs, from app_slot_identity.h.
fn classify(head: &[u8; 512]) -> SlotImage {
    if head.iter().all(|&v| v == 0xff) { return SlotImage::Empty; }
    if head[0] != 0xe9 || head[32..36] != 0xabcd5432u32.to_le_bytes() { return SlotImage::Invalid; }
    if &head[288..304] == b"NEWT-X3-APP\0\x01\0\0\0" { SlotImage::Newt } else { SlotImage::Other }
}
pub fn firmware_name(head: &[u8; 512]) -> FirmwareName {
    let project = &head[80..112];
    if project.starts_with(b"CrossInk\0") { FirmwareName::CrossInk }
    else if project.starts_with(b"crosspoint-reader\0") { FirmwareName::CrossPoint }
    else { FirmwareName::Unknown }
}

pub fn read_newt_offer(files: &mut dyn Files, scratch: &mut [u8; MAX_BYTES + 1]) -> UpdateOffer {
    let mut offer = UpdateOffer::default();
    let fd = files.open(NEWT_MANIFEST);
    if fd < 0 { return offer; }
    offer.present = true;
    let mut length = 0;
    let mut got = 0;
    let limit = MAX_BYTES + 1;
    while length < limit {
        got = files.read(fd, &mut scratch[length..limit]);
        if got <= 0 { break; }
        if got as usize > limit - length { got = -1; break; }
        length += got as usize;
    }
    files.close(fd);
    offer.readable = got >= 0 && length <= MAX_BYTES && parse_update_manifest(Some(scratch), length, &mut offer.manifest);
    offer
}
pub fn read_firmware_offer(files: &mut dyn Files, offer: &mut UpdateOffer, head: &mut [u8; 512]) {
    offer.present = true;
    offer.manifest.kind = UpdateKind::Firmware;
    let path = firmware_path(&offer.file);
    let Ok(path) = CStr::from_bytes_until_nul(&path) else { return };
    let fd = files.open(path);
    if fd < 0 { return; }
    let info = files.stat(fd);
    let read = info.is_some_and(|i| i.regular && i.size >= 512 && i.size <= i64::from(APP_SLOT_BYTES)) &&
        read_whole(files, fd, head);
    files.close(fd);
    if !read { return; }
    let Some(info) = info else { return };
    offer.image = classify(head);
    offer.name = firmware_name(head);
    offer.manifest.size = info.size as u32;
    offer.manifest.version.copy_from_slice(&head[48..80]);
    offer.manifest.elf_sha256.copy_from_slice(&head[176..208]);
    offer.readable = offer.image == SlotImage::Other;
}
pub fn read_card_offers(files: &mut dyn Files, directory: &mut dyn Directory, offers: &mut [UpdateOffer; OFFERS], more_files: &mut bool, scratch: &mut [u8; MAX_BYTES + 1]) {
    offers.fill(UpdateOffer::default());
    offers[0] = read_newt_offer(files, scratch);
    let files_out = &mut offers[1..];
    let mut count = 0;
    *more_files = false;
    if directory.open(DIRECTORY) {
        while let Some(name) = directory.next() {
            if !firmware_file_name(name) { continue; }
            let mut at = count;
            while at > 0 {
                let previous = &files_out[at - 1].file;
                let length = previous.iter().position(|&v| v == 0).unwrap_or(FILE_BYTES);
                if name.to_bytes() >= &previous[..length] { break; }
                at -= 1;
            }
            if count == FIRMWARE_FILES { *more_files = true; }
            if at == FIRMWARE_FILES { continue; }
            let end = if count < FIRMWARE_FILES { count += 1; count - 1 } else { count - 1 };
            for i in (at + 1..=end).rev() { files_out[i] = files_out[i - 1]; }
            files_out[at] = UpdateOffer::default();
            let name = name.to_bytes_with_nul();
            files_out[at].file[..name.len()].copy_from_slice(name);
        }
        directory.close();
    }
    let Ok(head) = <&mut [u8; 512]>::try_from(&mut scratch[..512]) else { return };
    for offer in &mut files_out[..count] { read_firmware_offer(files, offer, head); }
}

#[path = "slot_installer.rs"] pub mod slot_installer;
