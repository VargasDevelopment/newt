// SPDX-License-Identifier: MIT
//! Borrowed ReaderScheduler::visit/visitProducer (reader_scheduler.h:148–174).
//! The runtime owns the one cadence; the open scope owns its optional profile.
#![forbid(unsafe_code)]

use core::cell::Cell;
use newt_c3_board::MonotonicClock;
use newt_core::{
    admission::{WorkEvent, WorkKind, WorkPhase},
    cache_names::segment::{MAX_PAGES, PAGES},
    reader_scheduler::YieldCadence,
    reader_work_profile::ReaderWorkProfile,
};
use newt_input_policy::{Snapshot, UiControl};

/// Maximum phase-start renewals allowed per foreground operation.
const MAX_PHASE_RENEWALS: u8 = 32;

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
    phase_renewals: Cell<u8>,
    progress_step: Cell<u8>,
}

impl<'loan, 'task> ReaderDuty<'loan, 'task> {
    pub fn new(
        control: &'loan UiControl<'task>,
        cadence: &'loan YieldCadence,
        clock: &'loan MonotonicClock,
        profile: Option<&'loan ReaderWorkProfile>,
        action: crate::reader_action::Trace<'loan>,
    ) -> Self {
        Self {
            control,
            cadence,
            clock,
            profile,
            action,
            renew: false,
            phase_renewals: Cell::new(0),
            progress_step: Cell::new(0),
        }
    }

    /// Foreground opens, turns and Contents jumps can lay out chapters up to
    /// `MAX_PAGES` (4,096 pages, #103). With `renew`, each bounded work phase
    /// and each monotonic 128-page milestone up to `MAX_PAGES` gets a fresh
    /// 20 s budget: a stalled pass still times out, and Back/Power stays latched.
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
        if self.renew {
            match event.kind {
                WorkKind::Phase if event.phase != WorkPhase::Idle => {
                    let count = self.phase_renewals.get();
                    if count < MAX_PHASE_RENEWALS {
                        self.phase_renewals.set(count + 1);
                        self.progress_step.set(0);
                        self.control.restart(self.clock.millis());
                    }
                }
                WorkKind::LayoutProgress => {
                    if event.result > 0 {
                        let pages = event.result as u32;
                        let step = pages / PAGES;
                        if pages <= MAX_PAGES && (step as u8) > self.progress_step.get() {
                            self.progress_step.set(step as u8);
                            self.control.restart(self.clock.millis());
                        }
                    }
                    return;
                }
                _ => {}
            }
        } else if event.kind == WorkKind::LayoutProgress {
            return;
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

// Two u8 counters sit inside `renew`'s tail padding, adding zero bytes.
const _: () = {
    type WithoutCounters<'loan, 'task> = (
        &'loan UiControl<'task>,
        &'loan YieldCadence,
        &'loan MonotonicClock,
        Option<&'loan ReaderWorkProfile>,
        crate::reader_action::Trace<'loan>,
        bool,
    );
    assert!(MAX_PAGES / PAGES <= u8::MAX as u32);
    assert!(size_of::<ReaderDuty<'static, 'static>>() == size_of::<WithoutCounters<'static, 'static>>());
};
