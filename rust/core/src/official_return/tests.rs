extern crate std;
use super::*;
use crate::update_card::slot_installer::identity::newt_app_identity;

fn source() -> NewtAppIdentity { newt_app_identity(c"0.5.1-dev.20261125.1", Some(c"0123456789ab")) }
fn target() -> UpdateManifest {
    let mut out = UpdateManifest { size: 4096, sha256: [7; 32], ..UpdateManifest::default() };
    out.version[..5].copy_from_slice(b"0.5.0"); out
}

#[test]
fn only_compatible_latest_official_on_the_off_channel_can_issue_a_return() {
    let current = source(); let candidate = target();
    assert!(ReturnAuthorization::from_check(&current, &candidate, TestBuilds::Off).is_some());
    assert!(ReturnAuthorization::from_check(&current, &candidate, TestBuilds::On).is_none());
    for version in [c"0.5.1", c"0.5.1-alpha", c"0.5.1-dev.invalid", c"0.5.1-dev.20261125.0"] {
        assert!(ReturnAuthorization::from_check(&newt_app_identity(version, Some(c"0123456789ab")), &candidate, TestBuilds::Off).is_none());
    }
    for version in [b"0.4.1".as_slice(), b"0.5.1-dev.20261124.1", b"0.5.1", b"0.6.0"] {
        let mut wrong = candidate; wrong.version.fill(0); wrong.version[..version.len()].copy_from_slice(version);
        assert!(ReturnAuthorization::from_check(&current, &wrong, TestBuilds::Off).is_none());
    }
}

#[test]
fn cold_restore_requires_the_complete_receipt_same_source_and_same_target() {
    // Only serialized bytes survive; reconstruct all transient owners afterwards.
    let bytes = { ReturnAuthorization::from_check(&source(), &target(), TestBuilds::Off).unwrap().receipt() };
    let restored = ReturnAuthorization::restore(&bytes, &source(), &target(), TestBuilds::Off).unwrap();
    assert!(restored.targets(c"0.5.1-dev.20261125.1", &target()));
    assert!(restored.confirm().matches(&source(), &target()));
    for length in [0, RECEIPT_BYTES - 1] {
        assert!(ReturnAuthorization::restore(&bytes[..length], &source(), &target(), TestBuilds::Off).is_err());
    }
    for index in [0, 4, 8, 32, 48, 80] {
        let mut bad = bytes; bad[index] ^= 1;
        assert!(ReturnAuthorization::restore(&bad, &source(), &target(), TestBuilds::Off).is_err());
    }
    let foreign = newt_app_identity(c"0.5.1-dev.20261125.1", Some(c"abcdef012345"));
    assert!(ReturnAuthorization::restore(&bytes, &foreign, &target(), TestBuilds::Off).is_err());
    let mut changed = target(); changed.sha256[0] ^= 1;
    assert!(ReturnAuthorization::restore(&bytes, &source(), &changed, TestBuilds::Off).is_err());
    assert!(ReturnAuthorization::restore(&bytes, &source(), &target(), TestBuilds::On).is_err());
}

#[test]
fn minimum_official_supports_the_expanded_preference_and_resume_words() {
    use crate::{preferences::{Preferences, ViewSettings, Orientation, TextSize},
        durable::Codec, sleep_intent::{SleepResumeCodec, SleepResumeIntent}, wire::get32};
    assert_eq!(MINIMUM, c"0.5.0");
    for (orientation, text_size, version, packed) in [
        (Orientation::Portrait, TextSize::Px30, 2, 0x500),
        (Orientation::LandscapeCw, TextSize::Px30, 3, 0x511),
        (Orientation::Portrait, TextSize::Px22, 4, 0x160500),
    ] {
        let settings = ViewSettings { orientation, text_size, slim_progress_bar: false, refresh_pages: 5 };
        let record = Preferences { sequence: 42, settings }.encode().unwrap();
        assert_eq!(get32(&record, 4), version);
        assert_eq!(Preferences::decode(&record).unwrap().settings, settings);
        assert_eq!(settings.packed(), packed);
        let intent = SleepResumeIntent { book: 7, checkpoint: 9, settings: packed | 0xc };
        let mut bytes = [0; crate::sleep_intent::BYTES];
        assert!(SleepResumeCodec::encode(intent, 42, &mut bytes));
        assert_eq!(get32(&bytes, 4), 1, "same version, expanded payload semantics");
        assert_eq!(SleepResumeCodec::decode(&bytes), Some(intent));
    }
}
