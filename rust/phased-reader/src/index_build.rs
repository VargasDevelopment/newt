//! One checked index pass before reader opening.
#![forbid(unsafe_code)]
use newt_core::{admission::{self, Yield}, book_index::{self, Identity},
    book_index_build::WorkFile, page_cache::CacheStorage, xml_session::ParserBackend,
    zip_container::Status, zip_window::source::BookSource};
use crate::phase_storage::{BuildLoan, ReadScope, Storage};
use crate::CachePresence;

/// Optional physical handoff, confined to the index pass. Read-only fixture
/// sources keep the default refusal; product selected sources reopen their
/// exact catalog path. Cancellation remains observable while no FD is held.
pub trait ReleasableSource: BookSource {
    fn release(&mut self) -> bool { false }
    fn reopen(&mut self) -> bool { false }
    fn cancelled(&self) -> bool { false }
}
struct HeldSource<'a>(&'a mut dyn BookSource);
impl BookSource for HeldSource<'_> {
    fn size(&self) -> u64 { self.0.size() }
    fn read_at(&mut self, at: u64, bytes: &mut [u8]) -> i32 { self.0.read_at(at, bytes) }
}
impl ReleasableSource for HeldSource<'_> {}

/// One attempt, over the same card owner as Pages. No source is retained.
pub trait Store: WorkFile + CacheStorage {
    fn census(&mut self, cancelled: &dyn Fn() -> bool);
    fn spilled(&self) -> bool;
    fn release_pages(&mut self) -> bool;
}

/// Proof that this attempt found or published an index for the full digest.
/// Only a successful index pass can construct it.
///
/// ```compile_fail
/// use newt_phased_reader::index_build::Indexed;
/// let _ = Indexed { digest: [0; 32], verified_now: true, reopened: false };
/// ```
#[derive(Debug)]
pub struct Indexed { digest: [u8; 32], verified_now: bool, reopened: bool }
impl Indexed {
    pub fn digest(&self) -> &[u8; 32] { &self.digest }
    pub fn verified_now(&self) -> bool { self.verified_now }
    pub fn source_reopened(&self) -> bool { self.reopened }
}

/// A hit needs no full-source hash. A miss must finish verification before
/// the builder sees any bytes; scratch comes from the existing parse loan.
pub enum Verification<'a> {
    Verified,
    Pending(&'a mut dyn FnMut(&mut dyn BookSource, &mut [u8]) -> Result<(), Status>),
}

// Additional live adapter state is bounded independently of B2's 2,304-byte
// builder budget; the 32 KiB run and 7,552-byte join state stay in the banks.
const _: () = assert!(core::mem::size_of::<ObservedStore<'static, 'static>>() <= 40);
#[inline(always)]
pub fn open<B: ParserBackend + ?Sized, C: ReadScope + CachePresence>(
    source: &mut dyn BookSource, digest: &[u8; 32],
    storage: &mut Storage<'_>, lender: &mut impl BuildLoan, backend: &mut B,
    store: &mut C, hook: Yield<'_>, verification: Verification<'_>) -> Result<Indexed, Status> {
    let mut held = HeldSource(source);
    open_releasing(&mut held, digest, storage, lender, backend, store, hook, verification)
}

#[inline(always)]
pub fn open_releasing<B: ParserBackend + ?Sized, C: ReadScope + CachePresence>(
    source: &mut dyn ReleasableSource, digest: &[u8; 32],
    storage: &mut Storage<'_>, lender: &mut impl BuildLoan, backend: &mut B,
    store: &mut C, hook: Yield<'_>, verification: Verification<'_>) -> Result<Indexed, Status> {
    let identity = Identity { digest: *digest, policy: admission::POLICY_VERSION };
    stage(hook, 0);
    let existed = match probe(source, &identity, lender, store, hook) {
        Ok((true, _)) => { result(hook, 0, Status::Ok, true); return Ok(Indexed { digest: *digest, verified_now: false, reopened: false }); }
        Ok((false, existed)) => existed,
        Err(status) => { result(hook, 3, status, true); return Err(status); }
    };
    let verified_now = if let Verification::Pending(finish) = verification {
        if let Err(status) = lender.with_parse(|scratch| finish(source, scratch)).and_then(|result| result) {
            result(hook, 3, status, true);
            return Err(status);
        }
        true
    } else { false };
    let reopened = attempt(source, digest, identity, storage, lender, backend, store, hook, existed)?;
    Ok(Indexed { digest: *digest, verified_now: verified_now && !reopened, reopened })
}

// Out of line so the work state and the build's frames are not on the stack
// during the probe, and the probe's are not on it during the build.
#[inline(never)]
fn attempt<B: ParserBackend + ?Sized, C: CachePresence>(source: &mut dyn ReleasableSource, digest: &[u8; 32],
    identity: Identity, storage: &mut Storage<'_>, lender: &mut impl BuildLoan, backend: &mut B,
    store: &mut C, hook: Yield<'_>, existed: bool) -> Result<bool, Status> {
    store.with_index(digest, |store| {
        build(source, identity, storage, lender, backend, store, hook, existed)
    }).unwrap_or_else(|| {
        result(hook, 3, Status::IoError, true);
        Err(Status::IoError)
    })
}

// Header scratch dies before census and the build take the same parse loan.
#[inline(never)]
fn probe(source: &mut dyn BookSource, identity: &Identity, lender: &mut impl BuildLoan,
    store: &mut impl ReadScope, hook: Yield<'_>) -> Result<(bool, bool), Status> {
    let name=*book_index::FileName::new(&identity.digest,book_index::FileKind::Index).as_bytes();
    let existed = store.file_size(&name) >= 0;
    let hit = lender.with_parse(|scratch| {
        store.query_index(&name,&identity.digest, true, scratch, &mut || source.read_at(0, &mut []) == 0, |verified| {
            book_index::Reader::open(verified, identity).map(|index| {
                (index.header().members, index.header().spine)
            })
        })
    })?;
    if let Ok((Some((members, spine)), true)) = hit {
        counts(hook, members, spine);
        return Ok((true, existed));
    }
    Ok((false, existed))
}

fn build<B: ParserBackend + ?Sized>(source: &mut dyn ReleasableSource, identity: Identity,
    storage: &mut Storage<'_>, lender: &mut impl BuildLoan, backend: &mut B,
    store: &mut dyn Store, hook: Yield<'_>, existed: bool) -> Result<bool, Status> {
    if !store.release_pages() { result(hook, 3, Status::IoError, true); return Err(Status::IoError); }
    stage(hook, 1);
    {
        let source = core::cell::RefCell::new(&mut *source);
        store.census(&|| {
            if let Some(hook) = hook { hook.yield_now(); }
            source.borrow_mut().read_at(0, &mut []) != 0
        });
    }
    let mut store = ObservedStore { inner: store, hook, work_removed: true };
    let built = storage.with_loan(lender, |phase| {
        phase.build_index(source, backend, identity, &mut store, hook)
    });
    store.remove_work();
    let removed = store.work_removed;
    store.release_pages();
    match built {
        Ok((true, work_removed, reopened)) => {
            result(hook, if existed { 2 } else { 1 }, Status::Ok, removed && work_removed);
            Ok(reopened)
        }
        Ok((false, _, _)) => { result(hook, 4, Status::Ok, removed); Err(Status::Unsupported) }
        Err(status) => { result(hook, 3, status, removed); Err(status) }
    }
}
pub(crate) fn stage(hook: Yield<'_>, stage: i32) {
    #[cfg(feature = "dev-usb")]
    admission::emit_to(hook, None, admission::WorkKind::IndexStage, stage);
    #[cfg(not(feature = "dev-usb"))]
    let _ = (hook, stage);
}
pub(crate) fn result(hook: Yield<'_>, outcome: i32, status: Status, removed: bool) {
    #[cfg(feature = "dev-usb")]
    {
        let mut event = admission::WorkEvent::new(admission::WorkKind::IndexResult, outcome);
        event.compressed_bytes = status as u32;
        event.method = u8::from(removed);
        admission::observe(hook, None, event);
    }
    #[cfg(not(feature = "dev-usb"))]
    let _ = (hook, outcome, status, removed);
}


struct ObservedStore<'a, 'h> { inner: &'a mut dyn Store, hook: Yield<'h>, work_removed: bool }
impl Store for ObservedStore<'_, '_> {
    fn census(&mut self, cancelled: &dyn Fn() -> bool) { self.inner.census(cancelled); }
    fn spilled(&self) -> bool { self.inner.spilled() }
    fn release_pages(&mut self) -> bool { self.inner.release_pages() }
}
impl WorkFile for ObservedStore<'_, '_> {
    fn append(&mut self, bytes: &[u8]) -> bool {
        let appended = self.inner.append(bytes);
        if appended { io(self.hook, bytes.len(), 0); }
        appended
    }
    fn read_at(&mut self, at: u32, out: &mut [u8]) -> bool {
        let read = WorkFile::read_at(self.inner, at, out);
        if read { io(self.hook, 0, out.len()); }
        read
    }
    fn remove_work(&mut self) -> bool {
        let removed = self.inner.remove_work();
        self.work_removed &= removed;
        removed
    }
    fn sync_work(&mut self) -> bool { self.inner.sync_work() }
}
impl CacheStorage for ObservedStore<'_, '_> {
    fn remove(&mut self, name: &[u8]) -> bool { self.inner.remove(name) }
    fn file_size(&mut self, name: &[u8]) -> i64 { self.inner.file_size(name) }
    fn read_at(&mut self, name: &[u8], at: u32, out: &mut [u8]) -> i32 { CacheStorage::read_at(self.inner, name, at, out) }
    fn begin_write(&mut self, name: &[u8]) -> bool { self.inner.begin_write(name) }
    fn write(&mut self, bytes: &[u8]) -> bool { self.inner.write(bytes) }
    fn end_write(&mut self) -> bool { self.inner.end_write() }
    fn end_write_checked(&mut self, live: &mut dyn FnMut() -> bool) -> bool {
        stage(self.hook, 6);
        let committed = self.inner.end_write_checked(live);
        stage(self.hook, 0);
        committed
    }
    fn read_back_at(&mut self, at: u32, out: &mut [u8]) -> i32 { self.inner.read_back_at(at, out) }
}
fn io(hook: Yield<'_>, written: usize, read: usize) {
    #[cfg(feature = "dev-usb")]
    {
        let mut event = admission::WorkEvent::new(admission::WorkKind::IndexIo, 0);
        event.compressed_bytes = u32::try_from(written).unwrap_or(u32::MAX);
        event.expanded_bytes = u32::try_from(read).unwrap_or(u32::MAX);
        admission::observe(hook, None, event);
    }
    #[cfg(not(feature = "dev-usb"))]
    let _ = (hook, written, read);
}

#[cfg(test)]
#[path = "index_build_cases.rs"]
mod tests;

pub(crate) fn counts(hook: Yield<'_>, members: u32, spine: u32) {
    #[cfg(feature = "dev-usb")]
    {
        let mut event = admission::WorkEvent::new(admission::WorkKind::IndexCounts, 0);
        event.compressed_bytes = members;
        event.expanded_bytes = spine;
        admission::observe(hook, None, event);
    }
    #[cfg(not(feature = "dev-usb"))]
    let _ = (hook, members, spine);
}
