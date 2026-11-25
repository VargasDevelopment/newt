// SPDX-License-Identifier: MIT
//! Sleep screen "Holiday outfits": three bits packed above TimeZone in
//! `zone_holidays`, restored and committed from the `newt_holiday` card record.
use super::{HomeClockSource, commit};
use crate::HolidayOutfits;
use newt_core::durable::{Backend, Commit, HolidayOutfitsCodec, restore};

const HOLIDAY_SHIFT: u32 = 5;
const HOLIDAY_BITS: u8 = 7 << HOLIDAY_SHIFT;

impl<C> HomeClockSource<C> {
    pub const fn holiday_outfits(&self) -> HolidayOutfits {
        let bits = (self.zone_holidays >> HOLIDAY_SHIFT) & 7;
        HolidayOutfits {
            halloween: bits & 1 != 0,
            thanksgiving: bits & 2 != 0,
            christmas: bits & 4 != 0,
        }
    }

    /// Missing, damaged or unreadable storage shows every outfit Off, unrepaired.
    #[inline(never)]
    pub(super) fn restore_holiday_outfits<B: Backend>(&mut self, store: &mut B) {
        self.publish_holiday_outfits(restore::<B, HolidayOutfitsCodec, 20>(store, 0));
    }

    /// One record holds all three outfits; an explicit all-Off supersedes an
    /// older opt-in.
    #[inline(never)]
    pub fn commit_holiday_outfits<B: Backend>(
        &mut self,
        store: &mut B,
        choices: HolidayOutfits,
    ) -> Commit {
        let intended = holiday_record(choices);
        let committed = commit::<B, HolidayOutfitsCodec>(store, intended, self.holiday_record());
        if committed == Commit::Saved {
            self.publish_holiday_outfits(intended);
        }
        committed
    }

    fn publish_holiday_outfits(&mut self, record: u32) {
        self.zone_holidays = (self.zone_holidays & !HOLIDAY_BITS)
            | (((record as u8) & 7) << HOLIDAY_SHIFT);
    }

    fn holiday_record(&self) -> u32 {
        u32::from((self.zone_holidays >> HOLIDAY_SHIFT) & 7)
    }
}

fn holiday_record(choices: HolidayOutfits) -> u32 {
    u32::from(choices.halloween)
        | (u32::from(choices.thanksgiving) << 1)
        | (u32::from(choices.christmas) << 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use newt_core::durable::{BlobRead, DurableWrite, StorageOpen};
    use newt_core::time_zone::TimeZone;

    struct Slot {
        bytes: [u8; 20],
        len: usize,
    }
    struct MemStore {
        zones: [Slot; 2],
        holidays: [Slot; 2],
        active: u8, // 1 zone, 2 holiday
        fail_writes: bool,
        durable: bool,
    }
    impl MemStore {
        fn new() -> Self {
            Self {
                zones: [Slot { bytes: [0; 20], len: 0 }, Slot { bytes: [0; 20], len: 0 }],
                holidays: [Slot { bytes: [0; 20], len: 0 }, Slot { bytes: [0; 20], len: 0 }],
                active: 0,
                fail_writes: false,
                durable: true,
            }
        }
        fn slots(&mut self) -> Option<&mut [Slot; 2]> {
            match self.active { 1 => Some(&mut self.zones), 2 => Some(&mut self.holidays), _ => None }
        }
    }
    impl Backend for MemStore {
        fn begin(&mut self, namespace: &str, readonly: bool) -> StorageOpen {
            self.active = match namespace {
                "newt_zone" => 1,
                "newt_holiday" => 2,
                _ => return StorageOpen::Missing,
            };
            if readonly && self.slots().is_some_and(|s| s[0].len == 0 && s[1].len == 0) {
                return StorageOpen::Missing;
            }
            StorageOpen::Opened
        }
        fn end(&mut self) { self.active = 0; }
        fn read_blob(&mut self, key: &str, output: &mut [u8]) -> BlobRead {
            let idx = usize::from(key == "slot1");
            let Some(slots) = self.slots() else { return BlobRead::Missing; };
            let len = slots[idx].len;
            if len == 0 { return BlobRead::Missing; }
            output[..len].copy_from_slice(&slots[idx].bytes[..len]);
            BlobRead::Present(len)
        }
        fn put_bytes(&mut self, _: DurableWrite, key: &str, bytes: &[u8]) -> usize {
            if self.fail_writes { return 0; }
            let idx = usize::from(key == "slot1");
            let Some(slots) = self.slots() else { return 0; };
            slots[idx].bytes[..bytes.len()].copy_from_slice(bytes);
            slots[idx].len = bytes.len();
            bytes.len()
        }
        fn readback_durable(&self) -> bool { self.durable }
    }

    #[test]
    fn holiday_outfits_pack_with_zone_and_survive_restore_off_corrupt_and_failure() {
        let mut store = MemStore::new();
        let mut clock = HomeClockSource::<()>::new();
        assert_eq!(clock.holiday_outfits(), HolidayOutfits::default());
        assert_eq!(clock.commit_time_zone(&mut store, TimeZone::AustraliaEastern), Commit::Saved);
        let chosen = HolidayOutfits { halloween: true, thanksgiving: false, christmas: true };
        assert_eq!(clock.commit_holiday_outfits(&mut store, chosen), Commit::Saved);
        assert_eq!(clock.zone(), TimeZone::AustraliaEastern);
        assert_eq!(clock.holiday_outfits(), chosen);
        // Changing zone must keep packed holiday bits intact.
        assert_eq!(clock.commit_time_zone(&mut store, TimeZone::UsPacific), Commit::Saved);
        assert_eq!(clock.holiday_outfits(), chosen);
        // Cold restore into a brand-new HomeClockSource preserves both.
        let mut restored = HomeClockSource::<()>::new();
        restored.restore_preferences(&mut store);
        assert_eq!(restored.zone(), TimeZone::UsPacific);
        assert_eq!(restored.holiday_outfits(), chosen);
        // Failed commit keeps runtime choice unchanged.
        store.fail_writes = true;
        assert_eq!(restored.commit_holiday_outfits(&mut store, HolidayOutfits::default()), Commit::Unconfirmed);
        assert_eq!(restored.holiday_outfits(), chosen);
        // Explicit all-Off persists and overrides prior On on cold restore.
        store.fail_writes = false;
        assert_eq!(restored.commit_holiday_outfits(&mut store, HolidayOutfits::default()), Commit::Saved);
        let mut after_off = HomeClockSource::<()>::new();
        after_off.restore_preferences(&mut store);
        assert_eq!(after_off.holiday_outfits(), HolidayOutfits::default());
        // Corrupt record restores all Off without repairing the bytes.
        assert_eq!(after_off.commit_holiday_outfits(&mut store, chosen), Commit::Saved);
        for slot in &mut store.holidays {
            for b in &mut slot.bytes { *b ^= 0x5a; }
        }
        let snapshot = [store.holidays[0].bytes, store.holidays[1].bytes];
        let mut corrupt = HomeClockSource::<()>::new();
        corrupt.restore_preferences(&mut store);
        assert_eq!(corrupt.holiday_outfits(), HolidayOutfits::default());
        assert_eq!([store.holidays[0].bytes, store.holidays[1].bytes], snapshot);
    }
}
