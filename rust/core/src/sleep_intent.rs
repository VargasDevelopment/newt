//! NWBR v1 durable intent codec from firmware/sleep_resume_store.h (MIT).
//! This module does not implement sleep lifecycle, RTC, or startup wiring.
use crate::durable::{Codec, RecordStore};
use crate::preferences::{PACKED_SETTINGS, ViewSettings};
use crate::progress::Progress;
use crate::wire::{fnv1a, get32, put32};

pub const BYTES: usize = 32;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SleepResumeIntent {
    pub book: u64,
    pub checkpoint: u32,
    pub settings: u32,
}

impl SleepResumeIntent {
    pub fn valid(self) -> bool {
        if self.book == 0 {
            return self.checkpoint == 0 && self.settings == 0;
        }
        // Bit 2 marks a reading intent and bit 3 its page anchor.
        (self.settings & 4) != 0 && (self.settings & !(PACKED_SETTINGS | 0xc)) == 0
            && ViewSettings::unpacked(self.settings).is_some()
    }
}

pub struct SleepResumeCodec;
impl Codec<BYTES> for SleepResumeCodec {
    type Value = SleepResumeIntent;
    const NAMESPACE: &'static str = "newt_resume";

    fn known(bytes: &[u8]) -> bool {
        bytes.len() == BYTES && get32(bytes, 0) == 0x5242_574e && get32(bytes, 4) == 1
    }

    fn decode(bytes: &[u8]) -> Option<SleepResumeIntent> {
        if !Self::known(bytes) || get32(bytes, 28) != fnv1a(&bytes[..28]) {
            return None;
        }
        let value = SleepResumeIntent {
            book: u64::from(get32(bytes, 12)) | (u64::from(get32(bytes, 16)) << 32),
            checkpoint: get32(bytes, 20),
            settings: get32(bytes, 24),
        };
        value.valid().then_some(value)
    }

    fn encode(value: SleepResumeIntent, sequence: u32, bytes: &mut [u8; BYTES]) -> bool {
        if !value.valid() {
            return false;
        }
        put32(bytes, 0, 0x5242_574e);
        put32(bytes, 4, 1);
        put32(bytes, 8, sequence);
        put32(bytes, 12, value.book as u32);
        put32(bytes, 16, (value.book >> 32) as u32);
        put32(bytes, 20, value.checkpoint);
        put32(bytes, 24, value.settings);
        let checksum = fnv1a(&bytes[..28]);
        put32(bytes, 28, checksum);
        true
    }
}

// A corrupt peer might be a newer Home tombstone. Loading an older reading
// generation would resurrect a destination the owner explicitly left.
pub type SleepResumeStore<'a, B> = RecordStore<'a, B, SleepResumeCodec, BYTES, false>;

pub fn progress_checkpoint(saved: &Progress) -> u32 {
    saved.encode().map_or(0, |bytes| get32(&bytes, 48))
}
