// Included inside phase_storage: only the existing checked Parts initializes views.
impl Phase<'_> {
    pub(crate) fn watched_layout<B: ParserBackend + ?Sized, R>(
        &mut self, backend: &mut B, breaker: &mut dyn LineBreak,
        operation: impl FnOnce(&mut styles::Slot<'_>, &mut LayoutResources<'_>,
            &mut ParseResources<'_, B>, &mut [u8]) -> R,
    ) -> Result<R, Status> {
        // Every watched chapter is sized; only a thin source reference varies.
        // The watch is above the loaned slot, as in frozen kWatch/kLoanOffset.
        let (parse, watch) = top::<crate::watch::Watch<'_,
            crate::admission::ScheduledSource<'_, dyn crate::zip_window::source::BookSource>>>(self.parse)?;
        let mut phase = Phase { policy: self.policy, shared: &mut *self.shared,
            parse, held: &mut *self.held };
        phase.layout(backend, breaker, |slot, layout, io| operation(slot, layout, io, watch))
    }

    /// Frozen settleStyles: an intentional stop voids its parse status.
    /// All watch/window views have ended before the full bank is reused here.
    pub(crate) fn settle_watched<S: crate::zip_window::source::BookSource + ?Sized,
        B: ParserBackend + ?Sized>(
        &mut self, source: &mut S, backend: &mut B, selected: &crate::Selection,
        spine: u32, verdict: crate::watch::Verdict, status: Status, on_yield: newt_core::admission::Yield<'_>,
    ) -> Result<(Status, bool), Status> {
        if !verdict.stopped {
            let crate::watch::Settlement::Return(status) =
                self.edit_slot(|slot| crate::watch::settle(slot, verdict, status))?
            else { return Err(Status::Unsupported); };
            if status == Status::OutOfMemory { *self.held = false; }
            return Ok((status, false));
        }
        let status = self.settle(source, backend, selected, spine, verdict, on_yield)?;
        let status = if status == Status::Ok && !self.ready(spine) { Status::OutOfMemory } else { status };
        Ok((status, status == Status::Ok))
    }
}

pub(crate) fn with_style_watch<'a, S: crate::zip_window::source::BookSource, R>(
    storage: &'a mut [u8], source: &'a mut S, state: styles::State,
    operation: impl for<'watch> FnOnce(&mut crate::watch::Watch<'watch, S>) -> R,
) -> Result<R, Status> {
    let watch = Parts(storage).one(crate::watch::Watch::begin(source, state))?;
    Ok(operation(watch))
}
