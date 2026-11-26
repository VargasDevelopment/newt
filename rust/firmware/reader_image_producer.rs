// SPDX-License-Identifier: MIT
//! The attempt-local frozen ReadingCacheIdle deadlines and real UI input.
use core::cell::Cell;
use newt_core::producer_policy::Stop;
use newt_c3_board::MonotonicClock;
use newt_input_policy::native::UiQueue;
use crate::{reader_duty::ReaderDuty, serial_console::SerialConsole};

pub(super) struct Poll<'a, 'task> {
    pub(super) duty: &'a ReaderDuty<'a, 'task>,
    pub(super) clock: MonotonicClock,
    pub(super) queue: &'a UiQueue<'task>,
    pub(super) console: &'a SerialConsole,
    pub(super) view_valid: bool,
    pub(super) card_missing: bool,
    pub(super) power: &'a Cell<bool>,
    pub(super) progress: &'a Cell<u32>,
    pub(super) battery: &'a Cell<u32>,
    pub(super) began: Cell<u32>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reason { None, Card, View, Power, Limit, Input, DevByte, Progress, Battery }
impl Poll<'_, '_> {
    fn reason(&self, began: u32) -> Reason {
        if self.card_missing { return Reason::Card; }
        // UI/reader/selection/display views cannot change while the operation
        // owns their exclusive loans; paused work receives none of those views.
        if !self.view_valid { return Reason::View; }
        if !self.power.get() { return Reason::Power; }
        let now = self.clock.millis();
        if now.wrapping_sub(began) >= 120000 { return Reason::Limit; }
        if self.queue.pending() { return Reason::Input; }
        #[cfg(feature = "dev-usb")]
        if self.console.available() > 0 { return Reason::DevByte; }
        if now.wrapping_sub(self.progress.get()) >= 30000 { return Reason::Progress; }
        if now.wrapping_sub(self.battery.get()) >= 60000 { return Reason::Battery; }
        Reason::None
    }
    fn observed(reason: Reason) -> Stop {
        match reason {
            Reason::None => Stop::None,
            Reason::View | Reason::Input | Reason::DevByte => Stop::Interrupted,
            _ => Stop::Terminal,
        }
    }
    pub(super) fn entry(&self, began: u32) -> crate::reader_book::ProducerEntry {
        use crate::reader_book::ProducerEntry;
        self.began.set(began);
        match self.reason(began) {
            Reason::None => ProducerEntry::Ready,
            Reason::Power | Reason::Card => ProducerEntry::Denied,
            _ => ProducerEntry::Blocked,
        }
    }
    pub(super) fn stop(&self, began: u32) -> Stop {
        self.began.set(began);
        match self.reason(began) {
            Reason::Progress | Reason::Battery => Stop::None,
            reason => Self::observed(reason),
        }
    }
    pub(super) fn pause_request(&self, began: u32, pauses: u8) -> Option<Stop> {
        self.began.set(began);
        match self.reason(began) {
            Reason::Progress | Reason::Battery if pauses < 8 => None,
            reason => Some(Self::observed(reason)),
        }
    }
    pub(super) fn yield_now(&self) {
        self.duty.visit_producer(&|| self.stop(self.began.get()) != Stop::None);
    }
    /// Frozen trace boundaries, outside the producer's parser and file loans.
    #[cfg(all(feature = "rust-app", feature = "dev-usb"))]
    pub(super) fn trace(&self, event: crate::reader_book::ProducerEvent) {
        use crate::{app_runtime::trace::row, reader_book::ProducerEvent};
        let at = self.clock.millis();
        match event {
            ProducerEvent::Admission { result, units } => {
                row(self.console, 80, format_args!("PC admit: at={} r={} u={}\n", at, result as u8, units));
            }
            ProducerEvent::Start { spine, ahead } => {
                row(self.console, 64, format_args!("PC start: at={} from={} next={}\n", at, spine, ahead));
            }
            ProducerEvent::Finish { status, interrupted } => {
                row(self.console, 96, format_args!("PC finish: at={} status={} interrupted={}\n", at, status as u8, u8::from(interrupted)));
            }
            #[cfg(feature = "page-turn-trace")]
            ProducerEvent::Grant { warm, census_us, files, outcome, units, entries } => {
                row(self.console, 159, format_args!(
                    "PC grant: warm={} census_us={} files={} outcome={} units={} entries={}\n",
                    u8::from(warm), census_us, files, outcome as u8, units, entries));
            }
            #[cfg(feature = "page-turn-trace")]
            ProducerEvent::Segment(seg) => {
                row(self.console, 159, format_args!(
                    "PC seg: idx={} pages={} bytes={} layout_us={} tee_us={} write_us={} fin_us={} commit_us={}\n",
                    seg.idx, seg.pages, seg.bytes, seg.layout_us, seg.tee_us,
                    seg.seal.write_us, seg.seal.finalize_us, seg.seal.commit_us));
                let (heap, largest, stack) = newt_c3_board::hardware::sample_memory_lows();
                row(self.console, 159, format_args!(
                    "PC seal: idx={} read={} crc={} dir={} sync={} verify={} heap={} largest={} stack={}\n",
                    seg.idx, seg.seal.read_us, seg.seal.crc_us, seg.seal.dir_us,
                    seg.seal.sync_us, seg.seal.verify_us, heap, largest, stack));
            }
        }
    }
    pub(super) fn after_duty(&self, deferred: bool, failed: bool) -> Stop {
        // Real failed work wins over input which arrived during its commit.
        if failed { return Stop::Terminal; }
        if self.clock.millis().wrapping_sub(self.battery.get()) >= 60000 { return Stop::Terminal; }
        let reason = self.reason(self.began.get());
        if matches!(reason, Reason::Input | Reason::DevByte | Reason::View) {
            return Self::observed(reason);
        }
        if deferred { return Stop::Interrupted; }
        Self::observed(reason)
    }
}
