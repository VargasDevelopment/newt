//! CanonicalBook: catalog, encryption, container, OPF. Navigation selection is
//! returned; admission expands a present selection, never parses optional TOC.
use crate::admission::{self as admission, Observer, WorkKind};
use crate::member_check::{self, Checked};
use crate::package_parsers::{self as package, ManifestItem, PackageResult, Workflow};
use crate::xml_session::{Attributes, Handler, ParserBackend};
use crate::zip_container::{Catalog, Entry, InflateScratch, Status, hash_path};
use crate::zip_window::{arena::{Arena, CStringRef}, source::BookSource};

pub struct Resources<'a, B: ParserBackend + ?Sized> {
    pub policy: Option<&'a dyn crate::xml_session::SourcePolicy>,
    pub scan: &'a mut [u8; 4099],
    pub sequential: &'a mut [u8; 1024],
    pub entries: &'a mut [Entry],
    pub manifest: &'a mut [ManifestItem],
    pub spine_hashes: &'a mut [u32],
    pub spine: &'a mut [u16],
    pub backend: &'a mut B,
    pub decoder: Option<InflateScratch<'a>>,
    pub chunk: Option<&'a mut [u8; 2048]>,
    pub xml_buffer: Option<&'a mut [u8; 2048]>,
    pub filter: Option<&'a mut crate::xml_input::EntityFilter>,
}
/// Scratch only; metadata backing is requested at the actual parser boundaries.
/// Callbacks return initialized, disjoint slices valid for the returned Book.
pub struct OpenResources<'a, B: ParserBackend + ?Sized> {
    pub policy: Option<&'a dyn crate::xml_session::SourcePolicy>,
    pub scan: &'a mut [u8; 4099],
    pub sequential: &'a mut [u8; 1024],
    pub backend: &'a mut B,
    pub decoder: Option<InflateScratch<'a>>,
    pub chunk: Option<&'a mut [u8; 2048]>,
    pub xml_buffer: Option<&'a mut [u8; 2048]>,
    pub filter: Option<&'a mut crate::xml_input::EntityFilter>,
}
/// XML/package resources after the ZIP scan ends. Small readers can reuse
/// their same scratch bank instead of retaining dead catalog scan buffers.
pub struct PackageResources<'a, B: ParserBackend + ?Sized> {
    pub policy: Option<&'a dyn crate::xml_session::SourcePolicy>,
    pub backend: &'a mut B,
    pub decoder: Option<InflateScratch<'a>>,
    pub chunk: Option<&'a mut [u8; 2048]>,
    pub xml_buffer: Option<&'a mut [u8; 2048]>,
    pub filter: Option<&'a mut crate::xml_input::EntityFilter>,
}
/// Immutable borrowed catalog view. No reset or allocation capability escapes.
#[derive(Clone, Copy)]
pub struct CatalogView<'a, 'storage> {
    entries: &'a [Entry],
    arena: &'a Arena<'storage>,
    directory_offset: u32,
}
impl<'a, 'storage> CatalogView<'a, 'storage> {
    pub fn new(entries: &'a [Entry], arena: &'a Arena<'storage>, directory_offset: u32) -> Self {
        Self { entries, arena, directory_offset }
    }
    /// Check a found entry's local header before anything reads it. `find`
    /// only answers whether a name exists; reads take a [`Checked`] from here.
    pub fn check<S: BookSource + ?Sized>(&self, source: &mut S, entry: &Entry) -> Result<Checked, Status> {
        let name = self.name(entry).ok_or(Status::Damaged)?;
        member_check::check(source, entry, name, self.directory_offset).map(|(member, _)| member)
    }
    /// `find` then `check`: a missing member is `NotFound`.
    pub fn member<S: BookSource + ?Sized>(&self, source: &mut S, path: &[u8]) -> Result<(usize, Checked), Status> {
        let (index, entry) = self.find(path).ok_or(Status::NotFound)?;
        Ok((index, self.check(source, &entry)?))
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn entry(&self, index: usize) -> Option<&'a Entry> {
        self.entries.get(index)
    }
    pub fn name(&self, entry: &Entry) -> Option<&'a [u8]> {
        self.arena.c_string(entry.name?)
    }
    pub fn find(&self, path: &[u8]) -> Option<(usize, Entry)> {
        let path = &path[..path.iter().position(|b| *b == 0).unwrap_or(path.len())];
        let h = hash_path(path);
        self.entries
            .iter()
            .enumerate()
            .find(|(_, e)| e.name_hash == h && self.name(e) == Some(path))
            .map(|(i, e)| (i, *e))
    }
}
struct EncryptionScan {
    encrypted: bool,
}
impl Handler for EncryptionScan {
    fn start_element(&mut self, name: &[u8], atts: &mut dyn Attributes) {
        if !crate::xml_input::local_is(name, b"EncryptionMethod") {
            return;
        }
        // Iterate all qualified Algorithm attributes, as the original does.
        while let Some((key, value)) = atts.next() {
            if crate::xml_input::local_is(key, b"Algorithm")
                && value != b"http://www.idpf.org/2008/embedding"
                && value != b"http://ns.adobe.com/pdf/enc#RC"
            {
                self.encrypted = true;
            }
        }
    }
}
pub struct BookParts<'a, 'storage, S: BookSource + ?Sized, B: ParserBackend + ?Sized> {
    pub workflow: Workflow<'a, S, B>,
    pub arena: &'a mut Arena<'storage>,
    pub entries: &'a [Entry],
    pub manifest: &'a mut [ManifestItem],
    pub spine: &'a mut [u16],
    pub chunk: Option<&'a mut [u8; 2048]>,
    pub result: PackageResult,
    pub directory_offset: u32,
}
pub struct Book<'a, 'storage, S: BookSource + ?Sized, B: ParserBackend + ?Sized> {
    workflow: Workflow<'a, S, B>,
    arena: &'a mut Arena<'storage>,
    entries: &'a [Entry],
    manifest: &'a mut [ManifestItem],
    spine: &'a mut [u16],
    chunk: Option<&'a mut [u8; 2048]>,
    result: PackageResult,
    directory_offset: u32,
}
impl<'a, 'storage, S: BookSource + ?Sized, B: ParserBackend + ?Sized> Book<'a, 'storage, S, B> {
    /// End the Book borrow without losing its concrete metadata or workflow.
    pub fn into_parts(self) -> BookParts<'a, 'storage, S, B> {
        BookParts {
            workflow: self.workflow,
            arena: self.arena,
            entries: self.entries,
            manifest: self.manifest,
            spine: self.spine,
            chunk: self.chunk,
            result: self.result,
            directory_offset: self.directory_offset,
        }
    }
    pub fn package(&self) -> &PackageResult {
        &self.result
    }
    pub fn arena(&self) -> &Arena<'_> {
        self.arena
    }
    pub fn catalog(&self) -> CatalogView<'_, '_> {
        CatalogView::new(self.entries, self.arena, self.directory_offset)
    }
    /// [`CatalogView::check`] against this book's own source.
    pub fn check(&mut self, entry: &Entry) -> Result<Checked, Status> {
        CatalogView::new(self.entries, self.arena, self.directory_offset)
            .check(self.workflow.source, entry)
    }
    /// [`CatalogView::member`] against this book's own source.
    pub fn member(&mut self, path: &[u8]) -> Result<(usize, Checked), Status> {
        CatalogView::new(self.entries, self.arena, self.directory_offset)
            .member(self.workflow.source, path)
    }
    /// [`Book::member`] for a path held in this book's arena (an OPF href).
    pub fn member_at(&mut self, path: CStringRef) -> Result<(usize, Checked), Status> {
        let path = self.arena.c_string(path).ok_or(Status::NotFound)?;
        CatalogView::new(self.entries, self.arena, self.directory_offset)
            .member(self.workflow.source, path)
    }
    pub fn manifest_item(&self, index: usize) -> Option<&ManifestItem> {
        self.manifest[..self.result.manifest_count].get(index)
    }
    pub fn spine_item(&self, index: usize) -> Option<&ManifestItem> {
        let index = *self.spine[..self.result.spine_count].get(index)? as usize;
        self.manifest_item(index)
    }
    pub fn navigation_item(&self) -> Option<&ManifestItem> {
        self.manifest_item(self.result.nav_index.or(self.result.ncx_index)?)
    }
    pub fn navigation_is_epub3(&self) -> bool {
        self.result.nav_index.is_some()
    }
    fn required(&mut self, index: usize, member: &Checked, on_yield: crate::admission::Yield<'_>, observer: Observer) -> Status {
        let Some(name) = CatalogView::new(self.entries, self.arena, self.directory_offset).name(&self.entries[index]) else {
            return Status::Unsupported;
        };
        let decoder = self.workflow.decoder.as_mut().map(|s| InflateScratch {
            state: &mut *s.state,
            dictionary: &mut *s.dictionary,
            input: &mut *s.input,
        });
        admission::expand(
            self.workflow.source,
            member,
            index as u16,
            name,
            decoder,
            self.chunk.as_deref_mut(),
            on_yield,
            observer,
        )
    }
    pub fn validate_selection(mut self, on_yield: crate::admission::Yield<'_>, observer: Observer) -> Status {
        if let Some(href) = self.navigation_item().and_then(|m| m.href) {
            match self.member_at(href) {
                Err(Status::NotFound) => {}
                Err(status) => return status,
                Ok((index, member)) => {
                    let status = self.required(index, &member, on_yield, observer);
                    if status != Status::Ok {
                        return status;
                    }
                }
            }
        }
        for i in 0..self.result.spine_count {
            let href = self
                .spine_item(i)
                .and_then(|m| m.href)
                .and_then(|h| self.arena.c_string(h));
            let Some(href) = href else {
                return Status::NotFound;
            };
            if self.catalog().find(href).is_none() {
                return Status::NotFound;
            }
            if !crate::phased_metadata::admissible_href(href, 512) {
                return Status::Unsupported;
            }
        }
        Status::Ok
    }
    fn open_package<'scratch>(
        &mut self,
        required: bool,
        on_yield: crate::admission::Yield<'_>,
        observer: Observer,
        allocate_manifest: impl FnOnce(
            &mut Arena<'storage>,
            usize,
            usize,
        )
            -> Result<(&'a mut [ManifestItem], &'scratch mut [u32]), Status>,
        allocate_spine: impl FnOnce(&mut Arena<'storage>, usize) -> Result<&'a mut [u16], Status>,
    ) -> Status {
        match self.member(b"META-INF/encryption.xml") {
            Err(Status::NotFound) => {}
            Err(status) => return status,
            Ok((index, member)) => {
                if required {
                    let s = self.required(index, &member, on_yield, observer);
                    if s != Status::Ok {
                        return s;
                    }
                }
                let mut scan = EncryptionScan { encrypted: false };
                let status = self.workflow.pass(&member, &mut scan);
                if status != Status::Ok || scan.encrypted {
                    return Status::Encrypted;
                }
            }
        }
        let (index, member) = match self.member(b"META-INF/container.xml") {
            Ok(found) => found,
            Err(Status::NotFound) => return Status::NotEpub,
            Err(status) => return status,
        };
        if required {
            let s = self.required(index, &member, on_yield, observer);
            if s != Status::Ok {
                return s;
            }
        }
        let mut path = None;
        let status = package::parse_container(&mut self.workflow, &member, self.arena, &mut path);
        if status != Status::Ok {
            return status;
        }
        let (index, member) = match path.map(|p| self.member_at(p)) {
            Some(Ok(found)) => found,
            None | Some(Err(Status::NotFound)) => return Status::NotEpub,
            Some(Err(status)) => return status,
        };
        if required {
            let s = self.required(index, &member, on_yield, observer);
            if s != Status::Ok {
                return s;
            }
        }
        let Some(path) = path.and_then(|p| self.arena.c_string(p)) else {
            return Status::NotEpub;
        };
        let mut directory = [0; 512];
        if !crate::xml_input::dir_name(path, &mut directory) {
            return Status::Unsupported;
        }
        let len = directory
            .iter()
            .position(|b| *b == 0)
            .unwrap_or(directory.len());
        match package::parse_package_allocated(
            &mut self.workflow,
            &member,
            &directory[..len],
            self.arena,
            allocate_manifest,
            allocate_spine,
            &mut self.result,
        ) {
            Ok(storage) => {
                self.manifest = storage.manifest;
                self.spine = storage.spine;
                Status::Ok
            }
            Err(status) => status,
        }
    }
}
/// Returned book holds exclusive arena borrow; its read-only views cannot reset
/// or allocate. Failure leaves arena allocations as original open does; admit
/// wraps this and resets both arenas on every result.
pub fn open<'a, 'storage, S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    source: &'a mut S,
    arena: &'a mut Arena<'storage>,
    r: &'a mut Resources<'_, B>,
    required: bool,
    on_yield: crate::admission::Yield<'_>,
    observer: Observer,
) -> Result<Book<'a, 'storage, S, B>, Status> {
    let entries = &mut *r.entries;
    let manifest = &mut *r.manifest;
    let hashes = &mut *r.spine_hashes;
    let spine = &mut *r.spine;
    open_allocated(
        source,
        arena,
        OpenResources {
                policy: r.policy,
            scan: r.scan,
            sequential: r.sequential,
            backend: r.backend,
            decoder: r.decoder.as_mut().map(|s| InflateScratch {
                state: &mut *s.state,
                dictionary: &mut *s.dictionary,
                input: &mut *s.input,
            }),
            chunk: r.chunk.as_deref_mut(),
            xml_buffer: r.xml_buffer.as_deref_mut(),
            filter: r.filter.as_deref_mut(),
        },
        required,
        on_yield,
        observer,
        |_, _| Ok(entries),
        |_, _, _| Ok((manifest, hashes)),
        |_, _| Ok(spine),
    )
}

/// Counted opening returns the same Book and runs the same selection/admission
/// policy as open. No Book is published until package filling/resolution succeeds.
/// Allocation failure retains partial arena writes, matching the slice route.
#[allow(clippy::too_many_arguments)]
pub fn open_allocated<'a, 'scratch, 'storage, S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    source: &'a mut S,
    arena: &'a mut Arena<'storage>,
    r: OpenResources<'a, B>,
    required: bool,
    on_yield: crate::admission::Yield<'_>,
    observer: Observer,
    allocate_entries: impl FnOnce(&mut Arena<'storage>, usize) -> Result<&'a mut [Entry], Status>,
    allocate_manifest: impl FnOnce(
        &mut Arena<'storage>,
        usize,
        usize,
    ) -> Result<(&'a mut [ManifestItem], &'scratch mut [u32]), Status>,
    allocate_spine: impl FnOnce(&mut Arena<'storage>, usize) -> Result<&'a mut [u16], Status>,
) -> Result<Book<'a, 'storage, S, B>, Status> {
    if required {
        admission::emit_to(on_yield, observer, WorkKind::CatalogBegin, 0);
    }
    let mut catalog = Catalog::new(source, arena, &mut []);
    let status = crate::package_parsers::metadata_capacity_error(
        catalog.open_allocated(r.scan, r.sequential, allocate_entries));
    if status != Status::Ok {
        if required {
            admission::emit_to(on_yield, observer, WorkKind::CatalogEnd, status as i32);
        }
        return Err(status);
    }
    let directory_offset = catalog.directory_offset();
    let (source, arena, entries) = catalog.into_parts();
    if required { admission::emit_to(on_yield, observer, WorkKind::CatalogEnd, 0); }
    open_catalog((source, arena, entries, directory_offset), PackageResources {
        policy: r.policy, backend: r.backend, decoder: r.decoder, chunk: r.chunk,
        xml_buffer: r.xml_buffer, filter: r.filter,
    }, required, on_yield, observer, allocate_manifest, allocate_spine)
}

/// Continue the same open after the actual catalog scan loan ends. The source,
/// arena, entries and directory offset are that catalog's unchanged result; the
/// package passes and counted allocation boundaries stay shared with open_allocated.
pub fn open_catalog<'a, 'scratch, 'storage, S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    (source, arena, entries, directory_offset): (&'a mut S, &'a mut Arena<'storage>, &'a mut [Entry], u32),
    mut r: PackageResources<'a, B>, required: bool,
    on_yield: crate::admission::Yield<'_>, observer: Observer,
    allocate_manifest: impl FnOnce(&mut Arena<'storage>, usize, usize)
        -> Result<(&'a mut [ManifestItem], &'scratch mut [u32]), Status>,
    allocate_spine: impl FnOnce(&mut Arena<'storage>, usize) -> Result<&'a mut [u16], Status>,
) -> Result<Book<'a, 'storage, S, B>, Status> {
    if required {
        // The OPF inflate has not started, so its idle dictionary holds the order.
        let Some(order) = r.decoder.as_mut().and_then(|d| d.dictionary.get_mut(..2 * entries.len())) else {
            return Err(Status::OutOfMemory);
        };
        admission::emit_to(on_yield, observer, WorkKind::CompareBegin, 0);
        let (duplicate, comparisons) =
            duplicate_names(CatalogView::new(entries, arena, directory_offset), order);
        admission::emit_to(on_yield, observer, WorkKind::CompareEnd, comparisons);
        if duplicate {
            return Err(Status::Unsupported);
        }
    }
    let mut book = Book {
        workflow: Workflow {
                policy: r.policy,
            source,
            backend: r.backend,
            decoder: r.decoder,
            buffer: r.xml_buffer,
            filter: r.filter,
        },
        arena,
        entries,
        manifest: &mut [],
        spine: &mut [],
        chunk: r.chunk,
        result: PackageResult::default(),
        directory_offset,
    };
    let status = book.open_package(
        required,
        on_yield,
        observer,
        allocate_manifest,
        allocate_spine,
    );
    if status == Status::Ok {
        Ok(book)
    } else {
        Err(status)
    }
}

/// Exact duplicate member names, in O(n log n) with no heap. Heapsorts a u16
/// index permutation, kept as byte pairs in `order`, by (name hash, exact name),
/// then compares neighbours. Equal names sort together; the bytes decide, so a
/// hash collision is never a duplicate. Returns (duplicate, comparisons).
fn duplicate_names(view: CatalogView<'_, '_>, order: &mut [u8]) -> (bool, i32) {
    struct Sort<'v, 's, 'o> {
        view: CatalogView<'v, 's>,
        order: &'o mut [u8],
        comparisons: i32,
    }
    impl Sort<'_, '_, '_> {
        fn at(&self, i: usize) -> &Entry {
            &self.view.entries[usize::from(u16::from_le_bytes([self.order[2 * i], self.order[2 * i + 1]]))]
        }
        fn less(&mut self, a: usize, b: usize) -> bool {
            self.comparisons += 1;
            let (a, b) = (self.at(a), self.at(b));
            match a.name_hash.cmp(&b.name_hash) {
                core::cmp::Ordering::Equal => self.view.name(a) < self.view.name(b),
                order => order.is_lt(),
            }
        }
        fn swap(&mut self, a: usize, b: usize) {
            self.order.swap(2 * a, 2 * b);
            self.order.swap(2 * a + 1, 2 * b + 1);
        }
        fn sift_down(&mut self, mut root: usize, end: usize) {
            loop {
                let mut child = 2 * root + 1;
                if child >= end {
                    return;
                }
                if child + 1 < end && self.less(child, child + 1) {
                    child += 1;
                }
                if !self.less(root, child) {
                    return;
                }
                self.swap(root, child);
                root = child;
            }
        }
    }
    let n = view.len();
    debug_assert_eq!(order.len(), 2 * n);
    for (i, pair) in order.chunks_exact_mut(2).enumerate() {
        pair.copy_from_slice(&(i as u16).to_le_bytes());
    }
    let mut sort = Sort { view, order, comparisons: 0 };
    for root in (0..n / 2).rev() {
        sort.sift_down(root, n);
    }
    for end in (1..n).rev() {
        sort.swap(0, end);
        sort.sift_down(0, end);
    }
    for i in 1..n {
        sort.comparisons += 1;
        if sort.view.name(sort.at(i - 1)) == sort.view.name(sort.at(i)) {
            return (true, sort.comparisons);
        }
    }
    (false, sort.comparisons)
}

include!("canonical_semantic.rs");
