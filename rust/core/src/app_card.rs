// SPDX-License-Identifier: MIT
//! Finds the one app file waiting in `/Newt/Apps` and reads it, read-only.
//! The listing keeps its errors (a missing folder is not a failed read, and a
//! failed `next` is not the end), so a partial listing never yields an offer.
//! Newt never writes, moves or deletes card files for apps; only the
//! development bridge's card write (dev_put) can put one there.
use crate::app_store::{self, PACKAGE_BYTES, Package, Payload, Refusal};
use crate::crossink_import::{EntryKind, Files as Listing, IoError};
use crate::update_card::{FILE_BYTES, Files, read_whole};
use core::ffi::CStr;

pub const DIRECTORY: &CStr = c"/newt-sd/Newt/Apps";
const PATH_BYTES: usize = DIRECTORY.to_bytes().len() + 1 + FILE_BYTES;

/// The one candidate's full path, copied out of the listing's entry loan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Candidate {
    path: [u8; PATH_BYTES],
}

impl Candidate {
    fn new(name: &[u8]) -> Self {
        let directory = DIRECTORY.to_bytes();
        let mut path = [0; PATH_BYTES];
        path[..directory.len()].copy_from_slice(directory);
        path[directory.len()] = b'/';
        // app_file_name keeps a name under FILE_BYTES, so a NUL always follows.
        path[directory.len() + 1..][..name.len()].copy_from_slice(name);
        Self { path }
    }

    pub fn path(&self) -> &CStr {
        // Always terminated (see new); an empty path would only fail to open.
        CStr::from_bytes_until_nul(&self.path).unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scan {
    /// No folder, or no app file in it: Apps opens as it does today.
    Nothing,
    One(Candidate),
    Refused(Refusal),
}

/// The update card's name rules (not hidden, under FILE_BYTES) with a
/// case-insensitive `.newt` extension. Hidden names skip macOS `._` files.
fn app_file_name(name: &[u8]) -> bool {
    name.first() != Some(&b'.')
        && name.len() > 5
        && name.len() < FILE_BYTES
        && name[name.len() - 5..].eq_ignore_ascii_case(b".newt")
}

/// Lists the folder once. A second candidate stops the scan. Entries the
/// card cannot type are typed by path, so a folder named `x.newt` is never
/// counted, whatever the filesystem reports.
pub fn scan<L: Listing>(listing: &mut L) -> Scan {
    let mut directory = match listing.open_directory(DIRECTORY) {
        Ok(directory) => directory,
        Err(IoError::Missing) => return Scan::Nothing,
        Err(_) => return Scan::Refused(Refusal::Unreadable),
    };
    let mut found = None;
    let outcome = loop {
        let (candidate, kind) = match listing.next(&mut directory) {
            Err(_) => break Some(Refusal::Unreadable),
            Ok(None) => break None,
            Ok(Some(entry)) if entry.kind == EntryKind::Directory => continue,
            Ok(Some(entry)) if !app_file_name(entry.name.to_bytes()) => continue,
            Ok(Some(entry)) => (Candidate::new(entry.name.to_bytes()), entry.kind),
        };
        if kind == EntryKind::Unknown {
            match listing.is_directory(candidate.path()) {
                Ok(true) | Err(IoError::Missing) => continue,
                Ok(false) => {}
                Err(_) => break Some(Refusal::Unreadable),
            }
        }
        if found.replace(candidate).is_some() {
            break Some(Refusal::Many);
        }
    };
    let closed = listing.close_directory(directory);
    match (outcome, closed, found) {
        (Some(refusal), ..) => Scan::Refused(refusal),
        (None, Err(_), _) => Scan::Refused(Refusal::Unreadable),
        (None, Ok(()), Some(candidate)) => Scan::One(candidate),
        (None, Ok(()), None) => Scan::Nothing,
    }
}

/// Reads the candidate whole into `wire`. Only a regular file of exactly one
/// package is read; any other size reads just its header, so a newer package
/// format is told apart from damage.
pub fn read(files: &mut dyn Files, candidate: &Candidate, wire: &mut [u8; PACKAGE_BYTES]) -> Result<(), Refusal> {
    let fd = files.open(candidate.path());
    if fd < 0 {
        return Err(Refusal::Unreadable);
    }
    let result = match files.stat(fd) {
        None => Err(Refusal::Unreadable),
        Some(info) if !info.regular => Err(Refusal::Damaged),
        Some(info) => {
            let whole = info.size == PACKAGE_BYTES as i64;
            let length = if whole { PACKAGE_BYTES } else { info.size.clamp(0, 8) as usize };
            if !read_whole(files, fd, &mut wire[..length]) {
                Err(Refusal::Unreadable)
            } else if app_store::newer_package(&wire[..length]) {
                Err(Refusal::Newer)
            } else if whole {
                Ok(())
            } else {
                Err(Refusal::Damaged)
            }
        }
    };
    files.close(fd);
    result
}

/// Reads and judges the candidate; `digest` names the package it offers.
pub fn offer(
    files: &mut dyn Files,
    payload: &mut impl Payload,
    candidate: &Candidate,
    wire: &mut [u8; PACKAGE_BYTES],
    digest: &mut [u8; 32],
) -> Result<(), Refusal> {
    read(files, candidate, wire)?;
    match app_store::identify(payload, wire, digest) {
        Package::Valid => Ok(()),
        Package::Unsupported => Err(Refusal::Newer),
        Package::Failed => Err(Refusal::HashFailed),
        Package::Corrupt => Err(Refusal::Damaged),
    }
}
