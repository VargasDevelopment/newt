# NRUST-39 source packet

Overlay `rust/` onto a fresh complete copy of
`.build/rust/staged-foundations/xml-session-integration/rust`. Do not overwrite
that base. This packet contains replacements/additions only. `SOURCE_BINDINGS.json`
records every payload's exact bytes and the prior baseline hash for replacements.
The existing baseline frozen SDK, reference patch, licenses, original oracle,
vectors, cases, controls and all unrelated modules remain supplied by the base.

Root must run the existing complete XML gate under its build lock, including
ordinary controls and the producer extension, and the target gate with both
runtime-borrowed probe symbols. The entrypoint remains `xml_session_gate.check`;
its local runner budget is 600 seconds (the enclosing gate budget still applies).
Use the base's approved toolchain and dependencies. No tool acquisition is needed.
Preserve `check.py` integration and the baseline gate family. No separate giant
runner is introduced. Gate execution is **not performed by this packet author**.

The producer oracle is compiled separately from a temporary copy of the pristine
SDK. `producer/recipe.py` authenticates the reference patch hash, each selected
source hash, exact hunk context and full patched hash before compiling. Only the
three XML dependency files are patched. The authenticated ChapterLayout hunks are
retained in the reference but are neither needed nor applied for this XmlSax task.
No C++ session, reader, entity filter or miniz object is linked into Rust. Rust's
native backend links only original Expat and the C bridge. This remains a
provisional host backend, not a Rust XML parser or target-native acceptance.

`X` transport and baseline suite retain their semantics. New `Y` requests are:

`Y hex max fail filter stop producer cancel method local uncompressed actions`

Shared fields use the baseline grammar. producer/local are 0/1; cancel is -1 or a
zero-based cancellation poll number; method is 0/8; uncompressed is <= 1 MiB.
Local requests include a 30-byte ZIP local header (fixtures use no name/extra).
Raw entries must satisfy the reader's ordinary raw-entry contract. Policy records
are `Q poll cancelled`. Result/read/callback records retain baseline grammar.
The producer parser validates complete syntax, LF framing, command result order,
close/open callback constraints and monotonically numbered polls before mutation
comparison. Transport failure, crash, nonzero exit or compiler failure is never
accepted as a semantic-control witness. Malformed requests are checked separately.

The policy reference is explicitly borrowed and remains installed for the whole
synchronous attempt. NoPolicy is zero-sized and does not require a producer
backend. The core does not allocate policy storage, clone ownership, retain an
Rc, or call result(): the authenticated XmlSax source has no result() call.

## Evidence still required

Only rustfmt and Python AST parsing were performed. No tests, builds, compilers,
firmware, device operations, dependencies, network, planner changes, commits or
subagents were run. All hand-authored independent traces and controls require
root's execution; no passing outcome is claimed. In particular verify Expat's
current-byte-index at the inclusive 4096 boundary and exact callback segmentation.
Source/target compilation, sanitizers, mutation kills and RV32 symbol inspection
remain unverified. Typed OOM is not exact C++ arena accounting. The scoped real
Expat create-failure case uses XML_ParserCreate_MM's per-call failing allocator,
not global allocator replacement and not an arena-capacity equivalent.

All internal subsets are rejected before declaration processing. Consequently an
external entity declared inside such a subset never reaches the external handler
in a normal producer parse. The external handler is installed and returns
XML_STATUS_ERROR exactly as the authenticated patch specifies; this packet does
not claim a reachable producer end-to-end external-handler callback witness or
an independent mutation kill for that otherwise masked defense. General default
Expat parameter-entity parsing behavior is unchanged.

The Rust session retains conservative baseline borrows after close, requiring
session drop/recreation to reuse caller storage. The explicit policy must describe
the same logical source for the attempt; this association is a caller contract,
not discovery through a mutable source method. Arbitrary implementations of the
backend trait are not XML/security proofs. Hardware, target parser, ChapterLayout
step limits and upstream producer result settlement remain outside this packet.
