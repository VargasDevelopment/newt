# Font-layout integrated gate

`rust/font_layout_gate.py` exposes `check(gate, snapshot, positive_only)` and
`verify_target(gate, artifact)`. The parent owns normal-gate and target-probe wiring.
The snapshot is the actual parent workspace containing `rust/core` and `rust/host`.
No standalone workspace is generated.

The check uses the approved offline tools and shared runner (signal cleanup,
command logs, byte-preserving stdout), two build jobs, two Rust test threads,
`-Dwarnings`, and one 300-second runner deadline. All run artifacts, temporary
files, Cargo homes and caches live beneath a unique directory in `gate.BUILD`.
It compiles frozen original Gpos.cpp/Gsub.cpp with fatal ASAN/UBSAN, freshly
compiles actual newt-core and font-layout-host, and runs the focused
`core/tests/font_layout.rs` test target. It acquires nothing.

Every run, including positive-only runs, requires the entire frozen case
manifest: exact names, SHA-256 input bytes and independently authored expected
reply bytes. The manifest was enumerated from the immutable original packet's
Python corpus definitions, without executing validation. Protocol controls reject
empty/erased corpora, missing cases, duplicates and same-count substitutions of
names, inputs or expectations. Both pristine binaries must match every expected
reply exactly. Raw LF is required; P replies use signed i32 bounds and S replies
use unsigned u32 glyph bounds. Request grammar mirrors the transport, including
canonical hex/u32 glyphs and exact field counts.

Each of 17 semantic controls copies the actual workspace, changes exactly one
unique site in its copied font_layout module, and freshly compiles actual core
and transport into a separate target directory with warnings denied. Detection
requires a successful process exit and exactly one complete valid reply differing
from that witness's original C++ result. Compile failures, crashes, sanitizer
failures, timeouts and malformed replies fail the gate; none count as detections.
Malformed requests are checked separately on both pristine binaries: exactly
exit 2 with empty stdout/stderr. Their raw logs and receipts remain under the run
directory. These checks never run on mutants.

Evidence includes comparisons, positive-case fingerprints, semantic controls,
command receipts, raw log hashes, binary/source hashes and frozen provenance.
The target hook checks exact nonempty defined text symbols
`newt_font_pair_borrowed` and `newt_font_ligature_borrowed` with approved nm;
it does not execute the target.

This is native host raw-table parity evidence only. It provides no hardware,
font-engine integration, resource or performance claim. This source-only harness
has not been compiled or executed by its implementation worker; parent validation
and parent integration remain pending.
