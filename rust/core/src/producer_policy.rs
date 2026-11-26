// MIT. Chapter cache producer cancellation and admission policy.
//! One synchronous producer attempt, above the authenticated source boundary.
#![forbid(unsafe_code)]
use core::cell::Cell;
use crate::xml_session::ProcessingPolicy;
use crate::zip_window::source::BookSource;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Stop {
    None,
    Interrupted,
    Terminal,
}

/// Callbacks may not reenter a reader or close its I/O. A pause runs only
/// after the parser step unwinds; it may observe this same attempt's latch.
pub struct Hooks<'ctx, C: ?Sized> {
    pub context: &'ctx C,
    pub stop: Option<fn(&C) -> Stop>,
    pub yield_now: Option<fn(&C)>,
    pub pause: Option<fn(&C, &Cancellation<'ctx, C>) -> Stop>,
}

/// No source, parser or I/O owner is retained. Shared borrows expose one Cell
/// to the XML policy and source wrapper without cloning cancellation state.
pub struct Cancellation<'ctx, C: ?Sized> {
    hooks: Hooks<'ctx, C>,
    reason: Cell<Stop>,
}

impl<'ctx, C: ?Sized> Cancellation<'ctx, C> {
    pub fn new(hooks: Hooks<'ctx, C>) -> Self {
        Self { hooks, reason: Cell::new(Stop::None) }
    }
    pub fn interrupted(&self) -> bool {
        self.reason.get() == Stop::Interrupted
    }
    pub fn terminal(&self) -> bool {
        self.reason.get() == Stop::Terminal
    }
    /// Unconditionally latch a terminal failure, even over an earlier interruption.
    pub fn fail_terminal(&self) {
        self.reason.set(Stop::Terminal);
    }
    pub fn stopped(&self) -> bool {
        if self.reason.get() == Stop::None {
            self.reason.set(self.hooks.stop.map_or(Stop::Terminal,
                |stop| stop(self.hooks.context)));
        }
        self.reason.get() != Stop::None
    }
    pub fn checkpoint(&self) -> bool {
        if self.stopped() {
            return false;
        }
        if let Some(yield_now) = self.hooks.yield_now {
            yield_now(self.hooks.context);
        }
        !self.stopped()
    }
    pub fn pause_point(&self) -> bool {
        self.pause_point_with(|same| self.hooks.pause.map_or(Stop::None,
            |pause| pause(self.hooks.context, same)))
    }
    /// A short native-owner loan uses the same ordering and the same latch.
    pub fn pause_point_with(&self, pause: impl FnOnce(&Self) -> Stop) -> bool {
        if !self.checkpoint() { return false; }
        let reason = pause(self);
        // Terminal failures from pause override any concurrently latched
        // interruption; otherwise preserve the first latched reason.
        if reason == Stop::Terminal || self.reason.get() == Stop::None {
            self.reason.set(reason);
        }
        self.checkpoint()
    }
    pub fn result(&self, ok: bool) -> bool {
        if !ok && self.reason.get() == Stop::None {
            self.reason.set(Stop::Terminal);
        }
        ok
    }
    pub fn step(&self, operation: impl FnOnce() -> bool) -> bool {
        if !self.checkpoint() {
            return false;
        }
        self.result(operation()) && self.checkpoint()
    }
}

/// XmlStream owns the mutable reference value, while the source borrows the
/// same immutable cancellation owner. Neither gets a copied latch.
impl<C: ?Sized> ProcessingPolicy for &Cancellation<'_, C> {
    fn cancelled(&mut self) -> bool {
        !self.checkpoint()
    }
}

/// A producer revision's private receipt authority, never a general book read.
/// Implementations must settle and restart the actual immutable receipt scope.
pub trait ProducerReceipt {
    fn producer_receipt_live(&mut self) -> bool;
}

#[derive(Clone, Copy)]
pub enum ProducerCheck { Source, Receipt }

pub struct ProducerBookSource<'loan, 'ctx, C: ?Sized, S: BookSource + ?Sized> {
    source: &'loan mut S,
    cancellation: &'loan Cancellation<'ctx, C>,
}

impl<'loan, 'ctx, 'pause, C: ?Sized, S: BookSource + ?Sized>
    ProducerBookSource<'loan, 'ctx, C, S>
{
    pub fn new(source: &'loan mut S, cancellation: &'loan Cancellation<'ctx, C>) -> Self {
        Self { source, cancellation }
    }
    /// Frozen finishWhole uses one cancellation step around the ORIGINAL
    /// source's size/probe; going through this adapter would add read polls.
    pub fn validate_source(&mut self, expected_bytes: u64) -> bool {
        self.cancellation.step(|| crate::zip_window::source::validate(
            self.source, expected_bytes,
        ) == crate::zip_container::Status::Ok)
    }
}

impl<C: ?Sized, S: BookSource + ?Sized> BookSource for ProducerBookSource<'_, '_, C, S> {
    fn size(&self) -> u64 {
        self.source.size()
    }
    fn read_at(&mut self, offset: u64, output: &mut [u8]) -> i32 {
        if !self.cancellation.checkpoint() {
            return -1;
        }
        let want = output.len().min(2048);
        let got = self.source.read_at(offset, &mut output[..want]);
        let ok = got >= 0 && got as usize <= want
            && (got > 0 || want == 0 || offset >= self.source.size());
        if self.cancellation.result(ok) && self.cancellation.checkpoint() {
            got
        } else {
            -1
        }
    }
}

impl<C: ?Sized> crate::xml_session::SourcePolicy for Cancellation<'_, C> {
    fn cancelled(&self) -> bool { !self.checkpoint() }
    fn result(&self, ok: bool) -> bool { Cancellation::result(self, ok) }
}

impl<C: ?Sized, S: BookSource + ProducerReceipt + ?Sized> ProducerBookSource<'_, '_, C, S> {
    /// One dispatcher serializes source and receipt checks on the same owner.
    pub fn with_checks<R>(&mut self, bytes: u64, wrapped: bool,
        operation: impl FnOnce(&mut dyn FnMut(ProducerCheck) -> bool) -> R,
    ) -> R {
        operation(&mut |kind| match kind {
            ProducerCheck::Source if wrapped => crate::zip_window::source::validate(self, bytes)
                == crate::zip_container::Status::Ok,
            ProducerCheck::Source => crate::zip_window::source::validate(self.source, bytes)
                == crate::zip_container::Status::Ok,
            ProducerCheck::Receipt => self.source.producer_receipt_live(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_failures_override_interrupted_monotonically() {
        let stop = Cell::new(Stop::Interrupted);
        let cancel = Cancellation::new(Hooks {
            context: &stop,
            stop: Some(|s: &Cell<Stop>| s.get()),
            yield_now: None,
            pause: None,
        });
        assert!(cancel.stopped());
        assert!(cancel.interrupted());
        assert!(!cancel.terminal());
        // Ordinary result(false) preserves an already-latched interruption.
        assert!(!cancel.result(false));
        assert!(cancel.interrupted());
        assert!(!cancel.terminal());
        // fail_terminal overrides a latched interruption.
        cancel.fail_terminal();
        assert!(cancel.terminal());
        assert!(!cancel.interrupted());

        // pause_point_with returning Stop::Terminal overrides an interruption
        // latched during the pause callback.
        stop.set(Stop::None);
        let cancel = Cancellation::new(Hooks {
            context: &stop,
            stop: Some(|s: &Cell<Stop>| s.get()),
            yield_now: None,
            pause: None,
        });
        assert!(!cancel.pause_point_with(|same| {
            stop.set(Stop::Interrupted);
            assert!(same.stopped());
            assert!(same.interrupted());
            Stop::Terminal
        }));
        assert!(cancel.terminal());
        assert!(!cancel.interrupted());
        // A later pause cannot downgrade a latched terminal stop.
        assert!(!cancel.pause_point_with(|_| Stop::Interrupted));
        assert!(cancel.terminal());

        // With no prior latch, result(false) latches Stop::Terminal.
        stop.set(Stop::None);
        let cancel = Cancellation::new(Hooks {
            context: &stop,
            stop: Some(|s: &Cell<Stop>| s.get()),
            yield_now: None,
            pause: None,
        });
        assert!(!cancel.result(false));
        assert!(cancel.terminal());
    }
}
