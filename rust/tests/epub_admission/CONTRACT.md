# EPUB admission and canonical metadata

Reference: Newt `bc9ffda52b85f277e46c63718ac88d62fba5d368`;
FreeInk SDK `cd6f5b831c8fad899723b6d406586f00c575f021`.
The Newt-owned reference, frozen input provenance, licenses and existing SDK
path map are in this directory. `integration-map.json` reuses 148 matching
SDK files rather than copying another SDK tree.

Actual Core `admission`, `canonical` and `phased_metadata` use the existing
ZIP, inflater, XML and package-parser types. `Catalog::into_parts` transfers
the existing source/arena loans; it does not turn borrowed metadata into owned
data. Window reads before the directory retain their original source bounds.
A canonical Book and its views cannot survive resetting their backing arena.

The host transport exercises NoPolicy admission, metadata selection and chapter
streaming. Independent cases cover central-flag encryption, damaged local
headers, malformed required metadata, unsupported features, read failures and
typed shortages. Status, payload, source reads and work events are compared to
`expected-v3.json.zlib`. Arena diagnostics are recorded, never compared.

## Why the C++ comparison ended (#60)

Until #60 the reference was `expected-v2`, captured from the deleted C++ Newt's
eager admission (NEWT-90). For long books (NEWT-166) the owner decided that
one code path serves every book, so #60 moved every local-header check from
admission to when each member opens (`core/src/member_check.rs`). The reads
and events then differ from C++ by design, so `expected-v3` is captured from
the Newt Rust host instead. Before it replaced v2, a one-time review put every
v2/v3 difference in one of these classes, with none left unexplained:

- a, same outcome: status, metadata, package, chapter payload and every event
  outside the duplicate-name compare are equal, and the reads, minus local-
  header reads, are a subset of v2's. Each member's local header (30 B) and
  local name are now read when that member opens, not at admission. The
  metadata path, which never checked headers, now reads them for the container
  and OPF. v2's two extra read-failure cases failed a local-header read that
  admission no longer makes;
- b, as a, but the duplicate-name compare changed: it is one CompareBegin/End
  pair whose result is the comparison total, not one pair per entry. It borrows
  the idle inflate dictionary, so `typed-no-decoder` now stops before the first
  member;
- c, the two cases the owner decision moves: `central_flags_decide_admission`
  is Encrypted (it was Unsupported from a local flag mismatch), and
  `member_name_is_checked_when_it_opens` is Damaged when admission opens the
  container (it was Unsupported at admission). Both still refuse the book;
- d, domain cases: the v2 recipe compared them only with `corpus.py`, never
  with C++. Their status is `corpus.py`'s own expectation before and after #60,
  and v3 is the first time Newt's trace for them is frozen;
- h, read failures that moved: of the 43 read-failure cases, 37 match v2 read
  for read. The other 6 fail a local-header read, which now happens when the
  member opens. They keep their status, and they fail at that member's
  MemberEnd: admission runs each member's open-time check inside its
  MemberBegin/MemberEnd pair, so every failure names its stage;
- added: `member_header_is_checked_when_it_opens` (a damaged container local
  method is Damaged at the container's MemberEnd) is new coverage.

Per-member coverage now comes from `member_name_is_checked_when_it_opens`,
`member_header_is_checked_when_it_opens`, `central_flags_decide_admission`,
the read-failure sweep and `core/src/member_check_tests.rs`.

`expected-v3` is Newt's own output, so it cannot catch a change it shares. The
independent expectations in `corpus.py`, the read-failure sweep (its count
frozen from the v3 trace) and the ten mutation controls stay. A runtime control
proves that a regenerated reference which skips a required member still fails
`corpus.py`. Any future change to `expected-v3` needs the same diff-by-class
review, with its table, in its PR.

The recipe rejects stderr, malformed responses and build failures before
classifying semantic differences. `--positive-only` omits the mutant sweep and
is explicitly partial. The aggregate adapter reports those scopes separately.

RV32 Core/static-probe compilation includes borrowed inspect/chapter bodies.
It does not retain or link a complete native XML admission path. Consequently
`result.complete` remains false while `host_comparison_complete` can be true.
Full processing-policy cancellation, native target linking, reader composition,
physical storage, resource margins, bootable firmware and device performance
remain separate acceptance requirements.
