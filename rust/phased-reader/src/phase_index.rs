use newt_core::zip_window::source::BookSource;
// Included inside phase_storage: all scratch belongs to the one physical loan.
impl Phase<'_> {
    pub(crate) fn build_index<B: ParserBackend + ?Sized>(&mut self,
        source: &mut dyn crate::index_build::ReleasableSource, backend: &mut B,
        identity: newt_core::book_index::Identity, store: &mut dyn crate::index_build::Store,
        hook: newt_core::admission::Yield<'_>,
    ) -> Result<(bool, bool, bool), Status> {
        use newt_core::{admission::ScheduledSource, book_index_build::Phase as Step};
        use crate::index_build::stage;
        // Preserve the resolved style slot at the top of shared. The run buffer
        // uses 32 KiB of its dead metadata prefix; no reader state borrows it.
        let (shared, _) = top::<Home>(self.shared)?;
        let (run, rest) = shared.split_at_mut(32768);
        let mut scheduled = ScheduledSource::new(source, hook, self.policy);
        let live = core::cell::RefCell::new(&mut scheduled);
        let members = core::cell::Cell::new(0u32);
        let spine = core::cell::Cell::new(0u32);
        let released = core::cell::Cell::new(false);
        let mut progress = |p: newt_core::book_index_build::Progress| {
            if p.phase == Step::Central {
                members.set(u32::try_from(p.done + 1).unwrap_or(0));
            } else if p.phase == Step::Rank {
                spine.set(u32::try_from(p.total).unwrap_or(0).saturating_sub(members.get()));
            }
            stage(hook, match p.phase { Step::Central => 2, Step::Package => 3,
                Step::Join | Step::Merge | Step::Rank => 4, Step::Publish => 5 });
            if let Some(hook) = hook { hook.yield_now(); }
            if released.get() { !live.borrow().source.cancelled() }
            else { live.borrow_mut().read_at(0, &mut []) == 0 }
        };
        let (prepared, mut state) = prepare(run, rest, self.parse, self.policy, backend, &live,
            identity, store, &mut progress, hook)?;
        crate::index_build::counts(hook, members.get(), spine.get());
        if store.spilled() {
            stage(hook,7);
            let closed=live.borrow_mut().source.release();
            stage(hook,4);
            if !closed { return Ok((false, true, false)); }
            released.set(true);
        }
        if !store.release_pages() { return Err(Status::IoError); }
        stage(hook, 5);
        let mut receipt = || if released.get() { !live.borrow().source.cancelled() }
            else { live.borrow_mut().read_at(0, &mut []) == 0 };
        let buffer = rest.first_chunk_mut::<{newt_core::book_index_build::PUBLISH_BUFFER_BYTES}>()
            .ok_or(Status::OutOfMemory)?;
        let published = publish(prepared, store, &mut state, buffer, &mut progress, &mut receipt);
        // Publish consumes/removes work before reopening, so even a live
        // receipt leaves room for the source under the three-file mount cap.
        if released.get() { stage(hook,8); }
        let reopened = !released.get() || live.borrow_mut().source.reopen();
        stage(hook,0);
        if !reopened { return Err(Status::IoError); }
        let (members, spine, removed) = published?;
        crate::index_build::counts(hook, members, spine);
        Ok((true, removed, released.get()))
    }
}
/// Out of line so publish's frame, the deepest of the build, is not added to
/// the frame that owns the work state and the source views.
#[inline(never)]
fn publish(prepared: newt_core::book_index_build::Prepared<'_>, store: &mut dyn crate::index_build::Store,
    state: &mut newt_core::book_index_build::JoinState<'_>,
    buffer: &mut [u8; newt_core::book_index_build::PUBLISH_BUFFER_BYTES],
    progress: &mut dyn FnMut(newt_core::book_index_build::Progress) -> bool,
    receipt: &mut dyn FnMut() -> bool,
) -> Result<(u32, u32, bool), Status> {
    let report = prepared.publish_buffered(store, state, buffer, progress, receipt).map_err(index_error)?;
    Ok((report.header.members, report.header.spine, report.work_removed))
}
/// Central pass, container, OPF parse and join. Out of line so the Builder and
/// Parsed values, the scan views and the parse loans leave the stack before
/// publish, whose own frame is the deepest part of the build.
#[inline(never)]
fn prepare<'r, 'p, 's, 'o, B: ParserBackend + ?Sized>(
    run: &'r mut [u8], rest: &mut [u8], parse: &'p mut [u8],
    policy: Option<&dyn newt_core::xml_session::SourcePolicy>, backend: &mut B,
    live: &core::cell::RefCell<&mut newt_core::admission::ScheduledSource<'s, dyn crate::index_build::ReleasableSource + 'o>>,
    identity: newt_core::book_index::Identity, store: &mut dyn crate::index_build::Store,
    progress: &mut dyn FnMut(newt_core::book_index_build::Progress) -> bool,
    hook: newt_core::admission::Yield<'_>,
) -> Result<(newt_core::book_index_build::Prepared<'r>, newt_core::book_index_build::JoinState<'p>), Status> {
    use newt_core::{admission::{self, ScheduledSource}, book_index_build::Builder,
        package_parsers::{self, Workflow}, xml_input};
    use crate::index_build::stage;
    let mut rest = Parts(rest);
    let scan = rest.fixed::<4099>()?;
    let path_bytes = rest.bytes(512, 1)?;
    let directory = rest.fixed::<512>()?;
    let mut builder = Builder::new(run, 512, 32768).map_err(index_error)?;
    stage(hook, 2);
    let mut view = IndexSource(live);
    let mut central = ScheduledSource::new(&mut view as &mut dyn BookSource, None, policy);
    let status = admission::inspect_with_visitor(&mut central,
        &mut Arena::new(&mut *parse), scan, |member| builder.visit(store, member, &mut *progress));
    if status != Status::Ok { return Err(status); }
    stage(hook, 3);
    let container = index_member(&mut **live.borrow_mut(), &mut *parse, scan, b"META-INF/container.xml")?;
    let mut paths = Arena::new(path_bytes);
    let mut path = None;
    {
        let mut parts = Parts(&mut *parse);
        let mut r = io(&mut parts, &mut *backend)?;
        let mut source = live.borrow_mut();
        let mut workflow = Workflow { source: &mut **source as &mut dyn BookSource, backend: r.backend,
            decoder: r.decoder.take(), buffer: r.xml_buffer.take(), filter: r.filter.take(), policy };
        let status = package_parsers::parse_container(&mut workflow, &container, &mut paths, &mut path);
        if status != Status::Ok { return Err(status); }
    }
    let path = path.and_then(|p| paths.c_string(p)).ok_or(Status::NotEpub)?;
    if !xml_input::dir_name(path, directory) { return Err(Status::PathTooLong); }
    let entry = index_member(&mut **live.borrow_mut(), &mut *parse, scan, path)?;
    let parsed = {
        let mut parts = Parts(&mut *parse);
        let mut r = io(&mut parts, &mut *backend)?;
        // Source reads and builder progress alternate. Progress probes the
        // original source, so do not hold a RefCell loan across XML callbacks.
        let mut view = IndexSource(live);
        builder.parse(Workflow { source: &mut view as &mut dyn BookSource, backend: r.backend,
            decoder: r.decoder.take(), buffer: r.xml_buffer.take(), filter: r.filter.take(), policy },
            &entry, xml_input::c_bytes(directory), identity, store, &mut *progress).map_err(index_error)?
    };
    stage(hook, 4);
    let mut state = carve_join_state(&mut Parts(parse))?;
    let prepared = parsed.join(store, &mut state, &mut *progress).map_err(index_error)?;
    Ok((prepared, state))
}
struct IndexSource<'a, 'b, 's, 'o>(&'a core::cell::RefCell<&'b mut newt_core::admission::ScheduledSource<'s, dyn crate::index_build::ReleasableSource + 'o>>);
impl BookSource for IndexSource<'_, '_, '_, '_> {
    fn size(&self) -> u64 { self.0.borrow().size() }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 { self.0.borrow_mut().read_at(at, out) }
}
fn index_error(error: newt_core::book_index_build::Error) -> Status {
    match error { newt_core::book_index_build::Error::Book(status) => status, _ => Status::IoError }
}
fn index_member(
    source: &mut dyn BookSource, parse: &mut [u8],
    scan: &mut [u8; 4099], path: &[u8],
) -> Result<Checked, Status> {
    use newt_core::{admission, book_index, zip_container, member_check};
    let mut found = None;
    let status = admission::inspect_with_visitor(&mut newt_core::admission::ScheduledSource::new(source, None, None), &mut Arena::new(parse), scan, |m| {
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
