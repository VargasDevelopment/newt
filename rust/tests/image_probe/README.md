# NRUST-37 ImageProbe — source-only handoff

This packet translates the complete `ImageProbe.cpp` and `.h` from official
FreeInk SDK commit `cd6f5b831c8fad899723b6d406586f00c575f021`.
Only this ignored directory was written. Original bytes were obtained with
`git show`, never from the live vendor patch. Per-file SHA-256 values cover the
complete local original-source/header/miniz closure, LICENSE and NOTICE.

**Proof pending:** no compilers, tests, gates, device access, network,
dependencies, planner actions, commits or subagents were run. Python AST parsing
and corpus-definition enumeration are the only local validation performed.
The included semantic controls are proposed freshly compiling controls; only the
root's execution can establish that each compiles and is killed by its witness.

The overlay under `rust/` is intended for root adoption into the actual workspace.
It has no standalone Cargo workspace and adds no Cargo dependencies. See
[INTEGRATION.md](INTEGRATION.md) for the exact required wiring and root checks,
and [CONTRACT.md](CONTRACT.md) for parity and resource boundaries.

Full decoder, full-picture loading, page layout, firmware, physical hardware,
and performance acceptance remain pending and outside this packet. The borrowed
RV32 function is source for a future linked inspection, not target execution
or firmware evidence. Reading remains offline.

Root integration centralizes all rejection-driver transport launches in
`tests/inflate/run_support.py::inherited_child`. Drivers inherit their enclosing
Run process group; they cannot create detached transport sessions. Mandatory
`owned_children_test.py` checks every driver for that rule and exercises actual
SIGINT/SIGTERM cleanup, including deliberate detached-child negatives.
