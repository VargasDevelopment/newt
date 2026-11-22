# Shared Rust inflater validation

The actual decoder is `rust/core/src/inflate.rs` in newt-core. The real host
transport is `rust/host/src/bin/inflate.rs`; core integration tests and the
borrowed target probe live in the normal workspace. This bundle contains only
validation/provenance sources, never a second Rust engine.

`rust/check.py` always runs `rust/inflate_gate.py` alongside every previous
family. It compiles sanitized pinned-original C32/C64 and pristine native64,
then freshly compiles the actual core/transport. Python stdlib zlib plus hand
vectors supply independent expected output/checksums/errors. All valid first
block types must be observed. The linear harness compares exact status, input
consumption, output count, Adler and bytes for every call. Ring/nonwrapping,
byte/heterogeneous/full chunks, corruption, truncation and repeat calls remain
explicit. Large packed streams use heterogeneous/full chunks; byte-at-a-time
applies to <=8192 packed bytes, including small stored fixtures. This is a test
sampling rule, never an engine input limit.

All thirteen semantic controls must compile the whole actual core and transport
freshly. A mismatch counts only after complete valid grammar and clean process
exits; crashes, malformed output, hangs, timeouts and compiler errors fail the
gate. Runtime borrowed RISC-V code must exist under the exact nonempty export
`newt_rust_inflate_probe` in the normal target archive. Its symbol attribute is
reviewed separately; the decoder core still forbids unsafe code.

The shared machine flock and agreed job/deadline limits apply to every command.
No dependencies are acquired. Both target compilation and host comparisons are
separate from a linked image, runtime stack/heap, panel, physical storage,
power-lifecycle or firmware-size/performance acceptance. The value constructor
remains available; physical parse/cover owners initialize the same storage
directly. `storage.rs` runs the actual Core and placement integration tests;
linked stack inspection remains separate from runtime stack acceptance.

Full miniz Unlicense and copyright notices are retained in frozen/ and described
in CONTRACT.md. Never label these frozen sources simply MIT.
