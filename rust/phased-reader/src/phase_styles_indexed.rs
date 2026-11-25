//! Style lookup runs after the chapter scanner releases its source.
use super::*;
use core::cell::RefCell;

impl Phase<'_> {
    // Style parser workspace is not live during index selection or TOC binding.
    #[inline(never)]
    pub(crate) fn styles_indexed<B: ParserBackend + ?Sized, C: ReadScope>(
        &mut self, source: &mut dyn BookSource, backend: &mut B, store: &mut C,
        digest: &[u8; 32], index: &newt_core::book_index::Reader, selected: &crate::Selection,
        hook: newt_core::admission::Yield<'_>,
    ) -> Result<Status, Status> {
        let base = self.shared.as_ptr() as usize;
        let (shared, _) = top::<Home>(self.shared)?;
        let mut shared = Parts(shared);
        let query = shared.bytes(CONTENTS_QUERY_BYTES, 16)?;
        let scan = shared.fixed::<4099>()?;
        let name = shared.fixed::<512>()?;
        let directory = shared.fixed::<512>()?;
        let text = shared.fixed::<{styles::EMBEDDED}>()?;
        let (parse, record_bytes) = top::<SourceRecord>(self.parse)?;
        let record = Parts(record_bytes).one(SourceRecord::default())?;
        record.capacity = text.len();
        record.offset = text.as_ptr() as usize - base;
        let mut zip = newt_core::zip_container::Directory::default();
        let status = newt_core::zip_container::locate(source, scan, &mut zip);
        if status != Status::Ok { return Ok(status); }
        let access = RefCell::new((store, query, name));
        let lookup = |source: &mut dyn BookSource, path: &[u8]| {
            let mut access = access.borrow_mut();
            let (store, query, name) = &mut *access;
            let member = crate::reader::query_index_store(source, *store, digest, query, |store| {
                index.find(store, newt_core::book_index::name_hash(path)).map(|hit| hit.map(|(_, member)| member))
                    .map_err(|_| Status::IoError)
            })?;
            let Some(member) = member else {
                return Ok(styles::Member::unresolved(styles::Link::Unsupported));
            };
            let checked = newt_core::member_check::check_indexed(source, &member, Some(path), &mut **name, zip.offset)?;
            Ok(if member.stylesheet() { styles::Member::from_checked(&checked) }
                else { styles::Member::unresolved(styles::Link::Unsupported) })
        };
        let mut parts = Parts(parse);
        let mut r = io(&mut parts, backend)?;
        let mut paths = Arena::new(parts.0);
        Ok(record.with_text(text, |sources| styles::resolve_indexed(&lookup, source,
            &selected.entry, Some(selected.href()), r.backend, styles::ResolveScratch {
                directory, paths: &mut paths, decoder: r.decoder.take(),
                xml: crate::xml_session::Resources { buffer: r.xml_buffer.take(), filter: r.filter.take() },
            }, hook, sources, self.policy)))
    }
}
