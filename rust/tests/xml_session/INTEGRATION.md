# Source-only integration handoff

Implementation is staged inside this candidate only. No compiler, test, gate,
network, device, dependency acquisition or commit was run. Existing rustfmt was
used on the new Rust sources. Python AST parsing is a syntax check only.
The baseline's static Astra review is inherited context, not a review or runtime
validation of this integration.

Root must add a mandatory `xml_session_gate.check(gate, snapshot, positive_only)`
call to the aggregate gate, and `verify_target(gate, artifact)` to target symbol
verification. Neither hook has been added by this worker. `positive_only` skips
only the seven semantic mutants; native builds/tests, complete independent traces,
malformed request/response controls and provenance checks remain mandatory.
Do not call this family validated until the root has compiled and run it.

## Exact logical bindings

- SDK XmlSax.cpp/h lifecycle → `core/src/xml_session.rs`, exported by
  `core/src/lib.rs` as `xml_session`. Internal imports use `crate`.
- Baseline lifecycle witnesses → `core/tests/xml_session_lifecycle.rs`. Ordinary
  `cargo test -p newt-core` includes these without any native feature or flags.
  These mocks cover scheduling/admission/cleanup, not native allocator failures.
- Baseline host transport → `host/src/bin/xml-session-host.rs`, the actual
  `newt-parity` bin. `host/Cargo.toml` requires `xml-native`; defaults are empty.
- Native adapter → `host/src/xml_session/expat.rs`; native tests are a test module
  of that host bin, `host/src/xml_session/native_tests.rs`.
- Native bridge → `tests/xml_session/expat_bridge.c`. Rust links ONLY this bridge
  and the three frozen Expat C wrappers. There is no C++ reader/filter/session in
  the Rust process, no system Expat, crate acquisition or build.rs.
- Original oracle → `tests/xml_session/oracle/main.cpp` plus complete frozen
  XmlSax.cpp, ZipCatalog.cpp, EntityFilter.cpp, miniz and Expat sources.
- Borrowed RV32 path → `target-probe/src/xml_session_probe.rs`, appended as a module
  in target `lib.rs`, with nonempty symbol `newt_rust_xml_session_probe`. Runtime
  caller supplies dyn BookSource, dyn ParserBackend, dyn Handler, entry, decoder,
  filter and buffer. This proves no target-native XML parser.

The receipt records the live source inventory used by the run. PROVENANCE.json binds the
whole frozen closure and both separate Newt producer reference files to exact
bytes. Frozen MIT, Expat COPYING, miniz LICENSE and original NOTICE are preserved.
The Newt reference patch is never applied to the baseline oracle.

## Root-owned execution

The callable gate reuses `tests/inflate/run_support.py`, its verified executable
and Rust installation receipts, jobs=2/test threads=2, offline frozen Cargo and
owned process-group cleanup. All C is built with the approved clang++ in C99
mode; the original oracle uses C++17, ASAN+UBSAN and fatal non-recovering flags.
Host-only explicit RUSTFLAGS select that approved clang++ linker, sanitizer
runtimes and individual local Expat/bridge objects. They live only in this gate's
Run environment and must never be forwarded to RV32/aggregate environments.
The root must confirm this explicit sanitizer linkage on its approved toolchain.

Each baseline/mutant compiles the actual feature-enabled host test harness and
fresh core+host transport in separate empty Cargo targets. Baseline native tests
are executed; mutant test harnesses are compile-only so a test assertion cannot
be mistaken for a semantic kill. Every killed mutant must exit zero, emit a
complete valid LF/action grammar, and differ from the original oracle on its
named witness. Build errors, crashes, timeouts and malformed output fail the gate.
Each positive compares both complete responses with the independent authored
trace, with mandatory case IDs and request/trace fingerprints. Rejection drivers
use `inherited_child` in the enclosing Run-owned group; the existing ownership
control scans this driver too. The protocol control script tests malformed output
independently of the runtimes.

## Evidence limits / pending

The native corpus has raw/stored entries only: attributes, namespace spelling,
UTF-8, XML/filtered HTML entities, malformed/empty XML, I/O failure, short reads,
initial stop, same-chunk stop/error, stepped stop and finish/close/count behavior.
It does not establish compressed XML × filter × parser × decoder failure parity.
The core mock missing-decoder case is not that cross-product proof. Seven native
semantic mutations are runnable; the baseline's ten mock-oriented proposals are
retained separately as history, not reported as executed controls.

Producer-policy patch parity, a pure Rust XML backend, target native backend,
native allocation-failure injection, complete compressed interaction coverage,
and all actual compilation/runtime/target checks remain pending. No firmware,
hardware, performance, resource-budget or whole-port acceptance is claimed.
