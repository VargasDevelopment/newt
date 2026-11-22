//! Quick Start NWQS card-resume record and policy. MIT; see LICENSE.
#![forbid(unsafe_code)]

use crate::durable::{Backend, Codec, Commit, Load, RecordStore};
use crate::wire::fnv1a;

pub const BYTES: usize = 20;
const MAGIC: u32 = 0x5351_574e;
const VERSION: u32 = 1;

/// Durable identifiers: append only. Presentation order is a separate policy.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum Step {
    #[default]
    Welcome = 0,
    Reading = 1,
    TimeZone = 2,
    Wifi = 3,
    Books = 4,
    Done = 5,
}

impl Step {
    pub const fn from_id(id: u32) -> Option<Self> {
        match id {
            0 => Some(Self::Welcome),
            1 => Some(Self::Reading),
            2 => Some(Self::TimeZone),
            3 => Some(Self::Wifi),
            4 => Some(Self::Books),
            5 => Some(Self::Done),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Record {
    pub finished: bool,
    pub step: Step,
    pub from_settings: bool,
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("validated NWQS extent"))
}

pub struct QuickStartCodec;
impl Codec<BYTES> for QuickStartCodec {
    type Value = Record;
    const NAMESPACE: &'static str = "newt_quick";
    const REPLACE_DAMAGED: bool = true;

    fn known(bytes: &[u8]) -> bool {
        bytes.len() == BYTES && word(bytes, 0) == MAGIC && word(bytes, 4) == VERSION
    }

    fn decode(bytes: &[u8]) -> Option<Record> {
        if !Self::known(bytes) || word(bytes, 16) != fnv1a(&bytes[..16]) {
            return None;
        }
        let payload = word(bytes, 12);
        if payload & !0x3ff != 0 {
            return None;
        }
        Some(Record {
            step: Step::from_id(payload & 0xff)?,
            from_settings: payload & 0x100 != 0,
            finished: payload & 0x200 != 0,
        })
    }

    fn encode(record: Record, sequence: u32, bytes: &mut [u8; BYTES]) -> bool {
        let payload = record.step as u32
            | (u32::from(record.from_settings) << 8)
            | (u32::from(record.finished) << 9);
        for (at, value) in [MAGIC, VERSION, sequence, payload].into_iter().enumerate() {
            bytes[at * 4..at * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        let checksum = fnv1a(&bytes[..16]);
        bytes[16..].copy_from_slice(&checksum.to_le_bytes());
        true
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CardHistory {
    Unknown,
    New,
    RanNewt,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Resume {
    pub show: bool,
    pub at: Record,
}

/// The card alone decides; history is consulted only without a loaded record.
pub fn resume<B: Backend>(backend: &mut B, history: impl FnOnce(&mut B) -> CardHistory) -> Resume {
    let loaded = RecordStore::<_, QuickStartCodec, BYTES>::new(backend).load();
    match loaded {
        Load::Loaded(at) => Resume {
            show: !at.finished,
            at,
        },
        _ => Resume {
            show: history(backend) == CardHistory::New,
            at: Record::default(),
        },
    }
}

/// Unconfirmed remains unconfirmed unless the backend vouches for durability.
pub fn commit<B: Backend>(backend: &mut B, record: Record) -> Commit {
    if RecordStore::<_, QuickStartCodec, BYTES>::new(backend).save(record) == Commit::Saved {
        return Commit::Saved;
    }
    if backend.readback_durable()
        && RecordStore::<_, QuickStartCodec, BYTES>::new(backend).load() == Load::Loaded(record)
    {
        Commit::Saved
    } else {
        Commit::Unconfirmed
    }
}

#[cfg(test)]
#[path = "quick_start_tests.rs"]
mod tests;

// Explicit #272 pure policy overlay; durable alpha.15 codec remains above.
#[path = "quick_start_policy.rs"]
mod policy;
pub use policy::{Facts, Intent, Notice, ORDER, State, dispatch, enter_step, orientation, saved};
