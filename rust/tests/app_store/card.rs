// SPDX-License-Identifier: MIT
//! The card scan and candidate read (`newt_core::app_card`) over a fake card
//! that answers both read-only card interfaces: the error-preserving listing
//! and the update card's open/stat/read.
use crate::package::X3Payload;
use crate::{reader_hash, sha256, titled, with_failing_hash};
use newt_core::app_card::{self, Candidate, Scan};
use newt_core::app_store::{PACKAGE_BYTES, Refusal};
use newt_core::crossink_import::{Entry, EntryKind, Files as Listing, IoError};
use newt_core::update_card::{FileInfo, Files};
use std::ffi::{CStr, CString};

#[derive(Clone)]
enum Node {
    File { bytes: Vec<u8>, regular: bool },
    Folder,
}

/// Clone gives the listing its own handle on the same files (offer.rs).
#[derive(Clone)]
pub(crate) struct Card {
    folder: Result<(), IoError>,
    entries: Vec<(CString, EntryKind, Node)>,
    /// `next` fails instead of returning this entry index (or the end).
    fail_next_at: Option<usize>,
    close_fails: bool,
    typing_fails: bool,
    stat_fails: bool,
    /// Reads fail once this many bytes were returned.
    read_limit: Option<usize>,
    /// Open descriptors, each with its node and read position.
    open: Vec<(i32, usize, usize)>,
    listed: bool,
}

impl Card {
    pub(crate) fn new() -> Self {
        Self {
            folder: Ok(()),
            entries: Vec::new(),
            fail_next_at: None,
            close_fails: false,
            typing_fails: false,
            stat_fails: false,
            read_limit: None,
            open: Vec::new(),
            listed: false,
        }
    }
    fn entry(mut self, name: &str, kind: EntryKind, node: Node) -> Self {
        self.entries.push((CString::new(name).unwrap(), kind, node));
        self
    }
    pub(crate) fn file(self, name: &str, bytes: &[u8]) -> Self {
        self.entry(name, EntryKind::Other, Node::File { bytes: bytes.to_vec(), regular: true })
    }
    /// The named file's bytes replaced in place, as a computer would.
    pub(crate) fn rewrite(&mut self, name: &str, bytes: &[u8]) {
        let (.., node) = self.entries.iter_mut().find(|(entry, ..)| entry.to_str() == Ok(name)).expect("a file");
        *node = Node::File { bytes: bytes.to_vec(), regular: true };
    }
    fn path(name: &CStr) -> String {
        format!("/newt-sd/Newt/Apps/{}", name.to_str().unwrap())
    }
    fn find(&self, path: &CStr) -> Option<usize> {
        self.entries.iter().position(|(name, ..)| Self::path(name) == path.to_str().unwrap())
    }
    fn bytes(&self, fd: i32) -> &[u8] {
        let (_, node, _) = self.open.iter().find(|open| open.0 == fd).expect("open descriptor");
        match &self.entries[*node].2 {
            Node::File { bytes, .. } => bytes,
            Node::Folder => unreachable!(),
        }
    }
}

/// The listing owns its names, as a native directory owns its entry buffer.
pub(crate) struct Directory {
    names: Vec<(CString, EntryKind)>,
    at: usize,
}

impl Listing for Card {
    type File = ();
    type Directory = Directory;
    fn open(&mut self, _: &CStr) -> Result<(), IoError> { unreachable!("files open through the update card") }
    fn size(&mut self, _: &mut ()) -> Result<u64, IoError> { unreachable!() }
    fn read(&mut self, _: &mut (), _: &mut [u8]) -> Result<usize, IoError> { unreachable!() }
    fn close(&mut self, _: ()) -> Result<(), IoError> { unreachable!() }
    fn open_directory(&mut self, path: &CStr) -> Result<Directory, IoError> {
        assert_eq!(path, c"/newt-sd/Newt/Apps");
        assert!(!self.listed, "one listing per scan");
        self.folder?;
        self.listed = true;
        Ok(Directory { names: self.entries.iter().map(|(name, kind, _)| (name.clone(), *kind)).collect(), at: 0 })
    }
    fn next<'dir>(&mut self, directory: &'dir mut Directory) -> Result<Option<Entry<'dir>>, IoError> {
        if self.fail_next_at == Some(directory.at) {
            return Err(IoError::Failed);
        }
        let Some((name, kind)) = directory.names.get(directory.at) else { return Ok(None) };
        directory.at += 1;
        Ok(Some(Entry { name, kind: *kind }))
    }
    fn close_directory(&mut self, _: Directory) -> Result<(), IoError> {
        assert!(std::mem::replace(&mut self.listed, false), "close an open listing");
        if self.close_fails { Err(IoError::Failed) } else { Ok(()) }
    }
    fn is_directory(&mut self, path: &CStr) -> Result<bool, IoError> {
        if self.typing_fails {
            return Err(IoError::Failed);
        }
        let node = self.find(path).ok_or(IoError::Missing)?;
        Ok(matches!(self.entries[node].2, Node::Folder))
    }
}

impl Files for Card {
    fn open(&mut self, path: &CStr) -> i32 {
        match self.find(path) {
            Some(node) if matches!(self.entries[node].2, Node::File { .. }) => {
                let fd = 3 + node as i32;
                self.open.push((fd, node, 0));
                fd
            }
            _ => -1,
        }
    }
    fn stat(&mut self, fd: i32) -> Option<FileInfo> {
        let regular = match &self.entries[self.open.iter().find(|open| open.0 == fd)?.1].2 {
            Node::File { regular, .. } => *regular,
            Node::Folder => false,
        };
        (!self.stat_fails).then(|| FileInfo { regular, size: self.bytes(fd).len() as i64 })
    }
    fn read(&mut self, fd: i32, out: &mut [u8]) -> i32 {
        let at = self.open.iter().find(|open| open.0 == fd).unwrap().2;
        if self.read_limit.is_some_and(|limit| at >= limit) {
            return -1;
        }
        let bytes = self.bytes(fd);
        let count = out.len().min(bytes.len() - at).min(100);
        out[..count].copy_from_slice(&bytes[at..at + count]);
        self.open.iter_mut().find(|open| open.0 == fd).unwrap().2 += count;
        count as i32
    }
    fn seek(&mut self, _: i32, _: u32) -> i64 { unreachable!("the app file is read once, in order") }
    fn close(&mut self, fd: i32) {
        let at = self.open.iter().position(|open| open.0 == fd).expect("close an open file");
        self.open.remove(at);
    }
}

fn scan(card: &mut Card) -> Scan {
    let scan = app_card::scan(card);
    assert!(!card.listed, "the listing is closed");
    scan
}

fn one(card: &mut Card) -> Candidate {
    match scan(card) {
        Scan::One(candidate) => candidate,
        other => panic!("one candidate expected, got {other:?}"),
    }
}

/// The offer read: Ok is the offered package's digest.
fn offer(card: &mut Card) -> Result<[u8; 32], Refusal> {
    let candidate = one(card);
    let (mut wire, mut digest) = ([0; PACKAGE_BYTES], [0; 32]);
    let result = app_card::offer(card, &mut X3Payload, &candidate, &mut wire, &mut digest);
    assert!(card.open.is_empty(), "the file is closed");
    result.map(|()| digest)
}

fn named(name: &str) -> Card { Card::new().file(name, &titled(b"Card")) }

#[test]
fn a_missing_folder_offers_nothing_and_an_unreadable_one_refuses() {
    let mut card = Card::new();
    card.folder = Err(IoError::Missing);
    assert_eq!(scan(&mut card), Scan::Nothing);
    for error in [IoError::Interrupted, IoError::Failed] {
        let mut card = Card::new();
        card.folder = Err(error);
        assert_eq!(scan(&mut card), Scan::Refused(Refusal::Unreadable), "{error:?}");
    }
}

#[test]
fn exactly_one_app_file_is_a_candidate() {
    assert_eq!(scan(&mut Card::new()), Scan::Nothing);
    let candidate = one(&mut named("calm.newt"));
    assert_eq!(candidate.path(), c"/newt-sd/Newt/Apps/calm.newt");
    let mut two = named("a.newt").file("b.NEWT", &titled(b"Other"));
    assert_eq!(scan(&mut two), Scan::Refused(Refusal::Many));
}

#[test]
fn names_follow_the_update_card_rules() {
    let longest = format!("{}.newt", "n".repeat(42));
    assert_eq!(one(&mut named(&longest)).path().to_bytes().len(), "/newt-sd/Newt/Apps/".len() + 47);
    for ignored in [".hidden.newt", "._calm.newt", ".newt", "calm.newt.txt", "calm.bin", &format!("{}.newt", "n".repeat(43))] {
        assert_eq!(scan(&mut named(ignored)), Scan::Nothing, "{ignored}");
    }
    assert!(matches!(scan(&mut named("Calm.NeWt")), Scan::One(_)));
}

#[test]
fn folders_are_never_candidates_however_the_card_types_them() {
    let mut card = Card::new().entry("folder.newt", EntryKind::Directory, Node::Folder);
    assert_eq!(scan(&mut card), Scan::Nothing);
    // The FAT listing may not type an entry; the path's own stat decides.
    let mut card = Card::new()
        .entry("folder.newt", EntryKind::Unknown, Node::Folder)
        .entry("calm.newt", EntryKind::Unknown, Node::File { bytes: titled(b"Calm").to_vec(), regular: true });
    assert_eq!(one(&mut card).path(), c"/newt-sd/Newt/Apps/calm.newt");
    let mut card = Card::new().entry("calm.newt", EntryKind::Unknown, Node::Folder);
    card.typing_fails = true;
    assert_eq!(scan(&mut card), Scan::Refused(Refusal::Unreadable));
}

#[test]
fn a_listing_that_fails_partway_never_offers() {
    let mut before = named("calm.newt");
    before.fail_next_at = Some(0);
    assert_eq!(scan(&mut before), Scan::Refused(Refusal::Unreadable));
    // One candidate is already found; a later failure may hide a second.
    let mut after = named("calm.newt");
    after.fail_next_at = Some(1);
    assert_eq!(scan(&mut after), Scan::Refused(Refusal::Unreadable));
    let mut closing = named("calm.newt");
    closing.close_fails = true;
    assert_eq!(scan(&mut closing), Scan::Refused(Refusal::Unreadable));
}

#[test]
fn a_valid_file_is_offered_by_its_digest() {
    let wire = titled(b"Card");
    assert_eq!(offer(&mut named("calm.newt")), Ok(sha256(&wire)));
}

#[test]
fn only_a_regular_file_of_one_package_is_read() {
    let wire = titled(b"Card");
    let special = Node::File { bytes: wire.to_vec(), regular: false };
    assert_eq!(offer(&mut Card::new().entry("calm.newt", EntryKind::Other, special)), Err(Refusal::Damaged));
    let mut long = wire.to_vec();
    long.push(0);
    for bytes in [Vec::new(), wire[..614].to_vec(), long, vec![0; 1 << 20]] {
        assert_eq!(offer(&mut Card::new().file("calm.newt", &bytes)), Err(Refusal::Damaged), "{} bytes", bytes.len());
    }
}

#[test]
fn a_newer_package_format_is_not_called_damaged() {
    let mut newer = titled(b"Card").to_vec();
    newer[7] = b'2';
    assert_eq!(offer(&mut Card::new().file("calm.newt", &newer)), Err(Refusal::Newer));
    newer.resize(900, 0);
    assert_eq!(offer(&mut Card::new().file("calm.newt", &newer)), Err(Refusal::Newer));
}

#[test]
fn the_decoder_rejects_a_damaged_package() {
    // (what changes, the byte, its new value)
    let damage = [("bad padding after the title", 70, b'x'), ("an unknown card action", 318 + 98, 9), ("a control byte in the title", 59, 7)];
    for (what, at, value) in damage {
        let mut wire = titled(b"Card");
        wire[at] = value;
        assert_eq!(offer(&mut Card::new().file("calm.newt", &wire)), Err(Refusal::Damaged), "{what}");
    }
}

#[test]
fn a_file_that_cannot_be_read_is_unreadable() {
    let mut partway = named("calm.newt");
    partway.read_limit = Some(300);
    assert_eq!(offer(&mut partway), Err(Refusal::Unreadable));
    let mut stat = named("calm.newt");
    stat.stat_fails = true;
    assert_eq!(offer(&mut stat), Err(Refusal::Unreadable));
    // The file went away after the listing.
    let mut card = named("calm.newt");
    let candidate = one(&mut card);
    card.entries.clear();
    assert_eq!(app_card::read(&mut card, &candidate, &mut [0; PACKAGE_BYTES]), Err(Refusal::Unreadable));
}

#[test]
fn a_hash_failure_is_not_called_damage() {
    let wire = titled(b"Card");
    let result = with_failing_hash(reader_hash::Fail::Input(wire.to_vec()), || offer(&mut named("calm.newt")));
    assert_eq!(result, Err(Refusal::HashFailed));
}
