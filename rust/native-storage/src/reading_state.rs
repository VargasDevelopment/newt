// SPDX-License-Identifier: MIT

//! SD reading-state backend and namespace router.
//!
//! One 4,096-byte file per namespace, `<State>/<namespace>.nrs` with '_'
//! written as '-'. Sector 0 binds the namespace. Each key (`digest`, `slot0`,
//! `slot1`) owns two sectors holding one 256-byte record copy each. A write
//! goes to the copy that does not hold the key's newest valid record, then
//! fsync; reads return the newest valid copy. A copy failing its CRC reads as
//! absent; an intact record naming another key fails closed.
//!
//! A failed sync may leave bytes in FAT's buffers that read back like synced
//! ones. Before another transaction reads, or a retry picks its write target,
//! a one-byte write in unused sector 7 and a sync force FatFs's
//! modified-handle volume barrier; a clean handle's sync does nothing. So a
//! write never overwrites the only synced copy of an acknowledged value while
//! newer bytes are buffered.
//!
//! Creation fills a fresh O_EXCL temp, syncs, closes and renames it. Until a
//! State file exists, legacy NVS stays authoritative and read-only: creation
//! copies its blobs verbatim. Statistics live in `<Stats>/`, have no legacy
//! NVS, and a damaged statistics file waits for set_aside or repair_in_place.
use crate::text::Text;
use crate::{Directory, Error, Kind, Native, Open, Result};
use newt_sdk_heap::Fixed;
#[path = "active.rs"]
mod active;
use active::{Active, Record};
#[path = "reading_state/transaction.rs"]
mod transaction;
pub use transaction::ScopedTransaction;
use core::fmt::Write as _;
use newt_core::durable::{Backend, BlobRead, DurableWrite, SLOT_KEYS, StorageOpen};
#[path = "quick_start_namespace.rs"]
mod namespace;
use namespace::{numbered_namespace, reading_state_namespace, stats_namespace};
use newt_core::reading_stats_store::StatsBackend;
use newt_core::sd_record::{
    FILE_BYTES, HEADER_BYTES, PAYLOAD_BYTES, RECORD_BYTES, SECTOR_BYTES, header, intact, seal,
};
use newt_core::stats_service_backend::NamespaceBackend;

/// Firmware folders (NEWT_READING_STATE_ROOT "/newt-sd/Newt").
pub const FIRMWARE_STATE: &str = "/newt-sd/Newt/State";
pub const FIRMWARE_STATS: &str = "/newt-sd/Newt/Stats";

// The original's `char file[80]` and its `sizeof file+16` side names.
type FileName = Text<80>;
type SideName = Text<96>;
const KEYS: [&str; 3] = ["digest", SLOT_KEYS[0], SLOT_KEYS[1]];
const ATTEMPTS: u32 = 32;
// Newest-copy results: a copy index, or none (no intact copy), failed (a read
// error or a foreign record), or unknown (not yet seen this transaction).
const NONE: i8 = -1;
const FAILED: i8 = -2;
const UNKNOWN: i8 = -3;
const BARRIER: u32 = (FILE_BYTES - SECTOR_BYTES) as u32;
const _: () = assert!(copy_offset(2, 1) + SECTOR_BYTES as u32 <= BARRIER);

const fn copy_offset(key: usize, copy: usize) -> u32 {
    ((1 + 2 * key + copy) * SECTOR_BYTES) as u32
}

fn field(record: &[u8; RECORD_BYTES], at: usize) -> u32 {
    u32::from_le_bytes([record[at], record[at + 1], record[at + 2], record[at + 3]])
}

/// The selection's owners the store borrows on every operation. STATE and
/// STATS are sibling folders under one root, fixed at compile time.
pub trait StateMedium<N: Native> {
    const STATE: &'static str;
    const STATS: &'static str;
    /// Withdraw only the optional reader Pages lease, through its owner. A
    /// paused producer has already suspended its writer; never abort it here.
    fn release_pages_for_state(&mut self);
    /// Mount, or confirm, the card. State I/O is never cancelled.
    fn mount(&mut self, native: Option<&mut N>) -> bool;
}

#[path = "reading_state/facts.rs"]
mod facts;
pub use facts::StateFacts;

/// Native calls under the original's errno discipline: every failing call is
/// remembered, including ignored ones, until one of the original's clearing
/// points (mount, a read, a put, each readdir).
struct Io<'s, N> {
    // None: no card, so no native I/O authority. The no-card medium never
    // mounts, so no call gets here; one that did would fail as a missing medium.
    native: Option<&'s mut N>,
    errno: Option<Error>,
}

impl<N> Io<'_, N> {
    fn call<T>(&mut self, call: impl FnOnce(&mut N) -> Result<T>) -> Result<T> {
        let result = match self.native.as_deref_mut() {
            Some(native) => call(native),
            None => Err(Error::Unavailable),
        };
        if let Err(error) = result {
            self.errno = Some(error);
        }
        result
    }
}

/// sd_cache_file_port.h's InterruptedSyscall: at most four calls per run of
/// interruptions; any completed call starts a new run.
struct Interrupted(u32);

impl Interrupted {
    fn retry<T>(&mut self, result: &Result<T>) -> bool {
        match result {
            Ok(_) => {
                self.0 = 0;
                false
            }
            Err(Error::Interrupted) => {
                self.0 += 1;
                self.0 < 4
            }
            Err(_) => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Found {
    Valid,
    Absent,
    Damaged,
    Failed,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Match {
    Damaged,
    Intact,
    Foreign,
}

/// A key's record copy: an intact copy of `name`, damaged (ignored), or
/// intact but foreign.
fn matches(record: &[u8; RECORD_BYTES], name: &str) -> Match {
    if !intact(record, b"NRSK") {
        return Match::Damaged;
    }
    let stored = &record[4..12];
    let stored = &stored[..stored.iter().position(|&b| b == 0).unwrap_or(stored.len())];
    if stored != name.as_bytes() || field(record, 12) > PAYLOAD_BYTES as u32 {
        Match::Foreign
    } else {
        Match::Intact
    }
}

// Header comparison follows the read: the same exact encoded-header compare.
#[inline(never)]
fn matches_header(found: &[u8; RECORD_BYTES], name: &str) -> bool {
    let mut expected = [0; RECORD_BYTES];
    header(&mut expected, name.as_bytes()) && *found == expected
}

fn side_name(file: &str, nonce: u32, suffix: &str) -> Option<SideName> {
    let mut name = SideName::empty();
    write!(name, "{file}.{nonce:08x}.{suffix}").ok()?;
    Some(name)
}

/// Makes every component of the State folder (EEXIST is fine), each of which
/// must then stat as a directory.
fn directory<N: Native>(io: &mut Io<'_, N>, state: &str) -> bool {
    let bytes = state.as_bytes();
    for end in (1..=bytes.len()).filter(|&at| at == bytes.len() || bytes[at] == b'/') {
        let path = &state[..end];
        if matches!(io.call(|n| n.mkdir(path)), Err(error) if error != Error::Exists)
            || !matches!(io.call(|n| n.stat(path, false)), Ok(info) if info.kind == Kind::Directory)
        {
            return false;
        }
    }
    true
}

struct Newest {
    best: i8,
    // The copy whose bytes are in the caller's record.
    loaded: i8,
    generation: u32,
}

impl<T> Active<T> {
    /// Allocate after mount; the legacy handle/open flags belong to this owner.
    fn allocate<N: Native, M: StateMedium<N>>(device: T, name: &str) -> Option<Fixed<Self>> {
        let mut slot = Fixed::try_new(Self { record: Record::NEW, device })?;
        slot.record.path::<N, M>(name).then_some(slot)
    }
}

impl Record {
    fn path<N: Native, M: StateMedium<N>>(&mut self, name: &str) -> bool {
        self.stats = stats_namespace(name);
        let mut file = FileName::empty();
        if file
            .write_str(if self.stats { M::STATS } else { M::STATE })
            .is_err()
            || file.write_char('/').is_err()
            || name
                .chars()
                .any(|c| file.write_char(if c == '_' { '-' } else { c }).is_err())
            || file.write_str(".nrs").is_err()
        {
            return false;
        }
        self.file = file;
        true
    }

    fn open<N: Native>(&mut self, io: &mut Io<'_, N>, mode: Open, name: &str) -> Found {
        let fd = match io.call(|n| n.open(&self.file, mode)) {
            Ok(fd) => fd,
            Err(Error::Missing) => return Found::Absent,
            Err(_) => return Found::Failed,
        };
        self.fd = fd;
        let mut found = [0; RECORD_BYTES];
        let Ok(info) = io.call(|n| n.fstat(fd)) else {
            return Found::Failed;
        };
        if info.size == FILE_BYTES as i64 {
            if !self.read_at(io, 0, &mut found) {
                return Found::Failed;
            }
            if matches_header(&found, name) {
                return Found::Valid;
            }
        }
        self.close(io);
        Found::Damaged
    }

    fn read_at<N: Native>(
        &mut self,
        io: &mut Io<'_, N>,
        offset: u32,
        record: &mut [u8; RECORD_BYTES],
    ) -> bool {
        let fd = self.fd;
        if fd < 0 { return false; }
        if io.call(|n| n.seek(fd, offset)) != Ok(u64::from(offset)) {
            return false;
        }
        let mut interrupted = Interrupted(0);
        let mut got = 0;
        while got < RECORD_BYTES {
            let n = io.call(|n| n.read(fd, &mut record[got..]));
            if interrupted.retry(&n) {
                continue;
            }
            match n {
                Ok(n) if n > 0 && n <= RECORD_BYTES - got => got += n,
                _ => return false,
            }
        }
        true
    }

    fn put<N: Native>(
        &mut self,
        io: &mut Io<'_, N>,
        offset: u32,
        record: &[u8; RECORD_BYTES],
    ) -> bool {
        let fd = self.fd;
        if fd < 0 { return false; }
        if io.call(|n| n.seek(fd, offset)) != Ok(u64::from(offset)) {
            return false;
        }
        let mut interrupted = Interrupted(0);
        let mut done = 0;
        while done < RECORD_BYTES {
            let n = io.call(|n| n.write(fd, &record[done..]));
            if interrupted.retry(&n) {
                continue;
            }
            match n {
                Ok(n) if n > 0 && n <= RECORD_BYTES - done => done += n,
                _ => return false,
            }
        }
        true
    }

    /// The newest intact copy of a key. A remembered copy is reread and must
    /// still match, else both copies are scanned; a remembered none came from
    /// a scan of both copies in this transaction and is trusted.
    fn newest<N: Native>(
        &mut self,
        io: &mut Io<'_, N>,
        key: usize,
        name: &str,
        record: &mut [u8; RECORD_BYTES],
    ) -> Newest {
        let mut newest = Newest {
            best: NONE,
            loaded: NONE,
            generation: 0,
        };
        let known = self.known[key];
        if known >= 0 {
            if !self.read_at(io, copy_offset(key, known as usize), record) {
                newest.best = FAILED;
                return newest;
            }
            newest.loaded = known;
            if matches(record, name) == Match::Intact
                && field(record, 16) == self.known_generation[key]
            {
                newest.best = known;
                newest.generation = self.known_generation[key];
                return newest;
            }
        }
        if known == NONE {
            return newest;
        }
        for copy in 0..2 {
            if !self.read_at(io, copy_offset(key, copy), record) {
                newest.best = FAILED;
                return newest;
            }
            newest.loaded = copy as i8;
            let found = matches(record, name);
            if found == Match::Foreign {
                newest.best = FAILED;
                return newest;
            }
            let value = field(record, 16);
            // Signed wrapping order; a tie or a half-range gap keeps copy 0.
            if found == Match::Intact
                && (newest.best < 0 || (value.wrapping_sub(newest.generation) as i32) > 0)
            {
                newest.best = copy as i8;
                newest.generation = value;
            }
        }
        self.known[key] = newest.best;
        self.known_generation[key] = newest.generation;
        newest
    }

    fn read<N: Native>(&mut self, io: &mut Io<'_, N>, key: &str, output: &mut [u8]) -> BlobRead {
        let mut record = [0; RECORD_BYTES];
        let Some(index) = KEYS.iter().position(|&k| k == key) else {
            return BlobRead::Failed;
        };
        let newest = self.newest(io, index, key, &mut record);
        if newest.best == NONE {
            return BlobRead::Missing;
        }
        // The scan may have ended on the other copy: reread the newest and check it again.
        if newest.best < 0
            || (newest.best != newest.loaded
                && (!self.read_at(io, copy_offset(index, newest.best as usize), &mut record)
                    || matches(&record, key) != Match::Intact
                    || field(&record, 16) != newest.generation))
        {
            return BlobRead::Failed;
        }
        let observed = field(&record, 12) as usize;
        if observed == output.len() && observed != 0 {
            output.copy_from_slice(&record[HEADER_BYTES..HEADER_BYTES + observed]);
        }
        BlobRead::Present(observed)
    }

    fn write<N: Native>(&mut self, io: &mut Io<'_, N>, key: &str, bytes: &[u8]) -> bool {
        let mut record = [0; RECORD_BYTES];
        let Some(index) = KEYS.iter().position(|&k| k == key) else {
            return false;
        };
        let fd = self.fd;
        if fd < 0 { return false; }
        if bytes.len() > PAYLOAD_BYTES {
            return false;
        }
        let newest = self.newest(io, index, key, &mut record);
        if newest.best == FAILED {
            return false;
        }
        let target = if newest.best == 0 { 1 } else { 0 };
        let generation = if newest.best == NONE {
            1
        } else {
            newest.generation.wrapping_add(1)
        };
        record[HEADER_BYTES..HEADER_BYTES + bytes.len()].copy_from_slice(bytes);
        self.known[index] = UNKNOWN; // until the copy is written and synced
        if !seal(
            &mut record,
            b"NRSK",
            key.as_bytes(),
            bytes.len(),
            generation,
        ) || !self.put(io, copy_offset(index, target), &record)
            || io.call(|n| n.sync(fd)).is_err()
        {
            return false;
        }
        self.known[index] = target as i8;
        self.known_generation[index] = generation;
        // Leave the header in FAT's sector buffer so read-back comes from the card.
        self.read_at(io, 0, &mut record)
    }

    // ff.c gates sync_fs on FA_MODIFIED and clears it even when sync_fs fails.
    // Dirty the unused sector, never an acknowledged record; this handle's
    // successful sync then drains the volume.
    fn settle<N: Native>(&mut self, io: &mut Io<'_, N>) -> bool {
        self.buffered = true;
        let fd = self.fd;
        if fd < 0 { return false; }
        if io.call(|n| n.seek(fd, BARRIER)) != Ok(u64::from(BARRIER)) {
            return false;
        }
        let mut interrupted = Interrupted(0);
        loop {
            let n = io.call(|n| n.write(fd, &[0]));
            if interrupted.retry(&n) {
                continue;
            }
            if n != Ok(1) {
                return false;
            }
            break;
        }
        if io.call(|n| n.sync(fd)).is_err() {
            return false;
        }
        self.buffered = false;
        true
    }

    // Statistics sit beside State, so once directory() has made the root they
    // need one more level.
    fn folder_ready<N: Native, M: StateMedium<N>>(&self, io: &mut Io<'_, N>) -> bool {
        directory(io, M::STATE)
            && (!self.stats
                || (matches!(io.call(|n| n.mkdir(M::STATS)), Ok(()) | Err(Error::Exists))
                    && matches!(io.call(|n| n.stat(M::STATS, false)), Ok(info) if info.kind == Kind::Directory)))
    }

    /// A fresh O_EXCL temp gets every record written explicitly (header,
    /// legacy blobs as generation 1 in the first copy, empty records
    /// elsewhere), so nothing depends on how the file system fills new
    /// clusters. It is synced, closed and renamed into place. Taken names are
    /// skipped; stale temps are never truncated or deleted.
    fn create<N: Native, M: StateMedium<N>, L: Backend + ?Sized>(
        &mut self,
        io: &mut Io<'_, N>,
        facts: &mut StateFacts,
        name: &str,
        mut seed: Option<&mut L>,
        damaged: bool,
    ) -> bool {
        let mut temp = SideName::empty();
        if !self.folder_ready::<N, M>(io) {
            return false;
        }
        let mut attempt = 0;
        while attempt < ATTEMPTS && self.fd < 0 {
            let Some(candidate) = side_name(&self.file, facts.next_nonce(), "tmp") else {
                return false;
            };
            temp = candidate;
            match io.call(|n| n.open(&temp, Open::CreateExclusive)) {
                Ok(fd) => self.fd = fd,
                Err(Error::Exists) => {}
                Err(_) => return false,
            }
            attempt += 1;
        }
        let fd = self.fd;
        if fd < 0 { return false; }
        let mut ok;
        {
            let mut record = [0; RECORD_BYTES];
            ok = io.call(|n| n.truncate(fd, FILE_BYTES as u32)).is_ok()
                && matches!(io.call(|n| n.fstat(fd)), Ok(info) if info.size == FILE_BYTES as i64)
                && header(&mut record, name.as_bytes())
                && self.put(io, 0, &record);
            for (key, id) in KEYS.into_iter().enumerate() {
                if !ok {
                    break;
                }
                let mut result = BlobRead::Missing;
                if let Some(device) = seed.as_deref_mut() {
                    // The legacy backend copies only on an exact size; ask again at the observed size.
                    result = device
                        .read_blob(id, &mut record[HEADER_BYTES..HEADER_BYTES + PAYLOAD_BYTES]);
                    if let BlobRead::Present(observed) = result
                        && observed < PAYLOAD_BYTES
                    {
                        result = device
                            .read_blob(id, &mut record[HEADER_BYTES..HEADER_BYTES + observed]);
                    }
                }
                ok = result != BlobRead::Failed
                    && !matches!(result, BlobRead::Present(n) if n > PAYLOAD_BYTES);
                match result {
                    BlobRead::Present(observed) if ok => {
                        ok = seal(&mut record, b"NRSK", id.as_bytes(), observed, 1)
                    }
                    _ => record.fill(0),
                }
                ok = ok && self.put(io, copy_offset(key, 0), &record);
                record.fill(0);
                ok = ok && self.put(io, copy_offset(key, 1), &record);
            }
            ok = ok && io.call(|n| n.sync(fd)).is_ok();
        }
        self.close(io);
        if ok && damaged {
            ok = self.set_aside(io, facts);
        }
        if ok {
            match io.call(|n| n.rename(&temp, &self.file)) {
                Ok(()) => return true,
                Err(Error::Exists | Error::Missing) => {}
                // A rename that failed for another reason may have written the
                // new directory entry already, so that temp is left like any stale one.
                Err(_) => return false,
            }
        }
        let _ = io.call(|n| n.unlink(&temp));
        false
    }

    /// Set the damaged file aside under a name that is not taken; FAT refuses
    /// to rename onto an existing name, so nothing is overwritten.
    fn set_aside<N: Native>(&mut self, io: &mut Io<'_, N>, facts: &mut StateFacts) -> bool {
        for _ in 0..ATTEMPTS {
            let Some(aside) = side_name(&self.file, facts.next_nonce(), "bad") else {
                return false;
            };
            match io.call(|n| n.rename(&self.file, &aside)) {
                Ok(()) | Err(Error::Missing) => return true,
                Err(Error::Exists) => {}
                Err(_) => return false,
            }
        }
        false
    }

    /// Evidence first: the file's bytes go to a fresh O_EXCL ".bad", synced.
    /// Then the file is rewritten in place. The header is invalidated and
    /// synced before any key sector changes, and rewritten last, so a cut
    /// leaves either a damaged file (repaired again) or the complete new one.
    fn repair<N: Native>(
        &mut self,
        io: &mut Io<'_, N>,
        facts: &mut StateFacts,
        name: &str,
        payload: &[u8],
    ) -> bool {
        let mut record = [0; RECORD_BYTES];
        if payload.len() > PAYLOAD_BYTES {
            return false;
        }
        let Ok(fd) = io.call(|n| n.open(&self.file, Open::SourceReadOnly)) else {
            return false;
        };
        self.fd = fd;
        let mut copy = None;
        let mut attempt = 0;
        while attempt < ATTEMPTS && copy.is_none() {
            let Some(aside) = side_name(&self.file, facts.next_nonce(), "bad") else {
                break;
            };
            match io.call(|n| n.open(&aside, Open::CreateExclusiveWriteOnly)) {
                Ok(opened) => copy = Some(opened),
                Err(Error::Exists) => {}
                Err(_) => break,
            }
            attempt += 1;
        }
        let mut ok = copy.is_some();
        if let Some(copy) = copy {
            // One retry budget for the whole copy, as in the original.
            let mut interrupted = Interrupted(0);
            while ok {
                let n = io.call(|n| n.read(fd, &mut record));
                if interrupted.retry(&n) {
                    continue;
                }
                let n = match n {
                    Ok(0) => break,
                    Ok(n) if n <= RECORD_BYTES => n,
                    _ => {
                        ok = false;
                        break;
                    }
                };
                let mut done = 0;
                while ok && done < n {
                    let w = io.call(|x| x.write(copy, &record[done..n]));
                    if interrupted.retry(&w) {
                        continue;
                    }
                    match w {
                        Ok(w) if w > 0 && w <= n - done => done += w,
                        _ => ok = false,
                    }
                }
            }
            ok = ok && io.call(|n| n.sync(copy)).is_ok();
            ok = io.call(|n| n.close(copy)).is_ok() && ok;
        }
        self.close(io);
        if !ok {
            return false;
        }
        let Ok(fd) = io.call(|n| n.open(&self.file, Open::ReadWrite)) else {
            return false;
        };
        self.fd = fd;
        record.fill(0);
        ok = io.call(|n| n.truncate(fd, FILE_BYTES as u32)).is_ok()
            && self.put(io, 0, &record)
            && io.call(|n| n.sync(fd)).is_ok();
        'records: for key in 0..3 {
            for at in 0..2 {
                if !ok {
                    break 'records;
                }
                record.fill(0);
                if key == 1 && at == 0 {
                    record[HEADER_BYTES..HEADER_BYTES + payload.len()].copy_from_slice(payload);
                    ok = seal(
                        &mut record,
                        b"NRSK",
                        SLOT_KEYS[0].as_bytes(),
                        payload.len(),
                        1,
                    );
                }
                ok = ok && self.put(io, copy_offset(key, at), &record);
            }
        }
        ok = ok
            && io.call(|n| n.sync(fd)).is_ok()
            && header(&mut record, name.as_bytes())
            && self.put(io, 0, &record)
            && io.call(|n| n.sync(fd)).is_ok();
        self.close(io);
        ok
    }

    fn close<N: Native>(&mut self, io: &mut Io<'_, N>) {
        if self.fd >= 0 {
            let _ = io.call(|n| n.close(self.fd));
        }
        self.fd = -1;
    }
}

enum Listing<T> {
    Opened(T, [u8; 2]),
    Empty,
    Refused,
}

enum Entry {
    Name([u8; 15]),
    Skip,
    End,
    Failed,
}

/// The SD transaction owns its legacy state. Only native-call authority is
/// borrowed from the router while its ordinary NVS transaction is closed.
struct SdReadingState<'s, N: Native, M: StateMedium<N>, T> {
    facts: &'s mut StateFacts,
    medium: &'s mut M,
    io: Io<'s, N>,
    active: Option<Fixed<Active<T>>>,
}

impl<'s, N: Native, M: StateMedium<N>, T> SdReadingState<'s, N, M, T> {
    fn parts(&mut self) -> Option<(&mut Record, &mut Io<'s, N>, &mut StateFacts)> {
        let active = &mut self.active.as_mut()?.record;
        Some((active, &mut self.io, &mut *self.facts))
    }

    fn transaction_parts(&mut self) -> Option<(&mut Record, &mut T, &mut Io<'s, N>, &mut StateFacts)> {
        let active = &mut **self.active.as_mut()?;
        Some((&mut active.record, &mut active.device, &mut self.io, &mut *self.facts))
    }

    // Withdraw the optional Pages lease before EVERY durable read or write.
    fn mounted(&mut self) -> bool {
        self.medium.release_pages_for_state();
        self.facts.unavailable = !self.medium.mount(self.io.native.as_deref_mut());
        self.io.errno = None; // only a failing call after this may classify the medium
        !self.facts.unavailable
    }

    fn fail<L: ScopedTransaction<State = T>>(&mut self, legacy: &mut L) {
        self.facts.note(self.io.errno);
        self.end(legacy);
    }

    fn begin<L: ScopedTransaction<State = T>>(&mut self, legacy: &mut L, name: &str, readonly: bool) -> StorageOpen {
        self.end(legacy);
        if !reading_state_namespace(name) {
            return StorageOpen::Failed;
        }
        if !self.mounted() {
            return if readonly {
                StorageOpen::Missing
            } else {
                StorageOpen::Failed
            };
        }
        self.active = Active::allocate::<N, M>(L::CLOSED, name);
        let Some((active, device, io, facts)) = self.transaction_parts() else {
            return StorageOpen::Failed;
        };
        let mode = if readonly && !facts.sync_failed {
            Open::SourceReadOnly
        } else {
            Open::ReadWrite
        };
        let found = active.open(io, mode, name);
        if found == Found::Valid {
            if facts.sync_failed && !active.settle(io) {
                self.fail(legacy);
                return StorageOpen::Failed;
            }
            facts.sync_failed = false;
            return StorageOpen::Opened;
        }
        if found == Found::Failed {
            self.fail(legacy);
            return StorageOpen::Failed;
        }
        if active.stats {
            return self.begin_stats(legacy, name, readonly, found);
        }
        // Absent, or damaged until the first write sets it aside: legacy NVS is
        // authoritative until a valid file exists.
        let previous = legacy.with_transaction(device, |legacy| legacy.begin(name, true))
            .unwrap_or(StorageOpen::Failed);
        if previous == StorageOpen::Failed {
            self.end(legacy);
            return StorageOpen::Failed;
        }
        if readonly {
            if previous == StorageOpen::Missing {
                self.end(legacy);
                return StorageOpen::Missing;
            }
            active.legacy = true;
            return StorageOpen::Opened;
        }
        let created = legacy.with_transaction(device, |legacy| {
            let seed = (previous == StorageOpen::Opened).then_some(legacy);
            active.create::<N, M, dyn Backend + '_>(io, facts, name, seed, found == Found::Damaged)
        }).unwrap_or(false);
        if !created || active.open(io, Open::ReadWrite, name) != Found::Valid
        {
            self.fail(legacy);
            return StorageOpen::Failed;
        }
        let _ = legacy.with_transaction(device, |legacy| legacy.end());
        StorageOpen::Opened
    }

    // Statistics have no legacy NVS: damaged waits for repair; a write creates.
    #[inline(never)]
    fn begin_stats<L: ScopedTransaction<State = T>>(
        &mut self,
        legacy: &mut L,
        name: &str,
        readonly: bool,
        found: Found,
    ) -> StorageOpen {
        if found == Found::Damaged {
            self.end(legacy);
            return StorageOpen::Damaged;
        }
        if readonly {
            self.end(legacy);
            return StorageOpen::Missing;
        }
        let Some((active, io, facts)) = self.parts() else {
            return StorageOpen::Failed;
        };
        if !active.create::<N, M, L>(io, facts, name, None, false)
            || active.open(io, Open::ReadWrite, name) != Found::Valid
        {
            self.fail(legacy);
            return StorageOpen::Failed;
        }
        StorageOpen::Opened
    }

    fn end<L: ScopedTransaction<State = T>>(&mut self, legacy: &mut L) {
        let Some(mut slot) = self.active.take() else {
            return;
        };
        slot.record.close(&mut self.io);
        let _ = legacy.with_transaction(&mut slot.device, |legacy| legacy.end());
    }

    fn read_blob<L: ScopedTransaction<State = T>>(&mut self, legacy: &mut L, key: &str, output: &mut [u8]) -> BlobRead {
        let Some((active, device, io, facts)) = self.transaction_parts() else {
            return BlobRead::Failed;
        };
        if active.legacy {
            return legacy.with_transaction(device, |legacy| legacy.read_blob(key, output))
                .unwrap_or(BlobRead::Failed);
        }
        io.errno = None;
        let result = active.read(io, key, output);
        if result == BlobRead::Failed {
            facts.note(io.errno);
        }
        result
    }

    fn put_bytes(&mut self, key: &str, bytes: &[u8]) -> usize {
        let Some((active, io, facts)) = self.parts() else {
            return 0;
        };
        if active.legacy {
            return 0;
        }
        io.errno = None;
        // The write picks its target copy from what it reads: effective barrier first.
        if facts.sync_failed && !active.settle(io) {
            facts.note(io.errno);
            return 0;
        }
        facts.sync_failed = false;
        if active.write(io, key, bytes) {
            return bytes.len();
        }
        facts.note(io.errno);
        facts.sync_failed = true;
        active.buffered = true;
        0
    }

    // A stat, so any open record would stay open.
    fn state_folder(&mut self) -> StorageOpen {
        if !self.mounted() {
            return StorageOpen::Failed;
        }
        match self.io.call(|n| n.stat(M::STATE, false)) {
            Ok(_) => StorageOpen::Opened,
            Err(Error::Missing) => StorageOpen::Missing,
            Err(_) => {
                self.facts.note(self.io.errno);
                StorageOpen::Failed
            }
        }
    }

    fn readback_durable(&self) -> bool {
        self.active.as_ref().is_none_or(|slot| !slot.record.buffered)
    }

    /// Opens a statistics name for a repair, outside any transaction.
    fn open_stats<L: ScopedTransaction<State = T>>(&mut self, legacy: &mut L, name: &str) -> bool {
        self.end(legacy);
        if !stats_namespace(name) || !self.mounted() {
            return false;
        }
        self.active = Active::allocate::<N, M>(L::CLOSED, name);
        self.active.is_some()
    }

    fn finish<L: ScopedTransaction<State = T>>(&mut self, legacy: &mut L, ok: bool) -> bool {
        if !ok {
            self.facts.note(self.io.errno);
            self.facts.sync_failed = true;
        }
        self.end(legacy);
        ok
    }

    fn set_aside<L: ScopedTransaction<State = T>>(&mut self, legacy: &mut L, name: &str) -> bool {
        if !self.open_stats(legacy, name) {
            return false;
        }
        let ok = self
            .parts()
            .is_some_and(|(active, io, facts)| active.set_aside(io, facts));
        self.finish(legacy, ok)
    }

    fn repair_in_place<L: ScopedTransaction<State = T>>(&mut self, legacy: &mut L, name: &str, payload: &[u8]) -> bool {
        if !self.open_stats(legacy, name) {
            return false;
        }
        let ok = self
            .parts()
            .is_some_and(|(active, io, facts)| active.repair(io, facts, name, payload));
        self.finish(legacy, ok)
    }

    fn open_listing<L: ScopedTransaction<State = T>>(&mut self, legacy: &mut L, prefix: &str) -> Listing<N::Listing>
    where
        N: Directory,
    {
        self.end(legacy);
        let stats = prefix == "sb" || prefix == "ci";
        if (!stats && prefix != "nb") || !self.mounted() {
            return Listing::Refused;
        }
        match self
            .io
            .call(|n| n.open_dir(if stats { M::STATS } else { M::STATE }))
        {
            Ok(listing) => Listing::Opened(listing, [prefix.as_bytes()[0], prefix.as_bytes()[1]]),
            Err(Error::Missing) => Listing::Empty,
            Err(_) => {
                self.facts.note(self.io.errno);
                Listing::Refused
            }
        }
    }

    fn next_listed(&mut self, listing: &mut N::Listing, prefix: [u8; 2]) -> Entry
    where
        N: Directory,
    {
        self.io.errno = None;
        let mut entry = [0; 19];
        match self.io.call(|n| n.read_dir(listing, &mut entry)) {
            Err(_) => {
                self.facts.note(self.io.errno);
                Entry::Failed
            }
            Ok(None) => Entry::End,
            // `<prefix><13 hex>.nrs`; temps and set-aside files carry other suffixes.
            Ok(Some(length)) if length == entry.len() && entry[15..] == *b".nrs" => {
                let mut stem = [0; 15];
                stem.copy_from_slice(&entry[..15]);
                match core::str::from_utf8(&stem) {
                    Ok(name) if numbered_namespace(name, prefix, 13, true) => Entry::Name(stem),
                    _ => Entry::Skip,
                }
            }
            Ok(Some(_)) => Entry::Skip,
        }
    }

    fn close_listing(&mut self, listing: N::Listing)
    where
        N: Directory,
    {
        let _ = self.io.call(|n| n.close_dir(listing));
    }
}

impl<N: Native, M: StateMedium<N>, T> Drop for SdReadingState<'_, N, M, T> {
    // Backstop only: the router has already ended any transaction.
    fn drop(&mut self) {
        if let Some(slot) = self.active.as_mut() {
            slot.record.close(&mut self.io);
        }
    }
}

/// NvsProgressBackend with reading state attached: every reading-state and
/// statistics namespace goes to SD, nothing falls back to NVS writes, and
/// every other namespace behaves as the plain NVS backend. One transaction at
/// a time: begin ends any transaction still open. Build it inside the UI I/O
/// scope, or with new_unmounted while Board has no card; `facts` outlives
/// every scope of the boot and passes from one router to the next.
pub struct ReadingStateRouter<'s, L: ScopedTransaction, N: Native, M: StateMedium<N>> {
    nvs: L,
    state: SdReadingState<'s, N, M, L::State>,
    routed: bool,
}

impl<'s, L: ScopedTransaction, N: Native, M: StateMedium<N>> ReadingStateRouter<'s, L, N, M> {
    /// Exact physical transaction allocation, for host/target layout witnesses.
    pub const ACTIVE_BYTES: usize = core::mem::size_of::<Active<L::State>>();
    pub const ACTIVE_ALIGN: usize = core::mem::align_of::<Active<L::State>>();

    pub fn new(nvs: L, facts: &'s mut StateFacts, medium: &'s mut M, native: &'s mut N) -> Self {
        Self::with(nvs, facts, medium, Some(native))
    }

    /// No card, so no native I/O: `medium` must answer every mount false.
    /// Reading state and statistics then fail as the original's unmounted
    /// store does; other namespaces stay plain NVS. Nothing reads or writes
    /// on construction.
    pub fn new_unmounted(nvs: L, facts: &'s mut StateFacts, medium: &'s mut M) -> Self {
        Self::with(nvs, facts, medium, None)
    }

    fn with(
        nvs: L,
        facts: &'s mut StateFacts,
        medium: &'s mut M,
        native: Option<&'s mut N>,
    ) -> Self {
        Self {
            nvs,
            state: SdReadingState {
                facts,
                medium,
                io: Io {
                    native,
                    errno: None,
                },
                active: None,
            },
            routed: false,
        }
    }

    /// Whether the card holds Newt's State folder: Missing only when a
    /// mounted card answers that it does not.
    pub fn state_folder(&mut self) -> StorageOpen {
        self.end();
        self.state.state_folder()
    }
}

impl<L: ScopedTransaction, N: Native, M: StateMedium<N>> Backend for ReadingStateRouter<'_, L, N, M> {
    fn begin(&mut self, namespace: &str, readonly: bool) -> StorageOpen {
        self.end();
        self.routed = reading_state_namespace(namespace);
        if !self.routed {
            return self.nvs.begin(namespace, readonly);
        }
        self.state.begin(&mut self.nvs, namespace, readonly)
    }

    fn end(&mut self) {
        if !self.routed {
            self.nvs.end();
            return;
        }
        self.state.end(&mut self.nvs);
        self.routed = false;
    }

    fn read_blob(&mut self, key: &str, output: &mut [u8]) -> BlobRead {
        if !self.routed {
            return self.nvs.read_blob(key, output);
        }
        self.state.read_blob(&mut self.nvs, key, output)
    }

    fn put_bytes(&mut self, permit: DurableWrite, key: &str, bytes: &[u8]) -> usize {
        if !self.routed {
            return self.nvs.put_bytes(permit, key, bytes);
        }
        self.state.put_bytes(key, bytes)
    }

    // Only the active SD transaction's buffered bytes are unconfirmed; opening
    // another SD transaction first settles them.
    fn readback_durable(&self) -> bool {
        self.state.readback_durable()
    }
}

impl<L: ScopedTransaction, N: Native, M: StateMedium<N>> StatsBackend for ReadingStateRouter<'_, L, N, M> {
    fn reading_state_unavailable(&self) -> bool {
        self.state.facts.unavailable
    }

    fn set_aside(&mut self, name: &str) -> bool {
        self.end();
        self.state.set_aside(&mut self.nvs, name)
    }

    fn repair_in_place(&mut self, name: &str, payload: &[u8]) -> bool {
        self.end();
        self.state.repair_in_place(&mut self.nvs, name, payload)
    }
}

impl<L: ScopedTransaction, N: Native + Directory, M: StateMedium<N>> NamespaceBackend
    for ReadingStateRouter<'_, L, N, M>
{
    fn for_each_namespace(
        &mut self,
        prefix: &str,
        visitor: &mut dyn FnMut(&mut Self, &str) -> bool,
    ) -> bool {
        self.end();
        if prefix == "nb" && self.state.facts.legacy_pending {
            return false;
        }
        let (mut listing, prefix) = match self.state.open_listing(&mut self.nvs, prefix) {
            Listing::Opened(listing, prefix) => (listing, prefix),
            Listing::Empty => return true,
            Listing::Refused => return false,
        };
        let mut ok = true;
        loop {
            match self.state.next_listed(&mut listing, prefix) {
                Entry::Name(stem) => {
                    // next_listed admitted only ASCII hex stems.
                    let Ok(name) = core::str::from_utf8(&stem) else {
                        continue;
                    };
                    if !visitor(self, name) {
                        ok = false;
                        break;
                    }
                }
                Entry::Skip => {}
                Entry::End => break,
                Entry::Failed => {
                    ok = false;
                    break;
                }
            }
        }
        self.state.close_listing(listing);
        ok
    }
}

#[path = "reading_state/legacy.rs"]
mod legacy;

impl<L: ScopedTransaction, N: Native, M: StateMedium<N>> Drop for ReadingStateRouter<'_, L, N, M> {
    fn drop(&mut self) {
        self.end();
    }
}

// The fixtures use part of the model the protocol driver shares.
#[cfg(test)]
#[path = "../../tests/sd_reading_state/fixtures.rs"]
mod fixtures;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../tests/sd_reading_state/model.rs"]
mod model;
