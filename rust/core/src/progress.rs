//! NWTR v1 progress records and modular two-slot arbitration.
//! Source: include/newt/reading_progress.h and src/reading_progress.cpp.
use crate::wire::{crc32, get32, put32};

pub const BYTES: usize = 52;
pub const NAMESPACE: &str = "newt_reader";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Locator {
    pub spine: u32,
    pub character: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Progress {
    pub sequence: u32,
    /// RAM only, deliberately excluded from the wire encoding.
    pub session_milliseconds: u32,
    pub book: u64,
    pub locator: Locator,
    pub active_milliseconds: u64,
    pub forward_pages: u32,
    pub qualified_sessions: u32,
}

/// Frozen SampleReader initialProgress: only this book's saved session enters
/// its tracker. Both embedded and external storage use this same value policy.
pub fn initial_progress(saved: Progress, identity: u64) -> Progress {
    if saved.book == identity { saved }
    else { Progress { book: identity, ..Progress::default() } }
}

impl Progress {
    pub fn encode(&self) -> Option<[u8; BYTES]> {
        if self.book == 0 {
            return None;
        }
        let mut bytes = [0; BYTES];
        bytes[..4].copy_from_slice(b"NWTR");
        put32(&mut bytes, 4, 1);
        put32(&mut bytes, 8, self.sequence);
        bytes[12..20].copy_from_slice(&self.book.to_le_bytes());
        put32(&mut bytes, 20, self.locator.spine);
        put32(&mut bytes, 24, self.locator.character);
        bytes[28..36].copy_from_slice(&self.active_milliseconds.to_le_bytes());
        put32(&mut bytes, 36, self.forward_pages);
        put32(&mut bytes, 40, self.qualified_sessions);
        let checksum = crc32(&bytes[..48]);
        put32(&mut bytes, 48, checksum);
        Some(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != BYTES
            || &bytes[..4] != b"NWTR"
            || get32(bytes, 4) != 1
            || get32(bytes, 44) != 0
            || get32(bytes, 48) != crc32(&bytes[..48])
        {
            return None;
        }
        let book = u64::from_le_bytes(bytes[12..20].try_into().ok()?);
        if book == 0 {
            return None;
        }
        Some(Self {
            sequence: get32(bytes, 8),
            session_milliseconds: 0,
            book,
            locator: Locator {
                spine: get32(bytes, 20),
                character: get32(bytes, 24),
            },
            active_milliseconds: u64::from_le_bytes(bytes[28..36].try_into().ok()?),
            forward_pages: get32(bytes, 36),
            qualified_sessions: get32(bytes, 40),
        })
    }

    fn same_state(&self, other: &Self) -> bool {
        self.book == other.book
            && self.locator == other.locator
            && self.active_milliseconds == other.active_milliseconds
            && self.forward_pages == other.forward_pages
            && self.qualified_sessions == other.qualified_sessions
    }

    /// Whether this loaded durable record supersedes a warm or pending
    /// `candidate` for the same book.
    ///
    /// `attempted_sequence` is the highest sequence `candidate`'s session has
    /// attempted to write to durable storage (confirmed or not). When
    /// `candidate_saved` is true, durable wins on any strictly newer modular
    /// sequence (including exact-antipode ambiguity) or an equal-sequence state
    /// mismatch. When `candidate_saved` is false, durable wins only when its
    /// sequence is strictly past both `candidate.sequence` and
    /// `attempted_sequence`, or when durable confirms this exact live state at
    /// or ahead of `candidate.sequence` (reconciling a lost acknowledgement).
    pub fn supersedes(
        &self,
        candidate: &Self,
        candidate_saved: bool,
        attempted_sequence: u32,
    ) -> bool {
        if self.book == 0 || self.book != candidate.book {
            return false;
        }
        let floor = if !candidate_saved
            && attempted_sequence.wrapping_sub(candidate.sequence) < 0x8000_0000
        {
            attempted_sequence
        } else {
            candidate.sequence
        };
        let delta = self.sequence.wrapping_sub(floor);
        if delta != 0 && delta <= 0x8000_0000 {
            return true;
        }
        if candidate_saved {
            delta == 0 && !self.same_state(candidate)
        } else {
            self.sequence.wrapping_sub(candidate.sequence) < 0x8000_0000
                && self.same_state(candidate)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Selection {
    None,
    First(Progress),
    Second(Progress),
    Ambiguous,
}

pub fn select(first: &[u8], second: &[u8]) -> Selection {
    match (Progress::decode(first), Progress::decode(second)) {
        (None, None) => Selection::None,
        (Some(a), None) => Selection::First(a),
        (None, Some(b)) => Selection::Second(b),
        (Some(a), Some(b)) => {
            let delta = a.sequence.wrapping_sub(b.sequence);
            if delta == 0x8000_0000 || (delta == 0 && first != second) {
                Selection::Ambiguous
            } else if delta < 0x8000_0000 {
                Selection::First(a)
            } else {
                Selection::Second(b)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SavePlan {
    pub target_slot: usize,
    pub value: Progress,
}

/// Planning alone does not acknowledge a save. The durable store must still
/// reject present-but-unreadable storage, commit, and verify exact readback.
pub fn plan(first: &[u8], second: &[u8], proposed: Progress) -> Option<SavePlan> {
    if proposed.book == 0 {
        return None;
    }
    let (target_slot, sequence) = match select(first, second) {
        Selection::None => (0, 1),
        Selection::First(value) => (1, value.sequence.wrapping_add(1)),
        Selection::Second(value) => (0, value.sequence.wrapping_add(1)),
        Selection::Ambiguous => return None,
    };
    Some(SavePlan {
        target_slot,
        value: Progress {
            sequence,
            ..proposed
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(sequence: u32, character: u32) -> Progress {
        Progress {
            sequence,
            session_milliseconds: 0,
            book: 0x1122_3344_5566_7788,
            locator: Locator { spine: 1, character },
            active_milliseconds: 1000,
            forward_pages: 2,
            qualified_sessions: 1,
        }
    }

    #[test]
    fn durable_supersedes_by_modular_sequence_and_saved_mismatch() {
        assert!(sample(6, 500).supersedes(&sample(5, 100), true, 5));
        assert!(sample(6, 500).supersedes(&sample(5, 100), false, 5));
        assert!(sample(0, 500).supersedes(&sample(u32::MAX, 100), false, u32::MAX));
        assert!(!sample(u32::MAX, 100).supersedes(&sample(0, 500), true, 0));
        assert!(sample(0x8000_0005, 500).supersedes(&sample(5, 100), false, 5));
        assert!(!sample(5, 100).supersedes(&sample(5, 500), false, 5));
        assert!(sample(5, 500).supersedes(&sample(5, 100), true, 5));
        assert!(!sample(5, 500).supersedes(&sample(5, 500), true, 5));
        let with_session = Progress { session_milliseconds: 999, ..sample(5, 500) };
        assert!(!with_session.supersedes(&sample(5, 500), true, 5));
        assert!(!Progress { book: 0, ..sample(6, 500) }.supersedes(&sample(5, 100), true, 5));
        assert!(!Progress { book: 9, ..sample(6, 500) }.supersedes(&sample(5, 100), true, 5));
    }

    #[test]
    fn unconfirmed_candidate_uses_attempted_sequence_provenance() {
        // Lost ack at seq 3 (char 1123), then later forward (1686) or backward (0)
        // turn whose write failed: durable seq 3 must not replace the live page.
        assert!(!sample(3, 1123).supersedes(&sample(2, 1686), false, 3));
        assert!(!sample(3, 1123).supersedes(&sample(2, 1686), false, 4));
        assert!(!sample(3, 1123).supersedes(&sample(2, 0), false, 3));
        assert!(!sample(3, 1123).supersedes(&sample(2, 0), false, 4));
        // Lost ack with no later page change reconciles and promotes to saved.
        assert!(sample(3, 1123).supersedes(&sample(2, 1123), false, 3));
        assert!(sample(3, 1123).supersedes(&sample(2, 1123), false, 4));
        // Durable strictly past the attempted mark still supersedes stale unsaved RTC (#111).
        assert!(sample(4, 1123).supersedes(&sample(2, 1686), false, 3));
        assert!(sample(5, 1123).supersedes(&sample(2, 1686), false, 4));
        // Modular wrap across u32::MAX -> 0 for attempted_sequence.
        assert!(!sample(0, 1123).supersedes(&sample(u32::MAX, 1686), false, 0));
        assert!(sample(0, 1123).supersedes(&sample(u32::MAX, 1123), false, 0));
        assert!(sample(1, 1123).supersedes(&sample(u32::MAX, 1686), false, 0));
        // Durable older than candidate.sequence never downgrades even on matching state.
        assert!(!sample(1, 500).supersedes(&sample(2, 500), false, 3));
        // Stale/behind attempted_sequence falls back to candidate.sequence.
        assert!(sample(3, 1123).supersedes(&sample(2, 1686), false, 1));
    }
}
