//! Optional TOC parsing releases inflate scratch before binding the index map.
use super::*;
use newt_core::{book_index::{self, Reader as BookIndex}, package_parsers::{self as package, Workflow, TocKind, TocBacking}};
use newt_reader_navigation::{Navigation, Section, ProgressInputs, index_view::IndexedBook, index_mapping};

impl Phase<'_> {
    pub fn navigation_indexed<B: ParserBackend + ?Sized, C: ReadScope>(
        &mut self, source: &mut dyn BookSource, backend: &mut B, store: &mut C, digest: &[u8; 32],
        index: &BookIndex, progress: ProgressInputs, navigation: &mut Navigation,
        menu: Option<(Section<'_>, u32)>, hook: newt_core::admission::Yield<'_>,
    ) -> Result<(), Status> {
        // Neither the TOC nor its mapping survives this callback. The retained
        // projection owns its labels and section identities.
        *self.held = false;
        let (shared, _) = top::<Home>(self.shared)?;
        let mut shared = Parts(shared);
        let directory = shared.fixed::<512>()?;
        let path = shared.fixed::<512>()?;
        let scan = shared.fixed::<4099>()?;
        let mut arena = Arena::new(shared.0);
        let nav = index.header().nav.min(index.header().ncx);
        let checked = if nav == book_index::NONE { Err(Status::NotFound) } else {
            let member = crate::reader::query_book_index(source, store, digest, self.parse, |index, store| {
                index.member(store, nav).ok_or(Status::IoError)
            })?;
            let mut zip = newt_core::zip_container::Directory::default();
            let status = newt_core::zip_container::locate(source, scan, &mut zip);
            if status != Status::Ok { return Err(status); }
            newt_core::member_check::check_indexed(source, &member, None, path, zip.offset)
        };
        let kind = if index.header().nav != book_index::NONE { TocKind::Nav } else { TocKind::Ncx };
        let count = parse_toc(self.parse, source, backend, &checked, path, directory,
            &mut arena, kind, None, hook, self.policy);
        let result = match count {
            Ok(count) => ScopedToc.with_toc(&mut arena, count, |arena, toc| {
                let filled = parse_toc(self.parse, source, backend, &checked, path, directory,
                    arena, kind, Some(toc), hook, self.policy);
                filled.and_then(|used| bind_indexed(self.parse, source, store, digest,
                    index, progress, arena, &toc[..used], navigation, menu, hook))
            }).and_then(|result| result),
            Err(status) => {
                Err(status)
            }
        };
        match result {
            Ok(()) => Ok(()),
            Err(status) if crate::reader::optional_toc_ends_open(status) => Err(status),
            Err(status) => {
                bind_indexed(self.parse, source, store, digest, index, progress,
                    &arena, &[], navigation, menu, hook)?;
                if menu.is_some() {
                    navigation.set_contents_fallback(crate::reader::contents_fallback(
                        index.header().navigation_declared, status));
                }
                Ok(())
            },
        }
    }
}

#[inline(always)]
fn parse_toc<B: ParserBackend + ?Sized>(parse: &mut [u8], source: &mut dyn BookSource,
    backend: &mut B, checked: &Result<Checked, Status>, path: &[u8], directory: &mut [u8; 512],
    arena: &mut Arena<'_>, kind: TocKind, entries: Option<&mut [TocEntry]>,
    hook: newt_core::admission::Yield<'_>, policy: Option<&dyn newt_core::xml_session::SourcePolicy>,
) -> Result<usize, Status> {
    let entry = checked.as_ref().map_err(|error| *error)?;
    if !newt_core::xml_input::dir_name(path, directory) { return Err(Status::Unsupported); }
    let mut parts = Parts(parse);
    let mut r = io(&mut parts, backend)?;
    let mut scheduled = crate::admission::ScheduledSource::new(source, hook, policy);
    let mut workflow = Workflow { source: &mut scheduled, backend: r.backend,
        decoder: r.decoder.take(), buffer: r.xml_buffer.take(), filter: r.filter.take(), policy };
    let dir = newt_core::xml_input::c_bytes(directory);
    match entries {
        Some(entries) => package::fill_toc(&mut workflow, entry, dir, arena, kind, entries),
        None => package::count_toc(&mut workflow, entry, dir, arena, kind),
    }
}

fn bind_indexed<C: ReadScope>(parse: &mut [u8], source: &mut dyn BookSource,
    store: &mut C, digest: &[u8; 32], index: &BookIndex, progress: ProgressInputs,
    arena: &Arena<'_>, toc: &[TocEntry], navigation: &mut Navigation,
    menu: Option<(Section<'_>, u32)>, hook: newt_core::admission::Yield<'_>,
) -> Result<(), Status> {
    let mut parts = Parts(parse);
    let query = parts.bytes(CONTENTS_QUERY_BYTES, 16)?;
    // At the chapter-change bind, 2000 * (12-byte rows + 4-byte map) and
    // the 8256-byte verified query leave at least 8 KiB in the parse bank.
    const _: () = assert!(CONTENTS_BATCH_ROWS * (12 + 4) + CONTENTS_QUERY_BYTES + 8192 <= BANK_BYTES);
    let failed = core::cell::Cell::new(None);
    let access = core::cell::RefCell::new((source, store, query));
    let lookup = |href: &[u8]| {
        if failed.get().is_some() { return book_index::NONE; }
        let mut access = access.borrow_mut();
        let (source, store, query) = &mut *access;
        let result = crate::reader::query_book_index(*source, *store, digest, query, |index, store| {
            index.find(store, book_index::name_hash(href)).map(|hit| hit.map_or(book_index::NONE,
                |(_, member)| member.contents_spine)).map_err(|_| Status::IoError)
        });
        match result { Ok(spine) => spine, Err(error) => { failed.set(Some(error)); book_index::NONE } }
    };
    let mut book = IndexedBook { header: index.header(), arena, toc, mapping: None,
        lookup: &lookup, progress };
    let batch = if toc.len() <= CONTENTS_BATCH_ROWS {
        let mapping = parts.array::<u32>(toc.len())?;
        let rows = parts.array::<index_mapping::HashRow>(toc.len())?;
        let mut access = access.borrow_mut();
        let (source, store, query) = &mut *access;
        crate::reader::query_book_index(*source, *store, digest, query, |index, store| {
            index_mapping::declared_spines(&book, index, store, mapping, rows).map_err(|_| Status::IoError)
        })?;
        Some(mapping)
    } else {
        crate::admission::open_route(hook, newt_core::reader_work_profile::ReaderOpenRoute::ContentsPerRow);
        None
    };
    book.mapping = batch.as_deref();
    let bound = if let Some((current, focus)) = menu {
        let mapping = parts.array::<u32>(toc.len())?;
        navigation.bind_menu(&book, current, focus, mapping)
    } else { navigation.bind(&book, progress.index) };
    if let Some(error) = failed.get() { return Err(error); }
    if bound { Ok(()) } else { Err(Status::OutOfMemory) }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::{vec, vec::Vec};
    use newt_core::{admission::{WorkEvent, WorkKind, WorkYield}, book_index::{Header, Identity, Member, SpineItem}, page_cache::CacheStorage};
    struct Source;
    impl BookSource for Source {
        fn size(&self) -> u64 { 1 }
        fn read_at(&mut self, _: u64, out: &mut [u8]) -> i32 { if out.is_empty() { 0 } else { -1 } }
    }
    struct Store { bytes: Vec<u8>, scopes: usize, reads: usize }
    impl CacheStorage for Store {
        fn file_size(&mut self, _: &[u8]) -> i64 { self.bytes.len() as i64 }
        fn read_at(&mut self, _: &[u8], at: u32, out: &mut [u8]) -> i32 {
            self.reads += 1;
            let Some(bytes) = self.bytes.get(at as usize..at as usize + out.len()) else { return -1; };
            out.copy_from_slice(bytes); out.len() as i32
        }
        fn remove(&mut self, _: &[u8]) -> bool { self.bytes.clear(); true }
        fn begin_write(&mut self, _: &[u8]) -> bool { false }
        fn write(&mut self, _: &[u8]) -> bool { false }
        fn end_write(&mut self) -> bool { false }
    }
    impl ReadScope for Store {
        fn query<R>(&mut self, _: &[u8], _: bool, _: &mut [u8], live: &mut dyn FnMut() -> bool,
            read: impl FnOnce(&mut dyn CacheStorage) -> R) -> Result<(R, bool), Status> {
            self.scopes += 1;
            assert!(live());
            let before = self.reads;
            let value = read(self);
            assert!(self.reads - before <= 128);
            Ok((value, live()))
        }
    }
    struct Trace(core::cell::Cell<bool>);
    impl WorkYield for Trace {
        fn yield_now(&self) {}
        fn observe(&self, event: WorkEvent) {
            if event.kind == WorkKind::OpenRoute && event.result == newt_core::reader_work_profile::ReaderOpenRoute::ContentsPerRow as i32 {
                self.0.set(true);
            }
        }
    }
    fn fixture() -> (Identity, Store, BookIndex) {
        let identity = Identity { digest: [1; 32], policy: crate::admission::POLICY_VERSION };
        let mut title = [0; book_index::TITLE_BYTES]; title[0] = b'T';
        let header = Header { identity, members: 1, spine: 1, spine_bytes: 1, nav: book_index::NONE,
            ncx: book_index::NONE, cover: book_index::NONE, language: Default::default(), stylesheets: false, navigation_declared: false, title };
        let mut store = Store { bytes: vec![0; header.payload_bytes().unwrap() as usize], scopes: 0, reads: 0 };
        header.encode((&mut store.bytes[..384]).try_into().unwrap());
        Member { hash: book_index::name_hash(b"c"), contents_spine: 0, ..Default::default() }
            .encode((&mut store.bytes[384..416]).try_into().unwrap());
        SpineItem { member: 0, before: 0 }.encode((&mut store.bytes[416..]).try_into().unwrap());
        let index = BookIndex::open(&mut store, &identity).unwrap();
        (identity, store, index)
    }
    #[test]
    fn chapter_bind_at_the_bound_batches_and_above_it_preserves_rows_and_marks_the_fallback() {
        let (identity, mut store, index) = fixture();
        let mut bytes = [0; 32]; let mut arena = Arena::new(&mut bytes);
        let entry = TocEntry { title: arena.copy_nul_ref(Some(b"Chapter")), href: arena.copy_nul_ref(Some(b"c")), ..Default::default() };
        let mut bank = vec![0; BANK_BYTES];
        for count in [CONTENTS_BATCH_ROWS, CONTENTS_BATCH_ROWS + 1] {
            let trace = Trace(core::cell::Cell::new(false));
            let mut navigation = Navigation::default();
            store.scopes = 0;
            bind_indexed(&mut bank, &mut Source, &mut store, &identity.digest, &index,
                ProgressInputs { index: 0, count: 1, total: 1, current: 1, valid: true, ..Default::default() },
                &arena, &vec![entry; count], &mut navigation, None, Some(&trace)).unwrap();
            assert_eq!(navigation.section(0).spine, 0);
            if count <= CONTENTS_BATCH_ROWS { assert_eq!(store.scopes, 1); }
            else { assert!(store.scopes >= count); }
            assert_eq!(trace.0.get(), cfg!(feature = "dev-usb") && count > CONTENTS_BATCH_ROWS);
        }
    }
    #[test]
    fn consumed_index_damage_removes_the_header_hit_for_the_next_open() {
        let (identity, mut store, _) = fixture();
        store.bytes[416..420].copy_from_slice(&2u32.to_le_bytes());
        assert!(BookIndex::open(&mut store, &identity).is_some(), "header alone still reads");
        let result = crate::reader::query_book_index(&mut Source, &mut store, &identity.digest,
            &mut [0; CONTENTS_QUERY_BYTES], |index, store| index.spine(store, 0).ok_or(Status::Damaged));
        assert_eq!(result, Err(Status::Damaged));
        assert!(BookIndex::open(&mut store, &identity).is_none(), "index-recovery: next open must rebuild");
    }

}
