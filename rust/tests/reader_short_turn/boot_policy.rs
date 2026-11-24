// SPDX-License-Identifier: MIT
use crate::{
    reader_restore_policy::{DurableCheckpoint, boot_restore_policy, pending_recovery},
    retained_record::{Recovery, SleepRecord},
};
use newt_core::preferences::{Orientation, ViewSettings};
use newt_reader_ui::RestorePolicy;

pub fn check() {
    use DurableCheckpoint::{Anchor, PageStart};
    use Recovery::{Ordinary, PendingAnchor, PendingExact};
    use RestorePolicy::{AllowInitialFallback, ColdCheckpoint, ExactPage, Validated};

    let settings = ViewSettings::default();
    let other = ViewSettings { orientation: Orientation::Portrait, ..settings };
    let record = |bits: u32, under: ViewSettings|
        SleepRecord { recovery: bits | 12, reader_settings: under.packed(), ..SleepRecord::default() };
    for (durable, bits, under, expected) in [
        (Some(PageStart), Ordinary as u32, settings, ColdCheckpoint),
        (Some(PageStart), PendingAnchor as u32, other, ColdCheckpoint),
        (Some(Anchor), PendingExact as u32, settings, Validated),
        (None, Ordinary as u32, settings, ExactPage),
        (None, PendingExact as u32, settings, ExactPage),
        (None, PendingAnchor as u32, settings, Validated),
        (None, 3, settings, ExactPage),
        // A sleep page start saved under other settings reopens at its locator (#107).
        (None, Ordinary as u32, other, ColdCheckpoint),
        (None, PendingExact as u32, other, ColdCheckpoint),
        (None, PendingAnchor as u32, other, Validated),
    ] {
        let retained = durable.is_none().then(|| record(bits, under));
        assert_eq!(boot_restore_policy(durable, retained.as_ref(), settings), expected,
            "startup restore policy (recovery bits={bits}, saved under {:?})", under.orientation);
    }
    assert_eq!(boot_restore_policy(None, None, settings), ExactPage, "no admitted record");

    for (policy, pending, wake, reload, fallback, exact, evidence) in [
        (AllowInitialFallback, PendingExact, ExactPage, true, true, false, 0),
        (ExactPage, PendingExact, ExactPage, false, false, true, 1),
        (Validated, PendingAnchor, Validated, true, false, false, 2),
        (ColdCheckpoint, PendingAnchor, Validated, false, false, false, 3),
    ] {
        let saved = pending_recovery(policy);
        assert_eq!(saved, pending, "checkpoint policy after an interrupted {policy:?} open");
        assert_eq!(boot_restore_policy(None, Some(&record(saved as u32, settings)), settings), wake,
            "restore after sleeping");
        assert_eq!(policy.reloads_saved_progress(), reload);
        assert_eq!(policy.allows_initial_fallback(), fallback);
        assert_eq!(policy.requires_exact_page(), exact);
        assert_eq!(policy as u8, evidence);
    }
    println!("PASS firmware boot and pending checkpoint policies");
}
