//! Ordinary RV32 call bodies. Compile/retention evidence awaits Root; no hardware claim.
use newt_core::app::{Action, Screen};
use newt_wake_evidence::*;
pub struct ReaderFacts {
    pub status: u8,
    pub failure: u8,
    pub orientation: u8,
    pub opened: bool,
    pub spine: u32,
    pub character: u32,
}
impl ReaderObservation for ReaderFacts {
    fn status(&self) -> u8 {
        self.status
    }
    fn restore_failure(&self) -> u8 {
        self.failure
    }
    fn orientation(&self) -> u8 {
        self.orientation
    }
    fn opened(&self) -> bool {
        self.opened
    }
    fn page(&self) -> (u32, u32) {
        (self.spine, self.character)
    }
}
#[inline(never)]
#[unsafe(no_mangle)]
pub fn wake_observe_attempt(e: &mut Evidence, r: &ReaderFacts, outcome: Attempt, policy: u8) {
    e.initial_reader_attempt(outcome, r, policy);
}
#[inline(never)]
#[unsafe(no_mangle)]
pub fn wake_observe_paint(e: &mut Evidence, s: Screen, generation: u32) {
    e.completed_paint(s, generation);
}
#[inline(never)]
#[unsafe(no_mangle)]
pub fn wake_observe_input(
    e: &mut Evidence,
    source: InputSource,
    button: u8,
    action: Action,
    now: u32,
    before: Screen,
    after: Screen,
    open: bool,
) {
    e.consumed_input(source, button, action, now, before, after, open);
}
#[inline(never)]
#[unsafe(no_mangle)]
pub fn wake_line(e: &Evidence, index: u32, out: &mut [u8]) {
    newt_device_ui::format_wake_evidence_line(e, index, out);
}
