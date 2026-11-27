// SPDX-License-Identifier: MIT
#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

mod counter;
use core::{cell::Cell, marker::PhantomData, sync::atomic::Ordering::SeqCst};
use counter::Counter;
#[cfg(any(target_arch = "riscv32", feature = "native-atomics"))]
pub use counter::NativeAtomicU32;

pub mod native;
const OPENING: u32 = 0x8000_0000;
const TIMED_OUT: u32 = 0x4000_0000;
// Bit 28 (not bit 29) so !(OPENING | CLOCKWISE) is 0x6fff_ffff rather than
// 0x5fff_ffff (which RISC-V materializes via lui 0x60000, the UART0 base).
const CLOCKWISE: u32 = 0x1000_0000;
const FLAGS: u32 = OPENING | TIMED_OUT | CLOCKWISE;
const CANCEL_BACK: u8 = 0x80;
pub const OPEN_BUDGET_MS: u32 = 20_000;
pub const POLL_MS: u32 = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Button {
    Back,
    Confirm,
    Left,
    Right,
    Up,
    Down,
    Power,
}
const SCAN: [Button; 7] = [
    Button::Back,
    Button::Confirm,
    Button::Left,
    Button::Right,
    Button::Up,
    Button::Down,
    Button::Power,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Home,
    EpubReading,
    Library,
    Other,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Previous,
    Next,
    Activate,
    Back,
    Sleep,
    Read,
}
pub fn action_for_button(button: Button, screen: Screen) -> Action {
    match button {
        Button::Back if screen == Screen::Home => Action::Read,
        Button::Back => Action::Back,
        Button::Confirm => Action::Activate,
        Button::Left | Button::Up => Action::Previous,
        Button::Right | Button::Down => Action::Next,
        Button::Power => Action::Sleep,
    }
}

/// Private cancellation tag cannot be manufactured by safe USB input code.
#[derive(Debug, PartialEq, Eq)]
pub struct PhysicalEvent {
    code: u8,
}
impl PhysicalEvent {
    pub fn queue_code(&self) -> u8 {
        self.code
    }
    /// # Safety
    /// Exactly one physical item received from the live queue; never a USB
    /// item or a second reconstruction of an already-retired queue item.
    pub unsafe fn from_physical_queue(code: u8) -> Self {
        Self { code }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Consumption {
    Action(Action),
    /// Always suppress ordinary navigation. Dispatch Back only where the
    /// cancelled work unwinds to, the reader or Library: a cancelled Library
    /// open then goes Home like any Back from Library. Elsewhere a later
    /// tagged Back in the same queue must not close what earlier presses opened.
    CancelBack {
        dispatch_back: bool,
    },
    Ignored,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub elapsed: u32,
    pub reason: u8,
    pub active: bool,
}

/// Only these two words cross tasks. No reader, SDK or UI clock is shared.
pub struct Shared {
    state: Counter,
    dropped: Counter,
}
#[cfg(not(any(target_arch = "riscv32", feature = "native-atomics")))]
impl Default for Shared {
    fn default() -> Self {
        Self::new()
    }
}
impl Shared {
    #[cfg(not(any(target_arch = "riscv32", feature = "native-atomics")))]
    pub const fn new() -> Self {
        Self {
            state: Counter::new(0),
            dropped: Counter::new(0),
        }
    }
    /// # Safety
    /// For distinct non-null pointers: two native-owned, initialized, zero-valued
    /// np_atomic_u32 objects
    /// have fixed addresses and outlive this capability and every loan. A lexical
    /// owner may return only before task start and after all loans end; once
    /// a task starts, its owner scope never returns until whole-program abort/reset.
    /// This is their sole acquisition:
    /// no other Shared/counter owner, reset, destruction, or plain-memory alias
    /// may exist while this capability or its loans exist, including forgotten
    /// ownership. Foreign atomic calls must satisfy counter.rs's ABI contract.
    /// Null/equal addresses are rejected; other validity cannot be checked here.
    /// This does not establish cross-task Send/Sync or a task stop handshake.
    #[cfg(any(target_arch = "riscv32", feature = "native-atomics"))]
    pub unsafe fn from_native(
        state: *mut NativeAtomicU32,
        dropped: *mut NativeAtomicU32,
    ) -> Option<Self> {
        if state == dropped {
            return None;
        }
        // SAFETY: the caller establishes both native objects' complete contract.
        Some(Self {
            state: unsafe { Counter::from_raw(state)? },
            dropped: unsafe { Counter::from_raw(dropped)? },
        })
    }
    /// Single producer and single UI endpoint for the duration of the borrow.
    pub fn split(&mut self) -> (Producer<'_>, UiControl<'_>) {
        let shared: &Self = self;
        (
            Producer {
                shared,
                power_ready: false,
            },
            UiControl {
                shared,
                started: Cell::new(0),
                _ui: PhantomData,
            },
        )
    }
}

/// Exact Newt polling policy; SDK debounce remains in InputIo::update.
pub struct Producer<'a> {
    shared: &'a Shared,
    power_ready: bool,
}
/// SDK/RTOS transport calls only. No application dispatch or parser reentry.
pub trait InputIo {
    fn update(&mut self);
    fn power_physically_pressed(&mut self) -> bool;
    fn power_logically_pressed(&mut self) -> bool;
    fn was_pressed(&mut self, button: Button) -> bool;
    fn observe_physical(&mut self);
    fn try_send_tail(&mut self, event: &PhysicalEvent) -> bool;
    /// One xQueueSendToFront(..., portMAX_DELAY) attempt, which may fail.
    fn send_front_wait(&mut self, event: &PhysicalEvent) -> bool;
    fn delay_ms(&mut self, ms: u32);
}
impl Producer<'_> {
    pub fn poll_once(&mut self, io: &mut impl InputIo) {
        io.update();
        if !io.power_physically_pressed() && !io.power_logically_pressed() {
            self.power_ready = true;
        }
        for button in SCAN {
            if !io.was_pressed(button) || (button == Button::Power && !self.power_ready) {
                continue;
            }
            let (event, priority) = self.queue_input(button);
            io.observe_physical();
            if !io.try_send_tail(&event) {
                if priority {
                    while !io.send_front_wait(&event) {}
                } else {
                    self.shared.dropped.fetch_add(1, SeqCst);
                }
            }
        }
        // Delay after all sends complete; no wall-clock/catch-up scheduling.
        io.delay_ms(POLL_MS);
    }
    pub fn run(&mut self, io: &mut impl InputIo) -> ! {
        loop {
            self.poll_once(io);
        }
    }
    fn queue_input(&self, button: Button) -> (PhysicalEvent, bool) {
        if button == Button::Power {
            self.shared.state.fetch_add(1, SeqCst);
            return (PhysicalEvent { code: button as u8 }, true);
        }
        let mut state = self.shared.state.load(SeqCst);
        loop {
            let back = if state & CLOCKWISE != 0 {
                Button::Right
            } else {
                Button::Back
            };
            if button != back {
                return (PhysicalEvent { code: button as u8 }, false);
            }
            if state & OPENING == 0 {
                return (PhysicalEvent { code: button as u8 }, true);
            }
            match self.shared.state.compare_exchange_weak(
                state,
                state.wrapping_add(1),
                SeqCst,
                SeqCst,
            ) {
                Ok(_) => return (PhysicalEvent { code: CANCEL_BACK }, true),
                Err(current) => state = current,
            }
        }
    }
}

/// UI task alone owns the timestamp and scheduler. Intentionally !Send/!Sync.
pub struct UiControl<'a> {
    shared: &'a Shared,
    started: Cell<u32>,
    _ui: PhantomData<*mut ()>,
}
struct FinishOpen<'s, 'a> {
    ui: &'s mut UiControl<'a>,
    owns: bool,
}
impl Drop for FinishOpen<'_, '_> {
    fn drop(&mut self) {
        if self.owns {
            self.ui
                .shared
                .state
                .fetch_and(!(OPENING | TIMED_OUT), SeqCst);
        }
    }
}
impl UiControl<'_> {
    /// Nested scopes borrow the outer open without resetting or finishing it.
    pub fn with_open<R>(&mut self, now: u32, body: impl FnOnce(&mut Self, bool) -> R) -> R {
        let owns = self.shared.state.fetch_or(OPENING, SeqCst) & OPENING == 0;
        if owns {
            self.started.set(now);
        }
        let guard = FinishOpen { ui: self, owns };
        body(&mut *guard.ui, owns)
    }
    pub fn active(&self) -> bool {
        self.shared.state.load(SeqCst) & OPENING != 0
    }
    pub fn timed_out(&self) -> bool {
        let state = self.shared.state.load(SeqCst);
        state & OPENING != 0 && state & TIMED_OUT != 0
    }
    pub fn restart(&self, now: u32) {
        if self.active() && !self.cancelled() {
            if self.elapsed(now) >= OPEN_BUDGET_MS {
                self.shared.state.fetch_or(TIMED_OUT, SeqCst);
            } else {
                self.started.set(now);
            }
        }
    }
    pub fn elapsed(&self, now: u32) -> u32 {
        if self.active() {
            now.wrapping_sub(self.started.get())
        } else {
            0
        }
    }
    /// Match which front button acts as Back (Right in clockwise book screens, Back elsewhere).
    pub fn set_clockwise_back(&self, clockwise: bool) {
        if clockwise {
            self.shared.state.fetch_or(CLOCKWISE, SeqCst);
        } else {
            self.shared.state.fetch_and(!CLOCKWISE, SeqCst);
        }
    }
    pub fn back_button(&self) -> Button {
        if self.shared.state.load(SeqCst) & CLOCKWISE != 0 {
            Button::Right
        } else {
            Button::Back
        }
    }
    pub fn cancelled(&self) -> bool {
        let state = self.shared.state.load(SeqCst);
        state & OPENING != 0 && state & !(OPENING | CLOCKWISE) != 0
    }
    pub fn cancelled_at(&self, now: u32) -> bool {
        if self.active() && !self.cancelled() && self.elapsed(now) >= OPEN_BUDGET_MS {
            self.shared.state.fetch_or(TIMED_OUT, SeqCst);
        }
        self.cancelled()
    }
    /// Observation includes an expired deadline but does not latch it.
    pub fn snapshot(&self, now: u32) -> Snapshot {
        let state = self.shared.state.load(SeqCst);
        if state & OPENING == 0 {
            return Snapshot::default();
        }
        let elapsed = now.wrapping_sub(self.started.get());
        Snapshot {
            elapsed,
            active: true,
            reason: u8::from(state & !FLAGS != 0)
                | (u8::from(state & TIMED_OUT != 0 || elapsed >= OPEN_BUDGET_MS) << 1),
        }
    }
    /// Invoke only for physical queue items, once, after the open unwinds.
    /// Sleep's suspended-task drain uses this same retirement operation.
    pub fn consume_physical(&mut self, event: PhysicalEvent, screen: Screen) -> Consumption {
        let back = event.code == CANCEL_BACK;
        if back || event.code == Button::Power as u8 {
            self.shared.state.fetch_sub(1, SeqCst);
        }
        if back {
            return Consumption::CancelBack {
                dispatch_back: matches!(screen, Screen::EpubReading | Screen::Library),
            };
        }
        let button = match event.code {
            0 => Button::Back,
            1 => Button::Confirm,
            2 => Button::Left,
            3 => Button::Right,
            4 => Button::Up,
            5 => Button::Down,
            6 => Button::Power,
            _ => return Consumption::Ignored,
        };
        Consumption::Action(action_for_button(button, screen))
    }
    pub fn dropped(&self) -> u32 {
        self.shared.dropped.load(SeqCst)
    }
    pub fn take_dropped(&mut self) -> u32 {
        self.shared.dropped.swap(0, SeqCst)
    }
}

/// Actual ReaderYieldCadence: visit all work hooks, throttle only the wait.
#[derive(Default)]
pub struct ReaderCadence {
    last_wait: u32,
}
impl ReaderCadence {
    pub const fn new() -> Self {
        Self { last_wait: 0 }
    }
    pub fn service(&mut self, now: u32, wait: impl FnOnce(), clock: impl FnOnce() -> u32) -> bool {
        if now.wrapping_sub(self.last_wait) < 5 {
            return false;
        }
        wait();
        self.last_wait = clock();
        true
    }
    pub fn visit(
        &mut self,
        control: &UiControl<'_>,
        mut clock: impl FnMut() -> u32,
        wait: impl FnOnce(),
    ) {
        let now = clock();
        control.cancelled_at(now);
        self.service(now, wait, clock);
    }
    /// Background producer cancellation stays separate from foreground opens.
    pub fn visit_producer(
        &mut self,
        mut stopped: impl FnMut() -> bool,
        mut clock: impl FnMut() -> u32,
        wait: impl FnOnce(),
    ) -> bool {
        if stopped() {
            return false;
        }
        self.service(clock(), wait, clock);
        !stopped()
    }
}

#[cfg(test)]
mod clockwise_back_tests {
    use super::{Action, Button, CANCEL_BACK, Consumption, InputIo, PhysicalEvent, Screen, Shared};

    struct FakeIo {
        pressed: Option<Button>,
        full: bool,
        tail: Option<u8>,
        front: Option<u8>,
    }
    impl FakeIo {
        fn new(pressed: Button, full: bool) -> Self {
            Self { pressed: Some(pressed), full, tail: None, front: None }
        }
    }
    impl InputIo for FakeIo {
        fn update(&mut self) {}
        fn power_physically_pressed(&mut self) -> bool { false }
        fn power_logically_pressed(&mut self) -> bool { false }
        fn was_pressed(&mut self, button: Button) -> bool { self.pressed == Some(button) }
        fn observe_physical(&mut self) {}
        fn try_send_tail(&mut self, event: &PhysicalEvent) -> bool {
            if self.full { return false; }
            self.tail = Some(event.queue_code());
            true
        }
        fn send_front_wait(&mut self, event: &PhysicalEvent) -> bool {
            self.front = Some(event.queue_code());
            true
        }
        fn delay_ms(&mut self, _ms: u32) {}
    }

    #[test]
    fn clockwise_back_cancels_on_right_and_never_on_front_one() {
        let mut shared = Shared::new();
        let (mut producer, mut control) = shared.split();

        // Clockwise book screen: front 4 (Right) is Back; front 1 (Back) is Page forward.
        control.set_clockwise_back(true);
        assert_eq!(control.back_button(), Button::Right);
        let cancel_code = control.with_open(100, |control, _| {
            assert!(!control.cancelled());
            assert_eq!(control.snapshot(110).reason, 0);

            // Pressing front 1 (Button::Back) while open does not cancel and queues normally.
            let mut page_next = FakeIo::new(Button::Back, false);
            producer.poll_once(&mut page_next);
            assert_eq!(page_next.tail, Some(Button::Back as u8));
            assert!(!control.cancelled());
            assert_eq!(control.snapshot(120).reason, 0);

            // When the queue is full, front 1 is dropped rather than preempting the front.
            let mut page_full = FakeIo::new(Button::Back, true);
            producer.poll_once(&mut page_full);
            assert_eq!(page_full.front, None);
            assert_eq!(control.take_dropped(), 1);
            assert!(!control.cancelled());

            // Pressing front 4 (Button::Right) cancels the open and takes priority on a full queue.
            let mut cw_back = FakeIo::new(Button::Right, true);
            producer.poll_once(&mut cw_back);
            assert_eq!(cw_back.front, Some(CANCEL_BACK));
            assert!(control.cancelled());
            assert_eq!(control.snapshot(130).reason, 1);
            cw_back.front.unwrap()
        });
        assert_eq!(
            control.consume_physical(PhysicalEvent { code: cancel_code }, Screen::EpubReading),
            Consumption::CancelBack { dispatch_back: true }
        );
        // After retirement, clockwise stays active and a new open starts uncancelled.
        assert_eq!(control.back_button(), Button::Right);
        control.with_open(200, |control, _| assert!(!control.cancelled()));

        // Leaving clockwise restores front 1 (Button::Back) as the cancel button.
        control.set_clockwise_back(false);
        assert_eq!(control.back_button(), Button::Back);
        let portrait_cancel = control.with_open(300, |control, _| {
            let mut right = FakeIo::new(Button::Right, false);
            producer.poll_once(&mut right);
            assert_eq!(right.tail, Some(Button::Right as u8));
            assert!(!control.cancelled());

            let mut back = FakeIo::new(Button::Back, false);
            producer.poll_once(&mut back);
            assert_eq!(back.tail, Some(CANCEL_BACK));
            assert!(control.cancelled());
            back.tail.unwrap()
        });
        assert_eq!(
            control.consume_physical(PhysicalEvent { code: portrait_cancel }, Screen::Home),
            Consumption::CancelBack { dispatch_back: false }
        );
        // A cancelled Library open queues Back, Next, Confirm, Back. The first
        // Back unwinds to Library and goes Home; Next and Confirm then open
        // another screen, where the second tagged Back must not close it.
        let queued = control.with_open(400, |control, _| {
            [Button::Back, Button::Right, Button::Confirm, Button::Back].map(|button| {
                let mut io = FakeIo::new(button, false);
                producer.poll_once(&mut io);
                io.tail.unwrap()
            })
        });
        assert!(queued[0] == CANCEL_BACK && queued[3] == CANCEL_BACK);
        assert_eq!(
            control.consume_physical(PhysicalEvent { code: queued[0] }, Screen::Library),
            Consumption::CancelBack { dispatch_back: true }
        );
        assert_eq!(
            control.consume_physical(PhysicalEvent { code: queued[1] }, Screen::Home),
            Consumption::Action(Action::Next)
        );
        assert_eq!(
            control.consume_physical(PhysicalEvent { code: queued[2] }, Screen::Home),
            Consumption::Action(Action::Activate)
        );
        assert_eq!(
            control.consume_physical(PhysicalEvent { code: queued[3] }, Screen::Other),
            Consumption::CancelBack { dispatch_back: false }
        );
        assert_eq!(
            control.consume_physical(PhysicalEvent { code: Button::Back as u8 }, Screen::EpubReading),
            Consumption::Action(Action::Back)
        );
    }
    /// Contents jumps restart the budget as each pass begins (#188). A restart
    /// gives a working pass a fresh budget; it never ends a timeout or undoes Back.
    #[test]
    fn a_restart_renews_working_passes_but_not_back_or_a_stuck_pass() {
        let mut shared = Shared::new();
        let (_, mut control) = shared.split();
        // Passes of 15 s each finish only because each one restarts the budget.
        control.with_open(0, |control, _| {
            for begun in [15_000, 30_000, 45_000] {
                assert!(!control.cancelled_at(begun));
                control.restart(begun);
            }
            assert!(!control.cancelled_at(45_000 + super::OPEN_BUDGET_MS - 1));
            assert!(control.cancelled_at(45_000 + super::OPEN_BUDGET_MS) && control.timed_out());
            control.restart(70_000);
            assert!(control.cancelled_at(70_001) && control.timed_out(), "a stuck pass stays timed out");
        });
        // An unpolled expired budget latches TIMED_OUT on restart instead of resetting started.
        control.with_open(0, |control, _| {
            control.restart(super::OPEN_BUDGET_MS);
            assert!(control.cancelled() && control.timed_out(), "expired unpolled budget must latch TIMED_OUT");
            control.restart(super::OPEN_BUDGET_MS + 1_000);
            assert!(control.cancelled_at(super::OPEN_BUDGET_MS + 1_001) && control.timed_out());
        });
        // Back in any pass ends the jump, whatever restarts follow.
        for back_in in 0..3 {
            let mut shared = Shared::new();
            let (mut producer, mut control) = shared.split();
            control.with_open(0, |control, _| {
                for pass in 0..3u32 {
                    let begun = pass * 15_000;
                    control.restart(begun);
                    if pass == back_in {
                        producer.poll_once(&mut FakeIo::new(Button::Back, false));
                    }
                    assert_eq!(control.cancelled_at(begun + 1), pass >= back_in, "pass {pass}, Back in {back_in}");
                }
                assert!(!control.timed_out());
            });
        }
    }
}
