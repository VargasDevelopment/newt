// SPDX-License-Identifier: MIT
//! Reborrow the same reader buffers while moving its scalar operation state.
use super::{Observer, PageInfo, PhasedReader, Proofs};
use core::mem::take;
use crate::admission::{ScopedWork, WorkEvent, WorkYield};

// Only return destinations are retained here, never the previous scalar values.
// Buffer/plan mutations already happen in their original backing.
struct Returned<'op, 'p> {
    reader: PhasedReader<'op, 'p>,
    pending: &'op mut Option<(u32, u32, u32, newt_phased_sink::State)>,
    pending_cached: &'op mut bool,
    pending_undeclared: &'op mut Option<bool>,
    active: &'op mut usize,
    source_bytes: &'op mut u64,
    landscape: &'op mut bool,
    opened: &'op mut bool,
    has_page: &'op mut bool,
    info: &'op mut PageInfo,
    proofs: &'op mut Proofs,
    styles_ready: &'op mut bool,
    undeclared_styles: &'op mut bool,
    malformed_utf8: &'op mut bool,
    empty_chapter: &'op mut bool,
    work: &'op mut Observer,
}
impl Drop for Returned<'_, '_> {
    fn drop(&mut self) {
        *self.pending = self.reader.pending.take();
        *self.pending_cached = take(&mut self.reader.pending_cached);
        *self.pending_undeclared = self.reader.pending_undeclared.take();
        *self.active = take(&mut self.reader.active);
        *self.source_bytes = take(&mut self.reader.source_bytes);
        *self.landscape = take(&mut self.reader.landscape);
        *self.opened = take(&mut self.reader.opened);
        *self.has_page = take(&mut self.reader.has_page);
        *self.info = take(&mut self.reader.info);
        *self.proofs = take(&mut self.reader.proofs);
        *self.styles_ready = take(&mut self.reader.styles_ready);
        *self.undeclared_styles = take(&mut self.reader.undeclared_styles);
        *self.malformed_utf8 = take(&mut self.reader.malformed_utf8);
        *self.empty_chapter = take(&mut self.reader.empty_chapter);
        *self.work = self.reader.work.take();
    }
}

impl<'m, 'p> PhasedReader<'m, 'p> {
    /// The operation uses the existing reader methods and original backing.
    /// All resulting scalar state returns on success, error or unwind; the
    /// required short hook ends here and never replaces the stored long hook.
    pub fn with_scoped_yield<'op, R>(
        &'op mut self,
        hook: &'op dyn Fn(),
        work: impl for<'view> FnOnce(&mut PhasedReader<'view, 'p>) -> R,
    ) -> R {
        // A sized adapter uses Core's blanket WorkYield implementation; the
        // callback accepts this shorter view and cannot retain the adapter.
        let call = || hook();
        self.with_yield_loan(&call, work)
    }

    /// Compose the operation's cadence and observer in one stack callback.
    /// The persistent Yield stays the same single fat pointer; its legacy
    /// Observer field keeps the original function-pointer representation.
    pub fn with_scoped_work<'op, R>(
        &'op mut self,
        hook: &'op dyn Fn(),
        observer: Option<&'op dyn Fn(WorkEvent)>,
        work: impl for<'view> FnOnce(&mut PhasedReader<'view, 'p>) -> R,
    ) -> R {
        let hooks = ScopedWork::new(hook, observer);
        self.with_yield_loan(&hooks, work)
    }

    fn with_yield_loan<'op, R>(
        &'op mut self,
        hook: &'op dyn WorkYield,
        work: impl FnOnce(&mut PhasedReader<'op, 'p>) -> R,
    ) -> R {
        // Exhaustive splitting makes a future reader field require an explicit
        // reloan/return decision. All moves/defaults below are scalar-only and
        // cannot invoke a callback before the return guard is fully installed.
        let PhasedReader {
            pending, pending_cached, pending_undeclared, selections, plan,
            active, source_bytes, landscape, opened, has_page, info, proofs,
            styles_ready, undeclared_styles, malformed_utf8, empty_chapter,
            on_yield: _, work: observer,
        } = self;
        let reader = PhasedReader {
            pending: pending.take(),
            pending_cached: take(pending_cached),
            pending_undeclared: pending_undeclared.take(),
            selections: &mut **selections,
            plan: plan.as_deref_mut(),
            active: take(active),
            source_bytes: take(source_bytes),
            landscape: take(landscape),
            opened: take(opened),
            has_page: take(has_page),
            info: take(info),
            proofs: take(proofs),
            styles_ready: take(styles_ready),
            undeclared_styles: take(undeclared_styles),
            malformed_utf8: take(malformed_utf8),
            empty_chapter: take(empty_chapter),
            on_yield: Some(hook),
            work: observer.take(),
        };
        let mut returned = Returned {
            reader, pending, pending_cached, pending_undeclared, active,
            source_bytes, landscape, opened, has_page, info, proofs,
            styles_ready, undeclared_styles, malformed_utf8, empty_chapter,
            work: observer,
        };
        work(&mut returned.reader)
    }
}
