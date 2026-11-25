// SPDX-License-Identifier: MIT

//! Durable storage namespace routing grammar.
//! Pure functions: no storage, allocation or I/O.
use crate::reading_stats_store::{CROSS_INK_NAMESPACE, LEDGER_NAMESPACE, STREAK_NAMESPACE};

/// Durable reading, clock, Sleep screen, last-seen build, saved Wi-Fi network,
/// Test builds and Holiday outfits names that route to SD. The retired Home name
/// stays recognized for compatibility; startup never opens or changes it.
const READING_STATE_NAMESPACES: [&str; 10] = [
    "newt_selected",
    "newt_resume",
    "newt_zone",
    "newt_clock",
    "newt_home",
    "newt_seen",
    "newt_net",
    "newt_sleep",
    "newt_builds",
    "newt_holiday",
];
/// CrossInk import's completion record; its global copy is CROSS_INK_NAMESPACE.
const IMPORT_NAMESPACE: &str = "newt_import";

/// `prefix` followed by exactly `digits` characters, each a digit or, if
/// `hex`, a lowercase a-f. Byte-wise like the original's C string scan.
pub fn numbered_namespace(name: &str, prefix: [u8; 2], digits: usize, hex: bool) -> bool {
    let bytes = name.as_bytes();
    bytes.len() == digits + 2
        && bytes[..2] == prefix
        && bytes[2..]
            .iter()
            .all(|&b| b.is_ascii_digit() || (hex && (b'a'..=b'f').contains(&b)))
}

/// Reading statistics: the ledger, closed months `sm<yyyymm>`, per-book facts
/// `sb<13 hex>`, per-book days `sd`/`se<13 hex>`, the streak cache and the
/// CrossInk import (`newt_stats_ci`, `newt_import`, `ci<13 hex>`).
pub fn stats_namespace(name: &str) -> bool {
    name == LEDGER_NAMESPACE
        || name == STREAK_NAMESPACE
        || numbered_namespace(name, *b"sm", 6, false)
        || numbered_namespace(name, *b"sb", 13, true)
        || numbered_namespace(name, *b"sd", 13, true)
        || numbered_namespace(name, *b"se", 13, true)
        || name == CROSS_INK_NAMESPACE
        || name == IMPORT_NAMESPACE
        || numbered_namespace(name, *b"ci", 13, true)
}

/// Every namespace the SD reading-state store owns once attached: the fixed
/// names, per-book progress `nb<13 hex>` and every statistics name.
pub fn reading_state_namespace(name: &str) -> bool {
    READING_STATE_NAMESPACES.contains(&name)
        || numbered_namespace(name, *b"nb", 13, true)
        || stats_namespace(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grammar_matches_the_pinned_routing_table() {
        for fixed in READING_STATE_NAMESPACES {
            assert!(reading_state_namespace(fixed) && !stats_namespace(fixed));
        }
        for name in [
            "nb0123456789abc",
            "sb0123456789abc",
            "ci0000000000000",
            "sm202611",
            "newt_import",
        ] {
            assert!(reading_state_namespace(name), "{name}");
        }
        for name in [
            "sb0123456789abc",
            "sd0123456789abc",
            "se0123456789abc",
            "newt_stats_sk",
        ] {
            assert!(stats_namespace(name), "{name}");
        }
        // Uppercase hex, short/long digit runs, hex months and NVS-only names stay out.
        for name in [
            "nb0123456789ABC",
            "nb0123456789ab",
            "nb0123456789abcd",
            "sm20261a",
            "newt_reader",
            "",
        ] {
            assert!(!reading_state_namespace(name), "{name}");
        }
    }
}
