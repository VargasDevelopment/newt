// SPDX-License-Identifier: MIT
//! The development USB bridge's one card write: a single file at
//! any validated card-relative path, over a cancellable lease that follows the bridge's PUT.
//!
//! The lease owns a bounded caller buffer and polls the FIFO with USB
//! backpressure; the ordinary SDK receive queue stays at 256 bytes. Bytes go to the card root's reserved temp name, are
//! synced, read back from the card and checked against the offered size and
//! SHA-256, and only then renamed into place, so the final name never holds a
//! partial file. DONE syncs a marker naming its target before any rename; a
//! replaced file waits in a backup until the new one holds its name, and a
//! failed rename moves it back. A marker a power loss left refuses the
//! writes to the card until the card is checked by hand. Physical input, a
//! disconnect, a stall or a new HELLO abandons the transfer up to DONE's
//! commit point; until then every exit closes the file and removes the temp.
use core::ffi::CStr;

#[path = "dev_put/lease.rs"]
mod lease;
pub use lease::run;

/// Maximum host frame. The active lease polls FIFO with hardware backpressure;
/// the normal SDK receive queue stays at 256 bytes.
pub const FRAME_BYTES: usize = 4352;
/// Raw bytes per DATA block; amortizes card writes and replies without Base64.
pub const CHUNK_BYTES: usize = 4096;
pub const LINE_BYTES: usize = FRAME_BYTES - 1;
const REPLY_BYTES: usize = 192;
/// One heap loan only while a lease owns the port and card.
pub const MEMORY_BYTES: usize = LINE_BYTES + REPLY_BYTES;
/// Native IO's path bound, including the mount prefix and trailing NUL.
pub const PATH_BYTES: usize = 512;
pub const IDLE_MS: u32 = 3_000;
const STALL_MS: u32 = 1_000;
const ROOT: &CStr = c"/newt-sd";
// 8.3 names have no alternate long-name aliases that could target our temp.
// One global lease and recovery marker; these names cannot be destinations.
const TEMP: &CStr = c"/newt-sd/NEWT.PUT";
const OLD: &CStr = c"/newt-sd/NEWT.OLD";
const MARKER: &CStr = c"/newt-sd/NEWT.MRK";

#[path = "dev_put/path.rs"]
mod path;
pub use path::name_allowed;
use path::target_path;

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
    /// Receives up to `out.len()` raw bytes, or 0 when none is waiting.
    fn read(&mut self, out: &mut [u8]) -> usize {
        if out.is_empty() { return 0; }
        let byte = self.read_byte();
        if byte < 0 { 0 } else { out[0] = byte as u8; 1 }
    }
    fn write(&mut self, bytes: &[u8]) -> usize;
    fn now(&mut self) -> u32;
    /// True once for each physical input since the last call (takeover).
    fn physical_input(&mut self) -> bool;
    /// Lets other tasks run while no byte is waiting.
    fn pause(&mut self);
    /// Optional DEV observations; no payload or path leaves the lease.
    fn observe(&mut self, _event: Observation) {}
}

/// Stable diagnostic codes. A failure is latched before its best-effort reply.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ExitReason { Done, Hello, PhysicalInput, Disconnected, Timeout, Line, Syntax, Session, StaleId, Refused, TxStall }

#[derive(Clone, Copy, Debug)]
pub enum Observation {
    Request(u32), Receiving { offset: u64, expected: u32 }, Writing,
    Commit, Reply, Waiting, Exit(ExitReason),
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
