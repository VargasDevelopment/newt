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
        crate::page_entry_census::entry_census(&mut *port.native, crate::page_entry_census::Request {
            policy: &crate::page_entry_census::PAGES, root: Some(port.route.root()), cancel, clock, wait,
            pinned: pinned.map(str::as_bytes), need,
        })
    }
    /// Book indexes: their own folder and quota. `pinned` is the open book's
    /// 8.3 index file name in that folder; only that `.IDX` is never evicted.
    /// Callers treat any outcome as advisory: a failed eviction must not fail
    /// the open.
    pub fn index_census(
        &mut self, cancel: &dyn Authority, clock: Option<fn() -> u32>,
        wait: Option<fn()>, pinned: Option<&[u8]>,
    ) -> crate::page_entry_census::Grant {
        self.index_policy_census(&crate::page_entry_census::INDEXES, cancel, clock, wait, pinned)
    }
    /// Only reserve the cap's slots before a build; leave reserve trimming for settled idle.
    pub fn index_write_census(&mut self, cancel: &dyn Authority) -> crate::page_entry_census::Grant {
        self.index_policy_census(&crate::page_entry_census::INDEX_WRITE, cancel, None, None, None)
    }
    /// The open `.IDX` survives and consumes no reserve in the idle trim.
    pub fn index_trim(&mut self, cancel: &dyn Authority, clock: fn() -> u32,
        wait: Option<fn()>, pinned: &[u8],
    ) -> crate::page_entry_census::Grant {
        self.index_policy_census(&crate::page_entry_census::INDEX_TRIM, cancel, Some(clock), wait, Some(pinned))
    }
    fn index_policy_census(&mut self, policy: &crate::page_entry_census::Policy,
        cancel: &dyn Authority, clock: Option<fn() -> u32>, wait: Option<fn()>, pinned: Option<&[u8]>,
    ) -> crate::page_entry_census::Grant {
        let port = &mut self.file;
        crate::page_entry_census::entry_census(&mut *port.native, crate::page_entry_census::Request {
            policy, root: Some(port.route.root()), cancel, clock, wait,
            pinned, need: None,
        })
    }
}
