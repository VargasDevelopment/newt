# NRUST-52: source-only reader style policy candidate

Not compiled, tested, gated, integrated or run on a target. Only existing rustfmt
and Python AST parsing were used. Parent owns the repository gate. All writes
are confined to this directory. No tracked edit, install, network, device,
commit, child agent or new harness/admission mechanism is part of this packet.

## Sources and scope

`original/newt/` contains exact `git show` output from
bc9ffda52b85f277e46c63718ac88d62fba5d368, including the six target files,
phased-reader call contract and corresponding tests/reference algorithms.
`original/sdk/` retains CSS originals and MIT license from the canonical frozen
CSS proof for SDK cd6f5b831c8fad899723b6d406586f00c575f021. No main lookup.
See both original LICENSE files. Rust translations retain attribution.

`src/scan.rs` is the complete raw CSS boundary scanner: UTF-8 replacement spans,
identifier/numeric token identity, six-digit escapes, comments, strings, URL and
bad-URL recovery, bounded 64-block nesting. `src/styles.rs` owns source capture,
provenance and path rejection, contradictory manifest detection, ordered build,
legacy small-source SDK parsing, projected complete-rule parsing, exact 2-bit
lexical projection checks, independent 512-rule parsing batches and 64 retained
rules. Priority compaction keeps the latest rules within each priority, preserves
their relative source order, ignores irrelevant rules once compacted, neutralizes
display visibility, and preserves the policy hash folding. Numeric NUL/seven-digit
failure atomically drops the profile. Partial CSS syntax, selectors and text/rule
truncation remain nonfatal; limited source/head/resource/content failures drop it.
External work-limit sources are skipped with Truncated|Resource, in source order.
Live unchanged damaged bytes degrade; failed/zero reads, stale source and OOM
remain errors. Entry names are stripped from retained link records.

`src/watch.rs` implements the finite markup watch over the decoded bytes layout
actually receives. A stopped read returns -1 before those bytes reach layout.
Deferred stops demand ordinary resolve/build without book metadata; Pending
stops settle to unstyled nesting-limited output. The caller must throw away a
stopped pass and restart; `require_advance` enforces strict state advancement.
Cleared prefix parses settle only on successful layout; an incomplete prefix
preserves the prior slot. A successful parse after a malformed head becomes OOM,
as in the original. At most three passes: Deferred -> Pending -> Settled.
`reconcile_capture` checks recaptured topology and restores original link facts.
The caller triggers recapture only for Resource with capture capacity <4096,
using full embedded storage and no Book before build, exactly as phased styles.

## Existing namespaces, typed domains and resources

The candidate has two *path* dependencies, no acquired dependencies:
canonical `newt-core::css`, and the read-only `newt-epub-admission-candidate`.
The latter supplies actual `admission::ChapterSource`, `inflate_input_bounded`,
`canonical::CatalogView`, `package_parsers::ManifestItem`, `xml_input` path
helpers, `xml_session::XmlStream` and ZIP/arena/source types. Its modules are
reexported, not copied/recreated. This avoids incompatible arena/entry/source
identities between the pending admission packet and core. Integration should
replace these reexports with canonical modules once admission lands. CSS builder
is canonical today. No C++ style implementation participates in Rust results.

Resolve takes independent immutable metadata and mutable path arenas; scoped
path allocations release on all returns. Decoder/session loans end before arena
release. Build uses no byte arena at all, so has no cursor to advance or restore;
its logical rule and working storage are typed loans. The C++ oracle independently
reports real arena restoration. `ResolveScratch.paths` must have 512 available
bytes for the largest result; directory is a separate 512-byte typed loan.
Source records must be resolve-produced (or equivalently validated), counts <=8,
links <=4, ranges within text, immutable source during resolve/build. Slot counts
must remain <=64. Metadata handles must belong to the supplied arena and remain
live. There is no unrelated-book special case.

Build requires caller-owned typed arrays: 64 held rules, 64 pending rules, 512 SDK
rules, 8192 working bytes, 8192 batch bytes, 2048 projection marks, 256 read bytes.
Missing `BuildScratch` is preflight OOM only when a profile actually needs build.
Decoder scratch is separately borrowed and absent compressed scratch yields OOM.
There is no byte reservation pretending to stand in for typed memory and no
production heap, unsafe, Rc, registry or VFS. Do NOT equate these logical limits
with identical total RAM or original arena admission thresholds: Rust layout,
alignment, record sizes, backend storage and lifetimes need measurement. The
canonical `xml_input::resolve_href` currently uses its existing two 512-byte
local arrays; replacing that helper's internal stack with borrowed scratch is
outside this write scope and remains an explicit target resource gap. This port
adds no large automatic arrays; scanner state includes only 8 input/64 block
bytes and is moved through a small local borrow-splitting value per byte.

## Proof material, deliberately not a new runner

`tests/policy.rs` has authored expectations for ordered declarations, neutralized
visibility, numeric atomic discard, structural selector rejection, latest/highest
priority retention, a 513-member selector list, protected delimiter injection,
path rejection, chunk splits, exact 14/15 nesting boundary, out-of-order reads,
and finite settlement. `tests/resolve.rs` uses an explicitly scripted SAX seam
for ordered embedded/link/embedded capture, manifest conflict, parse arena
restoration, backend create/parse failures, failed reads, text and typed capacity
limits. Scripted callbacks do not establish XML recognition. Native Expat is an
allowed host backend; it must implement the existing ParserBackend/Attributes
traits directly, without C++ style callbacks. Tests are source, not passed proof.
`tests/external.rs` supplies actual ChapterSource stored-byte reads, source I/O
failure, short-read fault, skipped work limits, deterministic link rejections
and scratch preflight/limited short-circuit expectations. Original heterogeneous
C++ fixture and scanner reference sources are retained
for broader differential integration, especially full CSS recovery boundaries.

`oracle/main.cpp` is a thin actual-original adapter. W reads raw decoded chapter
bytes with the original watch. R opens the supplied EPUB using pinned
CanonicalBook, resolves original sources, builds original rules, and prints all
12 declaration fields, issues, state, hash, spine, link facts and arena restoration.
It does not contain replacement CSS, path, XML or style logic. Compile it later
using the *existing* Run/check pattern and source lists from the frozen Newt
reader_styles_test.py / epub-admission proof. Use original/newt style .cpp/.h
first on include/source paths, and read-only epub-admission/original/newt plus
its SDK original tree for canonical admission dependencies. Need XmlSax, entity
filter, ZIP/miniz, SDK Css.cpp, native Expat and the existing producer-policy
patch path; reuse that packet's established native link recipe. No new sysroot,
inventory/manifest, registry, admission guard or build framework is proposed.
TODO: wire this adapter into the existing Run invocation; not done here.

Rust-host transport proposal: add one small bin to the existing host package,
using its byte/decimal validation conventions. Use the same R/W file inputs and
field-for-field output as this oracle. R opens canonical metadata through the
admission packet, loans disjoint metadata/path storage to resolve, releases Book
before build, lends slot+watch to layout and handles Settlement. Because Book
currently retains its workflow loans and exposes individual manifest entries,
its caller must expose a bounded read-only manifest slice plus disjoint workflow
reborrow for style resolution; this integration boundary still needs wiring.
No new registry or metadata inventory format. Extend the existing CSS transport
only if needed to inject ordered link/member/fault records; serialize all facts,
never just rule counts or success. Source faults, size changes and zero probes
must compare as observations separately from content outcomes.

Use `tests/mutations.py` tuples directly in the existing negative-control loop:
require exactly one original match, fresh isolated source copy per mutation,
compile succeeds, designated behavioral assertion fails, restore source and
rerun the positive. Twelve tuples identify direct sites and witnesses; NONE
have run. No compiler-error-only kill counts. Broader existing tokenizer,
projection, determinism and work-fixture cases must be ported to the shared
transport before claiming full differential parity. Additional executable
witnesses still needed for source disappearance/size change, malformed/truncated
and excess deflate, early limit preceding later fault, exact 2048/16384 windows,
recapture topology changes, all source-count bounds, and full Expat semantics.

## Target and adoption recipe (pending)

Include `probe/borrowed.rs` in the existing RV32 target-probe crate and link
exported entry points through its existing runtime dispatcher. Pass unpredictable
runtime source bytes, entries, hrefs, slot/rule arrays, scratch and parser backend;
consume the complete returned status/issue/hash/rule/state tuple. Do not replace
with constant fixtures or construct rule/decoder arrays on a task stack. Add
runtime scan and projected-large-source calls through build_probe. Resolve's
backend is generic, so monomorphize against the eventual real target XML backend;
a scripted backend or host Expat does not satisfy target XML acceptance.
Inspect the existing no_std RV32 artifact for reachable heap/unsafe stubs and
measure stack, static RAM, typed layouts and worst-case simultaneous loans.
No target artifact, target execution, stack measurement, same-total-RAM claim,
firmware build, physical device check or runtime parity is supplied here.

Before adoption: parent runs executable host compilation and positive/failure/
mutation witnesses through existing Run, integrates the pending XML processing
policy (cancel/progress/producer byte-index semantics; current base BookSource
has no forwarding method), then RV32 and full resource gate with measured
margins. Source-only implementation is complete for the policy/core slice;
executable integration and acceptance remain open. No ceiling/floor change.

Parent execution: focus01 passed17 typed regressions and RV32 library compilation.
Fresh static review found malformed nesting subtraction could panic before XML
validation. It now uses wrapping subtraction, preserving the original stop.
Tests now distinguish exact/local qualified attributes, put a high-priority rule
before70 lower-priority rules and verify retained order, and assert independent
successful source hashes. A hash-publication control was added (13 proposals).
These corrections and new witnesses require a new focused run; no native original
comparison or mutant execution has yet passed. No new guard mechanism is added.
