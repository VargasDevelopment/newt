// SPDX-License-Identifier: MIT
//! Focused unit test for page-turn-trace seal breakdown and census file count (#168).
#![cfg(feature = "page-turn-trace")]

extern crate alloc;
use core::cell::{Cell, RefCell};
use newt_core::{
    page_cache::CacheStorage,
    page_turn_trace::SegmentTrace,
    producer_visit::MissingProbe,
};
use newt_native_storage::{
    Authority, Error, Kind, Native, Open, ProducerPolicy, Result, RouteState, Stat,
    chapter_cache::{ChapterCacheState, ChapterCacheStorage},
    page_entries::{PageEntries, PageEntry},
    page_entry_census::{PAGES, Request, entry_census},
};
use std::collections::BTreeMap;

struct NeverCancel;
impl Authority for NeverCancel {
    fn cancelled(&self) -> bool {
        false
    }
}

struct TimedNative<'a> {
    clock: &'a Cell<u32>,
    files: BTreeMap<String, Vec<u8>>,
    open_fds: BTreeMap<i32, (String, usize)>,
    next_fd: i32,
    nonce: u32,
    fail_commit_rename: bool,
}
impl<'a> TimedNative<'a> {
    fn new(clock: &'a Cell<u32>) -> Self {
        Self {
            clock,
            files: BTreeMap::new(),
            open_fds: BTreeMap::new(),
            next_fd: 10,
            nonce: 0,
            fail_commit_rename: false,
        }
    }
    fn tick(&self, us: u32) {
        self.clock.set(self.clock.get().wrapping_add(us));
    }
}
impl Native for TimedNative<'_> {
    fn open(&mut self, path: &str, mode: Open) -> Result<i32> {
        match mode {
            Open::CreateExclusive => {
                if self.files.contains_key(path) {
                    return Err(Error::Exists);
                }
                self.files.insert(path.to_owned(), Vec::new());
            }
            Open::ReadNonblock | Open::SourceReadOnly => {
                if !self.files.contains_key(path) {
                    return Err(Error::Missing);
                }
            }
            _ => {
                self.files.entry(path.to_owned()).or_default();
            }
        }
        let fd = self.next_fd;
        self.next_fd += 1;
        self.open_fds.insert(fd, (path.to_owned(), 0));
        Ok(fd)
    }
    fn close(&mut self, fd: i32) -> Result<()> {
        self.open_fds.remove(&fd).ok_or(Error::Other)?;
        Ok(())
    }
    fn read(&mut self, fd: i32, out: &mut [u8]) -> Result<usize> {
        self.tick(11);
        let (path, pos) = self.open_fds.get_mut(&fd).ok_or(Error::Other)?;
        let data = self.files.get(path).ok_or(Error::Other)?;
        let n = out.len().min(data.len().saturating_sub(*pos));
        out[..n].copy_from_slice(&data[*pos..*pos + n]);
        *pos += n;
        Ok(n)
    }
    fn write(&mut self, fd: i32, input: &[u8]) -> Result<usize> {
        self.tick(17);
        let (path, pos) = self.open_fds.get_mut(&fd).ok_or(Error::Other)?;
        let data = self.files.get_mut(path).ok_or(Error::Other)?;
        if *pos + input.len() > data.len() {
            data.resize(*pos + input.len(), 0);
        }
        data[*pos..*pos + input.len()].copy_from_slice(input);
        *pos += input.len();
        Ok(input.len())
    }
    fn seek(&mut self, fd: i32, offset: u32) -> Result<u64> {
        let (_, pos) = self.open_fds.get_mut(&fd).ok_or(Error::Other)?;
        *pos = offset as usize;
        Ok(u64::from(offset))
    }
    fn truncate(&mut self, fd: i32, bytes: u32) -> Result<()> {
        let (path, _) = self.open_fds.get(&fd).ok_or(Error::Other)?;
        let data = self.files.get_mut(path).ok_or(Error::Other)?;
        data.resize(bytes as usize, 0);
        Ok(())
    }
    fn sync(&mut self, _fd: i32) -> Result<()> {
        self.tick(101);
        Ok(())
    }
    fn fstat(&mut self, fd: i32) -> Result<Stat> {
        let (path, _) = self.open_fds.get(&fd).ok_or(Error::Other)?;
        let data = self.files.get(path).ok_or(Error::Missing)?;
        Ok(Stat { kind: Kind::Regular, size: data.len() as i64, inode: 1, device: 1, seconds: 1, nanos: 0 })
    }
    fn stat(&mut self, path: &str, _no_follow: bool) -> Result<Stat> {
        if let Some(data) = self.files.get(path) {
            return Ok(Stat { kind: Kind::Regular, size: data.len() as i64, inode: 1, device: 1, seconds: 1, nanos: 0 });
        }
        if path == "/cache" || path == "/cache/Pages" || path.starts_with("/cache/") && !path.contains('.') {
            return Ok(Stat { kind: Kind::Directory, size: 0, inode: 2, device: 1, seconds: 1, nanos: 0 });
        }
        Err(Error::Missing)
    }
    fn mkdir(&mut self, _path: &str) -> Result<()> {
        Ok(())
    }
    fn unlink(&mut self, path: &str) -> Result<()> {
        self.files.remove(path).ok_or(Error::Missing)?;
        Ok(())
    }
    fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        self.tick(53);
        if self.fail_commit_rename {
            return Err(Error::Other);
        }
        let data = self.files.remove(from).ok_or(Error::Missing)?;
        self.files.insert(to.to_owned(), data);
        Ok(())
    }
    fn next_nonce(&mut self) -> u32 {
        self.nonce += 1;
        self.nonce
    }
}

impl PageEntries for TimedNative<'_> {
    fn walk_entries(
        &mut self,
        folder: &core::ffi::CStr,
        _sized: bool,
        visit: &mut dyn FnMut(&mut Self, PageEntry<'_>) -> bool,
    ) -> bool {
        let prefix = format!("{}/", folder.to_str().unwrap());
        let entries: Vec<(String, u64)> = self
            .files
            .iter()
            .filter_map(|(k, v)| k.strip_prefix(&prefix).map(|name| (name.to_owned(), v.len() as u64)))
            .collect();
        for (name, bytes) in entries {
            let entry = PageEntry {
                name: Some(name.as_bytes()),
                regular: true,
                bytes,
                time: 10,
            };
            if !visit(self, entry) {
                return false;
            }
        }
        visit(self, PageEntry { name: None, regular: false, bytes: 0, time: 0 })
    }
}

#[test]
fn producer_seal_breakdown_and_census_file_count_are_reported() {
    let clock = Cell::new(1000u32);
    let emitted: RefCell<Vec<SegmentTrace>> = RefCell::new(Vec::new());
    let mut native = TimedNative::new(&clock);
    let cancel = NeverCancel;
    let route_state = RouteState::new("/cache", "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef");
    let route = route_state.lend(&cancel, || 0);
    let mut cache_state = ChapterCacheState::new();
    let name = b"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef.fibp";
    {
        let mut storage = ChapterCacheStorage::lend(&mut cache_state, &mut native, &route).unwrap();
        storage.access().set_producer_allowance(64, 8);
        let read_clock = || clock.get();
        let on_seg = |trace: SegmentTrace| emitted.borrow_mut().push(trace);
        let policy = ProducerPolicy::new(&cancel).with_trace(&read_clock, &on_seg);
        let ok = storage.produce(&policy, |access| {
            assert!(access.begin_write(name));
            access.trace_segment_begin(2);
            clock.set(clock.get() + 500);
            let payload = vec![0x5au8; 5000];
            assert!(access.write(&payload));
            access.trace_add_tee_us(120);
            access.trace_segment_layout_done();
            let sealed = access.end_write();
            if sealed {
                access.trace_segment_end(128);
            }
            sealed
        });
        assert_eq!(ok, Some(true));
    }
    let traces = emitted.borrow();
    assert_eq!(traces.len(), 1);
    let t = traces[0];
    assert_eq!((t.idx, t.pages, t.bytes), (2, 128, 5000));
    assert_eq!(t.tee_us, 120);
    assert!(t.layout_us > 0, "layout_us should reflect wall minus tee");
    assert!(t.seal.write_us > 0, "payload write must be timed");
    assert!(t.seal.read_us > 0, "pass-1 readback must be timed");
    assert!(t.seal.dir_us > 0, "directory + manifest writes must be timed");
    assert!(t.seal.sync_us >= 101, "sync must be timed");
    assert!(t.seal.verify_us > 0, "pass-2 verification readback must be timed");
    assert!(t.seal.commit_us >= 53, "commit rename must be timed");
    assert!(
        t.seal.finalize_us >= t.seal.read_us + t.seal.crc_us + t.seal.dir_us + t.seal.sync_us + t.seal.verify_us,
        "finalize_us ({}) must cover all subphases",
        t.seal.finalize_us
    );

    // Census must report the 1 committed .fibp file in /cache/Pages.
    let grant = entry_census(
        &mut native,
        Request {
            policy: &PAGES,
            root: Some("/cache"),
            cancel: &cancel,
            clock: None,
            wait: None,
            pinned: None,
            need: None,
        },
    );
    assert_eq!(grant.outcome, MissingProbe::Ready);
    assert_eq!(grant.files, 1);

    // Negative control: a failed commit leaves no SegmentTrace emitted.
    drop(traces);
    emitted.borrow_mut().clear();
    native.fail_commit_rename = true;
    let name2 = b"fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210.fibp";
    {
        let mut storage = ChapterCacheStorage::lend(&mut cache_state, &mut native, &route).unwrap();
        storage.access().set_producer_allowance(64, 8);
        let read_clock = || clock.get();
        let on_seg = |trace: SegmentTrace| emitted.borrow_mut().push(trace);
        let policy = ProducerPolicy::new(&cancel).with_trace(&read_clock, &on_seg);
        let ok = storage.produce(&policy, |access| {
            assert!(access.begin_write(name2));
            access.trace_segment_begin(3);
            assert!(access.write(&[0x33u8; 256]));
            access.trace_segment_layout_done();
            let sealed = access.end_write();
            if sealed {
                access.trace_segment_end(12);
            }
            sealed
        });
        assert_eq!(ok, Some(false));
    }
    assert!(emitted.borrow().is_empty(), "failed commit must not emit a segment trace");
}
