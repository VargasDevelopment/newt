//! The existing native owner lends its pinned name and native operation together.
use super::*;
impl<S: crate::page_entries::PageEntries> ChapterCacheStorage<'_, S> {
    /// Census the same card while preserving the displayed descriptor's name.
    pub fn census(
        &mut self, cancel: &dyn Authority, clock: Option<fn() -> u32>,
        wait: Option<fn()>, need: Option<&newt_core::cache_names::namespace::PageCacheNeed>,
    ) -> crate::page_entry_census::Grant {
        let port = &mut self.file;
        let pinned = (self.state.read_active && port.state.fd.is_some() && !port.state.writing
            && port.state.suffix_bytes == 69
            && port.state.path[..port.state.final_bytes].ends_with(".fibp"))
            .then(|| port.state.path[..port.state.final_bytes].rsplit('/').next()).flatten();
        port.route.census(&mut *port.native, &crate::page_entry_census::PAGES, cancel, clock, wait,
            pinned.map(str::as_bytes), need)
    }
    /// Book indexes: their own folder and quota. `pinned` is the open book's
    /// index file name in that folder (`<key>-<digest>.fibx`); it and its
    /// `.bak` are never evicted. Run it only before writing a new index, never on a repeat
    /// open. Callers treat any outcome as advisory: a failed eviction must
    /// not fail the open.
    pub fn index_census(
        &mut self, cancel: &dyn Authority, clock: Option<fn() -> u32>,
        wait: Option<fn()>, pinned: Option<&[u8]>,
    ) -> crate::page_entry_census::Grant {
        let port = &mut self.file;
        port.route.census(&mut *port.native, &crate::page_entry_census::INDEXES, cancel, clock, wait,
            pinned, None)
    }
}
