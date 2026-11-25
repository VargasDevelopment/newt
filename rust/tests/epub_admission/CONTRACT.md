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
eager admission (NEWT-90). #60 moved every local-header check from admission
to when each member opens (`core/src/member_check.rs`), by owner decision. The
reads and events then differ from C++ by design, so `expected-v3` is captured
from the Newt Rust host instead. Before it replaced v2, every difference was
explained case by case:

- each member's local-header read (30 B) and local-name read moved from
  admission to when that member opens; removing those reads leaves every read
  trace identical. Admission alone reads less; the metadata path, which never
  checked headers, now reads them for the container and OPF;
- the duplicate-name check is one CompareBegin/End pair whose result is the
  comparison total, not one pair per entry. It borrows the idle inflate
  dictionary, so `typed-no-decoder` now stops before the first member;
- two cases moved: `central_flags_decide_admission` is Encrypted (it was
  Unsupported from a local flag mismatch), and
  `member_name_is_checked_when_it_opens` is Damaged when admission opens the
  container (it was Unsupported at admission). Both refuse the book;
- every other case keeps its status and payload, including each read-failure
  case, matched read for read.

`expected-v3` is Newt's own output, so it cannot catch a change it shares. The
independent expectations in `corpus.py`, the read-failure sweep and the ten
mutation controls stay. Any future change to `expected-v3` needs the same
case-by-case explanation in its PR.

The recipe rejects stderr, malformed responses and build failures before
classifying semantic differences. `--positive-only` omits the mutant sweep and
is explicitly partial. The aggregate adapter reports those scopes separately.

RV32 Core/static-probe compilation includes borrowed inspect/chapter bodies.
It does not retain or link a complete native XML admission path. Consequently
`result.complete` remains false while `host_comparison_complete` can be true.
Full processing-policy cancellation, native target linking, reader composition,
physical storage, resource margins, bootable firmware and device performance
remain separate acceptance requirements.
