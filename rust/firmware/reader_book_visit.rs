// SPDX-License-Identifier: MIT
//! Short producer visit returns the existing session word and selection bits.
use newt_core::{producer_visit::{Admission, ProducerVisit}, selection_authority::flag};

pub(crate) struct VisitParts<'a> {
    pub(crate) view: &'a mut u16,
    pub(crate) flags: &'a mut u16,
}
pub(super) struct Visit<'bank, 'state> {
    visit: Option<ProducerVisit<'bank>>,
    parts: Option<VisitParts<'state>>,
}
impl<'bank, 'state> Visit<'bank, 'state> {
    pub(super) fn new(ledger: &'bank mut [u8; 256], parts: Option<VisitParts<'state>>) -> Self {
        let visit = match &parts {
            None => ProducerVisit::new(ledger),
            Some(parts) => ProducerVisit::from_parts(ledger, *parts.view,
                match (*parts.flags & flag::PAGE_CACHE_MASK) >> 9 {
                    1 => Admission::Denied, 2 => Admission::Spent,
                    3 => Admission::Granted, _ => Admission::Pending,
                }, *parts.flags & flag::PAGE_CACHE_VISIT != 0),
        };
        Self { visit: Some(visit), parts }
    }
    pub(super) fn with<R>(&mut self, work: impl FnOnce(&mut ProducerVisit<'bank>) -> R) -> Option<R> {
        self.visit.as_mut().map(work)
    }
}
impl Drop for Visit<'_, '_> {
    fn drop(&mut self) {
        if let (Some(visit), Some(parts)) = (self.visit.take(), self.parts.as_mut()) {
            let (view, admission, visited) = visit.into_parts();
            *parts.view = view;
            let admission = match admission {
                Admission::Denied => 1, Admission::Spent => 2,
                Admission::Granted => 3, Admission::Pending | Admission::Stopped => 0,
            };
            *parts.flags = (*parts.flags & !(flag::PAGE_CACHE_MASK | flag::PAGE_CACHE_VISIT))
                | (admission << 9) | if visited { flag::PAGE_CACHE_VISIT } else { 0 };
        }
    }
}
