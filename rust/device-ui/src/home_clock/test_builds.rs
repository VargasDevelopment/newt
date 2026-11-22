// SPDX-License-Identifier: MIT
//! The Developer page's Test builds choice: one bit of the published flags,
//! restored and committed like the other durable choices. Update mode reads
//! it after its own restore_clock.
use super::{HomeClockSource, commit};
use newt_core::durable::{Backend, Commit, TestBuildsCodec, restore};
use newt_core::update_online::TestBuilds;

// Its bit in the published flags, above the Sleep screen's (SLEEP_BITS).
const TEST_BUILDS: u8 = 1 << 5;

impl<C> HomeClockSource<C> {
    pub const fn test_builds(&self) -> TestBuilds {
        if self.flags & TEST_BUILDS != 0 { TestBuilds::On } else { TestBuilds::Off }
    }

    /// Missing, damaged or unreadable storage shows Off, unrepaired.
    #[inline(never)]
    pub(super) fn restore_test_builds<B: Backend>(&mut self, store: &mut B) {
        let on = restore::<B, TestBuildsCodec, 20>(store, TestBuilds::Off as u32) == TestBuilds::On as u32;
        self.publish_test_builds(if on { TestBuilds::On } else { TestBuilds::Off });
    }

    /// As commit_time_zone; an explicit Off supersedes an older On.
    #[inline(never)]
    pub fn commit_test_builds<B: Backend>(&mut self, store: &mut B, choice: TestBuilds) -> Commit {
        let committed = commit::<B, TestBuildsCodec>(store, choice as u32, self.test_builds() as u32);
        if committed == Commit::Saved {
            self.publish_test_builds(choice);
        }
        committed
    }

    fn publish_test_builds(&mut self, choice: TestBuilds) {
        self.flags &= !TEST_BUILDS;
        if choice == TestBuilds::On {
            self.flags |= TEST_BUILDS;
        }
    }
}
