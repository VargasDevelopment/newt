//! Chapter cache producer retry and view state; caller owns the one 256-byte region.
#![forbid(unsafe_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingProbe {
    Ready,
    Complete,
    Stopped,
    Denied,
}


pub const LEDGER_BYTES: usize = 256;
const COMPLETE: u16 = 0x4000;
const DENIED: u16 = 0x8000;
const PREFETCH_TRY: u16 = 0x1000;
const PREFETCH_TRIES: u16 = 0x3000;
const AHEAD_STEP: u16 = 0x0400;
const AHEAD_MASK: u16 = 0x0c00;
const SPINE_MASK: u16 = AHEAD_STEP - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Admission { Pending, Denied, Spent, Granted, Stopped }

impl Admission {
    /// A completed probe or interruption starts the frozen idle settle again.
    pub fn quiet_after_admission(self, stopped: bool, now: impl FnOnce() -> u32) -> Option<u32> {
        ((self != Self::Granted || stopped) && self != Self::Denied).then(now)
    }
}

/// Successful and terminal attempts retain quiet; only interruption renews it.
pub fn quiet_after_attempt(successful: bool, interrupted: bool,
    now: impl FnOnce() -> u32,
) -> Option<u32> {
    (!successful && interrupted).then(now)
}

/// Snapshot of the one reader/session, taken outside parser loans.
#[derive(Clone, Copy)]
pub struct Position {
    pub spine: u32,
    pub spines: u32,
    pub verified: bool,
}

/// Borrowing this region excludes a deferred-digest view over the same bytes.
/// The owner constructs the visit only after full source verification. A new
/// visit clears counters on its first eligible idle call, never per attempt.
pub struct ProducerVisit<'region> {
    ledger: &'region mut [u8; LEDGER_BYTES],
    view: u16,
    admission: Admission,
    visited: bool,
}
impl<'region> ProducerVisit<'region> {
    pub fn new(ledger: &'region mut [u8; LEDGER_BYTES]) -> Self {
        Self { ledger, view: u16::MAX, admission: Admission::Pending, visited: false }
    }
    /// Relend the existing session word and selection flags without clearing
    /// their shared ledger. The caller returns these parts at every exit.
    pub fn from_parts(ledger: &'region mut [u8; LEDGER_BYTES], view: u16,
        admission: Admission, visited: bool,
    ) -> Self {
        Self { ledger, view, admission, visited }
    }
    pub fn into_parts(self) -> (u16, Admission, bool) {
        (self.view, self.admission, self.visited)
    }
    pub fn admission(&self) -> Admission { self.admission }
    pub fn target(&self, position: Option<Position>) -> u32 {
        let Some(p) = position else { return u32::MAX; };
        if u32::from(self.view & SPINE_MASK) == p.spine && self.view & COMPLETE != 0 {
            p.spine.wrapping_add(1).wrapping_add(u32::from((self.view & AHEAD_MASK) / AHEAD_STEP))
        } else { p.spine }
    }
    pub fn tries(&self, position: Option<Position>, spine: u32) -> u8 {
        let Some(p) = position else { return 3; };
        let spine = if spine == u32::MAX { p.spine } else { spine };
        if !p.verified || spine >= p.spines || spine >= (LEDGER_BYTES * 4) as u32 { return 3; }
        (self.ledger[spine as usize / 4] >> ((spine % 4) * 2)) & 3
    }
    pub fn deny_view(&mut self) { self.view |= DENIED; }
    pub fn deny_current_admission(&mut self) {
        self.deny_view();
        self.admission = Admission::Denied;
    }
    pub fn count_try(&mut self, position: Option<Position>, terminal: bool, spine: u32) {
        let Some(p) = position else { return; };
        let spine = if spine == u32::MAX { p.spine } else { spine };
        if !p.verified || spine >= p.spines || spine >= (LEDGER_BYTES * 4) as u32 { return; }
        if spine != p.spine {
            if terminal || self.view & PREFETCH_TRIES == 2 * PREFETCH_TRY { self.deny_view(); }
            else { self.view = self.view.wrapping_add(PREFETCH_TRY); }
            return;
        }
        let shift = (spine % 4) * 2;
        let count = if terminal { 3 } else { (self.tries(position, spine) + 1).min(3) };
        let byte = &mut self.ledger[spine as usize / 4];
        *byte = (*byte & !(3 << shift)) | (count << shift);
        if count == 3 && self.view & COMPLETE == 0 { self.deny_view(); }
    }
    pub fn work_pending(&mut self, position: Option<Position>) -> bool {
        let Some(p) = position else { return false; };
        if !p.verified { return false; }
        if self.view == u16::MAX || u32::from(self.view & SPINE_MASK) != p.spine {
            self.view = p.spine as u16;
        }
        let target = self.target(position);
        self.view & DENIED == 0 && target < p.spines &&
            (target == p.spine || self.tries(position, target) < 3)
    }
    pub fn complete(&mut self, position: Option<Position>, spine: u32, short: bool) {
        let Some(p) = position else { return; };
        if spine == p.spine { self.view = spine as u16 | COMPLETE; }
        else if short && self.view & AHEAD_MASK != AHEAD_MASK && spine.wrapping_add(1) < p.spines {
            self.view = self.view.wrapping_add(AHEAD_STEP);
        } else { self.deny_view(); }
    }
    /// Eligible means the actual active/open external reader is this session.
    pub fn pending(&mut self, position: Option<Position>, eligible: bool) -> bool {
        if !eligible || position.is_none_or(|p| !p.verified) { return false; }
        if !self.visited {
            if self.admission == Admission::Denied { return false; }
            self.ledger.fill(0);
            self.view = u16::MAX;
            self.visited = true;
        }
        if !self.work_pending(position) { return false; }
        self.admission = Admission::Pending;
        true
    }
    pub fn deny_admission(&mut self, position: Option<Position>) {
        self.admission = Admission::Denied;
        if self.visited { self.count_try(position, true, self.target(position)); }
    }
    pub fn spent(&mut self) { self.admission = Admission::Spent; }
    pub fn granted(&mut self) { self.admission = Admission::Granted; }
    /// A queued stop before admission spends no try. A stop after a grant
    /// counts the target before withdrawing native credit and checking again.
    pub fn stop_admission(&mut self, position: Option<Position>, eligible: bool,
        withdraw: impl FnOnce(),
    ) {
        if self.admission == Admission::Granted {
            self.count_try(position, false, self.target(position));
            self.admission = Admission::Pending;
        }
        withdraw();
        self.pending(position, eligible);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_and_attempt_returns_renew_only_the_frozen_quiet_cases() {
        use core::cell::Cell;
        for (admission, stopped, renewed) in [
            (Admission::Pending, false, true),
            (Admission::Denied, false, false),
            (Admission::Denied, true, false),
            (Admission::Spent, false, true),
            (Admission::Stopped, false, true),
            (Admission::Granted, false, false),
            (Admission::Granted, true, true),
        ] {
            let reads = Cell::new(0);
            let stamp = admission.quiet_after_admission(stopped, || {
                reads.set(reads.get() + 1);
                u32::MAX - 17
            });
            assert_eq!(stamp, renewed.then_some(u32::MAX - 17), "{admission:?}/{stopped}");
            assert_eq!(reads.get(), u8::from(renewed));
        }
        for (successful, interrupted, renewed) in [
            (true, false, false), (true, true, false),
            (false, false, false), (false, true, true),
        ] {
            let reads = Cell::new(0);
            let stamp = quiet_after_attempt(successful, interrupted, || {
                reads.set(reads.get() + 1);
                1234
            });
            assert_eq!(stamp, renewed.then_some(1234));
            assert_eq!(reads.get(), u8::from(renewed));
        }
    }

    #[test]
    fn returning_a_visit_preserves_attempts_across_short_operations() {
        let position = Some(Position { spine: 2, spines: 8, verified: true });
        let mut ledger = [0xa5; LEDGER_BYTES];
        let mut visit = ProducerVisit::new(&mut ledger);
        assert!(visit.pending(position, true));
        visit.count_try(position, false, 2);
        visit.granted();
        let (view, admission, visited) = visit.into_parts();

        let mut visit = ProducerVisit::from_parts(&mut ledger, view, admission, visited);
        assert_eq!(visit.admission(), Admission::Granted);
        assert_eq!(visit.tries(position, 2), 1);
        assert!(visit.pending(position, true));
        assert_eq!(visit.tries(position, 2), 1);
        visit.count_try(position, false, 2);
        let (view, admission, visited) = visit.into_parts();

        let mut visit = ProducerVisit::from_parts(&mut ledger, view, admission, visited);
        assert_eq!(visit.tries(position, 2), 2);
        visit.count_try(position, true, 2);
        assert_eq!(visit.tries(position, 2), 3);
        assert!(!visit.pending(position, true));
    }

    #[test]
    fn refused_initial_admission_does_not_clear_shared_region() {
        let position = Some(Position { spine: 2, spines: 8, verified: true });
        let mut ledger = [0xa5; LEDGER_BYTES];
        let mut visit = ProducerVisit::from_parts(&mut ledger, u16::MAX, Admission::Denied, false);
        assert!(!visit.pending(position, true));
        assert_eq!(visit.into_parts(), (u16::MAX, Admission::Denied, false));
        assert_eq!(ledger, [0xa5; LEDGER_BYTES]);
    }
}
