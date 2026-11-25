use crate::{Authority, Error, Kind, Native, Open, Result, Route, Stat};
use crate::{ProducerPolicy, text::Path};
use alloc::vec::Vec;
use core::fmt::Write;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freshness {
    Refused,
    Fresh,
    PathFailed,
}
pub use newt_core::producer_visit::MissingProbe;
#[path = "port_state.rs"]
mod state;
pub use state::{FilePortState, LoanError, CloseOutcome};

/// Short exclusive loan. It owns no descriptor/name allocation and has no Drop.
/// The backend must be the same native source domain that owns the live fd.
pub struct FilePort<'a, S: Native> {
    native: &'a mut S,
    route: &'a Route<'a>,
    directory: &'a str,
    state: &'a mut FilePortState,
}
impl<'a, S: Native> FilePort<'a, S> {
    pub fn lend(
        state: &'a mut FilePortState, native: &'a mut S,
        route: &'a Route<'a>, directory: &'a str,
    ) -> core::result::Result<Self, LoanError> {
        // Existing canonical bytes bind every named/live generation. Empty
        // state has never acquired a name or fd; no extra key/path is retained.
        if !state.path.is_empty()
            && !route.owns_name(&state.path[..state.final_bytes], state.suffix_bytes)
        {
            return Err(LoanError::SourceDomain);
        }
        Ok(Self { native, route, directory, state })
    }
    pub fn generation(&self) -> u64 {
        self.state.generation
    }
    pub fn operation(&mut self) -> Operation<'_, 'a, S> {
        Operation {
            port: self,
            producer: None,
            receipt_live: None,
        }
    }
    pub fn attach_producer<'op>(
        &'op mut self,
        policy: &'op ProducerPolicy<'_>,
    ) -> Option<Operation<'op, 'a, S>> {
        if self.state.fd.is_some() || self.state.token != 0 {
            return None;
        }
        Some(Operation {
            port: self,
            producer: Some(policy),
            receipt_live: None,
        })
    }
}
/// Short loan of persistent state and optional attempt policy. Drop ends the
/// loan only; the owner explicitly releases while policy is still attached.
pub struct Operation<'op, 'a, S: Native> {
    port: &'op mut FilePort<'a, S>,
    producer: Option<&'op ProducerPolicy<'op>>,
    receipt_live: Option<&'op mut dyn FnMut() -> bool>,
}
impl<S: Native> Operation<'_, '_, S> {
    fn boundary(&mut self) -> bool {
        self.producer.is_none_or(|p| p.boundary())
    }
    fn producer_failure(&mut self) {
        if let Some(p) = self.producer {
            p.record_failure();
        }
    }
    fn settle(&mut self, ok: bool) -> bool {
        if !ok {
            self.producer_failure()
        }
        ok
    }
    pub fn allowed(&mut self) -> bool {
        self.port.route.allowed() && self.boundary()
    }
    fn permitted(&mut self, lease: u64) -> bool {
        lease == self.port.state.generation && self.allowed() && lease == self.port.state.generation
    }
    fn io<T>(&mut self, op: impl FnOnce(&mut S) -> Result<T>) -> Result<T> {
        self.io_state(|native, _| op(native))
    }
    // Native calls borrow retained names instead of copying their 256B backing.
    fn io_state<T>(&mut self, op: impl FnOnce(&mut S, &FilePortState) -> Result<T>) -> Result<T> {
        if !self.boundary() {
            return Err(Error::Cancelled);
        }
        op(&mut *self.port.native, self.port.state)
    }
    fn named_io<T>(&mut self, op: impl FnOnce(&mut S, &Path, &[Path]) -> Result<T>) -> Result<T> {
        self.io_state(|native, state|
            op(native, &state.path, state.names.as_ref().expect("transaction names")))
    }
    fn close(&mut self, fd: i32) -> Result<()> {
        self.boundary();
        let r = self.port.native.close(fd);
        if r.is_err() {
            self.producer_failure()
        }
        r
    }
    pub fn generation(&self) -> u64 {
        self.port.state.generation
    }
    pub fn bytes(&self) -> u32 {
        self.port.state.bytes
    }
    pub fn writing(&self) -> bool {
        self.port.state.writing
    }
    pub fn absent(&self) -> bool {
        self.port.state.absent
    }
    pub fn invalid(&self) -> bool {
        self.port.state.invalid
    }
    pub fn checks_deferred(&self) -> bool {
        self.port.state.defer_depth != 0
    }
    pub fn immutable_read(&self) -> bool {
        self.port.state.immutable_depth != 0
    }
    pub fn released_cleanly(&self, lease: u64) -> bool {
        lease == self.port.state.generation && !self.port.state.failed && !self.port.state.deferred_failed
    }
    pub fn set_write_credit(&mut self, bytes: u64) {
        self.port.state.serial = bytes
    }
    pub fn write_credit(&self) -> u64 {
        self.port.state.serial
    }
    pub fn charge_write(&mut self, bytes: u64) {
        self.port.state.serial = self.port.state.serial.saturating_sub(bytes)
    }
    pub fn release(&mut self) -> u64 {
        if self.port.state.deferred_fresh {
            self.port.state.deferred_fresh = false;
            if !self.stable(self.port.state.generation) {
                self.port.state.deferred_failed = true
            }
        }
        self.port.state.deferred_fresh = false;
        self.port.state.generation = self.port.state.generation.wrapping_add(1);
        let fd = self.port.state.fd.take();
        let temporary = self.port.state.owns.then(|| self.port.state.path);
        if self.port.state.owns && newt_core::book_index::FileName::parse(self.suffix().as_bytes()).is_some() {
            self.port.state.path.truncate(self.port.state.final_bytes-3);
            self.port.state.path.write_str("IDX").expect("same-width index extension");
        }
        self.port.state.token = 0;
        self.port.state.owns = false;
        self.port.state.writing = false;
        self.port.state.failed = false;
        self.port.state.synced = false;
        self.port.state.bytes = 0;
        self.port.state.path.truncate(self.port.state.final_bytes);
        self.port.state.names = None;
        if let Some(fd) = fd {
            if self.close(fd).is_err() {
                self.port.state.failed = true
            }
        }
        if let Some(path) = temporary {
            self.boundary();
            let ok = self.port.native.unlink(&path).is_ok();
            if !ok {
                self.producer_failure();
                self.port.state.failed = true
            }
        }
        self.port.state.generation
    }
    pub fn fail(&mut self) -> bool {
        self.producer_failure();
        self.release();
        false
    }
    pub fn poison(&mut self) -> bool {
        self.producer_failure();
        self.port.state.failed = true;
        false
    }
    /// Renaming/reopening a live port is outside the supported exclusive caller
    /// domain. Reject it without discarding the live ownership.
    pub fn name(&mut self, suffix: &str, receipt: bool, directory: Option<&str>) -> bool {
        if self.port.state.fd.is_some()
            || self.port.state.owns
            || self.port.state.token != 0
            || !self.allowed()
            || (directory.unwrap_or(self.port.directory)=="Index" && !newt_core::book_index::FileName::parse(suffix.as_bytes())
                .is_some_and(|name|name.kind()==newt_core::book_index::FileKind::Index))
            || suffix.is_empty()
            || suffix.len() > 127
            || suffix.contains("..")
            || suffix.ends_with(".bak")
            || (!newt_core::book_index::FileName::parse(suffix.as_bytes()).is_some()
                && !suffix.bytes().all(|b| b.is_ascii_digit() || b.is_ascii_lowercase() || b == b'-' || b == b'.'))
        {
            return false;
        }
        let Some(path) =
            self.port
                .route
                .name(directory.unwrap_or(self.port.directory), suffix, receipt)
        else {
            return false;
        };
        self.port.state.final_bytes = path.len();
        self.port.state.suffix_bytes = suffix.len();
        self.port.state.path = path;
        true
    }
    pub fn same_name(&self, name: &str) -> bool {
        if self.port.state.owns {
            if let Some(current)=newt_core::book_index::FileName::parse(self.suffix().as_bytes()) {
                return current.with_kind(newt_core::book_index::FileKind::Index).as_str()==name;
            }
        }
        self.suffix() == name
    }
    pub fn suffix(&self) -> &str {
        &self.port.state.path[self.port.state.final_bytes - self.port.state.suffix_bytes..self.port.state.final_bytes]
    }
    // Native pageRead discriminator, widened to every block-envelope file
    // (pages and book indexes); page namespace membership is separately
    // checked by the existing canonical helper in belongs_to_page_namespace.
    fn envelope_read(&self) -> bool {
        self.port.state.fd.is_some()
            && !self.port.state.writing
            && newt_core::cache_names::namespace::envelope_kind(self.suffix().as_bytes()).is_some()
    }
    pub fn belongs_to_page_namespace(&self) -> bool {
        use newt_core::cache_names::namespace::{Kind as NameKind, PageName};
        PageName::parse(self.canonical().rsplit('/').next().map(str::as_bytes)).kind
            == NameKind::Page
    }
    /// The displayed page only; an open book index is pinned by its own census.
    pub fn pinned_page(&self) -> Option<&str> {
        (self.envelope_read() && self.suffix().ends_with(".fibp"))
            .then(|| self.port.state.path[..self.port.state.final_bytes].rsplit('/').next())
            .flatten()
    }
    fn snapshot(&mut self, path: &str, maximum: u32, exact: bool) -> bool {
        let Some(fd) = self.port.state.fd else { return false };
        let Ok(d) = self.io(|s| s.fstat(fd)) else {
            return self.settle(false);
        };
        if d.kind != Kind::Regular
            || d.size < 0
            || d.size > i64::from(maximum)
            || (exact && d.size != i64::from(maximum))
        {
            self.port.state.invalid = true;
            return false;
        }
        // FAT fstat has no directory-entry time. Freshness must use the
        // path timestamp here, as it does in stable().
        let Ok(p) = self.io(|s| s.stat(path, false)) else {
            return self.settle(false);
        };
        if p.kind != Kind::Regular || p.size != d.size || !self.allowed() {
            return self.settle(false);
        }
        self.port.state.stamp = (p.seconds, p.nanos);
        self.port.state.bytes = d.size as u32;
        true
    }
    pub fn open_read(&mut self, maximum: u32) -> bool {
        self.port.state.absent = false;
        self.port.state.invalid = false;
        if self.port.state.fd.is_some()
            || self.port.state.owns
            || self.canonical().is_empty()
            || !self.allowed()
        {
            return false;
        }
        let path = self.port.state.path;
        let mut opened = self.io(|s| s.open(&path, Open::ReadNonblock));
        if opened == Err(Error::Missing) && self.canonical().len() + 4 < 256 {
            let Some(backup)=self.backup_name() else {return self.fail();};
            let mut names = Vec::new();
            if names.try_reserve_exact(1).is_err() {
                return self.fail();
            }
            names.push(backup);
            self.port.state.names = Some(names);
            opened = self.io(|s| s.open(&backup, Open::ReadNonblock));
            self.port.state.absent = opened == Err(Error::Missing);
        }
        self.port.state.fd = opened.ok();
        if self.port.state.fd.is_none() {
            if self.port.state.absent {
                self.release();
                return false;
            }
            return self.fail();
        }
        let path = *self
            .port
            .state.names
            .as_ref()
            .map(|n| &n[0])
            .unwrap_or(&self.port.state.path);
        if !self.snapshot(&path, maximum, false) {
            if self.port.state.invalid {
                self.release();
                return false;
            }
            return self.fail();
        }
        if !self.allowed() {
            return self.fail();
        }
        true
    }
    fn make_directory(&mut self) -> bool {
        let Some((parents, _)) = self.port.state.path.rsplit_once('/') else {
            return false;
        };
        if parents.is_empty() || parents.len() >= 192 {
            return false;
        }
        let parents = Path::from_str(parents).expect("bounded parent");
        for end in
            (1..=parents.len()).filter(|&i| i == parents.len() || parents.as_bytes()[i] == b'/')
        {
            let part = &parents[..end];
            if !self.allowed() {
                return false;
            }
            if !matches!(self.io(|s| s.mkdir(part)), Ok(()) | Err(Error::Exists)) {
                return false;
            }
            let no_follow = self.producer.is_some();
            let found = self.io(|s| s.stat(part, no_follow));
            if !matches!(
                found,
                Ok(Stat {
                    kind: Kind::Directory,
                    ..
                })
            ) || !self.allowed()
            {
                return false;
            }
        }
        true
    }
    pub fn begin(&mut self, bytes: u32, fixed: bool) -> bool {
        if self.port.state.fd.is_some() || self.port.state.owns || self.canonical().is_empty() {
            return false;
        }
        if self.producer.is_some() && !self.destination_safe(false) {
            return false;
        }
        if !self.allowed() || !self.make_directory() || !self.allowed() {
            return false;
        }
        for _ in 0..32 {
            self.port.state.path.truncate(self.port.state.final_bytes);
            let short=newt_core::book_index::FileName::parse(self.suffix().as_bytes());
            let named=if short.is_some() {
                self.port.state.path.truncate(self.port.state.path.len()-3);
                self.port.state.path.write_str("TMP").is_ok()
            } else {
                let nonce=self.port.native.next_nonce();
                write!(self.port.state.path,".{nonce:08x}.tmp").is_ok()
            };
            if !named {
                self.port.state.path.truncate(self.port.state.final_bytes);
                return false;
            }
            let path = self.port.state.path;
            if short.is_some() && !matches!(self.io(|s|s.unlink(&path)),Ok(())|Err(Error::Missing)) {return false;}
            match self.io(|s| s.open(&path, Open::CreateExclusive)) {
                Ok(fd) => {
                    self.port.state.fd = Some(fd);
                    self.port.state.owns = true;
                    self.port.state.writing = true;
                    break;
                }
                Err(Error::Exists) => (),
                Err(_) => {
                    self.port.state.path.truncate(self.port.state.final_bytes);
                    return false;
                }
            }
        }
        let Some(fd) = self.port.state.fd else {
            self.port.state.path.truncate(self.port.state.final_bytes);
            return false;
        };
        let ready = (!fixed || self.io(|s| s.truncate(fd, bytes)).is_ok())
            && matches!(self.io(|s|s.fstat(fd)),Ok(Stat{kind:Kind::Regular,size,..}) if size==i64::from(bytes))
            && self.allowed();
        if !ready {
            return self.fail();
        }
        self.port.state.bytes = bytes;
        self.port.state.synced = false;
        true
    }
    pub fn read_freshness(&mut self, lease: u64) -> Freshness {
        if lease != self.port.state.generation
            || self.port.state.fd.is_none()
            || self.port.state.failed
            || !self.permitted(lease)
            || (self.port.state.writing && !self.port.state.owns)
        {
            self.settle(false);
            return Freshness::Refused;
        }
        if !self.port.state.writing
            && self.port.state.immutable_depth != 0
            && self.port.state.defer_depth != 0
            && self.port.state.deferred_fresh
        {
            return if self.permitted(lease) {
                Freshness::Fresh
            } else {
                Freshness::Refused
            };
        }
        let fd = self.port.state.fd.unwrap();
        let d = self.io(|s| s.fstat(fd));
        let Ok(d) = d else {
            self.settle(false);
            return Freshness::PathFailed;
        };
        if d.kind != Kind::Regular || d.size != i64::from(self.port.state.bytes) {
            self.settle(false);
            return Freshness::PathFailed;
        }
        if !self.port.state.writing && self.port.state.defer_depth != 0 && self.port.state.deferred_fresh {
            return if self.permitted(lease) {
                Freshness::Fresh
            } else {
                Freshness::Refused
            };
        }
        if !self.port.state.writing && self.port.state.names.is_some() {
            if !matches!(self.io_state(|s, state|
                s.stat(&state.path[..state.final_bytes], false)), Err(Error::Missing)) {
                self.settle(false);
                return Freshness::PathFailed;
            }
        }
        if self.port.state.writing && !self.port.state.synced {
            return if self.permitted(lease) {
                Freshness::Fresh
            } else {
                Freshness::Refused
            };
        }
        let p = self.io_state(|s, state| {
            let path = if !state.writing {
                state.names.as_ref().map(|n| &n[0]).unwrap_or(&state.path)
            } else { &state.path };
            s.stat(path, false)
        });
        let fresh = matches!(p,Ok(p) if p.kind==Kind::Regular && p.size==i64::from(self.port.state.bytes)
            && (d.inode==0 || p.inode==d.inode) && (d.device==0 || p.device==d.device) && (p.seconds,p.nanos)==self.port.state.stamp);
        if !fresh {
            self.settle(false);
            return Freshness::PathFailed;
        }
        if !self.permitted(lease) {
            return Freshness::Refused;
        }
        if self.port.state.defer_depth != 0 && !self.port.state.writing {
            self.port.state.deferred_fresh = true
        }
        Freshness::Fresh
    }
    pub fn stable(&mut self, lease: u64) -> bool {
        self.read_freshness(lease) == Freshness::Fresh
    }
    fn finish_path_check(&mut self) -> bool {
        let fresh = self.read_freshness(self.port.state.generation);
        if fresh == Freshness::PathFailed && self.envelope_read() && self.allowed() {
            self.release();
        }
        fresh == Freshness::Fresh
    }
    pub fn begin_deferred_path_checks(&mut self) {
        if self.port.state.defer_depth == u8::MAX {
            self.port.state.deferred_failed = true;
            return;
        }
        if self.port.state.defer_depth == 0 {
            self.port.state.deferred_fresh = false;
            self.port.state.deferred_failed = false
        }
        self.port.state.defer_depth += 1;
    }
    pub fn finish_deferred_path_checks(&mut self) -> bool {
        if self.port.state.defer_depth == 0 {
            return false;
        }
        self.port.state.defer_depth -= 1;
        if self.port.state.defer_depth != 0 {
            return !self.port.state.deferred_failed;
        }
        let fresh = !self.port.state.deferred_failed;
        let checked = self.port.state.deferred_fresh;
        self.port.state.deferred_fresh = false;
        self.port.state.deferred_failed = false;
        if checked {
            self.finish_path_check() && fresh
        } else {
            fresh
        }
    }
    pub fn begin_immutable_read(&mut self) {
        self.begin_deferred_path_checks();
        if self.port.state.immutable_depth == u8::MAX {
            self.port.state.deferred_failed = true
        } else {
            self.port.state.immutable_depth += 1
        }
    }
    pub fn finish_immutable_read(&mut self) -> bool {
        if self.port.state.immutable_depth == 0 {
            return false;
        }
        self.port.state.immutable_depth -= 1;
        if self.port.state.defer_depth == 1 {
            return self.finish_deferred_path_checks();
        }
        let checked = self.port.state.deferred_fresh;
        self.port.state.deferred_fresh = false;
        let fresh = !checked || self.finish_path_check();
        if !fresh {
            self.port.state.deferred_failed = true
        }
        self.finish_deferred_path_checks() && fresh
    }
    pub fn defer_page_path_check(&mut self) -> bool {
        if self.port.state.immutable_depth == 0
            || self.port.state.defer_depth == 0
            || !self.envelope_read()
            || self.port.state.failed
            || self.port.state.deferred_failed
            || !self.allowed()
        {
            return self.settle(false);
        }
        let fd = self.port.state.fd.unwrap();
        let d = self.io(|s| s.fstat(fd));
        if !matches!(d,Ok(Stat{kind:Kind::Regular,size,..}) if size==i64::from(self.port.state.bytes))
            || !self.allowed()
        {
            return self.settle(false);
        }
        self.port.state.deferred_fresh = true;
        true
    }
    pub fn begin_read(&mut self, lease: u64) -> u64 {
        if self.producer.is_some() || self.port.state.token != 0 || !self.stable(lease) {
            return 0;
        }
        self.port.state.serial = self.port.state.serial.wrapping_add(1);
        if self.port.state.serial == 0 {
            self.port.state.serial = 1
        }
        self.port.state.token = self.port.state.serial;
        self.port.state.token
    }
    pub fn stable_read(&mut self, lease: u64, token: u64) -> bool {
        token != 0
            && token == self.port.state.token
            && lease == self.port.state.generation
            && self.port.state.fd.is_some()
            && !self.port.state.failed
            && self.allowed()
            && (!self.port.state.writing || self.stable(lease))
    }
    pub fn end_read(&mut self, lease: u64, token: u64) -> bool {
        if token == 0 || token != self.port.state.token || lease != self.port.state.generation {
            return false;
        }
        self.port.state.token = 0;
        self.stable(lease)
    }
    pub fn cancel_read(&mut self, lease: u64, token: u64) {
        if token != 0 && token == self.port.state.token && lease == self.port.state.generation {
            self.port.state.token = 0
        }
    }
    pub fn read(&mut self, offset: u32, out: &mut [u8]) -> bool {
        if self.port.state.token != 0 {
            return false;
        }
        self.read_impl(self.port.state.generation, None, offset, out)
    }
    pub fn read_within(&mut self, lease: u64, token: u64, offset: u32, out: &mut [u8]) -> bool {
        self.read_impl(lease, Some(token), offset, out)
    }
    fn read_check(&mut self, lease: u64, token: Option<u64>) -> bool {
        match token {
            Some(t) => self.stable_read(lease, t),
            None => self.stable(lease),
        }
    }
    fn read_impl(
        &mut self,
        lease: u64,
        token: Option<u64>,
        mut offset: u32,
        mut out: &mut [u8],
    ) -> bool {
        if offset > self.port.state.bytes
            || out.len() > (self.port.state.bytes - offset) as usize
            || !self.read_check(lease, token)
        {
            return false;
        }
        let mut interrupted = 0;
        while !out.is_empty() {
            if !self.port.route.step() || !self.read_check(lease, token) {
                return false;
            }
            let fd = self.port.state.fd.unwrap();
            let seek = self.io(|s| s.seek(fd, offset));
            if !self.settle(seek == Ok(u64::from(offset)))
                || (token.is_some() && !self.read_check(lease, token))
            {
                return false;
            }
            let chunk = if token.is_none() && self.port.state.immutable_depth != 0 && !self.port.state.writing {
                4096
            } else {
                1024
            };
            let want = out.len().min(chunk);
            let r = self.io(|s| s.read(fd, &mut out[..want]));
            if r == Err(Error::Interrupted) {
                interrupted += 1;
                if interrupted < 4 {
                    continue;
                }
            }
            if r.is_ok() {
                interrupted = 0
            }
            let Ok(got) = r else {
                self.producer_failure();
                return false;
            };
            if got == 0 || got > want {
                self.producer_failure();
                return false;
            }
            if !self.port.route.cache_read_live() || !self.read_check(lease, token) {
                return false;
            }
            offset = offset.wrapping_add(got as u32);
            out = &mut out[got..];
        }
        self.read_check(lease, token)
    }
    pub fn write(&mut self, mut offset: u32, mut input: &[u8], append: bool) -> bool {
        let lease = self.port.state.generation;
        if !self.port.state.writing || !self.stable(lease) {
            return false;
        }
        let mut interrupted = 0;
        while !input.is_empty() {
            if !self.stable(lease) {
                return self.poison();
            }
            let fd = self.port.state.fd.unwrap();
            if self.io(|s| s.seek(fd, offset)) != Ok(u64::from(offset)) {
                return self.poison();
            }
            let want = input.len().min(1024);
            self.port.state.synced = false;
            let r = self.io(|s| s.write(fd, &input[..want]));
            if r == Err(Error::Interrupted) {
                interrupted += 1;
                if interrupted < 4 {
                    continue;
                }
            }
            if r.is_ok() {
                interrupted = 0
            }
            let Ok(got) = r else { return self.poison() };
            if got == 0 || got > want || !self.allowed() {
                return self.poison();
            }
            if append {
                self.port.state.bytes = self.port.state.bytes.wrapping_add(got as u32)
            }
            if !self.stable(lease) {
                return self.poison();
            }
            offset = offset.wrapping_add(got as u32);
            input = &input[got..];
        }
        self.stable(lease)
    }
    pub fn sync(&mut self) -> bool {
        if !self.port.state.writing || !self.stable(self.port.state.generation) {
            return self.fail();
        }
        self.port.state.synced = false;
        let fd = self.port.state.fd.unwrap();
        if self.io(|s| s.sync(fd)).is_err() {
            return self.fail();
        }
        let path = self.port.state.path;
        if !self.snapshot(&path, self.port.state.bytes, true) {
            return self.fail();
        }
        self.port.state.synced = true;
        if self.stable(self.port.state.generation) {
            true
        } else {
            self.fail()
        }
    }
    fn rollback(&mut self) -> bool {
        if matches!(self.named_io(|s, _, names| s.stat(&names[0], false)), Err(Error::Missing)) && self.allowed() {
            let _ = self.named_io(|s, _, names| s.rename_cstr(
                names[1].as_c_str().ok_or(Error::Other)?, names[0].as_c_str().ok_or(Error::Other)?));
        }
        false
    }
    fn promote(&mut self) -> bool {
        if self.producer.is_some() && !self.destination_safe(true) {
            return false;
        }
        match self.named_io(|s, temporary, names| s.rename_cstr(
            temporary.as_c_str().ok_or(Error::Other)?, names[0].as_c_str().ok_or(Error::Other)?)) {
            Ok(()) => return true,
            Err(Error::Exists) => (),
            Err(_) => return false,
        }
        if self.port.state.final_bytes + 4 >= 256 || !self.allowed() {
            return false;
        }
        {
            let Some(backup)=self.backup_name() else {return false;};
            self.port.state.names.as_mut().expect("transaction names")[1]=backup;
        }
        if !matches!(
            self.named_io(|s, _, names| s.stat(&names[0], false)),
            Ok(Stat {
                kind: Kind::Regular,
                ..
            })
        ) || !self.allowed()
        {
            return false;
        }
        let previous = self.named_io(|s, _, names| s.stat(&names[1], false));
        if self.producer.is_some() && previous.is_ok() {
            return self.settle(false);
        }
        let usable = matches!(
            previous,
            Ok(Stat {
                kind: Kind::Regular,
                ..
            }) | Err(Error::Missing)
        );
        if !self.settle(usable) || !self.allowed() {
            return false;
        }
        if previous.is_ok() && (self.named_io(|s, _, names| s.unlink(&names[1])).is_err() || !self.allowed()) {
            return false;
        }
        let moved = self.named_io(|s, _, names| s.rename_cstr(
            names[0].as_c_str().ok_or(Error::Other)?, names[1].as_c_str().ok_or(Error::Other)?)).is_ok();
        if !self.settle(moved) {
            return self.rollback();
        }
        if !self.allowed() {
            return false;
        }
        let published = self.named_io(|s, temporary, names| s.rename_cstr(
            temporary.as_c_str().ok_or(Error::Other)?, names[0].as_c_str().ok_or(Error::Other)?)).is_ok();
        if self.settle(published) {
            true
        } else {
            self.rollback()
        }
    }
    // Transaction construction ends before native publication starts.
    #[inline(never)]
    fn prepare_final_names(&mut self) -> bool {
        let mut names = Vec::new();
        if names.try_reserve_exact(2).is_err() {
            return false;
        }
        names.push(self.canonical());
        names.push(Path::empty());
        self.port.state.names = Some(names);
        true
    }
    pub fn commit(&mut self, maximum: u32, retain: bool) -> bool {
        if !self.port.state.writing || !self.port.state.synced || !self.stable(self.port.state.generation) {
            return self.fail();
        }
        let fd = self.port.state.fd.take().unwrap();
        if self.close(fd).is_err() || !self.allowed() {
            return self.fail();
        }
        if !self.prepare_final_names() {
            return self.fail();
        }
        if !self.allowed() || !self.promote() {
            return self.fail();
        }
        self.port.state.names = None;
        self.port.state.owns = false;
        self.port.state.writing = false;
        self.port.state.path.truncate(self.port.state.final_bytes);
        if !self.allowed() {
            return self.fail();
        }
        if retain && (!self.open_read(maximum) || !self.stable(self.port.state.generation)) {
            return self.fail();
        }
        true
    }
    pub fn producing(&self) -> bool {
        self.producer.is_some()
    }
    pub fn detach_producer(&mut self) {
        self.producer = None;
        self.port.state.token = 0;
    }
    fn destination_safe(&mut self, transaction: bool) -> bool {
        if transaction && self.producer_invalid() {
            return self.settle(false);
        }
        if self.canonical().len() + 5 > 256 || !self.allowed() {
            return false;
        }
        let canonical = self.canonical();
        let mut backup = canonical;
        if backup.write_str(".bak").is_err() {
            return false;
        }
        let c = self.io(|s| s.stat(&canonical, true));
        let safe_c = matches!(
            c,
            Ok(Stat {
                kind: Kind::Regular,
                ..
            }) | Err(Error::Missing)
        );
        if !self.settle(safe_c) || !self.allowed() {
            return false;
        }
        let b = self.io(|s| s.stat(&backup, true));
        self.settle(matches!(b, Err(Error::Missing))) && self.allowed()
    }
    pub fn suspend_producer_write(&mut self) -> bool {
        if self.producer.is_some()
            && !self.port.state.writing
            && self.port.state.fd.is_none()
            && !self.port.state.owns
        {
            return self.allowed();
        }
        if self.producer.is_none() || !self.port.state.writing || !self.port.state.owns || !self.sync() {
            return self.poison();
        }
        let Some(fd) = self.port.state.fd.take() else {
            return self.poison();
        };
        // Original suspension deliberately uses raw close rather than producerIo.
        if self.port.native.close(fd).is_ok() {
            true
        } else {
            self.poison()
        }
    }
    pub fn resume_producer_write(&mut self) -> bool {
        if self.producer.is_some()
            && !self.port.state.writing
            && self.port.state.fd.is_none()
            && !self.port.state.owns
        {
            return self.allowed();
        }
        if self.producer.is_none()
            || !self.port.state.writing
            || !self.port.state.owns
            || self.port.state.fd.is_some()
            || self.port.state.failed
            || !self.allowed()
        {
            return self.poison();
        }
        let path = self.port.state.path;
        self.port.state.fd = self.io(|s| s.open(&path, Open::ReadWriteNonblock)).ok();
        if !self.stable(self.port.state.generation) {
            return self.poison();
        }
        self.port.state.synced = false;
        true
    }
    pub fn probe_missing(&mut self, name: &str, cancel: &dyn Authority) -> MissingProbe {
        if cancel.cancelled() {
            return MissingProbe::Stopped;
        }
        if !self.port.route.allowed() {
            return MissingProbe::Denied;
        }
        let Some(path) = self.port.route.name(self.port.directory, name, false) else {
            return MissingProbe::Denied;
        };
        if path.len() + 5 > 256 {
            return MissingProbe::Denied;
        }
        let mut backup = path;
        if backup.write_str(".bak").is_err() {
            return MissingProbe::Denied;
        }
        for candidate in [path, backup] {
            if cancel.cancelled() {
                return MissingProbe::Stopped;
            }
            // Original census is raw LSTAT, independent of producer callbacks.
            let found = self.port.native.stat(&candidate, true);
            if !matches!(
                found,
                Ok(Stat {
                    kind: Kind::Regular,
                    ..
                }) | Err(Error::Missing)
            ) {
                return MissingProbe::Denied;
            }
            if cancel.cancelled() {
                return MissingProbe::Stopped;
            }
            if !self.port.route.allowed() {
                return MissingProbe::Denied;
            }
            if found.is_ok() {
                return MissingProbe::Complete;
            }
        }
        MissingProbe::Ready
    }
}

#[path = "source_hints.rs"]
mod source_hints;
pub use source_hints::*;

#[path = "chapter_cache.rs"]
pub mod chapter_cache;

include!("port_producer_check.rs");
