// SPDX-License-Identifier: MIT
#![forbid(unsafe_code)]
use newt_core::{
    durable::*, reading_stats::*, reading_stats_once::Sha256, reading_stats_service::*,
    reading_stats_store::StatsBackend, stats_clock::StatsClock,
    stats_service_backend::NamespaceBackend, tracker::ReadingDayMark,
};
pub struct Borrowed<'a> {
    pub names: &'a [&'a str],
    pub bytes: &'a mut [[[u8; 180]; 3]],
    pub lengths: &'a mut [[Option<usize>; 3]],
    pub present: &'a mut [bool],
    pub calls: &'a mut [u32; 6],
    pub selected: usize,
    pub fail_read: bool,
    pub fail_write: bool,
}
fn key(k: &str) -> usize {
    match k {
        "digest" => 0,
        "slot0" => 1,
        _ => 2,
    }
}
impl Backend for Borrowed<'_> {
    fn begin(&mut self, n: &str, ro: bool) -> StorageOpen {
        self.calls[0] = self.calls[0].wrapping_add(1);
        let Some(i) = self.names.iter().position(|s| *s == n) else {
            return StorageOpen::Failed;
        };
        self.selected = i;
        if ro && !self.present[i] {
            return StorageOpen::Missing;
        }
        self.present[i] = true;
        StorageOpen::Opened
    }
    fn end(&mut self) {
        self.calls[1] = self.calls[1].wrapping_add(1);
    }
    fn read_blob(&mut self, k: &str, out: &mut [u8]) -> BlobRead {
        self.calls[2] = self.calls[2].wrapping_add(1);
        if self.fail_read {
            return BlobRead::Failed;
        }
        let j = key(k);
        let Some(n) = self.lengths[self.selected][j] else {
            return BlobRead::Missing;
        };
        let count = out.len().min(n).min(180);
        out[..count].copy_from_slice(&self.bytes[self.selected][j][..count]);
        BlobRead::Present(n)
    }
    fn put_bytes(&mut self, _: DurableWrite, k: &str, b: &[u8]) -> usize {
        self.calls[3] = self.calls[3].wrapping_add(1);
        if self.fail_write || b.len() > 180 {
            return 0;
        }
        let j = key(k);
        self.bytes[self.selected][j][..b.len()].copy_from_slice(b);
        self.lengths[self.selected][j] = Some(b.len());
        b.len()
    }
    fn readback_durable(&self) -> bool {
        true
    }
}
impl StatsBackend for Borrowed<'_> {
    fn reading_state_unavailable(&self) -> bool {
        false
    }
    fn set_aside(&mut self, n: &str) -> bool {
        self.calls[4] = self.calls[4].wrapping_add(1);
        let Some(i) = self.names.iter().position(|s| *s == n) else {
            return false;
        };
        self.present[i] = false;
        self.lengths[i] = [None; 3];
        true
    }
    fn repair_in_place(&mut self, n: &str, b: &[u8]) -> bool {
        self.calls[5] = self.calls[5].wrapping_add(1);
        if self.fail_write || b.len() > 180 {
            return false;
        }
        let Some(i) = self.names.iter().position(|s| *s == n) else {
            return false;
        };
        self.present[i] = true;
        self.bytes[i][1][..b.len()].copy_from_slice(b);
        self.lengths[i] = [None, Some(b.len()), None];
        true
    }
}
impl NamespaceBackend for Borrowed<'_> {
    fn for_each_namespace(
        &mut self,
        p: &str,
        visit: &mut dyn FnMut(&mut Self, &str) -> bool,
    ) -> bool {
        for i in 0..self.names.len() {
            let name = self.names[i];
            if self.present[i] && name.starts_with(p) && !visit(self, name) {
                return false;
            }
        }
        true
    }
}
/// Concrete instantiation, not merely a generic declaration. All inputs and
/// storage are borrowed at runtime; outputs force retention of every service.
#[inline(never)]
pub fn probe(
    b: &mut Borrowed<'_>,
    op: u8,
    open: &StatsBook,
    clock: StatsClock,
    mark: &ReadingDayMark,
    cancel: &mut dyn FnMut() -> bool,
    sha: Sha256<'_>,
    summary: &mut StatsSummary,
    facts: &mut BookFacts,
    extras: &mut SleepExtras,
    unsaved: u32,
) -> StatsReport {
    match op {
        0 => seed_stats(b, Some(&open.digest), Some(cancel)),
        1 => fold_stats(b, open, clock, Some(cancel), sha),
        2 => mark_stats(b, &open.digest, clock, StatsMark::Finished, sha),
        3 => fold_marked_stats(b, open, clock, mark, Some(cancel), sha),
        4 => StatsReport {
            result: summarize_stored_stats(b, clock, summary),
            ..StatsReport::default()
        },
        5 => StatsReport {
            result: stored_book_facts(b, &open.digest, facts),
            ..StatsReport::default()
        },
        6 => {
            read_sleep_stats(
                b,
                clock,
                SleepScreenChoices {
                    habit: true,
                    week: true,
                },
                |_| unsaved,
                extras,
            );
            StatsReport {
                result: StatsResult::Done,
                ..StatsReport::default()
            }
        }
        _ => StatsReport {
            result: refresh_sleep_streak(b, clock, Some(cancel)),
            ..StatsReport::default()
        },
    }
}
