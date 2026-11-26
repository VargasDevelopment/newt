// SPDX-License-Identifier: MIT
//! The lease itself: its line protocol, the temp file and the checked rename.
use super::{Card, End, Entry, ExitReason, Observation, Session, Wire, CHUNK_BYTES, IDLE_MS, LINE_BYTES,
    MEMORY_BYTES, ROOT, PATH_BYTES, TEMP, MARKER, STALL_MS, FRAME_BYTES, target_path};
use crate::update_online::Hash;
use crate::wire::crc32;
use core::ffi::CStr;
use core::fmt::{self, Write as _};
use syntax::{digest_hex, wide_number, Cursor, Request, Verb, HEX};

#[path = "commit.rs"]
mod commit;
#[path = "syntax.rs"]
mod syntax;

/// One lease. `memory` is caller-owned for this call only; every exit closes
/// the file and removes the temp unless DONE reached its commit point.
pub fn run(session: Session, memory: &mut [u8; MEMORY_BYTES], wire: &mut dyn Wire,
    card: &mut dyn Card, hash: &mut dyn Hash) -> End {
    let (line, reply) = memory.split_at_mut(LINE_BYTES);
    let mut lease = Lease { session, line, reply, wire, card, hash, open: None };
    let end = lease.serve();
    lease.discard();
    end
}

struct Open {
    target: [u8; PATH_BYTES],
    fd: Option<i32>,
    size: u64,
    written: u64,
    sha256: [u8; 32],
}

struct Lease<'a> {
    session: Session,
    line: &'a mut [u8],
    reply: &'a mut [u8],
    wire: &'a mut dyn Wire,
    card: &'a mut dyn Card,
    hash: &'a mut dyn Hash,
    open: Option<Open>,
}

/// A refusal after a current request: answered, then the lease ends.
#[derive(Debug)]
enum Refused { Order, Arguments, Name, Size, Digest, Offset, Length, Short,
    CardFull(u64), Target, Temp, SizeMismatch, DigestMismatch, Hash, Card(&'static str, i32),
    /// A commit the lease cannot prove finished or undone; the card's
    /// scratch files and marker stay for a person to check.
    Recovery(&'static str, i32),
    /// DONE stopped before its first rename: answered as an abandoned lease.
    Abandoned(&'static str),
    /// The reply itself stalled or the host left: nothing more can be said.
    Reply }

enum Step { Next, Finished }

impl Lease<'_> {
    fn serve(&mut self) -> End {
        let id = self.session.request;
        if !self.send(id, format_args!("PUT line={LINE_BYTES} chunk={CHUNK_BYTES} path=hex data=raw")) {
            return End::Cancelled;
        }
        loop {
            let length = match self.read_line() {
                Ok(length) => length,
                Err(end) => return end,
            };
            let request = match self.parse(length) {
                Ok(request) => request,
                Err(end) => return end,
            };
            let id = request.id;
            self.session.request = id;
            self.wire.observe(Observation::Request(id));
            let step = match request.verb {
                Verb::File => self.file(request.arguments),
                Verb::Data => self.data(request.arguments),
                Verb::Done => self.done(),
            };
            match step {
                Ok(Step::Next) => {}
                Ok(Step::Finished) => {
                    self.wire.observe(Observation::Exit(ExitReason::Done));
                    return End::Kept { last_id: id };
                }
                Err(Refused::Reply) => return End::Cancelled,
                Err(Refused::Abandoned(reason)) => {
                    self.best_effort(reason);
                    return End::Cancelled;
                }
                Err(refused) => {
                    self.wire.observe(Observation::Exit(ExitReason::Refused));
                    self.refuse(id, &refused);
                    return End::Kept { last_id: id };
                }
            }
        }
    }

    /// One whole line into `line`, or the reason the lease ends.
    fn read_line(&mut self) -> Result<usize, End> {
        let mut length = 0;
        let mut activity = self.wire.now();
        loop {
            let byte = self.wire.read_byte();
            if byte < 0 || byte == i32::from(b'\n') {
                if let Some(reason) = self.abandoned(activity) {
                    self.best_effort(reason);
                    return Err(End::Cancelled);
                }
            }
            if byte < 0 {
                self.wire.pause();
                continue;
            }
            if byte == i32::from(b'\n') { return Ok(length); }
            if !(32..=126).contains(&byte) || length == self.line.len() {
                self.best_effort("line");
                return Err(End::Cancelled);
            }
            if length == 0 { activity = self.wire.now(); }
            self.line[length] = byte as u8;
            length += 1;
        }
    }

    fn abandoned(&mut self, activity: u32) -> Option<&'static str> {
        let idle = self.wire.now().wrapping_sub(activity) >= IDLE_MS;
        self.interruption().or(idle.then_some("timeout"))
    }

    /// Takeover or a disconnect: checked during reception and through DONE
    /// up to its commit point. DONE reads no line,
    /// so a HELLO sent meanwhile is read after DONE answers.
    fn interruption(&mut self) -> Option<&'static str> {
        if self.wire.physical_input() { return Some("physical-input"); }
        if !self.wire.connected() { return Some("disconnected"); }
        None
    }

    fn parse(&mut self, length: usize) -> Result<Request, End> {
        let line = &self.line[..length];
        if line == b"N1 HELLO" {
            self.wire.observe(Observation::Exit(ExitReason::Hello));
            return Err(End::Hello);
        }
        let Some(request) = Request::parse(line) else {
            self.best_effort("syntax");
            return Err(End::Cancelled);
        };
        if request.boot != self.session.boot || request.session != self.session.session {
            self.best_effort("session");
            return Err(End::Cancelled);
        }
        if request.id <= self.session.request {
            self.best_effort("stale-id");
            return Err(End::Cancelled);
        }
        Ok(request)
    }

    fn file(&mut self, arguments: (usize, usize)) -> Result<Step, Refused> {
        if self.open.is_some() { return Err(Refused::Order); }
        let text = core::str::from_utf8(&self.line[arguments.0..arguments.1]).map_err(|_| Refused::Name)?;
        let mut tokens = text.split(' ');
        let (Some(encoded), Some(size), Some(digest), None) =
            (tokens.next(), tokens.next(), tokens.next(), tokens.next()) else {
            return Err(Refused::Arguments);
        };
        let target = target_path(encoded).ok_or(Refused::Name)?;
        let size = wide_number(size).ok_or(Refused::Size)?;
        let sha256 = digest_hex(digest).ok_or(Refused::Digest)?;

        self.recover()?;
        // Check every parent without following links, including existing ones.
        let mut parent = target;
        for at in ROOT.to_bytes().len() + 1..path(&target).to_bytes().len() {
            if parent[at] != b'/' { continue; }
            parent[at] = 0;
            self.folder(path(&parent))?;
            parent[at] = b'/';
        }
        self.recover_legacy(path(&target))?;
        match self.card.entry(path(&target)).map_err(|code| Refused::Card("target", code))? {
            Entry::Missing | Entry::File => {}
            Entry::Folder | Entry::Other => return Err(Refused::Target),
        }
        self.remove_temp(TEMP)?;
        let free = self.card.free_bytes().map_err(|code| Refused::Card("free", code))?;
        if size > free { return Err(Refused::CardFull(free)); }
        // Recorded before create, so a failed create's partial temp is removed.
        self.open = Some(Open { target, fd: None, size, written: 0, sha256 });
        let fd = self.card.create(TEMP).map_err(|code| Refused::Card("create", code))?;
        if let Some(open) = &mut self.open { open.fd = Some(fd); }
        self.answer(format_args!("FILE free={free}"))
    }

    fn data(&mut self, arguments: (usize, usize)) -> Result<Step, Refused> {
        let Some(open) = &self.open else { return Err(Refused::Order) };
        let (written, size) = (open.written, open.size);
        let text = core::str::from_utf8(&self.line[arguments.0..arguments.1]).map_err(|_| Refused::Arguments)?;
        let mut fields = text.split(' ');
        let (Some(offset), Some(length), None) = (fields.next(), fields.next(), fields.next()) else { return Err(Refused::Arguments) };
        let offset = wide_number(offset).ok_or(Refused::Offset)?;
        if offset != written { return Err(Refused::Offset); }
        let decoded = wide_number(length).ok_or(Refused::Length)?;
        if decoded == 0 || decoded != (size - written).min(CHUNK_BYTES as u64) { return Err(Refused::Length); }
        let decoded = decoded as usize;
        let fd = open.fd.ok_or(Refused::Order)?;
        // Length framing makes every byte, including LF, NUL and HELLO, data.
        // The raw block fits the lease buffer; cancellation never writes a prefix.
        self.wire.observe(Observation::Receiving { offset: written, expected: decoded as u32 });
        let mut received = 0;
        let activity = self.wire.now();
        while received < decoded {
            if let Some(reason) = self.abandoned(activity) { return Err(Refused::Abandoned(reason)); }
            let remaining = decoded - received;
            let got = self.wire.read(&mut self.line[received..decoded]);
            if got == 0 { self.wire.pause(); continue; }
            if got > remaining { return Err(Refused::Abandoned("line")); }
            received += got;
        }
        self.wire.observe(Observation::Writing);
        write_all(self.card, fd, &self.line[..decoded]).map_err(|code| Refused::Card("write", code))?;
        let written = written + decoded as u64;
        if let Some(open) = &mut self.open { open.written = written; }
        self.answer(format_args!("DATA {written}"))
    }

    fn done(&mut self) -> Result<Step, Refused> {
        let Some(open) = &mut self.open else { return Err(Refused::Order) };
        if open.written != open.size { return Err(Refused::Short); }
        let (size, sha256, target) = (open.size, open.sha256, open.target);
        self.wire.observe(Observation::Commit);
        let fd = open.fd.take().ok_or(Refused::Order)?;
        let synced = self.card.sync(fd);
        let closed = self.card.close(fd);
        synced.map_err(|code| Refused::Card("sync", code))?;
        closed.map_err(|code| Refused::Card("close", code))?;
        // The card's bytes, not the received ones, are what gets checked.
        let digest = self.read_back(TEMP, size)?;
        if digest != sha256 { return Err(Refused::DigestMismatch); }
        let target = path(&target);
        // FAT's rename never replaces, so an old file steps aside first.
        let replacing = self.card.entry(target).map_err(|code| Refused::Card("target", code))? == Entry::File;
        self.mark(&target.to_bytes()[ROOT.to_bytes().len() + 1..])?;
        if let Some(reason) = self.interruption() {
            // No rename ran, so the temp the exit removes is no other name's.
            let _ = self.card.remove(MARKER);
            return Err(Refused::Abandoned(reason));
        }
        // The commit point: DONE now finishes, rolls back or reports
        // recovery-needed, and no exit removes the temp while the marker stays.
        self.open = None;
        self.commit(target, replacing)?;
        let mut hex = [0; 64];
        for (pair, byte) in hex.chunks_exact_mut(2).zip(digest) {
            pair[0] = HEX[usize::from(byte >> 4)];
            pair[1] = HEX[usize::from(byte & 15)];
        }
        let hex = core::str::from_utf8(&hex).unwrap_or_default();
        self.answer(format_args!("DONE size={size} sha256={hex}"))?;
        Ok(Step::Finished)
    }

    fn read_back(&mut self, temp: &CStr, size: u64) -> Result<[u8; 32], Refused> {
        let fd = self.card.open_read(temp).map_err(|code| Refused::Card("open", code))?;
        let result = self.hash_file(fd, size);
        let closed = self.card.close(fd);
        let digest = result?;
        closed.map_err(|code| Refused::Card("close", code))?;
        Ok(digest)
    }

    fn hash_file(&mut self, fd: i32, size: u64) -> Result<[u8; 32], Refused> {
        if !self.hash.start() { return Err(Refused::Hash); }
        let mut total: u64 = 0;
        loop {
            if let Some(reason) = self.interruption() { return Err(Refused::Abandoned(reason)); }
            let got = self.card.read(fd, &mut self.line[..CHUNK_BYTES]).map_err(|code| Refused::Card("read", code))?;
            let Some(bytes) = self.line.get(..got) else { return Err(Refused::Card("read", 0)) };
            if bytes.is_empty() { break; }
            total += got as u64;
            if total > size { return Err(Refused::SizeMismatch); }
            if !self.hash.update(bytes) { return Err(Refused::Hash); }
        }
        if total != size { return Err(Refused::SizeMismatch); }
        let mut digest = [0; 32];
        if !self.hash.finish(&mut digest) { return Err(Refused::Hash); }
        Ok(digest)
    }

    fn folder(&mut self, path: &CStr) -> Result<(), Refused> {
        match self.card.entry(path).map_err(|code| Refused::Card("folder", code))? {
            Entry::Folder => Ok(()),
            Entry::Missing => self.card.make_folder(path).map_err(|code| Refused::Card("folder", code)),
            Entry::File | Entry::Other => Err(Refused::Target),
        }
    }

    /// Remove only an abandoned temp after recovery has ruled out a commit.
    fn remove_temp(&mut self, path: &CStr) -> Result<(), Refused> {
        match self.card.entry(path).map_err(|code| Refused::Card("temp", code))? {
            Entry::Missing => Ok(()),
            Entry::File => self.card.remove(path).map_err(|code| Refused::Card("temp", code)),
            Entry::Folder | Entry::Other => Err(Refused::Temp),
        }
    }

    /// Every exit: close any open file and remove its temp.
    fn discard(&mut self) {
        let Some(open) = self.open.take() else { return };
        if let Some(fd) = open.fd { let _ = self.card.close(fd); }
        if self.card.entry(TEMP) == Ok(Entry::File) {
            let _ = self.card.remove(TEMP);
        }
    }

    fn answer(&mut self, body: fmt::Arguments<'_>) -> Result<Step, Refused> {
        if self.send(self.session.request, body) { Ok(Step::Next) } else { Err(Refused::Reply) }
    }

    fn refuse(&mut self, id: u32, refused: &Refused) {
        let _ = match refused {
            Refused::CardFull(free) => self.send(id, format_args!("ERROR card-full free={free}")),
            Refused::Card(step, code) => self.send(id, format_args!("ERROR card step={step} code={code}")),
            Refused::Recovery(step, code) => self.send(id, format_args!("ERROR recovery-needed step={step} code={code}")),
            other => self.send(id, format_args!("ERROR {}", other.reason())),
        };
    }

    /// The session is ending anyway; one attempt, answered with the last ID.
    fn best_effort(&mut self, reason: &str) {
        let reason_code = match reason {
            "physical-input" => ExitReason::PhysicalInput,
            "disconnected" => ExitReason::Disconnected,
            "timeout" => ExitReason::Timeout,
            "line" => ExitReason::Line,
            "session" => ExitReason::Session,
            "stale-id" => ExitReason::StaleId,
            _ => ExitReason::Syntax,
        };
        self.wire.observe(Observation::Exit(reason_code));
        let _ = self.send(self.session.request, format_args!("ERROR abandoned reason={reason}"));
    }

    /// One whole framed reply, or false when the channel stalls or drops.
    fn send(&mut self, id: u32, body: fmt::Arguments<'_>) -> bool {
        let Session { boot, session, .. } = self.session;
        let mut out = Cursor { bytes: self.reply, used: 0 };
        if write!(out, "!N1 {boot} {session} {id} {body}").is_err() {
            self.wire.observe(Observation::Exit(ExitReason::TxStall));
            return false;
        }
        let checksum = crc32(&out.bytes[..out.used]);
        if write!(out, " *{checksum:08x}\n").is_err() {
            self.wire.observe(Observation::Exit(ExitReason::TxStall));
            return false;
        }
        self.wire.observe(Observation::Reply);
        let mut sent = 0;
        let mut progress = self.wire.now();
        while sent < out.used {
            if !self.wire.connected() {
                self.wire.observe(Observation::Exit(ExitReason::Disconnected));
                return false;
            }
            let put = self.wire.write(&out.bytes[sent..out.used]);
            if put > out.used - sent {
                self.wire.observe(Observation::Exit(ExitReason::TxStall));
                return false;
            }
            if put != 0 {
                sent += put;
                progress = self.wire.now();
            } else if self.wire.now().wrapping_sub(progress) >= STALL_MS {
                self.wire.observe(Observation::Exit(ExitReason::TxStall));
                return false;
            } else {
                self.wire.pause();
            }
        }
        self.wire.observe(Observation::Waiting);
        true
    }
}

impl Refused {
    fn reason(&self) -> &'static str {
        match self {
            Self::Order => "order",
            Self::Arguments => "arguments",
            Self::Name => "name",
            Self::Size => "size",
            Self::Digest => "digest",
            Self::Offset => "offset",
            Self::Length => "length",
            Self::Short => "short",
            Self::Target => "target",
            Self::Temp => "temp",
            Self::SizeMismatch => "size-mismatch",
            Self::DigestMismatch => "sha256-mismatch",
            Self::Hash => "hash",
            Self::CardFull(_) => "card-full",
            Self::Card(..) => "card",
            Self::Recovery(..) => "recovery-needed",
            Self::Abandoned(reason) => reason,
            Self::Reply => "reply",
        }
    }
}

/// Every byte, through short writes; a write that stores nothing fails.
fn write_all(card: &mut dyn Card, fd: i32, mut bytes: &[u8]) -> Result<(), i32> {
    while !bytes.is_empty() {
        match card.write(fd, bytes)? {
            0 => return Err(0),
            put => bytes = bytes.get(put..).ok_or(0)?,
        }
    }
    Ok(())
}

fn path(bytes: &[u8; PATH_BYTES]) -> &CStr {
    // FILE validated the complete path and left its trailing NUL.
    CStr::from_bytes_until_nul(bytes).unwrap_or_default()
}

// Three u32 IDs, a u64 offset and length, LF, then the whole raw block.
const _: () = assert!(2 + 3 * 11 + 1 + 5 + 21 + 21 + CHUNK_BYTES <= FRAME_BYTES);
const _: () = assert!(LINE_BYTES + 1 <= FRAME_BYTES);
