//! Reader yield cadence and background producer scheduling. MIT.
#![forbid(unsafe_code)]
use core::cell::Cell;

/// UI-owned scalar shared by foreground and producer callbacks. Cell permits
/// the existing shared cancellation hooks; it cannot be shared across tasks.
#[derive(Default)]
pub struct YieldCadence {
    last_wait: Cell<u32>,
}
impl YieldCadence {
    pub const fn new() -> Self { Self { last_wait: Cell::new(0) } }
    pub fn service(&self, now: u32, wait: impl FnOnce(), clock: impl FnOnce() -> u32) -> bool {
        if now.wrapping_sub(self.last_wait.get()) < 5 { return false; }
        wait();
        self.last_wait.set(clock());
        true
    }
    /// Producer yields share the foreground wait clock without arming or
    /// cancelling the foreground open controller.
    pub fn visit_producer(&self, mut stopped: impl FnMut() -> bool,
        mut clock: impl FnMut() -> u32, wait: impl FnOnce(),
    ) -> bool {
        if stopped() { return false; }
        self.service(clock(), wait, &mut clock);
        !stopped()
    }
}
const _: () = assert!(core::mem::size_of::<YieldCadence>() == core::mem::size_of::<u32>());
