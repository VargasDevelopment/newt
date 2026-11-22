# XML-input validation integration

The mandatory `rust/check.py` family compares the actual shared Rust core with
the original pinned C++ implementation. `case_labels.json` fixes case identities,
inputs and independent expectations; it does not derive expected behavior from
the Rust implementation. Acceptance receipts are published separately.

## Hook

Load `rust/tests/xml_input/suite.py` by file path and call
`run(gate_module, snapshot, positive_only=False)`; `check` aliases `run` to match
`css_gate.check` and `zip_container_gate.check`. `snapshot` is the candidate root
containing `rust/Cargo.toml`, not the packet directory. The gate supplies `ROOT`
(the repository with approved tools) and `BUILD` (a directory within its resolved
`.build/rust`). The hook returns evidence with `comparisons`, `positive_cases`,
`semantic_controls`, provenance, binary/log hashes and recorded commands.
`positive_only=True` skips semantic mutants only; all positive records, witnesses,
protocol controls, rejection checks and fatal sanitizer controls still run.

The candidate must already wire `newt-parity --bin xml-input` to the actual
`newt_core::xml_input` module in `core/src/xml_input.rs`. Root owns these edits,
its no-allocator target probes, host transport and unit tests. Mutation metadata
uses the source packet's `src/lib.rs` name; the hook maps this one declared source
to the actual core module. Every listed mutation requires exactly one matching
site, a fresh actual-core/transport compilation with `-Dwarnings`, exit zero,
complete valid output and a semantic wire mismatch against the successful
OriginalCpp witness baseline. Compiler failures, crashes, invalid outputs and
timeouts fail the suite and never count as mutation detection.

## Oracle and harness

All frozen files are checked against PROVENANCE.json and the fixed SDK commit.
The oracle uses full original EntityFilter.cpp, PackageParsers.cpp and
ZipCatalog.cpp plus miniz_impl.c compiled as C99. All objects use approved LLVM,
fatal ASan/UBSan, function/data sections and linker garbage collection.
Unreferenced XML parser handlers are discarded. There is no native parser stub.
The existing inflate sanitizer controls prove fatal ASan and UBSan behavior.

The existing inflate `Run` provides isolated environment/cache locations,
process-group cleanup on timeout/signals, raw-LF-preserving logs and a bounded
300-second allowance. Cargo jobs and Rust test threads are explicitly capped at
two. Rejection and sanitizer subprocesses inherit that process group. Every
output, copied mutant workspace and cargo target stays inside `.build/rust`.

The independent manifest freezes exact unique case IDs, input hashes and hashes
of independent payload/whole-line expectations. Corpus definitions and all
mutation witnesses form the complete positive set. Existing independently
specified payload and whole-line expectations are checked on both original C++
and Rust. Responses are checked by command, action, exact row order, field arity,
wire widths, full destination lengths and raw LF encoding before complete wire
comparison. Malformed requests must return exactly 2 on both transports. Valid requests
include positive fail-call indices and only nonpositive fail-return values.

## Integration and limits

The transport parses fail-call indices as `-1` or `0..1048576`, separately from
nonpositive signed fault returns. Malformed Rust requests terminate with the
declared exit 2. All mutations must compile under `-Dwarnings`. The normal gate
invokes this family and checks exact nonempty runtime-borrowed target exports.
Focused core tests also run in the normal core test pass.

This slice validates entity filtering and supplied XML utility events, not a full
XML parser. It establishes no firmware, physical hardware, target ABI, memory
resource or performance proof. Linux `--gc-sections` follows this integration's
host convention; a different host linker requires an explicitly reviewed port.
