// SPDX-License-Identifier: MIT
//! The development USB bridge's one card write: a single file into one of
//! three fixed dev folders, over a short lease that follows the bridge's PUT.
//!
//! Every lease line fits the USB receive queue whole, in caller memory that
//! exists only for the lease. Bytes go to the folder's fixed temp name, are
//! synced, read back from the card and checked against the offered size and
//! SHA-256, and only then renamed into place, so the final name never holds a
//! partial file. DONE syncs a marker naming its target before any rename; a
//! replaced file waits in a backup until the new one holds its name, and a
//! failed rename moves it back. The folder's next FILE resolves a commit a
//! power loss stopped only where one name is left. Physical input, a
//! disconnect, a stall or a new HELLO abandons the transfer up to DONE's
//! commit point; until then every exit closes the file and removes the temp.
use core::ffi::CStr;

#[path = "dev_put/lease.rs"]
mod lease;
pub use lease::run;

/// The largest image an app slot holds, so a bench `newt.bin` fits.
pub const CAP_BYTES: u32 = crate::update_card::APP_SLOT_BYTES;
/// HWCDC's receive queue (its `begin` default). Its interrupt drops what a
/// full queue cannot take, so a lease line and its LF must fit it whole; the
/// host sends one line and waits for its answer.
pub const RX_QUEUE_BYTES: usize = 256;
/// Bytes per DATA line: 200 base64 characters.
pub const CHUNK_BYTES: usize = 150;
/// One lease line without its LF. The longest is DATA: a header of at most 36
/// bytes, `DATA `, an offset below the cap, a space and its base64 (249).
pub const LINE_BYTES: usize = RX_QUEUE_BYTES - 1;
/// Holds the longest reply (`DONE`, 137 bytes with its maximum header).
const REPLY_BYTES: usize = 192;
/// The lease's whole working memory, lent by the caller for one lease.
pub const MEMORY_BYTES: usize = LINE_BYTES + REPLY_BYTES;
pub const NAME_BYTES: usize = 32;
/// Left free after the write for Newt's own records and caches.
pub const RESERVE_BYTES: u64 = 1 << 20;
/// No complete line within this, or no reply progress within `STALL_MS`,
/// abandons the transfer.
pub const IDLE_MS: u32 = 3_000;
const STALL_MS: u32 = 1_000;
/// A hard bound on the whole lease: the cap at about 13 KB/s.
pub const TOTAL_MS: u32 = 600_000;

const NEWT: &CStr = c"/newt-sd/Newt";
const PATH_BYTES: usize = 64;

/// The only folders the bridge writes. Nothing in Newt reads `Newt/Dev`, so a
/// test file there cannot change what the device does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Folder { Apps, Update, Dev }
impl Folder {
    fn parse(token: &str) -> Option<Self> {
        match token {
            "apps" => Some(Self::Apps),
            "update" => Some(Self::Update),
            "dev" => Some(Self::Dev),
            _ => None,
        }
    }
    pub fn path(self) -> &'static CStr {
        match self {
            Self::Apps => crate::app_card::DIRECTORY,
            Self::Update => crate::update_card::DIRECTORY,
            Self::Dev => c"/newt-sd/Newt/Dev",
        }
    }
    /// Hidden, so neither the app scan nor the update scan offers it, and no
    /// allowed name (which starts with a letter or digit) can equal it.
    pub fn temp(self) -> &'static CStr {
        match self {
            Self::Apps => c"/newt-sd/Newt/Apps/.newt-put",
            Self::Update => c"/newt-sd/Newt/Update/.newt-put",
            Self::Dev => c"/newt-sd/Newt/Dev/.newt-put",
        }
    }
    /// Where a replaced file waits, hidden the same way, until the new one
    /// holds its name.
    pub fn old(self) -> &'static CStr {
        match self {
            Self::Apps => c"/newt-sd/Newt/Apps/.newt-old",
            Self::Update => c"/newt-sd/Newt/Update/.newt-old",
            Self::Dev => c"/newt-sd/Newt/Dev/.newt-old",
        }
    }
    /// DONE's target name, synced before its first rename.
    pub fn marker(self) -> &'static CStr {
        match self {
            Self::Apps => c"/newt-sd/Newt/Apps/.newt-old-name",
            Self::Update => c"/newt-sd/Newt/Update/.newt-old-name",
            Self::Dev => c"/newt-sd/Newt/Dev/.newt-old-name",
        }
    }
}

/// One plain name: 1 to 32 of `A-Z a-z 0-9 . _ -`, starting with a letter or
/// digit, with no `..` and no trailing dot. So no separator, parent, absolute
/// path or hidden name, and none FAT would strip to another name (FAT drops a
/// trailing dot; it also ignores case, which the host checks across a batch).
pub fn name_allowed(name: &[u8]) -> bool {
    (1..=NAME_BYTES).contains(&name.len())
        && name[0].is_ascii_alphanumeric()
        && name.iter().all(|&b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        && !name.windows(2).any(|pair| pair == b"..")
        && name.last() != Some(&b'.')
}

/// What the card holds at a path, read without following a link.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Entry { Missing, File, Folder, Other }

/// Synchronous card calls. Errors are the platform's own codes, reported to
/// the host as they are; the lease makes every decision.
pub trait Card {
    fn entry(&mut self, path: &CStr) -> Result<Entry, i32>;
    fn make_folder(&mut self, path: &CStr) -> Result<(), i32>;
    /// Create or truncate, write-only.
    fn create(&mut self, path: &CStr) -> Result<i32, i32>;
    fn open_read(&mut self, path: &CStr) -> Result<i32, i32>;
    fn write(&mut self, fd: i32, bytes: &[u8]) -> Result<usize, i32>;
    fn read(&mut self, fd: i32, out: &mut [u8]) -> Result<usize, i32>;
    fn sync(&mut self, fd: i32) -> Result<(), i32>;
    fn close(&mut self, fd: i32) -> Result<(), i32>;
    fn remove(&mut self, path: &CStr) -> Result<(), i32>;
    /// The destination must not exist.
    fn rename(&mut self, from: &CStr, to: &CStr) -> Result<(), i32>;
    fn free_bytes(&mut self) -> Result<u64, i32>;
}

/// The bridge's serial channel and the input owner's takeover observation.
pub trait Wire {
    fn connected(&mut self) -> bool;
    /// One received byte, or a negative value when none is waiting.
    fn read_byte(&mut self) -> i32;
    fn write(&mut self, bytes: &[u8]) -> usize;
    fn now(&mut self) -> u32;
    /// True once for each physical input since the last call (takeover).
    fn physical_input(&mut self) -> bool;
    /// Lets other tasks run while no byte is waiting.
    fn pause(&mut self);
}

/// Facts the card-write admission reads before any lease begins.
#[derive(Clone, Copy, Debug)]
pub struct Facts { pub home: bool, pub reader_open: bool, pub opening: bool }

/// Why a PUT was refused before its lease.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Busy { Reading, Opening, NotHome }
impl Busy {
    pub fn reply(self) -> &'static [u8] {
        match self {
            Self::Reading => b"ERROR busy reason=reading",
            Self::Opening => b"ERROR busy reason=opening",
            Self::NotHome => b"ERROR busy reason=not-home",
        }
    }
}

/// Writes only on Home with no book open or opening, so a lease never runs
/// beside the reader's own card work.
pub fn admit(facts: Facts) -> Result<(), Busy> {
    if facts.reader_open { return Err(Busy::Reading); }
    if facts.opening { return Err(Busy::Opening); }
    if !facts.home { return Err(Busy::NotHome); }
    Ok(())
}

/// The bridge session the PUT arrived in; `request` is the PUT's own ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Session { pub boot: u32, pub session: u32, pub request: u32 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum End {
    /// The lease answered its last request (DONE or a reported refusal); the
    /// session continues after `last_id`.
    Kept { last_id: u32 },
    /// Abandoned or out of protocol: the session needs a fresh HELLO.
    Cancelled,
    /// A HELLO arrived; the bridge answers it as a new handshake.
    Hello,
}
