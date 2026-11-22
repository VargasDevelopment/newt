# EPUB admission and canonical metadata

Reference: Newt `bc9ffda52b85f277e46c63718ac88d62fba5d368`;
FreeInk SDK `cd6f5b831c8fad899723b6d406586f00c575f021`.
The frozen Newt oracle, provenance, licenses and existing SDK path map are in
this directory. `integration-map.json` reuses 148 matching SDK files rather
than copying another SDK tree.

Actual Core `admission`, `canonical` and `phased_metadata` use the existing
ZIP, inflater, XML and package-parser types. `Catalog::into_parts` transfers
the existing source/arena loans; it does not turn borrowed metadata into owned
data. Window reads before the directory retain their original source bounds.
A canonical Book and its views cannot survive resetting their backing arena.

The host transport exercises NoPolicy admission, metadata selection and chapter
streaming. Independent cases cover ZIP/local disagreement, encryption, malformed
required metadata, unsupported features, read failures and typed shortages.
Status, payload, source reads and work events are compared to the actual pinned
C++ implementation. Diagnostics for typed Rust storage and C++ byte arenas are
checked separately: equal arena totals and equal low-capacity failure phases
are not claimed. Eight Rust-only cases and five distinct resource-domain cases
are excluded from the shared comparison count.

Canonical proof01 records 87 positive cases, 74 shared comparisons and ten
fresh compiling semantic controls in
`artifacts/rust-epub-admission-canonical-01.json` and its compressed full receipt.
The recipe rejects stderr, malformed responses and build failures before
classifying semantic differences. `--positive-only` omits the mutant sweep and
is explicitly partial. The aggregate adapter reports those scopes separately.

RV32 Core/static-probe compilation includes borrowed inspect/chapter bodies.
It does not retain or link a complete native XML admission path. Consequently
`result.complete` remains false while `host_comparison_complete` can be true.
Full processing-policy cancellation, native target linking, reader composition,
physical storage, resource margins, bootable firmware and device performance
remain separate acceptance requirements.
