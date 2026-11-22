# Provenance and adopted path map

The table and preparation paths below are historical input records. The adopted
crate contains `src/lib.rs`, `src/probe.rs`, `native/expat_bridge.c`, Cargo.toml,
SAFETY.md and the license directory. Historical host/test files are not adopted
here. The standalone C++ parity scripts (`menu.py` and `semantic.py`) were
retired; the gate exercises `newt-target-expat` via `rust/landing.py`
(`rust/tests/phased_reader/native.py`) and `tools/firmware_host_build.py`
against the existing pinned SDK.


FreeInk SDK pin: `cd6f5b831c8fad899723b6d406586f00c575f021`.
Actual-root authored Rust sources are the read-only inputs provided to this
implementation task (parent identifies its aggregate as Root334b2025).
Hashes below record read input bytes; no build/test outcome is implied.

| Owned path | Read-only source | Treatment | Source SHA-256 |
| --- | --- | --- | --- |
| `src/lib.rs` | `rust/host/src/xml_session/expat.rs` | Derived; manual ABI and adapter behavior retained; crate/module docs and formatting adapted | `ebf6159fdf253eb95f51be5b93e7306b80c511adbace2340ae9b992ec82b719d` |
| `native/expat_bridge.c` | `rust/tests/xml_session/expat_bridge.c` | Byte-identical copy, including original comment | `38fe9df1dfd3da94c03bb1c72b2e07a6372efcfe0675432a019e5d47735487eb` |
| `tests/host_reference.rs` | `rust/host/src/xml_session/native_tests.rs` | Derived; host-only integration test and adapter import | `12a1def0ad9fa806630e5c6ff87f77369382966d8d18dc171b29364f85dbbe97` |
| root `LICENSE` | `LICENSE` | Byte-identical notice | `6ae1f762295ed73a4694bc0ad106735ad42c0ad5ff5c57573d70b73476862ec5` |
| `licenses/FreeInk-MIT` | `vendor/freeink-sdk/LICENSE` | Byte-identical notice | `17dea5079d9905fbbc8323cde7bfffd0f24185949c43a5daad5177022b796588` |
| `licenses/Expat-COPYING` | `vendor/freeink-sdk/libs/book/FreeInkBook/third_party/expat/COPYING` | Byte-identical notice | `31b15de82aa19a845156169a17a5488bf597e561b2c318d159ed583139b25e87` |

`src/probe.rs`, `host/main.rs`, `host/regressions.rs`, `tests/driver.rs`,
`Cargo.toml` and the handoff documents are new authored MIT material.
The standalone crate is Newt MIT; linked Expat retains its own full COPYING
notice and original source notices. No vendor file is changed or vendored anew.

Core API path map (read-only, never copied into this leaf):

| Consumer | Actual Core source |
| --- | --- |
| Backend/Attributes/Handler/producer/session/policy | `rust/core/src/xml_session.rs` |
| Attribute local/exact rules and entity filtering | `rust/core/src/xml_input.rs` |
| Entry/Reader/InflateScratch/Status | `rust/core/src/zip_container.rs` |
| Source callback | `rust/core/src/zip_window/source.rs` |
| Two-pass package workflow/storage/result | `rust/core/src/package_parsers.rs` |
| Arena and typed string handles | `rust/core/src/zip_window/arena.rs` |

Native inputs stay in `vendor/freeink-sdk/libs/book/FreeInkBook/`:
`src/vendor/expat_xmlparse.c`, `expat_xmltok.c`, `expat_xmlrole.c`, and
`third_party/expat/` headers plus included source closure. Existing frozen
`rust/tests/xml_session/PROVENANCE.json` records the SDK pin and that closure,
including the Expat COPYING notice. Reuse that existing provenance discipline;
this packet creates no replacement machine-readable inventory.

The old host adapter is a behavioral reference, not a second linked backend.
Current safety boundaries are documented in SAFETY.md; focused comparison
receipts distinguish library compilation from target linking and device evidence.
No change to a root source, manifest, lockfile, inventory, guard, SDK or other
staging packet is part of this handoff.
