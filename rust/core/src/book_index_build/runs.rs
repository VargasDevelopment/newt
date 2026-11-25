//! Typed sorted runs, merged through windows lent from the one build buffer.
use super::{Error, Phase, Progress, WorkFile, records::{Record, wire_bytes}};
use crate::{page_envelope::crc, zip_container::Status};
use core::marker::PhantomData;
const HEADER: usize = 24;
const NONE: u32 = u32::MAX;
pub const CURSORS: usize = 64;

pub(super) fn io() -> Error { Error::Book(Status::IoError) }
pub(super) fn poll(progress: &mut dyn FnMut(Progress) -> bool, phase: Phase, done: u64, total: u64) -> Result<(), Error> {
    if progress(Progress { phase, done, total }) { Ok(()) } else { Err(Error::Book(Status::Stale)) }
}
#[derive(Clone, Copy, Default, Debug)]
pub struct Statistics { pub runs: u32, pub merge_passes: u32, pub written: u64, pub read: u64, pub read_calls: u64 }
#[derive(Default)]
pub(super) struct Io { pub end: u32, pub stats: Statistics }
impl Io {
    fn append<W: WorkFile + ?Sized>(&mut self, work: &mut W, bytes: &[u8]) -> Result<(), Error> {
        let next = self.end.checked_add(u32::try_from(bytes.len()).map_err(|_| io())?).ok_or_else(io)?;
        if !work.append(bytes) { return Err(io()); }
        self.end = next;
        self.stats.written += bytes.len() as u64;
        Ok(())
    }
    fn read<W: WorkFile + ?Sized>(&mut self, work: &mut W, at: u32, bytes: &mut [u8]) -> Result<(), Error> {
        if !work.read_at(at, bytes) { return Err(io()); }
        self.stats.read += bytes.len() as u64;
        self.stats.read_calls += 1;
        Ok(())
    }
    fn header<W: WorkFile + ?Sized>(&mut self, work: &mut W, at: u32) -> Result<(Run, u32, u32), Error> {
        let mut b = [0; HEADER];
        self.read(work, at, &mut b)?;
        let word = |i| crate::book_index::get32(&b, i);
        if crc(u32::MAX, &b[..20]) != word(20) { return Err(io()); }
        let (previous, start, count, checked, kind) = (word(0), word(4), word(8), word(12), word(16));
        let size = wire_bytes(kind).ok_or_else(io)? as u32;
        let end = start.checked_add(count.checked_mul(size).ok_or_else(io)?).ok_or_else(io)?;
        if count == 0 || !(start == at + HEADER as u32 || end <= at)
            || (previous != NONE && previous >= at) || end > self.end { return Err(io()); }
        Ok((Run { start, count, checked }, previous, kind))
    }
    fn link<W: WorkFile + ?Sized>(&mut self, work: &mut W, list: &mut List, run: Run, kind: u32) -> Result<(), Error> {
        let mut b = [0; HEADER];
        for (i, word) in [list.last.unwrap_or(NONE), run.start, run.count, run.checked, kind].iter().enumerate() {
            b[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        let checked = crc(u32::MAX, &b[..20]);
        b[20..24].copy_from_slice(&checked.to_le_bytes());
        let at = self.end;
        self.append(work, &b)?;
        list.last = Some(at);
        list.runs += 1;
        list.records += run.count;
        self.stats.runs += 1;
        Ok(())
    }
    pub fn known<T: Record, W: WorkFile + ?Sized>(&mut self, work: &mut W, list: &mut List, bytes: &[u8]) -> Result<(), Error> {
        if bytes.is_empty() { return Ok(()); }
        let run = Run { start: self.end + HEADER as u32, count: (bytes.len() / T::BYTES) as u32, checked: crc(u32::MAX, bytes) };
        self.link(work, list, run, T::KIND)?;
        self.append(work, bytes)
    }
}
#[derive(Clone, Copy, Default)]
pub(super) struct Region { pub start: usize, pub bytes: usize }
#[derive(Clone, Copy, Default)]
pub(super) struct List { pub last: Option<u32>, pub runs: u32, pub records: u32 }
#[derive(Clone, Copy, Default)]
pub(super) struct Stream { pub memory: Option<Region>, pub list: List, pub forward: Option<(u32, u32)> }
impl Stream {
    pub fn slots(self) -> usize {
        if self.memory.is_some() || self.forward.is_some() { 1 } else { self.list.runs as usize }
    }
}
#[derive(Clone, Copy, Default)]
struct Run { start: u32, count: u32, checked: u32 }
/// One merge input. Its 64-slot pool lives in JoinState, lent from the parse bank.
#[derive(Clone, Copy, Default)]
pub struct Cursor {
    start: u32, remaining: u32, expected: u32, checked: u32,
    used: u16, available: u16, head: [u8; 24], ready: bool,
}
impl Cursor {
    fn new<T: Record>(run: Run) -> Self {
        Self { start: run.start, remaining: run.count * T::BYTES as u32,
            expected: run.checked, checked: u32::MAX, ..Self::default() }
    }
    fn peek<T: Record, W: WorkFile + ?Sized>(&mut self, work: &mut W, io_state: &mut Io, block: &mut [u8]) -> Result<Option<T>, Error> {
        if self.ready { return Ok(Some(T::decode(&self.head[..T::BYTES]))); }
        if self.remaining == 0 && self.used == self.available {
            if self.checked != self.expected { return Err(io()); }
            return Ok(None);
        }
        let mut copied = 0;
        while copied < T::BYTES {
            if self.used == self.available {
                let take = block.len().min(self.remaining as usize);
                if take == 0 { return Err(io()); }
                io_state.read(work, self.start, &mut block[..take])?;
                self.checked = crc(self.checked, &block[..take]);
                self.start += take as u32;
                self.remaining -= take as u32;
                self.used = 0;
                self.available = take as u16;
            }
            let take = (T::BYTES - copied).min(usize::from(self.available - self.used));
            self.head[copied..copied + take].copy_from_slice(&block[usize::from(self.used)..][..take]);
            copied += take;
            self.used += take as u16;
        }
        self.ready = true;
        Ok(Some(T::decode(&self.head[..T::BYTES])))
    }
}
/// Publication lends the first 384 bytes to Writer. Its resident records are
/// copied once; all other phases use an empty prefix and the whole buffer.
pub(super) struct View<'a> { pub prefix: &'a [u8], pub tail: &'a mut [u8] }
impl View<'_> {
    pub fn read<T: Record>(&self, at: usize) -> T {
        let mut b = [0; 24];
        for (i, byte) in b[..T::MEMORY].iter_mut().enumerate() {
            *byte = if at + i < self.prefix.len() { self.prefix[at + i] } else { self.tail[at + i - self.prefix.len()] };
        }
        T::memory_decode(&b[..T::MEMORY])
    }
    pub fn slice(&mut self, start: usize, bytes: usize) -> &mut [u8] {
        &mut self.tail[start - self.prefix.len()..][..bytes]
    }
}
#[derive(Default)]
pub(super) struct Group {
    first: usize, count: usize, memory: Option<Region>, forward: Option<(u32, u32)>, forward_live: bool,
}
pub(super) struct Pool<'s> { cursor: &'s mut [Cursor; CURSORS], used: usize, pub base: usize, pub min_read: usize }
impl<'s> Pool<'s> {
    pub fn new(cursor: &'s mut [Cursor; CURSORS], base: usize, min_read: usize) -> Self {
        Self { cursor, used: 0, base, min_read }
    }
    pub fn bytes(&self) -> usize { self.base + self.used * self.min_read }
    /// Frees every slot for the next merge; cursors are rewritten on attach.
    fn clear(&mut self) { self.used = 0; }
    pub fn attach<T: Record, W: WorkFile + ?Sized>(&mut self, work: &mut W, io_state: &mut Io, stream: Stream) -> Result<Group, Error> {
        if let Some(region) = stream.memory { return Ok(Group { memory: Some(region), ..Group::default() }); }
        let count = stream.slots();
        if self.used + count > CURSORS { return Err(Error::Book(Status::MetadataTooLarge)); }
        let first = self.used;
        self.used += count;
        let mut list = stream.list;
        for i in first..first + count {
            if stream.forward.is_some() { break; }
            let at = list.last.ok_or_else(io)?;
            let (run, previous, kind) = io_state.header(work, at)?;
            if kind != T::KIND { return Err(io()); }
            self.cursor[i] = Cursor::new::<T>(run);
            list.last = (previous != NONE).then_some(previous);
        }
        Ok(Group { first, count, memory: None, forward: stream.forward, forward_live: false })
    }
    pub fn next<T: Record, W: WorkFile + ?Sized>(&mut self, group: &mut Group, view: &mut View<'_>, work: &mut W, io_state: &mut Io) -> Result<Option<T>, Error> {
        if let Some(region) = &mut group.memory {
            if region.bytes == 0 { return Ok(None); }
            let record = view.read::<T>(region.start);
            region.start += T::MEMORY;
            region.bytes -= T::MEMORY;
            return Ok(Some(record));
        }
        if let Some((mut at, end)) = group.forward {
            loop {
                if group.forward_live {
                    let cursor = &mut self.cursor[group.first];
                    let head = cursor.peek::<T, W>(work, io_state, view.slice(self.base + group.first * self.min_read, self.min_read))?;
                    if head.is_some() { cursor.ready = false; return Ok(head); }
                    group.forward_live = false;
                }
                if at == end { return Ok(None); }
                if at > end { return Err(io()); }
                let (run, _, kind) = io_state.header(work, at)?;
                if run.start != at + HEADER as u32 { return Err(io()); }
                at = run.start + run.count * wire_bytes(kind).ok_or_else(io)? as u32;
                group.forward = Some((at, end));
                if kind == T::KIND {
                    self.cursor[group.first] = Cursor::new::<T>(run);
                    group.forward_live = true;
                }
            }
        }
        let mut best: Option<(usize, T)> = None;
        for i in group.first..group.first + group.count {
            let record = self.cursor[i].peek::<T, W>(work, io_state, view.slice(self.base + i * self.min_read, self.min_read))?;
            if let Some(record) = record && best.is_none_or(|(_, old)| record.key() < old.key()) {
                best = Some((i, record));
            }
        }
        if let Some((i, record)) = best { self.cursor[i].ready = false; Ok(Some(record)) } else { Ok(None) }
    }
}
fn at<T: Record>(b: &[u8], i: usize, memory: bool) -> T {
    let size = if memory { T::MEMORY } else { T::BYTES };
    let b = &b[i * size..][..size];
    if memory { T::memory_decode(b) } else { T::decode(b) }
}
fn swap(b: &mut [u8], a: usize, c: usize, size: usize) { for i in 0..size { b.swap(a * size + i, c * size + i); } }
fn sift<T: Record>(b: &mut [u8], mut root: usize, end: usize, memory: bool) {
    let size = if memory { T::MEMORY } else { T::BYTES };
    while root * 2 + 1 < end {
        let mut child = root * 2 + 1;
        if child + 1 < end && at::<T>(b, child, memory).key() < at::<T>(b, child + 1, memory).key() { child += 1; }
        if at::<T>(b, root, memory).key() >= at::<T>(b, child, memory).key() { break; }
        swap(b, root, child, size); root = child;
    }
}
pub(super) fn sort<T: Record>(b: &mut [u8], memory: bool) {
    let size = if memory { T::MEMORY } else { T::BYTES };
    let n = b.len() / size;
    for root in (0..n / 2).rev() { sift::<T>(b, root, n, memory); }
    for end in (1..n).rev() { swap(b, 0, end, size); sift::<T>(b, 0, end, memory); }
}

pub(super) struct Output<T> { region: Region, used: usize, memory: bool, sorted: bool, list: List, marker: PhantomData<T> }
impl<T: Record> Output<T> {
    pub fn new(start: usize, bytes: usize, memory: bool, sorted: bool) -> Self {
        let size = if memory { T::MEMORY } else { T::BYTES };
        Self { region: Region { start, bytes: bytes / size * size }, used: 0, memory, sorted, list: List::default(), marker: PhantomData }
    }
    pub fn push<W: WorkFile + ?Sized>(&mut self, record: T, view: &mut View<'_>, work: &mut W, io_state: &mut Io) -> Result<(), Error> {
        let size = if self.memory { T::MEMORY } else { T::BYTES };
        if self.used + size > self.region.bytes {
            if self.memory { return Err(Error::Book(Status::MetadataTooLarge)); }
            self.flush(view, work, io_state)?;
        }
        let b = view.slice(self.region.start + self.used, size);
        if self.memory { record.memory_encode(b); } else { record.encode(b); }
        self.used += size;
        Ok(())
    }
    fn flush<W: WorkFile + ?Sized>(&mut self, view: &mut View<'_>, work: &mut W, io_state: &mut Io) -> Result<(), Error> {
        if self.used == 0 { return Ok(()); }
        let b = view.slice(self.region.start, self.used);
        if !self.sorted { sort::<T>(b, false); }
        io_state.known::<T, W>(work, &mut self.list, b)?;
        self.used = 0;
        Ok(())
    }
    pub fn finish<W: WorkFile + ?Sized>(mut self, view: &mut View<'_>, work: &mut W, io_state: &mut Io) -> Result<Stream, Error> {
        if self.memory {
            if !self.sorted { sort::<T>(view.slice(self.region.start, self.used), true); }
            Ok(Stream { memory: Some(Region { start: self.region.start, bytes: self.used }), ..Stream::default() })
        } else { self.flush(view, work, io_state)?; Ok(Stream { list: self.list, ..Stream::default() }) }
    }
}
/// Reduce only an overflowing list, and copy only enough runs to meet its share.
pub(super) fn reduce<T: Record, W: WorkFile + ?Sized>(stream: &mut Stream, wanted: usize, buffer: &mut [u8],
    pool: &mut Pool<'_>, work: &mut W, io_state: &mut Io, progress: &mut dyn FnMut(Progress) -> bool) -> Result<(), Error> {
    let min_read = pool.min_read;
    if stream.memory.is_some() || stream.forward.is_some() { return Ok(()); }
    let fan = ((buffer.len() - T::BYTES) / min_read).min(CURSORS);
    if wanted == 0 || fan < 2 { return Err(Error::Book(Status::MetadataTooLarge)); }
    while stream.list.runs as usize > wanted {
        io_state.stats.merge_passes += 1;
        let mut output = List::default();
        while stream.list.runs >= 2 && stream.list.runs as usize + output.runs as usize > wanted {
        let count = fan.min(stream.list.runs as usize).min(stream.list.runs as usize + output.runs as usize - wanted + 1);
        let mut batch = List { last: stream.list.last, runs: count as u32, records: 0 };
        let mut scan = batch.last;
        for _ in 0..count {
            let (run, previous, kind) = io_state.header(work, scan.ok_or_else(io)?)?;
            if kind != T::KIND { return Err(io()); }
            batch.records += run.count;
            scan = (previous != NONE).then_some(previous);
        }
        stream.list.last = scan;
        stream.list.runs -= count as u32;
        stream.list.records -= batch.records;
        pool.clear();
        let mut group = pool.attach::<T, W>(work, io_state, Stream { list: batch, ..Stream::default() })?;
        let start = io_state.end;
        let out_start = pool.bytes();
        let out_bytes = (buffer.len() - out_start) / T::BYTES * T::BYTES;
        let mut view = View { prefix: &[], tail: buffer };
        let mut used = 0;
        let mut checked = u32::MAX;
        while let Some(record) = pool.next::<T, W>(&mut group, &mut view, work, io_state)? {
            record.encode(view.slice(out_start + used, T::BYTES)); used += T::BYTES;
            if used == out_bytes {
                let b = view.slice(out_start, used); checked = crc(checked, b); io_state.append(work, b)?; used = 0;
                poll(progress, Phase::Merge, io_state.stats.read + io_state.stats.written, 0)?;
            }
        }
        if used != 0 { let b = view.slice(out_start, used); checked = crc(checked, b); io_state.append(work, b)?; }
        io_state.link(work, &mut output, Run { start, count: batch.records, checked }, T::KIND)?;
        poll(progress, Phase::Merge, io_state.stats.read + io_state.stats.written, 0)?;
        }
        // Link descriptors only; new outputs cannot be merged again in this
        // generation, so an unbalanced tail cannot repeatedly copy a large run.
        while let Some(at) = output.last {
            let (run, previous, kind) = io_state.header(work, at)?;
            if kind != T::KIND { return Err(io()); }
            io_state.link(work, &mut stream.list, run, kind)?;
            output.last = (previous != NONE).then_some(previous);
        }
    }
    Ok(())
}
