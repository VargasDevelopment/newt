use newt_core::zip_window::source::BookSource;
// Included inside phase_storage: all scratch belongs to the one physical loan.
impl Phase<'_> {
    pub(crate) fn build_index<B: ParserBackend + ?Sized>(&mut self,
        source: &mut dyn crate::zip_window::source::BookSource, backend: &mut B,
        identity: newt_core::book_index::Identity, store: &mut dyn crate::index_build::Store,
        hook: newt_core::admission::Yield<'_>,
    ) -> Result<(bool, bool), Status> {
        use newt_core::{admission::{self, ScheduledSource}, book_index_build::{Builder, Phase as Step},
            package_parsers::{self, Workflow}, xml_input};
        use crate::index_build::stage;
        // Preserve the resolved style slot at the top of shared. The run buffer
        // uses 32 KiB of its dead metadata prefix; no reader state borrows it.
        let (shared, _) = top::<Home>(self.shared)?;
        let (run, rest) = shared.split_at_mut(32768);
        let mut rest = Parts(rest);
        let scan = rest.fixed::<4099>()?;
        let path_bytes = rest.bytes(512, 1)?;
        let directory = rest.fixed::<512>()?;
        let mut scheduled = ScheduledSource::new(source, hook, self.policy);
        let live = core::cell::RefCell::new(&mut scheduled);
        let members = core::cell::Cell::new(0u32);
        let spine = core::cell::Cell::new(0u32);
        let mut progress = |p: newt_core::book_index_build::Progress| {
            if p.phase == Step::Central {
                members.set(u32::try_from(p.done + 1).unwrap_or(0));
            } else if p.phase == Step::Rank {
                spine.set(u32::try_from(p.total).unwrap_or(0).saturating_sub(members.get()));
            }
            stage(hook, match p.phase { Step::Central => 2, Step::Package => 3,
                Step::Join | Step::Merge | Step::Rank => 4, Step::Publish => 5 });
            if let Some(hook) = hook { hook.yield_now(); }
            live.borrow_mut().read_at(0, &mut []) == 0
        };
        let mut builder = Builder::new(run, 512, 32768).map_err(index_error)?;
        stage(hook, 2);
        let mut view = IndexSource(&live);
        let mut central = ScheduledSource::new(&mut view, None, self.policy);
        let status = admission::inspect_with_visitor(&mut central,
            &mut Arena::new(self.parse), scan, |member| builder.visit(store, member, &mut progress));
        if status != Status::Ok { return Err(status); }
        stage(hook, 3);
        let container = index_member(&mut **live.borrow_mut(), self.parse, scan, b"META-INF/container.xml")?;
        let mut paths = Arena::new(path_bytes);
        let mut path = None;
        {
            let mut parts = Parts(self.parse);
            let mut r = io(&mut parts, backend)?;
            let mut source = live.borrow_mut();
            let mut workflow = Workflow { source: &mut **source, backend: r.backend,
                decoder: r.decoder.take(), buffer: r.xml_buffer.take(), filter: r.filter.take(), policy: self.policy };
            let status = package_parsers::parse_container(&mut workflow, &container, &mut paths, &mut path);
            if status != Status::Ok { return Err(status); }
        }
        let path = path.and_then(|p| paths.c_string(p)).ok_or(Status::NotEpub)?;
        if !xml_input::dir_name(path, directory) { return Err(Status::PathTooLong); }
        let entry = index_member(&mut **live.borrow_mut(), self.parse, scan, path)?;
        let parsed = {
            let mut parts = Parts(self.parse);
            let mut r = io(&mut parts, backend)?;
            // Source reads and builder progress alternate. Progress probes the
            // original source, so do not hold a RefCell loan across XML callbacks.
            let mut view = IndexSource(&live);
            builder.parse(Workflow { source: &mut view, backend: r.backend,
                decoder: r.decoder.take(), buffer: r.xml_buffer.take(), filter: r.filter.take(), policy: self.policy },
                &entry, xml_input::c_bytes(directory), identity, store, &mut progress).map_err(index_error)?
        };
        stage(hook, 4);
        let mut state = carve_join_state(&mut Parts(self.parse))?;
        let prepared = parsed.join(store, &mut state, &mut progress).map_err(index_error)?;
        crate::index_build::counts(hook, members.get(), spine.get());
        if store.spilled() { return Ok((false, true)); }
        if !store.release_pages() { return Err(Status::IoError); }
        stage(hook, 5);
        let mut receipt = || live.borrow_mut().read_at(0, &mut []) == 0;
        let report = prepared.publish(store, &mut state, &mut progress, &mut receipt).map_err(index_error)?;
        crate::index_build::counts(hook, report.header.members, report.header.spine);
        Ok((true, report.work_removed))
    }
}
struct IndexSource<'a, 'b, S: ?Sized>(&'a core::cell::RefCell<&'b mut S>);
impl<S: crate::zip_window::source::BookSource + ?Sized> crate::zip_window::source::BookSource for IndexSource<'_, '_, S> {
    fn size(&self) -> u64 { self.0.borrow().size() }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 { self.0.borrow_mut().read_at(at, out) }
}
fn index_error(error: newt_core::book_index_build::Error) -> Status {
    match error { newt_core::book_index_build::Error::Book(status) => status, _ => Status::IoError }
}
fn index_member<S: crate::zip_window::source::BookSource + ?Sized>(
    source: &mut newt_core::admission::ScheduledSource<'_, S>, parse: &mut [u8],
    scan: &mut [u8; 4099], path: &[u8],
) -> Result<Checked, Status> {
    use newt_core::{admission, book_index, zip_container, member_check};
    let mut found = None;
    let status = admission::inspect_with_visitor(source, &mut Arena::new(parse), scan, |m| {
        if found.is_none() && m.hash == book_index::name_hash(path) { found = Some(m); }
        Status::Ok
    });
    if status != Status::Ok { return Err(status); }
    let m = found.ok_or(Status::NotFound)?;
    let mut directory = zip_container::Directory::default();
    let status = zip_container::locate(source, scan, &mut directory);
    if status != Status::Ok { return Err(status); }
    member_check::check(source, &Entry { compressed_size: m.compressed, uncompressed_size: m.uncompressed,
        local_header_offset: m.local_offset, method: m.method, flags: m.flags, ..Entry::default() },
        path, directory.offset).map(|(entry, _)| entry)
}


// This adapter adds only scalar owners and short views to B2's stack state.
// The scan, paths, decoder, run and join arrays are all bank-backed.
const INDEX_ADAPTER_STATE_BYTES: usize =
    core::mem::size_of::<newt_core::book_index::Identity>()
    + 2 * core::mem::size_of::<Checked>()
    + core::mem::size_of::<core::cell::RefCell<&mut dyn BookSource>>()
    + 2 * core::mem::size_of::<newt_core::admission::ScheduledSource<'_, dyn BookSource>>()
    + core::mem::size_of::<newt_core::zip_container::Directory>()
    + 2 * core::mem::size_of::<core::cell::Cell<u32>>();
const _: () = assert!(INDEX_ADAPTER_STATE_BYTES <= 256);
const _: () = assert!(newt_core::book_index_build::STACK_BYTES <= 2_304);
