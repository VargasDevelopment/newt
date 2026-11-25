use crate::{
    Authority,
    text::{Path, Text},
};
use newt_sdk_heap::Fixed;
use core::{cell::Cell, fmt::Write};
use newt_core::cache_names::namespace::hex;
#[derive(Clone, Copy)]
struct Budget {
    started: u32,
    limit: u32,
    attempts: u32,
    depth: u8,
}
/// Shared optional allowance. No backing allocation exists outside a lookup.
/// Budget access is synchronous and cannot call back into the route.
pub struct RouteState {
    root: Text<161>,
    key: Text<65>,
    valid: bool,
    budget: Cell<Option<Fixed<Budget>>>,
}
impl RouteState {
    pub fn new(root: &str, key: &str) -> Self {
        let valid = !root.is_empty()
            && root.len() <= 160
            && !root.contains("..")
            && !root.contains('\0')
            && key.len() == 64
            && hex(key.as_bytes());
        Self {
            root: Text::from_str(root).unwrap_or(Text::empty()),
            key: Text::from_str(key).unwrap_or(Text::empty()),
            valid,
            budget: Cell::new(None),
        }
    }
    pub fn lend<'op>(&'op self, cancel: &'op dyn Authority, now: fn() -> u32) -> Route<'op> {
        Route { state: self, cancel, now }
    }
}
/// Authority/clock are operation loans, never retained by RouteState.
/// Synchronous callbacks must not reenter this route while a budget is taken.
pub struct Route<'a> {
    state: &'a RouteState,
    cancel: &'a dyn Authority,
    now: fn() -> u32,
}
impl Route<'_> {
    pub fn allowed(&self) -> bool {
        self.state.valid && !self.cancel.cancelled()
    }
    /// Remove both names through the same bounded route codec. A failed
    /// canonical removal must not prevent the backup cleanup attempt.
    pub fn remove_record(&self, native: &mut impl crate::Native, directory: &str,
        suffix: &str, receipt: bool) -> bool {
        if !self.allowed() { return false; }
        let Some(mut path) = self.name(directory, suffix, receipt) else { return false; };
        if path.len() + 4 >= 256 { return false; }
        let canonical = matches!(native.unlink(&path), Ok(()) | Err(crate::Error::Missing));
        if path.write_str(".bak").is_err() { return false; }
        let backup = matches!(native.unlink(&path), Ok(()) | Err(crate::Error::Missing));
        canonical && backup && self.allowed()
    }
    pub fn begin_cache_read(&self, limit: u32, attempts: u32) -> bool {
        let mut budget = self.state.budget.take();
        if budget.is_none() {
            // Frozen nothrow order: no clock sample until allocation succeeds;
            // nested scopes neither allocate nor resample/replenish allowance.
            budget = Fixed::try_new_with(|| Budget {
                started: (self.now)(), limit, attempts, depth: 0,
            });
        }
        let ok = budget.as_mut().is_some_and(|b| {
            if b.depth == u8::MAX { return false; }
            b.depth += 1;
            true
        });
        self.state.budget.set(budget);
        ok
    }
    pub fn finish_cache_read(&self) -> bool {
        let Some(mut budget) = self.state.budget.take() else { return false; };
        if budget.depth == 0 {
            self.state.budget.set(Some(budget));
            return false;
        }
        let live = (self.now)().wrapping_sub(budget.started) < 3000;
        budget.depth -= 1;
        if budget.depth != 0 { self.state.budget.set(Some(budget)); }
        live
    }
    pub fn cache_read_live(&self) -> bool {
        let budget = self.state.budget.take();
        let live = budget.as_ref()
            .is_none_or(|b| (self.now)().wrapping_sub(b.started) < 3000);
        self.state.budget.set(budget);
        live
    }
    pub fn cache_read_size(&self, bytes: u32) -> bool {
        let budget = self.state.budget.take();
        let ok = budget.as_ref().is_none_or(|b| bytes <= b.limit);
        self.state.budget.set(budget);
        ok
    }
    pub(crate) fn step(&self) -> bool {
        if !self.cache_read_live() { return false; }
        let mut budget = self.state.budget.take();
        let ok = budget.as_mut().is_none_or(|b| {
            if b.attempts == 0 { return false; }
            b.attempts -= 1;
            true
        });
        self.state.budget.set(budget);
        ok
    }
    /// Compare the existing canonical name, not a second retained identity.
    /// Directory overrides stay allowed; root/key and receipt separator must
    /// belong to this exact route. Private state guarantees UTF-8 boundaries.
    pub(crate) fn owns_name(&self, path: &str, suffix_bytes: usize) -> bool {
        if !self.state.valid || suffix_bytes == 0 { return false; }
        let Some(tail) = path.strip_prefix(&*self.state.root).and_then(|p| p.strip_prefix('/'))
            else { return false; };
        let Some(key_end) = tail.len().checked_sub(suffix_bytes + 1) else { return false; };
        let Some(prefix) = tail.get(..key_end) else { return false; };
        let Some(directory) = prefix.strip_suffix(&*self.state.key) else { return false; };
        directory.ends_with('/') && matches!(tail.as_bytes().get(key_end), Some(b'.' | b'-'))
    }
    pub(crate) fn name(&self, directory: &str, suffix: &str, receipt: bool) -> Option<Path> {
        if !self.state.valid || directory.contains('\0') || suffix.contains('\0') {
            return None;
        }
        let mut path = Path::empty();
        write!(
            path,
            "{}/{}/{}{}{}",
            &*self.state.root,
            directory,
            &*self.state.key,
            if receipt { '.' } else { '-' },
            suffix
        )
        .ok()?;
        Some(path)
    }
}

impl Route<'_> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn census(&self, native: &mut impl crate::page_entries::PageEntries,
        policy: &crate::page_entry_census::Policy,
        cancel: &dyn Authority, clock: Option<fn() -> u32>, wait: Option<fn()>,
        pinned: Option<&[u8]>, need: Option<&newt_core::cache_names::namespace::PageCacheNeed>,
    ) -> crate::page_entry_census::Grant {
        crate::page_entry_census::entry_census(native,
            crate::page_entry_census::Request {
                policy, root: Some(&self.state.root), cancel, clock, wait, pinned, need,
            })
    }
}
