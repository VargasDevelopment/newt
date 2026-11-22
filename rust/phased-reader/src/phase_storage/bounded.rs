// SPDX-License-Identifier: MIT
//! Typed metadata remains in the sample's existing 8 KB bank between loans.
//! Only private offsets survive; the owning sample allocation binds them to
//! their unchanged, initialized bank. No source or parser loan is retained.
use super::{Parts, tail_array};
use crate::{bounded::{self, BOOK_BYTES, PARSE_BYTES}, canonical, package_parsers::{ManifestItem,
        PackageResult, TocEntry}, xml_session::ParserBackend, zip_container::{Catalog, Entry, Status},
    zip_window::{arena::{Arena, ArenaState}, source::BookSource}};
use core::{marker::PhantomData, mem::{align_of, size_of}};
use newt_reader_navigation::metadata::Metadata;

struct Span<T> { offset: usize, count: usize, kind: PhantomData<T> }
impl<T> Span<T> {
    fn new() -> Self { Self { offset: 0, count: 0, kind: PhantomData } }
    fn keep(&mut self, values: &[T], base: usize) {
        self.count = values.len();
        self.offset = if values.is_empty() { 0 } else { values.as_ptr() as usize - base };
    }
    /// # Safety
    /// The unchanged tail must contain this span's original initialized T array.
    unsafe fn get<'a>(&self, tail: &'a [u8], prefix: usize) -> Result<&'a [T], Status> {
        if self.count == 0 { return Ok(&[]); }
        let at = self.offset.checked_sub(prefix).ok_or(Status::Unsupported)?;
        let bytes = self.count.checked_mul(size_of::<T>()).ok_or(Status::Unsupported)?;
        let region = tail.get(at..at.checked_add(bytes).ok_or(Status::Unsupported)?)
            .ok_or(Status::Unsupported)?;
        if region.as_ptr().align_offset(align_of::<T>()) != 0 { return Err(Status::Unsupported); }
        // SAFETY: the caller binds this private span to its original bank and
        // initialized typed array; extent/alignment are checked before the cast.
        Ok(unsafe { core::slice::from_raw_parts(region.as_ptr().cast::<T>(), self.count) })
    }
}

pub struct MetadataState {
    arena: Option<ArenaState>,
    prefix: usize,
    entries: Span<Entry>,
    manifest: Span<ManifestItem>,
    spine: Span<u16>,
    toc: Span<TocEntry>,
    package: PackageResult,
    book_peak: usize,
}
impl Default for MetadataState {
    fn default() -> Self {
        Self { arena: None, prefix: 0, entries: Span::new(), manifest: Span::new(),
            spine: Span::new(), toc: Span::new(), package: PackageResult::default(), book_peak: 0 }
    }
}
impl MetadataState {
    pub fn book_peak(&self) -> usize { self.book_peak }
    pub fn spine_count(&self) -> u32 { self.package.spine_count as u32 }
    pub fn title<'a>(&self, bank: &'a [u8; BOOK_BYTES]) -> Option<&'a [u8]> {
        self.arena.as_ref()?;
        let at = self.package.metadata.title?.offset();
        let tail = bank.get(at..self.prefix)?;
        Some(&tail[..tail.iter().position(|&byte| byte == 0)?])
    }

    /// The same cheap catalog pass, reset, second catalog and package/TOC parse.
    /// All typed allocations use this book bank; scans and XML use the same
    /// parse bank sequentially, so dead scan bytes do not reduce XML capacity.
    pub fn load<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(&mut self,
        bank: &mut [u8; BOOK_BYTES], parse: &mut [u8; PARSE_BYTES],
        source: &mut S, backend: &mut B,
    ) -> Status {
        *self = Self::default();
        let base = bank.as_ptr() as usize;
        let mut arena = Arena::new(&mut *bank);
        let status = {
            let mut scratch = Parts(&mut *parse);
            let resources = (|| Ok::<_, Status>((scratch.fixed::<4099>()?, scratch.fixed::<1024>()?)))();
            let (scan, sequential) = match resources { Ok(parts) => parts, Err(error) => return error };
            let mut catalog = Catalog::new(source, &mut arena, &mut []);
            bounded::book::preflight(&mut catalog, scan, sequential, |arena, count|
                tail_array::<Entry>(arena, count))
        };
        self.book_peak = arena.high_water() + BOOK_BYTES - arena.capacity();
        if status != Status::Ok { return status; }
        drop(arena);
        let mut arena = Arena::new(&mut *bank);
        let Self { entries, manifest, spine, toc, .. } = self;
        let catalog = {
            let mut scratch = Parts(&mut *parse);
            let resources = (|| Ok::<_, Status>((scratch.fixed::<4099>()?, scratch.fixed::<1024>()?)))();
            let (scan, sequential) = match resources { Ok(parts) => parts, Err(error) => return error };
            let mut catalog = Catalog::new(source, &mut arena, &mut []);
            let status = catalog.open_allocated(scan, sequential, |arena, count| {
                let values = tail_array::<Entry>(arena, count)?;
                entries.keep(values, base);
                Ok(values)
            });
            if status != Status::Ok { return status; }
            catalog.into_parts()
        };
        let mut scratch = Parts(&mut *parse);
        let buffer = match scratch.fixed::<2048>() { Ok(buffer) => buffer, Err(error) => return error };
        let result = canonical::open_catalog(catalog, canonical::PackageResources {
            policy: None, backend, decoder: None, chunk: None, xml_buffer: Some(buffer), filter: None,
        }, false, None, None,
            |arena, count, hashes| {
                let values = tail_array::<ManifestItem>(arena, count)?;
                manifest.keep(values, base);
                Ok((values, scratch.array::<u32>(hashes)?))
            },
            |arena, count| {
                let values = tail_array::<u16>(arena, count)?;
                spine.keep(values, base);
                Ok(values)
            }).and_then(|book| bounded::book::load_toc(book.into_parts(), |arena, count| {
                let values = tail_array::<TocEntry>(arena, count)?;
                toc.keep(values, base);
                Ok(values)
            }));
        let result = match result {
            Ok(parts) => {
                toc.count = parts.toc.len();
                self.prefix = parts.arena.capacity();
                self.book_peak = self.book_peak.max(parts.arena.high_water() + BOOK_BYTES - self.prefix);
                self.package = parts.package;
                self.arena = Some(parts.arena.state());
                Status::Ok
            }
            Err(error) => error,
        };
        result
    }

    /// # Safety
    /// bank must be this state's original bank, with its initialized typed
    /// arrays and allocated strings unchanged since load. Keep state and bank
    /// private in the same owning sample allocation; raw bytes are not a proof.
    pub unsafe fn with_metadata<R>(&mut self, bank: &mut [u8; BOOK_BYTES],
        operation: impl FnOnce(&Metadata<'_, '_>) -> R,
    ) -> Result<R, Status> {
        let state = self.arena.take().ok_or(Status::NotFound)?;
        let (prefix, tail) = bank.split_at_mut(self.prefix);
        let arena = match Arena::from_state(prefix, state) {
            Ok(arena) => arena,
            Err(state) => { self.arena = Some(state); return Err(Status::Unsupported); }
        };
        let returned = ReturnedArena { arena, state: &mut self.arena };
        // SAFETY: the owning sample caller maintains the unchanged-bank
        // contract for every private typed span. All borrows end at operation.
        let (entries, manifest, spine, toc) = unsafe {
            (self.entries.get(tail, self.prefix)?, self.manifest.get(tail, self.prefix)?,
                self.spine.get(tail, self.prefix)?, self.toc.get(tail, self.prefix)?)
        };
        Ok(operation(&Metadata { arena: &returned.arena, entries, manifest, spine, toc,
            package: &self.package }))
    }
}
struct ReturnedArena<'a, 'state> { arena: Arena<'a>, state: &'state mut Option<ArenaState> }
impl Drop for ReturnedArena<'_, '_> {
    fn drop(&mut self) { *self.state = Some(self.arena.state()); }
}
