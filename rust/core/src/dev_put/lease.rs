// SPDX-License-Identifier: MIT
//! The lease itself: its line protocol, the temp file and the checked rename.
use super::{Card, End, Entry, Folder, Session, Wire, CAP_BYTES, CHUNK_BYTES, IDLE_MS, LINE_BYTES,
    MEMORY_BYTES, NAME_BYTES, NEWT, PATH_BYTES, RESERVE_BYTES, STALL_MS, TOTAL_MS, name_allowed};
use crate::update_online::Hash;
use crate::wire::crc32;
use core::ffi::CStr;
use core::fmt::{self, Write as _};

/// One lease. `memory` is caller-owned for this call only; every exit closes
/// the file and removes the temp unless DONE renamed it into place.
pub fn run(session: Session, memory: &mut [u8; MEMORY_BYTES], wire: &mut dyn Wire,
    card: &mut dyn Card, hash: &mut dyn Hash) -> End {
    let (line, reply) = memory.split_at_mut(LINE_BYTES);
    let started = wire.now();
    let mut lease = Lease { session, line, reply, wire, card, hash, started, open: None };
    let end = lease.serve();
    lease.discard();
    end
}

struct Open {
    folder: Folder,
    target: [u8; PATH_BYTES],
    fd: Option<i32>,
    size: u32,
    written: u32,
    sha256: [u8; 32],
}

struct Lease<'a> {
    session: Session,
    line: &'a mut [u8],
    reply: &'a mut [u8],
    wire: &'a mut dyn Wire,
    card: &'a mut dyn Card,
    hash: &'a mut dyn Hash,
    started: u32,
    open: Option<Open>,
}

/// A refusal after a current request: answered, then the lease ends.
#[derive(Debug)]
enum Refused { Order, Arguments, Folder, Name, Size, Digest, Offset, Length, Encoding, Short,
    CardFull(u64), Target, Temp, SizeMismatch, DigestMismatch, Hash, Card(&'static str, i32),
    /// The reply itself stalled or the host left: nothing more can be said.
    Reply }

enum Step { Next, Finished }

impl Lease<'_> {
    fn serve(&mut self) -> End {
        let id = self.session.request;
        if !self.send(id, format_args!("PUT line={LINE_BYTES} chunk={CHUNK_BYTES} cap={CAP_BYTES}")) {
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
            let step = match request.verb {
                Verb::File => self.file(request.arguments),
                Verb::Data => self.data(request.arguments),
                Verb::Done => self.done(),
            };
            match step {
                Ok(Step::Next) => {}
                Ok(Step::Finished) => return End::Kept { last_id: id },
                Err(Refused::Reply) => return End::Cancelled,
                Err(refused) => {
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
        let now = self.wire.now();
        if self.wire.physical_input() { return Some("physical-input"); }
        if !self.wire.connected() { return Some("disconnected"); }
        if now.wrapping_sub(self.started) >= TOTAL_MS { return Some("total-time"); }
        if now.wrapping_sub(activity) >= IDLE_MS { return Some("timeout"); }
        None
    }

    fn parse(&mut self, length: usize) -> Result<Request, End> {
        let line = &self.line[..length];
        if line == b"N1 HELLO" { return Err(End::Hello); }
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
        let (Some(folder), Some(name), Some(size), Some(digest), None) =
            (tokens.next(), tokens.next(), tokens.next(), tokens.next(), tokens.next()) else {
            return Err(Refused::Arguments);
        };
        let folder = Folder::parse(folder).ok_or(Refused::Folder)?;
        if !name_allowed(name.as_bytes()) { return Err(Refused::Name); }
        let size = number(size).filter(|&size| size <= CAP_BYTES).ok_or(Refused::Size)?;
        let sha256 = digest_hex(digest).ok_or(Refused::Digest)?;
        let mut target = [0; PATH_BYTES];
        let directory = folder.path().to_bytes();
        target[..directory.len()].copy_from_slice(directory);
        target[directory.len()] = b'/';
        target[directory.len() + 1..][..name.len()].copy_from_slice(name.as_bytes());

        self.folder(NEWT)?;
        self.folder(folder.path())?;
        match self.card.entry(path(&target)).map_err(|code| Refused::Card("target", code))? {
            Entry::Missing | Entry::File => {}
            Entry::Folder | Entry::Other => return Err(Refused::Target),
        }
        self.remove_hidden(folder.temp())?;
        let free = self.card.free_bytes().map_err(|code| Refused::Card("free", code))?;
        if u64::from(size) + RESERVE_BYTES > free { return Err(Refused::CardFull(free)); }
        // Recorded before create, so a failed create's partial temp is removed.
        self.open = Some(Open { folder, target, fd: None, size, written: 0, sha256 });
        let fd = self.card.create(folder.temp()).map_err(|code| Refused::Card("create", code))?;
        if let Some(open) = &mut self.open { open.fd = Some(fd); }
        self.answer(format_args!("FILE free={free}"))
    }

    fn data(&mut self, arguments: (usize, usize)) -> Result<Step, Refused> {
        let Some(open) = &self.open else { return Err(Refused::Order) };
        let (written, size) = (open.written, open.size);
        let space = self.line[arguments.0..arguments.1].iter().position(|&b| b == b' ').ok_or(Refused::Arguments)?;
        let offset = core::str::from_utf8(&self.line[arguments.0..arguments.0 + space]).ok()
            .and_then(number).ok_or(Refused::Offset)?;
        if offset != written { return Err(Refused::Offset); }
        let start = arguments.0 + space + 1;
        let decoded = decode_base64_in_place(&mut self.line[start..arguments.1]).ok_or(Refused::Encoding)?;
        if decoded != (size - written).min(CHUNK_BYTES as u32) as usize { return Err(Refused::Length); }
        let fd = open.fd.ok_or(Refused::Order)?;
        let mut bytes = &self.line[start..start + decoded];
        while !bytes.is_empty() {
            match self.card.write(fd, bytes) {
                Ok(0) => return Err(Refused::Card("write", 0)),
                Ok(put) => bytes = bytes.get(put..).ok_or(Refused::Card("write", 0))?,
                Err(code) => return Err(Refused::Card("write", code)),
            }
        }
        let written = written + decoded as u32;
        if let Some(open) = &mut self.open { open.written = written; }
        self.answer(format_args!("DATA {written}"))
    }

    fn done(&mut self) -> Result<Step, Refused> {
        let Some(open) = &mut self.open else { return Err(Refused::Order) };
        if open.written != open.size { return Err(Refused::Short); }
        let (folder, size, sha256, target) = (open.folder, open.size, open.sha256, open.target);
        let fd = open.fd.take().ok_or(Refused::Order)?;
        let synced = self.card.sync(fd);
        let closed = self.card.close(fd);
        synced.map_err(|code| Refused::Card("sync", code))?;
        closed.map_err(|code| Refused::Card("close", code))?;
        // The card's bytes, not the received ones, are what gets checked.
        let digest = self.read_back(folder.temp(), size)?;
        if digest != sha256 { return Err(Refused::DigestMismatch); }
        let target = path(&target);
        // FAT's rename never replaces, so the old file steps aside first.
        let replacing = self.card.entry(target).map_err(|code| Refused::Card("target", code))? == Entry::File;
        if replacing {
            self.remove_hidden(folder.old())?;
            self.card.rename(target, folder.old()).map_err(|code| Refused::Card("replace", code))?;
        }
        if let Err(code) = self.card.rename(folder.temp(), target) {
            if replacing { let _ = self.card.rename(folder.old(), target); }
            return Err(Refused::Card("rename", code));
        }
        self.open = None;
        // A failure here leaves only a hidden file, which the next replace removes.
        if replacing { let _ = self.card.remove(folder.old()); }
        let mut hex = [0; 64];
        for (pair, byte) in hex.chunks_exact_mut(2).zip(digest) {
            pair[0] = HEX[usize::from(byte >> 4)];
            pair[1] = HEX[usize::from(byte & 15)];
        }
        let hex = core::str::from_utf8(&hex).unwrap_or_default();
        self.answer(format_args!("DONE size={size} sha256={hex}"))?;
        Ok(Step::Finished)
    }

    fn read_back(&mut self, temp: &CStr, size: u32) -> Result<[u8; 32], Refused> {
        let fd = self.card.open_read(temp).map_err(|code| Refused::Card("open", code))?;
        let result = self.hash_file(fd, size);
        let closed = self.card.close(fd);
        let digest = result?;
        closed.map_err(|code| Refused::Card("close", code))?;
        Ok(digest)
    }

    fn hash_file(&mut self, fd: i32, size: u32) -> Result<[u8; 32], Refused> {
        if !self.hash.start() { return Err(Refused::Hash); }
        let mut total: u64 = 0;
        loop {
            let got = self.card.read(fd, self.line).map_err(|code| Refused::Card("read", code))?;
            let Some(bytes) = self.line.get(..got) else { return Err(Refused::Card("read", 0)) };
            if bytes.is_empty() { break; }
            total += got as u64;
            if total > u64::from(size) { return Err(Refused::SizeMismatch); }
            if !self.hash.update(bytes) { return Err(Refused::Hash); }
        }
        if total != u64::from(size) { return Err(Refused::SizeMismatch); }
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

    /// A temp or set-aside file a power loss left behind.
    fn remove_hidden(&mut self, path: &CStr) -> Result<(), Refused> {
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
        if self.card.entry(open.folder.temp()) == Ok(Entry::File) {
            let _ = self.card.remove(open.folder.temp());
        }
    }

    fn answer(&mut self, body: fmt::Arguments<'_>) -> Result<Step, Refused> {
        if self.send(self.session.request, body) { Ok(Step::Next) } else { Err(Refused::Reply) }
    }

    fn refuse(&mut self, id: u32, refused: &Refused) {
        let _ = match refused {
            Refused::CardFull(free) => self.send(id, format_args!("ERROR card-full free={free}")),
            Refused::Card(step, code) => self.send(id, format_args!("ERROR card step={step} code={code}")),
            other => self.send(id, format_args!("ERROR {}", other.reason())),
        };
    }

    /// The session is ending anyway; one attempt, answered with the last ID.
    fn best_effort(&mut self, reason: &str) {
        let _ = self.send(self.session.request, format_args!("ERROR abandoned reason={reason}"));
    }

    /// One whole framed reply, or false when the channel stalls or drops.
    fn send(&mut self, id: u32, body: fmt::Arguments<'_>) -> bool {
        let Session { boot, session, .. } = self.session;
        let mut out = Cursor { bytes: self.reply, used: 0 };
        if write!(out, "!N1 {boot} {session} {id} {body}").is_err() { return false; }
        let checksum = crc32(&out.bytes[..out.used]);
        if write!(out, " *{checksum:08x}\n").is_err() { return false; }
        let mut sent = 0;
        let mut progress = self.wire.now();
        while sent < out.used {
            if !self.wire.connected() { return false; }
            let put = self.wire.write(&out.bytes[sent..out.used]);
            if put > out.used - sent { return false; }
            if put != 0 {
                sent += put;
                progress = self.wire.now();
            } else if self.wire.now().wrapping_sub(progress) >= STALL_MS {
                return false;
            } else {
                self.wire.pause();
            }
        }
        true
    }
}

impl Refused {
    fn reason(&self) -> &'static str {
        match self {
            Self::Order => "order",
            Self::Arguments => "arguments",
            Self::Folder => "folder",
            Self::Name => "name",
            Self::Size => "size",
            Self::Digest => "digest",
            Self::Offset => "offset",
            Self::Length => "length",
            Self::Encoding => "encoding",
            Self::Short => "short",
            Self::Target => "target",
            Self::Temp => "temp",
            Self::SizeMismatch => "size-mismatch",
            Self::DigestMismatch => "sha256-mismatch",
            Self::Hash => "hash",
            Self::CardFull(_) => "card-full",
            Self::Card(..) => "card",
            Self::Reply => "reply",
        }
    }
}

#[derive(Clone, Copy)]
enum Verb { File, Data, Done }

/// A current lease request; `arguments` is the byte range after its verb.
struct Request { boot: u32, session: u32, id: u32, verb: Verb, arguments: (usize, usize) }
impl Request {
    /// `N1 BOOT SESSION ID VERB[ ARGUMENTS]`, with canonical numbers and
    /// single spaces, as the bridge's own commands.
    fn parse(line: &[u8]) -> Option<Self> {
        let text = core::str::from_utf8(line).ok()?;
        let mut fields = text.splitn(6, ' ');
        if fields.next()? != "N1" { return None; }
        let boot = number(fields.next()?)?;
        let session = number(fields.next()?)?;
        let id = number(fields.next()?)?;
        let verb = fields.next()?;
        let rest = fields.next();
        let verb = match (verb, rest.is_some()) {
            ("FILE", true) => Verb::File,
            ("DATA", true) => Verb::Data,
            ("DONE", false) => Verb::Done,
            _ => return None,
        };
        let start = rest.map_or(line.len(), |rest| line.len() - rest.len());
        Some(Self { boot, session, id, verb, arguments: (start, line.len()) })
    }
}

fn number(text: &str) -> Option<u32> {
    if text.is_empty() || (text.starts_with('0') && text.len() != 1) { return None; }
    text.bytes().try_fold(0u32, |value, byte| {
        if !byte.is_ascii_digit() { return None; }
        value.checked_mul(10)?.checked_add(u32::from(byte - b'0'))
    })
}

const HEX: &[u8; 16] = b"0123456789abcdef";

/// Exactly 64 lowercase hexadecimal characters.
fn digest_hex(text: &str) -> Option<[u8; 32]> {
    let text = text.as_bytes();
    if text.len() != 64 { return None; }
    let nibble = |b: u8| HEX.iter().position(|&h| h == b).map(|v| v as u8);
    let mut digest = [0; 32];
    for (byte, pair) in digest.iter_mut().zip(text.chunks_exact(2)) {
        *byte = (nibble(pair[0])? << 4) | nibble(pair[1])?;
    }
    Some(digest)
}

/// Strict, canonical standard base64, decoded over its own first bytes (the
/// output never overtakes the input).
fn decode_base64_in_place(text: &mut [u8]) -> Option<usize> {
    if text.is_empty() || text.len() % 4 != 0 { return None; }
    let value = |b: u8| -> Option<u32> {
        Some(u32::from(match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        }))
    };
    let groups = text.len() / 4;
    let mut out = 0;
    for group in 0..groups {
        let at = group * 4;
        let quad = [text[at], text[at + 1], text[at + 2], text[at + 3]];
        let last = group + 1 == groups;
        let padding = match quad {
            [_, _, b'=', b'='] if last => 2,
            [_, _, _, b'='] if last => 1,
            _ => 0,
        };
        let mut bits = 0;
        for &b in &quad[..4 - padding] { bits = (bits << 6) | value(b)?; }
        bits <<= 6 * padding as u32;
        let bytes = [(bits >> 16) as u8, (bits >> 8) as u8, bits as u8];
        // Canonical: bits under the padding must be zero.
        if bytes[3 - padding..].iter().any(|&b| b != 0) { return None; }
        text[out..out + 3 - padding].copy_from_slice(&bytes[..3 - padding]);
        out += 3 - padding;
    }
    Some(out)
}

fn path(bytes: &[u8; PATH_BYTES]) -> &CStr {
    // Every target has a NUL: folder and name together stay under PATH_BYTES.
    CStr::from_bytes_until_nul(bytes).unwrap_or_default()
}

struct Cursor<'a> { bytes: &'a mut [u8], used: usize }
impl fmt::Write for Cursor<'_> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let end = self.used.checked_add(value.len()).filter(|&end| end <= self.bytes.len()).ok_or(fmt::Error)?;
        self.bytes[self.used..end].copy_from_slice(value.as_bytes());
        self.used = end;
        Ok(())
    }
}

const _: () = assert!(c"/newt-sd/Newt/Update/".to_bytes().len() + NAME_BYTES < PATH_BYTES);
const _: () = assert!(CHUNK_BYTES % 3 == 0 && CHUNK_BYTES % 512 == 0);
const _: () = assert!(36 + 5 + 10 + 1 + CHUNK_BYTES / 3 * 4 <= LINE_BYTES);
