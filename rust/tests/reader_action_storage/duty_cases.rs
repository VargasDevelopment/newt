// SPDX-License-Identifier: MIT
//! Focused host proof for the actual firmware `ReaderDuty` budget renewal,
//! monotonicity, 4,096-page ceiling, stall timeout, and Back/Power cancellation (#103).
use core::marker::PhantomData;
use newt_core::{
    admission::{WorkEvent, WorkKind, WorkPhase},
    cache_names::segment::{MAX_PAGES, MAX_SEGMENTS, PAGES},
    reader_scheduler::YieldCadence,
};
use newt_input_policy::{Button, InputIo, OPEN_BUDGET_MS, PhysicalEvent, Shared};

unsafe extern "C" {
    fn fixture_clock_set(value: u32);
}

fn set_clock(ms: u32) {
    // SAFETY: single-threaded host test; updates host_fake.c's monotonic clock.
    unsafe { fixture_clock_set(ms) };
}

mod app_power {
    pub fn delay_ms(_: u32) {}
}

mod reader_action {
    #[derive(Clone, Copy)]
    pub struct Trace<'a>(pub core::marker::PhantomData<&'a ()>);
    impl Trace<'_> {
        pub fn event(self, _: &newt_core::admission::WorkEvent) -> bool { false }
        pub fn boundary(self, _: u8) {}
        pub fn route(self, _: u32) {}
        pub fn cache_miss(self, _: u8) {}
    }
}

#[path = "../../firmware/reader_duty.rs"]
mod reader_duty;
use reader_duty::ReaderDuty;

struct FakeIo(Option<Button>);
impl InputIo for FakeIo {
    fn update(&mut self) {}
    fn power_physically_pressed(&mut self) -> bool { false }
    fn power_logically_pressed(&mut self) -> bool { false }
    fn was_pressed(&mut self, button: Button) -> bool { self.0 == Some(button) }
    fn observe_physical(&mut self) {}
    fn try_send_tail(&mut self, _: &PhysicalEvent) -> bool { true }
    fn send_front_wait(&mut self, _: &PhysicalEvent) -> bool { true }
    fn delay_ms(&mut self, _: u32) {}
}

fn phase(phase: WorkPhase) -> WorkEvent {
    let mut event = WorkEvent::new(WorkKind::Phase, 0);
    event.phase = phase;
    event
}

fn progress(pages: i32) -> WorkEvent {
    WorkEvent::new(WorkKind::LayoutProgress, pages)
}

fn main() {
    let mut tokens = [0u8; 2];
    // SAFETY: host board fake accepts two distinct non-null pointers.
    let (board, pending, _) = unsafe {
        newt_c3_board::Board::acquire_x3_optional_sd(
            (&mut tokens[0] as *mut u8).cast(),
            (&mut tokens[1] as *mut u8).cast(),
        )
    }
    .unwrap();
    drop(pending);
    let clock = board.monotonic_clock();
    let cadence = YieldCadence::new();
    let trace = reader_action::Trace(PhantomData);

    // 1. Long chapter up to 4,096 pages (32 * 128) renews cleanly past 20 s.
    {
        let mut shared = Shared::new();
        let (_, mut control) = shared.split();
        set_clock(0);
        control.with_open(0, |control, _| {
            let duty = ReaderDuty::new(control, &cadence, &clock, None, trace).renewing(true);
            duty.event(&phase(WorkPhase::Layout));
            let mut now = 0u32;
            for step in 1..=MAX_SEGMENTS as u32 {
                now += 1_500;
                set_clock(now);
                duty.yield_now();
                assert!(
                    !duty.cancelled() && !control.timed_out(),
                    "long chapter up to 4096 pages must renew across milestones (step {step}, t={now})"
                );
                duty.event(&progress((step * PAGES) as i32));
            }
            now += 1_500;
            set_clock(now);
            duty.yield_now();
            assert!(
                !duty.cancelled() && !control.timed_out(),
                "long chapter up to 4096 pages must renew across milestones (finish t={now})"
            );
        });
    }

    // 2. Renewal off times out at 20 s despite regular LayoutProgress milestones.
    {
        let mut shared = Shared::new();
        let (_, mut control) = shared.split();
        set_clock(0);
        control.with_open(0, |control, _| {
            let duty = ReaderDuty::new(control, &cadence, &clock, None, trace).renewing(false);
            duty.event(&phase(WorkPhase::Layout));
            let mut now = 0u32;
            for step in 1..=13u32 {
                now += 1_500;
                set_clock(now);
                duty.yield_now();
                assert!(!duty.cancelled(), "must not time out before 20 s (t={now})");
                duty.event(&progress((step * PAGES) as i32));
            }
            set_clock(OPEN_BUDGET_MS - 1);
            assert!(!duty.cancelled(), "must not time out at 19,999 ms");
            set_clock(OPEN_BUDGET_MS);
            assert!(
                duty.cancelled() && control.timed_out(),
                "renewal off must time out at 20 s"
            );
        });
    }

    // 3a. Stalled chunk (0 progress for 20 s) times out at 20 s with renewal on,
    //     and later progress or phase events cannot undo TIMED_OUT.
    {
        let mut shared = Shared::new();
        let (_, mut control) = shared.split();
        set_clock(0);
        control.with_open(0, |control, _| {
            let duty = ReaderDuty::new(control, &cadence, &clock, None, trace).renewing(true);
            duty.event(&phase(WorkPhase::Layout));
            set_clock(3_000);
            duty.yield_now();
            duty.event(&progress(PAGES as i32));
            set_clock(3_000 + OPEN_BUDGET_MS - 1);
            assert!(!duty.cancelled());
            set_clock(3_000 + OPEN_BUDGET_MS);
            duty.yield_now();
            assert!(
                duty.cancelled() && control.timed_out(),
                "stalled chunk must time out at 20 s even with renewal on"
            );
            set_clock(3_000 + OPEN_BUDGET_MS + 1);
            duty.event(&progress((2 * PAGES) as i32));
            duty.event(&phase(WorkPhase::Layout));
            assert!(
                duty.cancelled() && control.timed_out(),
                "late progress or phase must not clear TIMED_OUT"
            );
        });
    }

    // 3b. Stalled chunk arriving directly at LayoutProgress >= 20 s (without prior
    //     yield_now) still latches TIMED_OUT instead of renewing itself.
    {
        let mut shared = Shared::new();
        let (_, mut control) = shared.split();
        set_clock(0);
        control.with_open(0, |control, _| {
            let duty = ReaderDuty::new(control, &cadence, &clock, None, trace).renewing(true);
            duty.event(&phase(WorkPhase::Layout));
            set_clock(3_000);
            duty.event(&progress(PAGES as i32));
            set_clock(3_000 + OPEN_BUDGET_MS);
            duty.event(&progress((2 * PAGES) as i32));
            assert!(
                duty.cancelled() && control.timed_out(),
                "stalled chunk must time out at 20 s even with renewal on"
            );
        });
    }

    // 4. Duplicate, regressive, non-positive, and post-MAX_PAGES milestones do not renew.
    {
        let mut shared = Shared::new();
        let (_, mut control) = shared.split();
        set_clock(0);
        control.with_open(0, |control, _| {
            let duty = ReaderDuty::new(control, &cadence, &clock, None, trace).renewing(true);
            duty.event(&phase(WorkPhase::Layout));
            set_clock(2_000);
            duty.event(&progress((2 * PAGES) as i32)); // step 2 -> renewed to 2_000
            for (at, bad) in [
                (8_000, (2 * PAGES) as i32), // duplicate
                (12_000, PAGES as i32),      // regressive
                (15_000, 0),                 // zero
                (18_000, -(PAGES as i32)),   // negative
            ] {
                set_clock(at);
                duty.event(&progress(bad));
            }
            set_clock(2_000 + OPEN_BUDGET_MS);
            assert!(
                duty.cancelled() && control.timed_out(),
                "duplicate, regressive or invalid milestones must not renew"
            );
        });
    }
    {
        let mut shared = Shared::new();
        let (_, mut control) = shared.split();
        set_clock(0);
        control.with_open(0, |control, _| {
            let duty = ReaderDuty::new(control, &cadence, &clock, None, trace).renewing(true);
            duty.event(&phase(WorkPhase::Layout));
            set_clock(10_000);
            duty.event(&progress(MAX_PAGES as i32)); // final allowed step 32 -> renewed to 10_000
            set_clock(20_000);
            duty.event(&progress((MAX_PAGES + PAGES) as i32)); // 4,224 -> ignored
            set_clock(25_000);
            duty.event(&progress((MAX_PAGES + 2 * PAGES) as i32)); // 4,352 -> ignored
            set_clock(10_000 + OPEN_BUDGET_MS);
            assert!(
                duty.cancelled() && control.timed_out(),
                "milestones past MAX_PAGES must not renew the open budget"
            );
        });
    }

    // 5. Phase renewals are strictly bounded (MAX_PHASE_RENEWALS = 32) and reset progress_step.
    {
        let mut shared = Shared::new();
        let (_, mut control) = shared.split();
        set_clock(0);
        control.with_open(0, |control, _| {
            let duty = ReaderDuty::new(control, &cadence, &clock, None, trace).renewing(true);
            let mut now = 0u32;
            for i in 0..32u32 {
                now += 10_000;
                set_clock(now);
                duty.event(&phase(WorkPhase::Layout));
                assert!(!duty.cancelled(), "phase {i} should renew");
                // Confirm progress_step reset on each new phase by renewing at step 1,
                // and leave progress_step == 1 on the final allowed phase (i == 31).
                if i == 0 || i == 1 || i == 31 {
                    now += 1_000;
                    set_clock(now);
                    duty.event(&progress(PAGES as i32));
                    assert!(!duty.cancelled());
                }
            }
            let last_renew = now;
            set_clock(last_renew + 10_000);
            duty.event(&phase(WorkPhase::Layout)); // 33rd phase -> ignored, must not reset progress_step
            set_clock(last_renew + 12_000);
            duty.event(&progress(PAGES as i32)); // repeated step 1 -> must not bypass phase cap
            set_clock(last_renew + OPEN_BUDGET_MS);
            assert!(
                duty.cancelled() && control.timed_out(),
                "phase renewals must be bounded even with repeated milestones"
            );
        });
    }

    // 6. Back and Power cancel immediately across renewals and never report timed_out.
    for button in [Button::Back, Button::Power] {
        let mut shared = Shared::new();
        let (mut producer, mut control) = shared.split();
        // Arm power_ready before polling Power.
        producer.poll_once(&mut FakeIo(None));
        set_clock(0);
        control.with_open(0, |control, _| {
            let duty = ReaderDuty::new(control, &cadence, &clock, None, trace).renewing(true);
            duty.event(&phase(WorkPhase::Layout));
            set_clock(2_000);
            duty.event(&progress(PAGES as i32));
            assert!(!duty.cancelled());
            producer.poll_once(&mut FakeIo(Some(button)));
            set_clock(2_500);
            assert!(
                duty.cancelled() && !control.timed_out(),
                "Back and Power must cancel immediately across renewals ({button:?})"
            );
            set_clock(3_000);
            duty.event(&progress((2 * PAGES) as i32));
            duty.event(&phase(WorkPhase::Layout));
            assert!(
                duty.cancelled() && !control.timed_out(),
                "Back and Power must cancel immediately across renewals ({button:?} after event)"
            );
        });
    }

    println!("PASS real ReaderDuty renewal: 4096-page ceiling, stall timeout, renewal-off control, phase cap, Back/Power cancel");
}
