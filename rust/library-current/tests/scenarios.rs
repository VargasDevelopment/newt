// Independent card bytes and failure scenarios. Source only: not run.
use newt_library_current::{
    list::{self, Files},
    scan::*,
    *,
};
use std::collections::BTreeMap;
use std::{format, vec, vec::Vec};
#[derive(Default)]
struct Card {
    paths: BTreeMap<Vec<u8>, Vec<u8>>,
    fds: BTreeMap<i32, Vec<u8>>,
    next: i32,
    fail_read: bool,
    fail_write: bool,
    fail_open: bool,
    fail_close: bool,
    removed: usize,
}
fn path(p: &[u8]) -> Vec<u8> {
    p[..p.iter().position(|&b| b == 0).unwrap()].to_vec()
}
impl Files for Card {
    fn open(&mut self, p: &[u8], write: bool) -> i32 {
        if self.fail_open {
            return -1;
        }
        let p = path(p);
        if write {
            self.paths.insert(p.clone(), Vec::new());
        }
        if !self.paths.contains_key(&p) {
            return -1;
        }
        self.next += 1;
        self.fds.insert(self.next, p);
        self.next
    }
    fn read(&mut self, fd: i32, at: u32, out: &mut [u8]) -> bool {
        let Some(p) = self.fds.get(&fd) else {
            return false;
        };
        let data = &self.paths[p];
        let at = at as usize;
        let n = out.len().min(data.len().saturating_sub(at));
        if n > 0 {
            out[..n].copy_from_slice(&data[at..at + n]);
        }
        !self.fail_read && n == out.len()
    }
    fn write(&mut self, fd: i32, at: u32, data: &[u8]) -> bool {
        let Some(p) = self.fds.get(&fd) else {
            return false;
        };
        let out = self.paths.get_mut(p).unwrap();
        let at = at as usize;
        let n = if self.fail_write {
            data.len() / 2
        } else {
            data.len()
        };
        out.resize(out.len().max(at + n), 0);
        out[at..at + n].copy_from_slice(&data[..n]);
        !self.fail_write
    }
    fn size(&mut self, fd: i32) -> i64 {
        self.fds.get(&fd).map_or(-1, |p| self.paths[p].len() as i64)
    }
    fn close(&mut self, fd: i32) -> bool {
        self.fds.remove(&fd);
        !self.fail_close
    }
    fn make_folder(&mut self, _: &[u8]) -> bool {
        true
    }
    fn remove(&mut self, p: &[u8]) -> bool {
        self.paths.remove(&path(p));
        self.removed += 1;
        true
    }
}
// Construct fixture independently from the production record/header helpers.
fn record(name: &[u8]) -> Vec<u8> {
    let mut r = vec![0u8; 100];
    r[..name.len()].copy_from_slice(name);
    let mut h = 0xcbf29ce484222325u64;
    for b in &r[..96] {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    r[96..].copy_from_slice(&((h ^ (h >> 32)) as u32).to_le_bytes());
    r
}
fn fixture(names: &[&[u8]]) -> Card {
    let mut c = Card::default();
    let mut bytes = vec![0; 32];
    for name in names {
        bytes.extend(record(name));
    }
    c.paths.insert(b"/list".to_vec(), bytes);
    c
}
#[test]
fn shelf_is_atomic_and_retries_open_but_removes_damage() {
    let mut c = fixture(&[b"a.epub", b"b.epub", b"c.epub", b"d.epub", b"e.epub"]);
    let mut catalog = Catalog::default();
    catalog.use_list(b"/list\0", 5, 0);
    c.fail_open = true;
    assert!(catalog.filename(&mut c, 0).is_none());
    assert_eq!(c.removed, 0);
    c.fail_open = false;
    assert_eq!(catalog.filename(&mut c, 3), Some(b"d.epub\0".as_slice()));
    c.paths.get_mut(b"/list".as_slice()).unwrap()[32 + 100 + 96] ^= 1;
    // Already-cached shelf is stable. Moving away and back validates again.
    assert!(catalog.filename(&mut c, 0).is_some());
    assert!(catalog.filename(&mut c, 4).is_some());
    assert!(!catalog.readable(&mut c, 0));
    assert_eq!(c.removed, 1);
    assert!(!c.paths.contains_key(b"/list".as_slice()));
}
#[test]
fn padding_short_read_and_close_policy() {
    let mut c = fixture(&[b"a.epub"]);
    let fd = c.open(b"/list\0", false);
    let mut n = [9; 96];
    c.fail_read = true;
    assert!(!list::read_record(&mut c, fd, 32, &mut n));
    assert_eq!(n, [9; 96]);
    c.fail_read = false;
    c.paths.get_mut(b"/list".as_slice()).unwrap().truncate(90);
    assert!(!list::read_record(&mut c, fd, 32, &mut n));
    let mut c = fixture(&[b"a.epub"]);
    c.fail_close = true;
    let mut catalog = Catalog::default();
    catalog.use_list(b"/list\0", 1, 0);
    assert!(catalog.readable(&mut c, 0));
    let fd = c.open(b"/list\0", true);
    c.fail_write = true;
    assert!(!list::write_record(&mut c, fd, 0, b"a.epub\0"));
    assert_eq!(c.paths[b"/list".as_slice()].len(), 50);
    let mut bad = record(b"a.epub");
    bad[30] = 1; // recompute checksum to isolate padding validation
    let mut h = list::hash(list::BASIS, &bad[..96]);
    h ^= h >> 32;
    bad[96..].copy_from_slice(&(h as u32).to_le_bytes());
    let mut c = fixture(&[]);
    c.paths.insert(b"/list".to_vec(), bad);
    let fd = c.open(b"/list\0", false);
    assert!(!list::read_record(&mut c, fd, 0, &mut n));
}
struct Root {
    count: usize,
    at: usize,
    failed: bool,
}
impl Folders for Root {
    fn open(&mut self, level: usize, _: &[u8]) -> bool {
        if level != 0 {
            return false;
        }
        self.at = 0;
        true
    }
    fn next(&mut self, _: usize, _: &[u8]) -> EntryRead {
        if self.failed {
            return EntryRead::Short(Read::Failed);
        }
        if self.at == self.count {
            return EntryRead::Short(Read::End);
        }
        let mut name = [0; 96];
        let s = format!("{:05}.epub", self.count - self.at);
        name[..s.len()].copy_from_slice(s.as_bytes());
        self.at += 1;
        EntryRead::Short(Read::Entry {
            name,
            bytes: 0,
            folder: false,
            hidden: false,
        })
    }
    fn close(&mut self, _: usize) -> bool {
        true
    }
}
#[test]
fn more_than_128_kept_rebuilt_and_failed_to_ram() {
    let mut f = Card::default();
    let mut folders = Root {
        count: 259,
        at: 0,
        failed: false,
    };
    let mut c = Catalog::default();
    let mut r = Report::default();
    assert_eq!(
        scan(
            b"/card\0",
            &mut c,
            &mut r,
            &mut folders,
            &mut f,
            None,
            &mut NoProgress,
            false
        ),
        LibraryStatus::Ok
    );
    assert_eq!(r.list, ListState::Built);
    assert_eq!(r.walks, 2);
    assert_eq!(r.runs, 3);
    assert!(c.on_card());
    assert_eq!(c.size(), 259);
    for i in 0..259 {
        let expected = format!(":{:05}.epub\0", i + 1);
        assert_eq!(c.filename(&mut f, i), Some(expected.as_bytes()));
    }
    scan(
        b"/card\0",
        &mut c,
        &mut r,
        &mut folders,
        &mut f,
        None,
        &mut NoProgress,
        false,
    );
    assert_eq!(r.list, ListState::Kept);
    assert_eq!(r.walks, 1);
    f.fail_write = true;
    folders.count = 260;
    scan(
        b"/card\0",
        &mut c,
        &mut r,
        &mut folders,
        &mut f,
        None,
        &mut NoProgress,
        false,
    );
    assert_eq!(r.list, ListState::Ram);
    assert_eq!(r.walks, 3);
    assert!(!c.on_card());
    assert_eq!(c.size(), 128);
    assert_eq!(c.omissions().full, 132);
    assert_eq!(c.filename(&mut f, 0), Some(b":00001.epub\0".as_slice()));
    assert!(f.fds.is_empty());
}
#[test]
fn entry_budget_and_root_failure() {
    let mut f = Card::default();
    let mut folders = Root {
        count: 32770,
        at: 0,
        failed: false,
    };
    let mut c = Catalog::default();
    let mut r = Report::default();
    scan(
        b"/card\0",
        &mut c,
        &mut r,
        &mut folders,
        &mut f,
        None,
        &mut NoProgress,
        true,
    );
    assert_eq!(r.entries, 32769);
    assert!(c.omissions().stopped);
    assert_eq!(c.size(), 128);
    folders.failed = true;
    assert_eq!(
        scan(
            b"/card\0",
            &mut c,
            &mut r,
            &mut folders,
            &mut f,
            None,
            &mut NoProgress,
            false
        ),
        LibraryStatus::Unavailable
    );
    assert_eq!(c.size(), 0);
}
#[test]
fn second_merge_pass() {
    let mut f = Card::default();
    let mut folders = Root {
        count: 16513,
        at: 0,
        failed: false,
    };
    let mut c = Catalog::default();
    let mut r = Report::default();
    scan(
        b"/card\0",
        &mut c,
        &mut r,
        &mut folders,
        &mut f,
        None,
        &mut NoProgress,
        false,
    );
    assert_eq!(r.list, ListState::Built);
    assert_eq!(r.runs, 130);
    assert_eq!(c.size(), 16513);
    for i in [0, 3, 4, 127, 128, 16383, 16512] {
        let expected = format!(":{:05}.epub\0", i + 1);
        assert_eq!(c.filename(&mut f, i), Some(expected.as_bytes()));
    }
}
#[test]
fn table_copy_preference_and_invalid_after_finish() {
    let mut f = Card::default();
    let mut c = Catalog::default();
    let mut same = |_: &[u8], _: &[u8]| true;
    assert_eq!(
        c.add(b":Other:a.epub\0", 12, Some(&mut same), true),
        Added::Listed
    );
    assert_eq!(c.add(b"z.epub\0", 12, Some(&mut same), true), Added::Copy);
    assert_eq!(c.copies(), 1);
    c.finish();
    assert_eq!(c.filename(&mut f, 0), Some(b"z.epub\0".as_slice()));
    assert_eq!(c.add(b"b.epub\0", 0, None, false), Added::Omitted);
    assert_eq!(c.status(), LibraryStatus::Unavailable);
    assert_eq!(c.size(), 0);
}

#[test]
fn header_is_only_a_checked_fingerprint_not_a_record_scan() {
    let mut f = fixture(&[b"a.epub"]);
    let mut header = [0u8; 32];
    header[..8].copy_from_slice(b"NEWTLST1");
    header[8..12].copy_from_slice(&1u32.to_le_bytes());
    header[12..16].copy_from_slice(&1u32.to_le_bytes());
    header[16..24].copy_from_slice(&17u64.to_le_bytes());
    // A nonzero reserved byte is accepted by actual alpha15 if the check holds.
    header[24] = 3;
    let mut h = 0xcbf29ce484222325u64;
    for &b in &header[..28] {
        h = (h ^ u64::from(b)).wrapping_mul(0x100000001b3);
    }
    header[28..].copy_from_slice(&((h ^ (h >> 32)) as u32).to_le_bytes());
    f.paths.get_mut(b"/list".as_slice()).unwrap()[..32].copy_from_slice(&header);
    assert!(list::current(&mut f, b"/list\0", 1, 1, 17));
    assert!(!list::current(&mut f, b"/missing\0", 1, 1, 17));
    assert!(!list::current(&mut f, b"/list\0", 1, 0, 17));
    f.paths.get_mut(b"/list".as_slice()).unwrap()[32] ^= 1;
    assert!(list::current(&mut f, b"/list\0", 1, 1, 17)); // corruption found on shelf load
    let mut catalog = Catalog::default();
    catalog.use_list(b"/list\0", 1, 1);
    assert!(!catalog.readable(&mut f, 0));
    f.paths.insert(b"/list".to_vec(), header.to_vec());
    assert!(!list::current(&mut f, b"/list\0", 1, 1, 17));
}
#[test]
fn firmware_progress_yields_and_consumes_loading_once() {
    struct Native {
        now: u32,
        yields: usize,
        loads: usize,
    }
    impl PaceNative for Native {
        fn milliseconds(&mut self) -> u32 {
            self.now
        }
        fn delay_one_millisecond(&mut self) {
            self.yields += 1;
        }
        fn loading(&mut self) {
            self.loads += 1;
        }
    }
    let mut native = Native {
        now: 0,
        yields: 0,
        loads: 0,
    };
    let mut progress = FirmwareProgress {
        native: &mut native,
        yielded: 0,
        loading_pending: true,
    };
    for elapsed in [49, 50, 99, 100, 500, 501, 550] {
        progress.progress(elapsed);
    }
    assert!(!progress.loading_pending);
    assert_eq!(native.yields, 4);
    assert_eq!(native.loads, 1);
}
