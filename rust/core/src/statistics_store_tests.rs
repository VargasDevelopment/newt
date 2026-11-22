// SPDX-License-Identifier: MIT
// Include as #[cfg(test)] mod stats_persistence_tests in the integrating core.
use crate::reading_stats_store::*;
#[test]
fn provider_failure_preserves_zero_initialized_digest() {
    let mut calls = 0;
    let n = cross_ink_source_namespace(b"source\0ignored", |name, digest| {
        calls += 1;
        assert_eq!(name, b"source");
        assert_eq!(*digest, [0; 32]);
        false
    });
    assert_eq!(calls, 1);
    assert_eq!(n.as_str(), "ci0000000000000");
}
#[test]
fn ignored_provider_status_preserves_partial_output() {
    let n = cross_ink_source_namespace(b"source", |_, digest| {
        digest[0] = 0xab;
        false
    });
    assert_eq!(n.as_str(), "ciab00000000000");
}
#[test]
fn namespace_formats_cover_all_u16_months() {
    for month in 0..=u16::MAX {
        let n = stats_month_namespace(month);
        let b = n.as_str().as_bytes();
        assert_eq!(&b[..2], b"sm");
        assert_eq!(b.len(), 8);
        let year = b[2..6]
            .iter()
            .fold(0u16, |n, &c| n * 10 + u16::from(c - b'0'));
        let m = (b[6] - b'0') * 10 + b[7] - b'0';
        assert_eq!(year, month / 12);
        assert_eq!(u16::from(m), month % 12 + 1);
    }
}
