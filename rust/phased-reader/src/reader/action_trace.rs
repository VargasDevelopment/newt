// SPDX-License-Identifier: MIT
//! Scalar observations borrow the already installed operation hook.
use super::PhasedReader;
use newt_core::{admission::{self, ActionScope}, reader_action_profile::{ActionStage, CacheMissReason},
    reader_work_profile::ReaderOpenRoute};

impl<'m, 'p> PhasedReader<'m, 'p> {
    #[inline]
    pub fn action_scope(&self, stage: ActionStage) -> ActionScope<'m> {
        ActionScope::new(self.on_yield, stage)
    }
    #[inline]
    pub fn trace_route(&self, route: ReaderOpenRoute) { admission::open_route(self.on_yield, route); }
    #[inline]
    pub fn trace_cache_miss(&self, reason: CacheMissReason) { admission::cache_miss(self.on_yield, reason); }
}
