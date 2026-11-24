// SPDX-License-Identifier: MIT
use crate::{
    reader_restore_policy::{DurableCheckpoint, boot_restore_policy, pending_recovery},
    retained_record::{Recovery, SleepRecord},
};
use newt_reader_ui::RestorePolicy;

pub fn check() {
    use DurableCheckpoint::{Anchor, PageStart};
    use Recovery::{Ordinary, PendingAnchor, PendingExact};
    use RestorePolicy::{AllowInitialFallback, ColdCheckpoint, ExactPage, Validated};

    for (durable, bits, expected) in [
        (Some(PageStart), Ordinary as u32, ColdCheckpoint),
        (Some(PageStart), PendingAnchor as u32, ColdCheckpoint),
        (Some(Anchor), PendingExact as u32, Validated),
        (None, Ordinary as u32, ExactPage),
        (None, PendingExact as u32, ExactPage),
        (None, PendingAnchor as u32, Validated),
        (None, 3, ExactPage),
    ] {
        let record = SleepRecord { recovery: bits | 12, ..SleepRecord::default() };
        assert_eq!(boot_restore_policy(durable, record.recovery_kind()), expected,
            "startup restore policy (recovery bits={bits})");
    }

    for (policy, pending, wake, reload, fallback, exact, evidence) in [
        (AllowInitialFallback, PendingExact, ExactPage, true, true, false, 0),
        (ExactPage, PendingExact, ExactPage, false, false, true, 1),
        (Validated, PendingAnchor, Validated, true, false, false, 2),
        (ColdCheckpoint, PendingAnchor, Validated, false, false, false, 3),
    ] {
        let saved = pending_recovery(policy);
        assert_eq!(saved, pending, "checkpoint policy after an interrupted {policy:?} open");
        assert_eq!(boot_restore_policy(None, Some(saved)), wake, "restore after sleeping");
        assert_eq!(policy.reloads_saved_progress(), reload);
        assert_eq!(policy.allows_initial_fallback(), fallback);
        assert_eq!(policy.requires_exact_page(), exact);
        assert_eq!(policy as u8, evidence);
    }
    println!("PASS firmware boot and pending checkpoint policies");
}
