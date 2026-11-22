# ZIP window packet handoff

Prepared, unexecuted implementation. No tracked integration and no production
algorithm changes. Frozen originals, SDK MIT and NOTICE are untouched.

Future command, from the repository root, only after machine release:

```sh
python3 -B .build/rust/staged-foundations/zip-window/check.py --jobs 2
```

The lead supplies the shared flock and external 120-second/two-job allowance.
The checker has no lease or polling process and no positive-only escape hatch.
Its subprocess allowance is also bounded to 120 seconds in total. Toolchain
verification is read-only. Every Cargo command is offline/frozen/locked/jobs=2.
No toolchain installer or root checker is imported. All writable state is local
to this packet, under `.build/rust`.

A run retains a source snapshot, each subprocess command/stdout/stderr and each
fresh mutant source/target under `out/run-*/`. Only all independent positives,
focused Rust tests, grammar controls, all 14 freshly compiled semantic controls,
host borrowed no_std compilation, optimized RISC-V static library compilation
and nonempty defined probe symbols permit atomic `evidence.json` publication.
Timeouts, process/compiler errors, malformed responses and surviving controls
abort the run without pass evidence. Frozen hashes and packet/toolchain stability
are checked. No existing witness/control is intentionally omitted.

All executable outcomes are PENDING. Hand-authored Cargo lockfile, compiler
compatibility, target codegen, symbol-tool compatibility, and runtime budget need
the lead's future execution. Static inspection found no material production
algorithm defect requiring redesign. A failed future checker is a blocker to
packet acceptance; do not downgrade controls to obtain a pass.

Target static-library evidence is compilation only. Firmware/resource gates,
boot behavior and physical hardware have not been checked by this packet.
