use newt_core::{durable::*, preferences::Preferences, progress::Progress, wire};

#[test]
fn independent_checksum_vectors() {
    assert_eq!(wire::crc32(b"123456789"), 0xcbf43926);
    assert_eq!(wire::crc32c(b"123456789"), 0xe3069283);
    assert_eq!(wire::fnv1a(b"hello"), 0x4f9f2cab);
}

#[test]
fn hostile_lengths_never_index_before_validation() {
    for length in 0..320 {
        for tag in [b"NWTR", b"NWTP", b"NWTZ", b"NWTC"] {
            let mut bytes = [0xff; 320];
            bytes[..4].copy_from_slice(tag);
            let _ = Progress::decode(&bytes[..length]);
            let _ = Preferences::decode(&bytes[..length]);
            let _ = TimeZoneCodec::decode(&bytes[..length]);
            let _ = ClockFormatCodec::decode(&bytes[..length]);
            let _ = SleepScreenCodec::decode(&bytes[..length]);
        }
    }
}

#[test]
fn session_credit_is_never_durable() {
    let value = Progress {
        book: u64::MAX,
        session_milliseconds: 59999,
        ..Default::default()
    };
    assert_eq!(
        Progress::decode(&value.encode().unwrap())
            .unwrap()
            .session_milliseconds,
        0
    );
}
