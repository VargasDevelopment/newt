// SPDX-License-Identifier: MIT
//! Borrowed ReaderScheduler::visit/visitProducer (reader_scheduler.h:148–174).
//! The runtime owns the one cadence; the open scope owns its optional profile.
#![forbid(unsafe_code)]

use newt_c3_board::MonotonicClock;
use newt_core::{
    admission::{WorkEvent, WorkKind, WorkPhase},
    reader_scheduler::YieldCadence,
    reader_work_profile::ReaderWorkProfile,
};
use newt_input_policy::{Snapshot, UiControl};

/// Short UI-only scheduling view. Shared references preserve the existing
/// cancellation/profile hooks without a second counter or a retained callback.
/// UiControl and the Cell-backed cadence/profile prevent Send and Sync.
pub struct ReaderDuty<'loan, 'task> {
    control: &'loan UiControl<'task>,
    cadence: &'loan YieldCadence,
    clock: &'loan MonotonicClock,
    profile: Option<&'loan ReaderWorkProfile>,
    action: crate::reader_action::Trace<'loan>,
    renew: bool,
}

impl<'loan, 'task> ReaderDuty<'loan, 'task> {
    pub fn new(
        control: &'loan UiControl<'task>,
        cadence: &'loan YieldCadence,
        clock: &'loan MonotonicClock,
        profile: Option<&'loan ReaderWorkProfile>,
        action: crate::reader_action::Trace<'loan>,
    ) -> Self {
        Self { control, cadence, clock, profile, action, renew: false }
    }

    /// A Contents jump to an anchor lays its chapter out twice, which can need
    /// more than one open budget in a long chapter (#188). With `renew`, each
    /// work phase that begins gets a fresh budget: it begins only after the last
    /// one ended, so a stuck pass still times out and a Back press stays latched.
    pub fn renewing(self, renew: bool) -> Self {
        Self { renew, ..self }
    }

    /// Cancellation is checked even when the shared five-ms cadence skips
    /// sleeping, and a latched cancellation does not skip this callback's work.
    pub fn yield_now(&self) {
        let now = self.clock.millis();
        self.control.cancelled_at(now);
        if let Some(profile) = self.profile { profile.callback(); }
        self.cadence.service(now, || {
            let before = self.profile.map(|_| self.clock.millis());
            crate::app_power::delay_ms(1);
            if let (Some(profile), Some(before)) = (self.profile, before) {
                profile.slept(self.clock.millis().wrapping_sub(before));
            }
        }, || self.clock.millis());
    }

    /// The actual producer stop hook is required and polled before and after
    /// the same cadence. No foreground arm, timeout poll or profile credit.
    pub fn visit_producer(&self, stopped: &dyn Fn() -> bool) -> bool {
        self.cadence.visit_producer(stopped, || self.clock.millis(), || {
            crate::app_power::delay_ms(1);
        })
    }

    pub fn cancelled(&self) -> bool {
        self.control.cancelled_at(self.clock.millis())
    }

    pub fn snapshot(&self) -> Snapshot {
        self.control.snapshot(self.clock.millis())
    }

    /// Event timestamps come from the same live SDK clock, only while the
    /// actual open scope lent its profile. No profile means no event clocks.
    pub fn event(&self, event: &WorkEvent) {
        if self.renew && event.kind == WorkKind::Phase && event.phase != WorkPhase::Idle {
            self.control.restart(self.clock.millis());
        }
        if self.action.event(event) {
            if event.kind == newt_core::admission::WorkKind::OpenRoute {
                if let Some(profile) = self.profile {
                    profile.open_route_tag(event.result as u32);
                }
            }
            return;
        }
        if let Some(profile) = self.profile {
            profile.event(event, self.clock.micros(), self.clock.millis());
        }
    }
    pub fn action_boundary(&self, stage: u8) { self.action.boundary(stage); }
    pub fn open_route(&self, route: u32) {
        self.action.route(route);
        if let Some(profile) = self.profile {
            profile.open_route_tag(route);
        }
    }
    pub fn cache_miss(&self, reason: u8) { self.action.cache_miss(reason); }
}
