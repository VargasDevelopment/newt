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
        port.route.census(&mut *port.native, cancel, clock, wait,
            pinned.map(str::as_bytes), need)
    }
}
