//! Metadata-only Session adapter over the same verified book index as the UI.
use super::Reader;
use crate::{index_build::Indexed, phase_storage::ReadScope, zip_container::Status,
    zip_window::source::BookSource};
use newt_core::{book_index, member_check::Checked};

pub struct MetadataReader<'a, C> {
    store: &'a mut C,
    indexed: &'a Indexed,
    query: &'a mut [u8],
    scan: &'a mut [u8; 4099],
    name: &'a mut [u8; 512],
    spines: u32,
    staged: Option<u32>,
    selected: Option<u32>,
}
impl<'a, C: ReadScope> MetadataReader<'a, C> {
    pub fn new(store: &'a mut C, indexed: &'a Indexed, query: &'a mut [u8],
        scan: &'a mut [u8; 4099], name: &'a mut [u8; 512],
    ) -> Self {
        Self { store, indexed, query, scan, name, spines: 0, staged: None, selected: None }
    }
    pub fn selected(&self) -> Option<u32> { self.selected }
}
impl<C: ReadScope> Reader for MetadataReader<'_, C> {
    fn close(&mut self) { self.spines = 0; self.staged = None; self.selected = None; }
    fn strict_open(&mut self, source: &mut dyn BookSource) -> Status {
        self.close();
        let bytes = source.size();
        if bytes == 0 { return Status::NotZip; }
        if bytes > newt_core::admission::EXTERNAL_MAX_BYTES { return Status::Unsupported; }
        let result = crate::reader::query_book_index(source, self.store, self.indexed.digest(), self.query,
            |index, _| Ok(index.header().spine));
        match result {
            Ok(spines) => {
                let status = crate::reader::validate(source, bytes);
                if status == Status::Ok { self.spines = spines; }
                status
            }
            Err(error) => error,
        }
    }
    fn spine_count(&self) -> u32 { self.spines }
    fn stage(&mut self, source: &mut dyn BookSource, spine: u32, _: bool) -> Result<Checked, Status> {
        self.staged = None;
        let member = crate::reader::query_book_index(source, self.store, self.indexed.digest(), self.query,
            |index, store| {
                if spine >= index.header().spine { return Err(Status::NotFound); }
                let item = index.spine(store, spine).ok_or(Status::IoError)?;
                if item.member == book_index::NONE { return Err(Status::NotFound); }
                index.member(store, item.member).ok_or(Status::IoError)
            })?;
        let mut directory = crate::zip_container::Directory::default();
        let status = crate::zip_container::locate(source, self.scan, &mut directory);
        if status != Status::Ok { return Err(status); }
        let entry = newt_core::member_check::check_indexed(source, &member, None, self.name, directory.offset)?;
        if !newt_core::phased_metadata::admissible_href(crate::xml_input::c_bytes(self.name), self.name.len()) {
            return Err(Status::Unsupported);
        }
        self.staged = Some(spine);
        Ok(entry)
    }
    fn commit(&mut self, spine: u32) {
        if self.staged == Some(spine) { self.selected = self.staged.take(); }
    }
}
