# Reader navigation policy

Reference: Newt `bc9ffda52b85f277e46c63718ac88d62fba5d368` and pinned
FreeInk SDK `cd6f5b831c8fad899723b6d406586f00c575f021`. Original navigation
translation units and provenance are here; unchanged headers, SDK and notices
reuse their existing frozen paths.

`newt-reader-navigation` depends on the actual shared Core. It owns its bounded
projection strings, retains twelve declaration records and a 640-byte string
budget, and borrows metadata only during binding. Anchor ambiguity, exact IDs,
staged required TOC, inherited sections, declared weights and five-row menu
policy follow the pinned C++ implementation. A borrowed view cannot outlive a
rebind. There is no package parser or source-I/O implementation in this leaf.

The original-C++ transport constructs a real SDK Book with live allocated
arrays, then supplies bounded descriptor fixtures. Both transports exclude only
the same three fixture scaffolding members. Inputs bound metadata to 128 entries
and anchors to 256, with live NUL-terminated C++ strings. Rust callers provide
one immutable valid metadata snapshot. Safe rejection of invalid estimates is
outside the C++ valid-input parity claim. Generic labels retain native unsigned
long width, so host evidence does not establish RV32 extreme-label behavior.

Canonical proof01 records 109 full traces, one independent complete expected
trace, fifteen unit tests, nine transport rejection cases and sixteen fresh
semantic controls. The actual canonical LTO target probe links a freestanding
ELF with four nonempty runtime-input bodies and no unresolved imports or
allocator symbols. The ELF is not executed on RV32.

The RV32 Navigation representation is 984 bytes against C++ 976 bytes. This
eight-byte increase is recorded, not accepted as a resource-budget change.
Optional XML degradation/source guards, complete PhasedReader integration,
firmware linking, runtime heap/stack and device performance remain pending.
