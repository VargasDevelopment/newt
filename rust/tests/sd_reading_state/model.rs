// SPDX-License-Identifier: MIT

//! In-memory FAT model for the SD reading-state fixtures and protocol driver.
//! Semantics are specified once in PROTOCOL.md ("Model"); oracle/model.hpp is
//! the C++ twin and must stay call-for-call identical. alloc only, no std.
use crate::{Directory, Error, Kind, Native, Open, Result, Stat};
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Op {
    Open,
    Close,
    Read,
    Write,
    Seek,
    Truncate,
    Sync,
    Fstat,
    Stat,
    Mkdir,
    Unlink,
    Rename,
    Opendir,
    Readdir,
    Closedir,
}

impl Op {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "open" => Self::Open,
            "close" => Self::Close,
            "read" => Self::Read,
            "write" => Self::Write,
            "seek" => Self::Seek,
            "truncate" => Self::Truncate,
            "sync" => Self::Sync,
            "fstat" => Self::Fstat,
            "stat" => Self::Stat,
            "mkdir" => Self::Mkdir,
            "unlink" => Self::Unlink,
            "rename" => Self::Rename,
            "opendir" => Self::Opendir,
            "readdir" => Self::Readdir,
            "closedir" => Self::Closedir,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Close => "close",
            Self::Read => "read",
            Self::Write => "write",
            Self::Seek => "seek",
            Self::Truncate => "truncate",
            Self::Sync => "sync",
            Self::Fstat => "fstat",
            Self::Stat => "stat",
            Self::Mkdir => "mkdir",
            Self::Unlink => "unlink",
            Self::Rename => "rename",
            Self::Opendir => "opendir",
            Self::Readdir => "readdir",
            Self::Closedir => "closedir",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Effect {
    /// The call fails with this errno and changes nothing.
    Errno(&'static str),
    /// A read or write transfers at most this many bytes.
    Short(usize),
    /// A write stores this many bytes, which reach the card, then fails EIO.
    Tear(usize),
    /// A rename takes effect, then reports this errno.
    After(&'static str),
}

/// Fires on the `first`..`first+count` eligible calls after arming. Eligible:
/// the same op and, for read/write, a requested length of at least `min`.
pub struct Fault {
    pub op: Op,
    pub first: u32,
    pub count: u32,
    pub effect: Effect,
    pub min: usize,
    seen: u32,
}

impl Fault {
    pub fn new(op: Op, first: u32, count: u32, effect: Effect, min: usize) -> Self {
        Self {
            op,
            first,
            count,
            effect,
            min,
            seen: 0,
        }
    }
}

pub const ERRNOS: [&str; 10] = [
    "EIO", "ENODEV", "ENXIO", "EINTR", "ENOENT", "EEXIST", "ENFILE", "ENOSPC", "EBADF", "EISDIR",
];

/// The native bridge's errno classification (native_io.c failure()).
pub fn classify(errno: &str) -> Error {
    match errno {
        "EIO" | "ENODEV" | "ENXIO" => Error::Unavailable,
        "EINTR" => Error::Interrupted,
        "ENOENT" => Error::Missing,
        "EEXIST" => Error::Exists,
        _ => Error::Other,
    }
}

#[derive(Clone, Default)]
pub struct File {
    pub durable: Vec<u8>,
    pub readable: Vec<u8>,
}

struct Handle {
    path: String,
    pos: usize,
    writable: bool,
    modified: bool,
}

#[derive(Default)]
pub struct Model {
    pub dirs: BTreeSet<String>,
    pub files: BTreeMap<String, File>,
    pub faults: Vec<Fault>,
    /// Bytes the file system leaves in new clusters (written every 512 bytes from 512).
    pub stale: Vec<u8>,
    pub trace: Vec<String>,
    /// When set, trace lines go here, in call order with the driver's own.
    pub sink: Option<fn(&str)>,
    /// Lose power after this completed call, before its acknowledgement.
    pub cut_after: Option<u32>,
    pub steps: u32,
    handles: BTreeMap<i32, Handle>,
    listings: BTreeMap<u32, (Vec<String>, usize)>,
    opens: i32,
    listed: u32,
    nonce: u32,
}

type Outcome<T> = core::result::Result<T, &'static str>;

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

impl Model {
    fn parent_exists(&self, path: &str) -> bool {
        let parent = parent(path);
        parent.is_empty() || self.dirs.contains(parent)
    }

    fn fire(&mut self, op: Op, len: usize) -> Option<Effect> {
        let mut fired = None;
        for fault in self
            .faults
            .iter_mut()
            .filter(|f| f.op == op && len >= f.min)
        {
            fault.seen += 1;
            if fired.is_none()
                && fault.seen >= fault.first
                && fault.seen - fault.first < fault.count
            {
                fired = Some(fault.effect);
            }
        }
        fired
    }

    fn step<T>(
        &mut self,
        op: Op,
        args: String,
        len: usize,
        body: impl FnOnce(&mut Self, Option<Effect>) -> Outcome<T>,
        show: impl FnOnce(&T) -> String,
    ) -> Result<T> {
        let effect = self.fire(op, len);
        let mut outcome = match effect {
            Some(Effect::Errno(errno)) => Err(errno),
            other => body(self, other),
        };
        self.steps += 1;
        if self.cut_after == Some(self.steps) {
            self.cut_after = None;
            self.power();
            outcome = Err("ENODEV");
        }
        let shown = match &outcome {
            Ok(value) => show(value),
            Err(errno) => errno.to_string(),
        };
        let line = format!("{} {} = {}", op.name(), args, shown);
        match self.sink {
            Some(sink) => sink(&line),
            None => self.trace.push(line),
        }
        outcome.map_err(classify)
    }

    /// A successful sync of a modified handle drains every buffered byte.
    fn drain(&mut self) {
        for file in self.files.values_mut() {
            file.durable.clone_from(&file.readable);
        }
        for handle in self.handles.values_mut() {
            handle.modified = false;
        }
    }

    /// Power loss: buffered bytes vanish, every handle and listing is gone.
    pub fn power(&mut self) {
        for file in self.files.values_mut() {
            file.readable.clone_from(&file.durable);
        }
        self.handles.clear();
        self.listings.clear();
    }

    fn handle(&mut self, fd: i32) -> Outcome<&mut Handle> {
        self.handles.get_mut(&fd).ok_or("EBADF")
    }

    fn position(&self, fd: i32) -> usize {
        self.handles.get(&fd).map_or(0, |h| h.pos)
    }

    fn store(bytes: &mut Vec<u8>, at: usize, data: &[u8]) {
        if bytes.len() < at + data.len() {
            bytes.resize(at + data.len(), 0);
        }
        bytes[at..at + data.len()].copy_from_slice(data);
    }
}

impl Native for Model {
    fn open(&mut self, path: &str, mode: Open) -> Result<i32> {
        let truncate = matches!(mode, Open::CreateTruncateWriteOnly);
        let (code, create, writable) = match mode {
            Open::SourceReadOnly => ("r", false, false),
            Open::ReadNonblock => ("rn", false, false),
            Open::ReadWriteNonblock => ("rwn", false, true),
            Open::ReadWrite => ("rw", false, true),
            Open::CreateExclusive => ("cx", true, true),
            Open::CreateExclusiveWriteOnly => ("cxw", true, true),
            Open::CreateTruncateWriteOnly => ("ctw", true, true),
        };
        self.step(
            Op::Open,
            format!("{path} {code}"),
            0,
            |m, _| {
                if m.dirs.contains(path) {
                    return Err(if create && !truncate { "EEXIST" } else { "EISDIR" });
                }
                if create {
                    if m.files.contains_key(path) && !truncate {
                        return Err("EEXIST");
                    }
                    if !m.parent_exists(path) {
                        return Err("ENOENT");
                    }
                    let file = m.files.entry(path.to_string()).or_default();
                    if truncate {
                        file.readable.clear();
                    }
                } else if !m.files.contains_key(path) {
                    return Err("ENOENT");
                }
                m.opens += 1;
                let handle = Handle {
                    path: path.to_string(),
                    pos: 0,
                    writable,
                    modified: truncate,
                };
                m.handles.insert(m.opens, handle);
                Ok(m.opens)
            },
            |fd| fd.to_string(),
        )
    }

    fn close(&mut self, fd: i32) -> Result<()> {
        let result = self.step(
            Op::Close,
            fd.to_string(),
            0,
            |m, _| {
                if m.handles.remove(&fd).ok_or("EBADF")?.modified {
                    m.drain();
                }
                Ok(())
            },
            |_| "0".to_string(),
        );
        // A failed close (FatFs's failed f_sync) still releases the descriptor.
        self.handles.remove(&fd);
        result
    }

    fn read(&mut self, fd: i32, out: &mut [u8]) -> Result<usize> {
        let pos = self.position(fd);
        let len = out.len();
        self.step(
            Op::Read,
            format!("{fd} {pos} {len}"),
            len,
            |m, effect| {
                let want = match effect {
                    Some(Effect::Short(n)) => n.min(len),
                    _ => len,
                };
                let handle = m.handle(fd)?;
                let path = handle.path.clone();
                let bytes = &m.files.get(&path).ok_or("EBADF")?.readable;
                let got = want.min(bytes.len().saturating_sub(pos));
                if got > 0 {
                    out[..got].copy_from_slice(&bytes[pos..pos + got]);
                }
                m.handle(fd)?.pos = pos + got;
                Ok(got)
            },
            |n| n.to_string(),
        )
    }

    fn write(&mut self, fd: i32, input: &[u8]) -> Result<usize> {
        let pos = self.position(fd);
        let len = input.len();
        self.step(
            Op::Write,
            format!("{fd} {pos} {len}"),
            len,
            |m, effect| {
                let handle = m.handle(fd)?;
                if !handle.writable {
                    return Err("EBADF");
                }
                handle.modified = true;
                let path = handle.path.clone();
                let (n, torn) = match effect {
                    Some(Effect::Short(n)) => (n.min(len), false),
                    Some(Effect::Tear(n)) => (n.min(len), true),
                    _ => (len, false),
                };
                let file = m.files.get_mut(&path).ok_or("EBADF")?;
                Self::store(&mut file.readable, pos, &input[..n]);
                if torn {
                    Self::store(&mut file.durable, pos, &input[..n]);
                }
                m.handle(fd)?.pos = pos + n;
                if torn { Err("EIO") } else { Ok(n) }
            },
            |n| n.to_string(),
        )
    }

    fn seek(&mut self, fd: i32, offset: u32) -> Result<u64> {
        self.step(
            Op::Seek,
            format!("{fd} {offset}"),
            0,
            |m, _| {
                m.handle(fd)?.pos = offset as usize;
                Ok(u64::from(offset))
            },
            |at| at.to_string(),
        )
    }

    fn truncate(&mut self, fd: i32, bytes: u32) -> Result<()> {
        self.step(
            Op::Truncate,
            format!("{fd} {bytes}"),
            0,
            |m, _| {
                let handle = m.handle(fd)?;
                if !handle.writable {
                    return Err("EBADF");
                }
                handle.modified = true;
                let path = handle.path.clone();
                let stale = m.stale.clone();
                let file = m.files.get_mut(&path).ok_or("EBADF")?;
                let size = bytes as usize;
                file.readable.resize(size, 0);
                let mut at = 512;
                while !stale.is_empty() && at + 512 <= size {
                    Self::store(&mut file.readable, at, &stale);
                    at += 512;
                }
                Ok(())
            },
            |_| "0".to_string(),
        )
    }

    fn sync(&mut self, fd: i32) -> Result<()> {
        let result = self.step(
            Op::Sync,
            fd.to_string(),
            0,
            |m, _| {
                if core::mem::take(&mut m.handle(fd)?.modified) {
                    m.drain();
                }
                Ok(())
            },
            |_| "0".to_string(),
        );
        // ff.c clears FA_MODIFIED even when sync_fs fails: nothing is drained.
        if let Some(handle) = self.handles.get_mut(&fd) {
            handle.modified = false;
        }
        result
    }

    fn fstat(&mut self, fd: i32) -> Result<Stat> {
        self.step(
            Op::Fstat,
            fd.to_string(),
            0,
            |m, _| {
                let path = m.handle(fd)?.path.clone();
                let size = m.files.get(&path).ok_or("EBADF")?.readable.len();
                Ok(stat(Kind::Regular, size))
            },
            |info| info.size.to_string(),
        )
    }

    fn stat(&mut self, path: &str, _no_follow: bool) -> Result<Stat> {
        self.step(
            Op::Stat,
            path.to_string(),
            0,
            |m, _| {
                if m.dirs.contains(path) {
                    return Ok(stat(Kind::Directory, 0));
                }
                let file = m.files.get(path).ok_or("ENOENT")?;
                Ok(stat(Kind::Regular, file.readable.len()))
            },
            |info| match info.kind {
                Kind::Directory => "d".to_string(),
                _ => format!("f{}", info.size),
            },
        )
    }

    fn mkdir(&mut self, path: &str) -> Result<()> {
        self.step(
            Op::Mkdir,
            path.to_string(),
            0,
            |m, _| {
                if m.dirs.contains(path) || m.files.contains_key(path) {
                    return Err("EEXIST");
                }
                if !m.parent_exists(path) {
                    return Err("ENOENT");
                }
                m.dirs.insert(path.to_string());
                Ok(())
            },
            |_| "0".to_string(),
        )
    }

    fn unlink(&mut self, path: &str) -> Result<()> {
        self.step(
            Op::Unlink,
            path.to_string(),
            0,
            |m, _| m.files.remove(path).map(|_| ()).ok_or("ENOENT"),
            |_| "0".to_string(),
        )
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        self.step(
            Op::Rename,
            format!("{from} {to}"),
            0,
            |m, effect| {
                if !m.files.contains_key(from) {
                    return Err("ENOENT");
                }
                // FAT refuses to rename onto an existing name.
                if m.files.contains_key(to) || m.dirs.contains(to) {
                    return Err("EEXIST");
                }
                if !m.parent_exists(to) {
                    return Err("ENOENT");
                }
                let file = m.files.remove(from).ok_or("ENOENT")?;
                m.files.insert(to.to_string(), file);
                for handle in m.handles.values_mut().filter(|h| h.path == from) {
                    handle.path = to.to_string();
                }
                match effect {
                    Some(Effect::After(errno)) => Err(errno),
                    _ => Ok(()),
                }
            },
            |_| "0".to_string(),
        )
    }

    fn next_nonce(&mut self) -> u32 {
        self.nonce = self.nonce.wrapping_add(1);
        self.nonce
    }
}

fn stat(kind: Kind, size: usize) -> Stat {
    Stat {
        kind,
        size: size as i64,
        inode: 0,
        device: 0,
        seconds: 0,
        nanos: 0,
    }
}

impl Directory for Model {
    type Listing = u32;

    fn open_dir(&mut self, path: &str) -> Result<u32> {
        self.step(
            Op::Opendir,
            path.to_string(),
            0,
            |m, _| {
                if !m.dirs.contains(path) {
                    return Err("ENOENT");
                }
                let children = m.dirs.iter().chain(m.files.keys());
                let mut names: Vec<String> = children
                    .filter(|p| parent(p) == path)
                    .map(|p| p[path.len() + 1..].to_string())
                    .collect();
                names.sort();
                m.listed += 1;
                m.listings.insert(m.listed, (names, 0));
                Ok(m.listed)
            },
            |id| id.to_string(),
        )
    }

    fn read_dir(&mut self, listing: &mut u32, name: &mut [u8]) -> Result<Option<usize>> {
        let id = *listing;
        self.step(
            Op::Readdir,
            id.to_string(),
            0,
            |m, _| {
                let (names, at) = m.listings.get_mut(&id).ok_or("EBADF")?;
                let Some(next) = names.get(*at) else {
                    return Ok(None);
                };
                *at += 1;
                let stored = next.len().min(name.len());
                name[..stored].copy_from_slice(&next.as_bytes()[..stored]);
                Ok(Some((next.clone(), next.len())))
            },
            |entry| {
                entry
                    .as_ref()
                    .map_or("end".to_string(), |(next, _)| next.clone())
            },
        )
        .map(|entry| entry.map(|(_, length)| length))
    }

    fn close_dir(&mut self, listing: u32) -> Result<()> {
        let result = self.step(
            Op::Closedir,
            listing.to_string(),
            0,
            |m, _| m.listings.remove(&listing).map(|_| ()).ok_or("EBADF"),
            |_| "0".to_string(),
        );
        self.listings.remove(&listing);
        result
    }
}
