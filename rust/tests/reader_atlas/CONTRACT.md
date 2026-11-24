# Reading atlas parity contract

The canonical `newt-reader-atlas` leaf implements the pinned Newt reading font
against the shared Core `Font` trait. No C++ font backend or allocation is used
by the Rust implementation. Algorithm and font data retain the staged source
bytes; this adoption changes package paths and target wiring only. Newt source
is pinned at bc9ffda52b85f277e46c63718ac88d62fba5d368; SDK source is pinned at
cd6f5b831c8fad899723b6d406586f00c575f021. `reader-atlas/provenance.json` records
original paths/digests. The production generator reads the pinned font input
under `reader-atlas/source/`; it does not depend on a test reference tree.
MIT and OFL notices accompany the sources and all four font faces.

The frozen corpus holds 8,471 independent requests, including full packed-frame
expectations, across four faces, fallback/ignorable/spacing codepoints, kerning,
scaling, clipping and orientations. Canonical Rust must satisfy the checked-in
metrics/status/frame outputs captured from C++; no reference compiles at test
time. Fourteen fresh semantic
mutants must compile, execute without diagnostics and produce valid protocol
before their designated metric or frame change counts as detection. The corpus draws only at 30 px, so native-size mutants live in
`unit_controls.json`: each compiles cleanly, then must fail its named unit
test with its witness. Rejection
fixtures and response-schema controls are separate from semantic kills.

Run `gate.py --root "$PWD" --output "$PWD/.build/rust/atlas-canonical-UNIQUE"`
under the shared machine flock and the standing offline two-job allowance. It
uses the canonical workspace lock and release profile for RV32 atlas/staticlib
compilation and a borrowed-input/output C retention caller. `--positive-only`
or `--only NAME` is diagnostic and cannot establish full control acceptance.
After successful source checks and receipt publication, passing source/build
scratch is deleted; logs, bindings and evidence remain. Failed runs are
reported before retry, then retained as logs according to the scratch policy.

A retained RV32 ELF proves compilation/linking and observable runtime-dependent
code retention only. Target execution, firmware linkage, heap/stack margins,
firmware size and device performance are pending. Historical staged receipts
do not validate this canonical adoption or its LTO settings.
