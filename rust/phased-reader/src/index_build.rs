//! Best-effort first-open index construction; reading still uses canonical metadata.
#![forbid(unsafe_code)]
use newt_core::{admission::{self, Yield}, book_index::{self, Identity},
    book_index_build::WorkFile, page_cache::CacheStorage, xml_session::ParserBackend,
    zip_container::Status, zip_window::source::BookSource};
use crate::phase_storage::{BuildLoan, ReadScope, Storage};
use crate::CachePresence;

/// One attempt, over the same card owner as Pages. No source is retained.
pub trait Store: WorkFile + CacheStorage {
    fn census(&mut self, cancelled: &dyn Fn() -> bool);
    fn spilled(&self) -> bool;
    fn release_pages(&mut self) -> bool;
}

// Additional live adapter state is bounded independently of B2's 2,304-byte
// builder budget; the 32 KiB run and 7,552-byte join state stay in the banks.
const _: () = assert!(core::mem::size_of::<ObservedStore<'static, 'static>>() <= 40);
pub fn open<B: ParserBackend + ?Sized, C: ReadScope + CachePresence>(
    source: &mut dyn BookSource, digest: &[u8; 32],
    storage: &mut Storage<'_>, lender: &mut impl BuildLoan, backend: &mut B,
    store: &mut C, hook: Yield<'_>) {
    let identity = Identity { digest: *digest, policy: admission::POLICY_VERSION };
    stage(hook, 0);
    let existed = match probe(source, &identity, lender, store, hook) {
        Ok((true, _)) => { result(hook, 0, Status::Ok, true); return; }
        Ok((false, existed)) => existed,
        Err(status) => { result(hook, 3, status, true); return; }
    };
    if store.with_index(digest, |store| {
        build(source, identity, storage, lender, backend, store, hook, existed)
    }).is_none() {
        result(hook, 3, Status::IoError, true);
    }
}

// Header scratch dies before census and the build take the same parse loan.
#[inline(never)]
fn probe(source: &mut dyn BookSource, identity: &Identity, lender: &mut impl BuildLoan,
    store: &mut impl ReadScope, hook: Yield<'_>) -> Result<(bool, bool), Status> {
    let mut name = [0; 69];
    book_index::name(&identity.digest, &mut name);
    let existed = store.file_size(&name) >= 0;
    let hit = lender.with_parse(|scratch| {
        store.query(&name, true, scratch, &mut || source.read_at(0, &mut []) == 0, |verified| {
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

fn build<B: ParserBackend + ?Sized>(source: &mut dyn BookSource, identity: Identity,
    storage: &mut Storage<'_>, lender: &mut impl BuildLoan, backend: &mut B,
    store: &mut dyn Store, hook: Yield<'_>, existed: bool) {
    if !store.release_pages() { result(hook, 3, Status::IoError, true); return; }
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
        Ok((true, work_removed)) => result(hook, if existed { 2 } else { 1 }, Status::Ok, removed && work_removed),
        Ok((false, _)) => result(hook, 4, Status::Ok, removed),
        Err(status) => result(hook, 3, status, removed),
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
