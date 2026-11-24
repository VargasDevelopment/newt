// SPDX-License-Identifier: MIT
//! Frozen ResumePictureSlot on the existing native file operations.
use newt_core::{
    frame::{Frame, FRAME_BYTES},
    page_envelope::crc,
    resume_picture::{Binding, Footer, Load, Record, Save, FILE_BYTES, HEADER_BYTES},
};
use newt_native_storage::{Error, Kind, Native, Open, Result};

pub(super) const PATH: &str = "/newt-sd/Newt/Cache/resume.pic";
pub(super) const PENDING: &str = "/newt-sd/Newt/Cache/resume.pic.tmp";
const CHUNK_BYTES: usize = 4096;

/// The original temporary cleanup needs link count, absent from Native::Stat.
/// Implementations lstat exactly PENDING once and preserve its native error.
pub(super) trait PictureNative: Native {
    fn pending_metadata(&mut self) -> Result<(Kind, u64)>;
}

/// A short native loan, without a retained descriptor or pixel allocation.
pub(super) struct Slot<'a, N: PictureNative> {
    native: &'a mut N,
    rename_replaces: bool,
}

impl<'a, N: PictureNative> Slot<'a, N> {
    pub(super) fn new(native: &'a mut N, rename_replaces: bool) -> Self {
        Self { native, rename_replaces }
    }

    pub(super) fn header(&mut self, record: &mut Record) -> Load {
        let mut bytes = [0; HEADER_BYTES];
        let fd = match self.native.open(PATH, Open::SourceReadOnly) {
            Ok(fd) => fd,
            Err(Error::Missing) => return Load::Missing,
            Err(_) => return Load::Unreadable,
        };
        let result = self.read_header(fd, &mut bytes, record);
        if self.native.close(fd).is_err() && result == Load::Shown {
            Load::Unreadable
        } else {
            result
        }
    }

    /// Failure leaves unverified pixels in frame; only Shown publishes footer.
    pub(super) fn load(&mut self, expected: &Binding, frame: &mut Frame, footer: &mut Footer) -> Load {
        let mut bytes = [0; HEADER_BYTES];
        let mut record = Record::default();
        let fd = match self.native.open(PATH, Open::SourceReadOnly) {
            Ok(fd) => fd,
            Err(Error::Missing) => return Load::Missing,
            Err(_) => return Load::Unreadable,
        };
        let mut result = self.read_header(fd, &mut bytes, &mut record);
        if result == Load::Shown && record.binding != *expected {
            result = Load::Mismatch;
        }
        let mut checksum = u32::MAX;
        for at in (0..FRAME_BYTES).step_by(CHUNK_BYTES) {
            if result != Load::Shown { break; }
            let take = CHUNK_BYTES.min(FRAME_BYTES - at);
            let chunk = &mut frame[at..at + take];
            match self.read_at(fd, (HEADER_BYTES + at) as u32, chunk) {
                Ok(got) if got == take => checksum = crc(checksum, chunk),
                _ => result = Load::Unreadable,
            }
        }
        if result == Load::Shown && checksum ^ u32::MAX != record.frame_crc {
            result = Load::Corrupt;
        }
        if self.native.close(fd).is_err() && result == Load::Shown {
            result = Load::Unreadable;
        }
        if result == Load::Shown { *footer = record.footer; }
        result
    }

    pub(super) fn holds(&mut self, binding: &Binding, footer: &Footer) -> bool {
        let mut record = Record::default();
        self.header(&mut record) == Load::Shown && record.binding == *binding && record.footer == *footer
    }

    pub(super) fn write(&mut self, record: &mut Record, frame: &Frame) -> Save {
        record.frame_crc = Record::crc(frame);
        let mut bytes = [0; HEADER_BYTES];
        record.encode(&mut bytes);
        if !self.remove_temporary() { return Save::Failed; }
        let fd = match self.native.open(PENDING, Open::CreateExclusiveWriteOnly) {
            Ok(fd) => fd,
            Err(_) => return Save::Failed,
        };
        let mut valid = self.write_all(fd, &bytes) && self.write_all(fd, frame)
            && self.native.sync(fd).is_ok();
        if self.native.close(fd).is_err() { valid = false; }
        if !valid {
            self.remove_temporary();
            return Save::Failed;
        }
        if !self.rename_replaces {
            match self.native.unlink(PATH) {
                Ok(()) | Err(Error::Missing) => {},
                Err(_) => return Save::Failed,
            }
        }
        if self.native.rename(PENDING, PATH).is_ok() { Save::Written } else { Save::Failed }
    }

    fn read_header(&mut self, fd: i32, bytes: &mut [u8; HEADER_BYTES], record: &mut Record) -> Load {
        let info = match self.native.fstat(fd) {
            Ok(info) => info,
            Err(_) => return Load::Unreadable,
        };
        if info.kind != Kind::Regular || info.size != FILE_BYTES as i64 { return Load::Corrupt; }
        if !self.read_at(fd, 0, bytes).is_ok_and(|got| got == HEADER_BYTES) {
            return Load::Unreadable;
        }
        if record.decode(bytes) { Load::Shown } else { Load::Corrupt }
    }

    fn read_at(&mut self, fd: i32, at: u32, bytes: &mut [u8]) -> Result<usize> {
        match self.native.seek(fd, at) {
            Ok(actual) if actual == u64::from(at) => self.native.read(fd, bytes),
            _ => Err(Error::Other),
        }
    }

    fn write_all(&mut self, fd: i32, mut bytes: &[u8]) -> bool {
        while !bytes.is_empty() {
            let take = CHUNK_BYTES.min(bytes.len());
            match self.native.write(fd, &bytes[..take]) {
                Ok(wrote) if wrote != 0 && wrote <= take => bytes = &bytes[wrote..],
                _ => return false,
            }
        }
        true
    }

    fn remove_temporary(&mut self) -> bool {
        match self.native.pending_metadata() {
            Err(Error::Missing) => true,
            Ok((kind, links)) => kind == Kind::Regular && links <= 1 && self.native.unlink(PENDING).is_ok(),
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use newt_native_storage::Stat;
    use std::{boxed::Box, collections::BTreeMap, string::{String, ToString}, vec, vec::Vec};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Fault { Read, SeekMismatch, Write, ZeroWrite, OversizedWrite, Sync, Close, Rename, Unlink }
    struct File { readable: Vec<u8>, durable: Vec<u8>, kind: Kind, links: u64 }
    #[derive(Default)]
    struct MemoryNative {
        files: BTreeMap<String, File>,
        handles: BTreeMap<i32, (String, usize, bool)>,
        next: i32,
        fault: Option<Fault>,
        fault_after: usize,
        short_read: bool,
        short_write: bool,
        calls: Vec<String>,
    }
    impl MemoryNative {
        fn hit(&mut self, fault: Fault) -> bool {
            self.calls.push(std::format!("{fault:?}"));
            if self.fault != Some(fault) { return false; }
            if self.fault_after != 0 { self.fault_after -= 1; return false; }
            self.fault = None;
            true
        }
        fn seed(&mut self, path: &str, bytes: Vec<u8>) {
            self.files.insert(path.to_string(), File { readable: bytes.clone(), durable: bytes, kind: Kind::Regular, links: 1 });
        }
        fn info(file: &File) -> Stat {
            Stat { kind: file.kind, size: file.readable.len() as i64, inode: 1, device: 1, seconds: 0, nanos: 0 }
        }
        fn cold(&mut self) {
            assert!(self.handles.is_empty());
            for file in self.files.values_mut() { file.readable.clone_from(&file.durable); }
        }
    }
    impl PictureNative for MemoryNative {
        fn pending_metadata(&mut self) -> Result<(Kind, u64)> {
            self.calls.push(std::format!("stat:{PENDING}"));
            self.files.get(PENDING).map(|file| (file.kind, file.links)).ok_or(Error::Missing)
        }
    }
    impl Native for MemoryNative {
        fn open(&mut self, path: &str, mode: Open) -> Result<i32> {
            self.calls.push(std::format!("open:{path}"));
            let writing = match mode {
                Open::SourceReadOnly => {
                    if !self.files.contains_key(path) { return Err(Error::Missing); }
                    false
                },
                Open::CreateExclusiveWriteOnly => {
                    if self.files.contains_key(path) { return Err(Error::Exists); }
                    self.seed(path, Vec::new());
                    true
                },
                _ => panic!("unexpected open mode"),
            };
            self.next += 1;
            self.handles.insert(self.next, (path.to_string(), 0, writing));
            Ok(self.next)
        }
        fn close(&mut self, fd: i32) -> Result<()> {
            assert!(self.handles.remove(&fd).is_some());
            if self.hit(Fault::Close) { Err(Error::Other) } else { Ok(()) }
        }
        fn read(&mut self, fd: i32, out: &mut [u8]) -> Result<usize> {
            if self.hit(Fault::Read) { return Err(Error::Interrupted); }
            let (path, at, writing) = self.handles.get_mut(&fd).unwrap();
            assert!(!*writing);
            let bytes = &self.files.get(path.as_str()).unwrap().readable;
            let mut got = out.len().min(bytes.len().saturating_sub(*at));
            if self.short_read { got = got.saturating_sub(1); }
            out[..got].copy_from_slice(&bytes[*at..*at + got]);
            *at += got;
            Ok(got)
        }
        fn write(&mut self, fd: i32, bytes: &[u8]) -> Result<usize> {
            if self.hit(Fault::Write) { return Err(Error::Interrupted); }
            if self.hit(Fault::ZeroWrite) { return Ok(0); }
            if self.hit(Fault::OversizedWrite) { return Ok(bytes.len() + 1); }
            let (path, at, writing) = self.handles.get_mut(&fd).unwrap();
            assert!(*writing);
            assert!(bytes.len() <= CHUNK_BYTES);
            let got = if self.short_write { bytes.len().min(73) } else { bytes.len() };
            let file = self.files.get_mut(path.as_str()).unwrap();
            file.readable.resize((*at + got).max(file.readable.len()), 0);
            file.readable[*at..*at + got].copy_from_slice(&bytes[..got]);
            *at += got;
            Ok(got)
        }
        fn seek(&mut self, fd: i32, at: u32) -> Result<u64> {
            if self.hit(Fault::SeekMismatch) { return Ok(u64::from(at) + 1); }
            self.handles.get_mut(&fd).unwrap().1 = at as usize;
            Ok(u64::from(at))
        }
        fn sync(&mut self, fd: i32) -> Result<()> {
            if self.hit(Fault::Sync) { return Err(Error::Other); }
            let path = &self.handles[&fd].0;
            let file = self.files.get_mut(path.as_str()).unwrap();
            file.durable.clone_from(&file.readable);
            Ok(())
        }
        fn fstat(&mut self, fd: i32) -> Result<Stat> {
            self.calls.push("fstat".to_string());
            Ok(Self::info(&self.files[&self.handles[&fd].0]))
        }
        fn stat(&mut self, path: &str, no_follow: bool) -> Result<Stat> {
            assert!(no_follow);
            self.calls.push(std::format!("stat:{path}"));
            self.files.get(path).map(Self::info).ok_or(Error::Missing)
        }
        fn unlink(&mut self, path: &str) -> Result<()> {
            self.calls.push(std::format!("unlink:{path}"));
            if self.hit(Fault::Unlink) { return Err(Error::Other); }
            self.files.remove(path).map(|_| ()).ok_or(Error::Missing)
        }
        fn rename(&mut self, from: &str, to: &str) -> Result<()> {
            self.calls.push(std::format!("rename:{from}:{to}"));
            if self.hit(Fault::Rename) { return Err(Error::Other); }
            let file = self.files.remove(from).ok_or(Error::Missing)?;
            self.files.insert(to.to_string(), file);
            Ok(())
        }
        fn truncate(&mut self, _: i32, _: u32) -> Result<()> { panic!("unexpected truncate") }
        fn mkdir(&mut self, _: &str) -> Result<()> { panic!("unexpected mkdir") }
        fn next_nonce(&mut self) -> u32 { panic!("unexpected nonce") }
    }

    fn picture() -> (Record, Box<Frame>) {
        let mut record = Record::default();
        record.binding.book[4] = 19;
        record.binding.anchor.character = 432;
        record.footer.page = 7;
        record.footer.label[..7].copy_from_slice(b"section");
        let mut frame: Box<Frame> = vec![0; FRAME_BYTES].into_boxed_slice().try_into().unwrap();
        for (at, byte) in frame.iter_mut().enumerate() { *byte = at.wrapping_mul(17) as u8; }
        (record, frame)
    }
    fn written() -> (MemoryNative, Record, Box<Frame>) {
        let (mut record, frame) = picture();
        let mut native = MemoryNative::default();
        assert_eq!(Slot::new(&mut native, true).write(&mut record, &frame), Save::Written);
        native.calls.clear();
        (native, record, frame)
    }
    fn rejected(native: &mut MemoryNative, expected: &Binding, want: Load) {
        let mut frame: Box<Frame> = vec![0xaa; FRAME_BYTES].into_boxed_slice().try_into().unwrap();
        let mut footer = Footer::default();
        footer.page = 999;
        let before = footer.clone();
        assert_eq!(Slot::new(native, true).load(expected, &mut frame, &mut footer), want);
        assert_eq!(footer, before);
        assert!(native.handles.is_empty());
    }

    #[test]
    fn synced_round_trip_short_writes_and_header_only_holds() {
        let (mut record, frame) = picture();
        let mut native = MemoryNative { short_write: true, ..MemoryNative::default() };
        assert_eq!(Slot::new(&mut native, true).write(&mut record, &frame), Save::Written);
        let sync = native.calls.iter().position(|c| c == "Sync").unwrap();
        let close = native.calls.iter().position(|c| c == "Close").unwrap();
        let rename = native.calls.iter().position(|c| c.starts_with("rename:")).unwrap();
        assert!(sync < close && close < rename);
        native.cold();
        let mut restored: Box<Frame> = vec![0; FRAME_BYTES].into_boxed_slice().try_into().unwrap();
        let mut footer = Footer::default();
        assert_eq!(Slot::new(&mut native, true).load(&record.binding, &mut restored, &mut footer), Load::Shown);
        assert_eq!(restored, frame);
        assert_eq!(footer, record.footer);
        native.calls.clear();
        assert!(Slot::new(&mut native, true).holds(&record.binding, &footer));
        assert_eq!(native.calls.iter().filter(|c| *c == "Read").count(), 1);
    }

    #[test]
    fn bad_size_header_pixels_and_binding_do_not_publish_footer() {
        for damage in 0..5 {
            let (mut native, record, _) = written();
            let file = native.files.get_mut(PATH).unwrap();
            let want = match damage {
                0 => { file.readable.pop(); Load::Corrupt },
                1 => { file.readable.push(0); Load::Corrupt },
                2 => { file.readable[40] ^= 1; Load::Corrupt },
                3 => { file.readable[HEADER_BYTES + 123] ^= 1; Load::Corrupt },
                _ => { file.kind = Kind::Directory; Load::Corrupt },
            };
            rejected(&mut native, &record.binding, want);
        }
        let (mut native, mut record, _) = written();
        record.binding.book[0] ^= 1;
        rejected(&mut native, &record.binding, Load::Mismatch);
        assert_eq!(native.calls.iter().filter(|c| *c == "Read").count(), 1);
    }

    #[test]
    fn short_read_seek_error_and_close_failure_are_unreadable() {
        for fault in [Fault::Read, Fault::SeekMismatch, Fault::Close] {
            let (mut native, record, _) = written();
            native.fault = Some(fault);
            rejected(&mut native, &record.binding, Load::Unreadable);
            if fault == Fault::SeekMismatch { assert!(!native.calls.iter().any(|c| c == "Read")); }
        }
        let (mut native, record, _) = written();
        native.fault = Some(Fault::Read);
        native.fault_after = 2;
        rejected(&mut native, &record.binding, Load::Unreadable);
        assert_eq!(native.calls.iter().filter(|c| *c == "Read").count(), 3);
        let (mut native, record, _) = written();
        native.short_read = true;
        rejected(&mut native, &record.binding, Load::Unreadable);
        assert_eq!(native.calls.iter().filter(|c| *c == "Read").count(), 1);
    }

    #[test]
    fn failed_writes_clean_only_pending_and_keep_crc_mutation() {
        for fault in [Fault::Write, Fault::ZeroWrite, Fault::OversizedWrite, Fault::Sync, Fault::Close] {
            let (mut native, mut record, mut frame) = written();
            let old = native.files[PATH].durable.clone();
            native.seed("unrelated", vec![3]);
            frame[4] ^= 1;
            native.fault = Some(fault);
            assert_eq!(Slot::new(&mut native, true).write(&mut record, &frame), Save::Failed);
            assert_eq!(record.frame_crc, Record::crc(&frame[..]));
            assert!(!native.files.contains_key(PENDING));
            assert_eq!(native.files[PATH].durable, old);
            assert_eq!(native.files["unrelated"].durable, [3]);
            assert!(native.handles.is_empty());
            assert!(!native.calls.iter().any(|c| c.starts_with("rename:")));
            native.cold();
            assert_eq!(native.files[PATH].readable, old);
        }
    }

    #[test]
    fn torn_temporary_is_removed_before_recovery_and_partial_write_is_never_published() {
        let (mut native, mut record, frame) = written();
        let original = native.files[PATH].durable.clone();
        native.seed(PENDING, vec![0x44; 79]);
        native.fault = Some(Fault::Write);
        native.fault_after = 2;
        assert_eq!(Slot::new(&mut native, true).write(&mut record, &frame), Save::Failed);
        assert!(!native.files.contains_key(PENDING));
        assert_eq!(native.files[PATH].durable, original);
        assert!(!native.calls.iter().any(|c| c == "Sync" || c.starts_with("rename:")));
        assert_eq!(Slot::new(&mut native, true).write(&mut record, &frame), Save::Written);
        native.cold();
        assert_eq!(native.files[PATH].readable, original);
    }

    #[test]
    fn missing_and_readable_unsynced_pixels_are_never_shown() {
        let mut native = MemoryNative::default();
        rejected(&mut native, &Binding::default(), Load::Missing);
        let (mut native, record, _) = written();
        native.files.get_mut(PATH).unwrap().readable[HEADER_BYTES] ^= 1;
        rejected(&mut native, &record.binding, Load::Corrupt);
        native.cold();
        assert!(Slot::new(&mut native, true).holds(&record.binding, &record.footer));
    }

    #[test]
    fn fat_unlinks_destination_only_after_sync_close_and_leaves_pending_on_failure() {
        let (mut native, mut record, frame) = written();
        native.fault = Some(Fault::Rename);
        assert_eq!(Slot::new(&mut native, false).write(&mut record, &frame), Save::Failed);
        assert!(!native.files.contains_key(PATH));
        assert_eq!(native.files[PENDING].readable, native.files[PENDING].durable);
        let close = native.calls.iter().position(|c| c == "Close").unwrap();
        let unlink = native.calls.iter().position(|c| c == &std::format!("unlink:{PATH}")).unwrap();
        assert!(close < unlink);
        let (mut native, mut record, frame) = written();
        native.fault = Some(Fault::Unlink);
        assert_eq!(Slot::new(&mut native, false).write(&mut record, &frame), Save::Failed);
        assert!(native.files.contains_key(PATH) && native.files.contains_key(PENDING));
        assert!(!native.calls.iter().any(|c| c.starts_with("rename:")));
    }

    #[test]
    fn stale_nonregular_pending_is_preserved_and_valid_header_survives_close_error() {
        let (mut native, mut record, frame) = written();
        native.seed(PENDING, vec![1]);
        native.files.get_mut(PENDING).unwrap().kind = Kind::Other;
        assert_eq!(Slot::new(&mut native, true).write(&mut record, &frame), Save::Failed);
        assert_eq!(native.files[PENDING].readable, [1]);
        native.files.get_mut(PENDING).unwrap().kind = Kind::Regular;
        native.files.get_mut(PENDING).unwrap().links = 2;
        native.calls.clear();
        assert_eq!(Slot::new(&mut native, true).write(&mut record, &frame), Save::Failed);
        assert_eq!(native.files[PENDING].readable, [1]);
        assert_eq!(native.calls, [std::format!("stat:{PENDING}")]);
        native.fault = Some(Fault::Close);
        let mut decoded = Record::default();
        assert_eq!(Slot::new(&mut native, true).header(&mut decoded), Load::Unreadable);
        assert_eq!(decoded, record);
        native.files.get_mut(PATH).unwrap().readable[0] ^= 1;
        native.fault = Some(Fault::Close);
        assert_eq!(Slot::new(&mut native, true).header(&mut decoded), Load::Corrupt);
        assert_eq!(decoded, record);
    }
}
