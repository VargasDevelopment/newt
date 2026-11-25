//! Actual canonical EPUB adapter; caller owns all parser, inflater and arena spans.
//! This implements metadata selection, not PhasedReader rendering/page geometry.
use super::Reader;
use crate::{
    admission::{self, Observer, ScheduledSource},
    canonical::{self, Resources},
    xml_session::ParserBackend,
    zip_container::Status,
    zip_window::{arena::Arena, source::BookSource},
};
use newt_core::member_check::Checked;
pub struct MetadataReader<'a, 'storage, 'resources, B: ParserBackend + ?Sized> {
    pub catalog: &'a mut Arena<'storage>,
    pub parse: &'a mut Arena<'storage>,
    pub resources: &'a mut Resources<'resources, B>,
    pub on_yield: newt_core::admission::Yield<'a>,
    pub observer: Observer,
    spines: u32,
    staged: Option<u32>,
    selected: Option<u32>,
}
impl<'a, 'storage, 'resources, B: ParserBackend + ?Sized>
    MetadataReader<'a, 'storage, 'resources, B>
{
    pub fn new(
        catalog: &'a mut Arena<'storage>,
        parse: &'a mut Arena<'storage>,
        resources: &'a mut Resources<'resources, B>,
        on_yield: newt_core::admission::Yield<'a>,
        observer: Observer,
    ) -> Self {
        Self {
            catalog,
            parse,
            resources,
            on_yield,
            observer,
            spines: 0,
            staged: None,
            selected: None,
        }
    }
    pub fn selected(&self) -> Option<u32> {
        self.selected
    }
}
impl<B: ParserBackend + ?Sized> Reader for MetadataReader<'_, '_, '_, B> {
    fn close(&mut self) {
        self.catalog.reset();
        self.parse.reset();
        self.spines = 0;
        self.staged = None;
        self.selected = None;
    }
    fn strict_open(&mut self, source: &mut dyn BookSource) -> Status {
        self.close();
        let mut scheduled = ScheduledSource::new(source, self.on_yield, None);
        let status = admission::inspect(&mut scheduled, self.parse, self.resources.scan);
        self.parse.reset();
        if status != Status::Ok {
            return status;
        }
        let status = match canonical::open(
            &mut scheduled,
            self.catalog,
            self.resources,
            true,
            self.on_yield,
            self.observer,
        ) {
            Ok(book) => {
                let count = book.package().spine_count as u32;
                let status = book.validate_selection(self.on_yield, self.observer);
                if status == Status::Ok {
                    self.spines = count;
                }
                status
            }
            Err(error) => error,
        };
        self.catalog.reset();
        status
    }
    fn spine_count(&self) -> u32 {
        self.spines
    }
    fn stage(
        &mut self,
        source: &mut dyn BookSource,
        spine: u32,
        _plain: bool,
    ) -> Result<Checked, Status> {
        self.staged = None;
        self.catalog.reset();
        // Book cannot survive a source Action withdrawal. Reborrow all its
        // owners and copy only the canonical ZIP descriptor before dropping it.
        let mut scheduled = ScheduledSource::new(source, self.on_yield, None);
        let result = (|| {
            let mut book = canonical::open(
                &mut scheduled,
                self.catalog,
                self.resources,
                false,
                self.on_yield,
                self.observer,
            )?;
            let href = book
                .spine_item(spine as usize)
                .and_then(|item| item.href)
                .and_then(|href| book.arena().c_string(href))
                .ok_or(Status::NotFound)?;
            if !newt_core::phased_metadata::admissible_href(href, 512) {
                return Err(Status::Unsupported);
            }
            let (_, entry) = book.catalog().find(href).ok_or(Status::NotFound)?;
            // Checked drops the arena name, which must not escape the Book.
            book.check(&entry)
        })();
        self.catalog.reset();
        if result.is_ok() {
            self.staged = Some(spine);
        }
        result
    }
    fn commit(&mut self, spine: u32) {
        if self.staged == Some(spine) {
            self.selected = self.staged.take();
        }
    }
}
