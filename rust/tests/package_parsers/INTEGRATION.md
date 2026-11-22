# NRUST-40 source packet (unvalidated)

Only this ignored packet was written. No compilation, test execution, gate,
network, dependency acquisition, device access, planner access or commit was
performed. Rustfmt and Python AST syntax inspection provide no behavioral proof.

## Mapping for root integration

| Packet | Destination relative to rust/ |
| --- | --- |
| core/package_parsers.rs | core/src/package_parsers.rs |
| integration/core/src/xml_session.rs | core/src/xml_session.rs (proposed replacement from accepted 0e8c47c2) |
| integration/host/src/xml_session/expat.rs | host/src/xml_session/expat.rs (proposed replacement from accepted 0e8c47c2) |
| host/package-parsers-host.rs | host/src/bin/package-parsers-host.rs |
| host/native_tests.rs | host/src/package_parsers/native_tests.rs |
| target-probe/package_parsers_probe.rs | target-probe/src/package_parsers_probe.rs |
| package_parsers_gate.py | package_parsers_gate.py |
| tests/* | tests/package_parsers/* |
| oracle/main.cpp | tests/package_parsers/oracle/main.cpp |
| frozen/** | tests/package_parsers/frozen/** |
| reference/** | tests/package_parsers/reference/** |
| PROVENANCE.json | tests/package_parsers/PROVENANCE.json |

Append `pub mod package_parsers;` to core/src/lib.rs. Append
`mod package_parsers_probe;` to target-probe/src/lib.rs. Add this bin stanza to
host/Cargo.toml:

```toml
[[bin]]
name = "package-parsers-host"
path = "src/bin/package-parsers-host.rs"
required-features = ["xml-native"]
```

Add a mandatory `package_parsers_gate.check(gate, snapshot, positive_only)` call
in the root gate and call `verify_target` on the root target artifact. Root must
reconcile aggregate budget, existing XML source bindings, code-health policy,
and concurrent producer-policy work. This packet neither reads nor changes that
worker's packet. The frozen Newt patches are historical reference only and must
not be applied to the original C++ oracle. Full SDK library objects, font header
closure, SDK MIT, miniz LICENSE, and Expat COPYING are retained from git objects.

## APIs and allocation domains

`parse_container`, `parse_package`, and `parse_toc` call the real accepted
`xml_session::parse_entry` with source, entry, backend, decoder, buffer and filter
reborrowed for each synchronous pass. All pinned package calls use the default
**unfiltered** XML path. Callers may supply compressed entries and reusable
InflateScratch; compressed package cross-product execution remains pending.

`PackageStorage` supplies explicit manifest, scratch spine-hash and output spine
capacities. TOC entries are another caller-owned typed slice. None of these
objects is constructed in the byte Arena. Metadata and all variable strings are
attempt-local ByteRange handles; missing metadata/missing media type represent
static empty strings without allocations. Caller must preserve the corresponding
Arena initialization and not reset/release/reinitialize it while consuming any
handle. A handle is not statically branded to its arena: mixing arenas is outside
the API contract, just as for the accepted Arena. Source, arena bytes, typed
arrays and parser/decoder resources must be disjoint. There is no heap/Rc or
whole-document buffer in the core.

Arrays can retain partial writes on a failure. Public output descriptors/counts
are changed only after success. String allocations are not rolled back. Typed
capacity checks occur at the corresponding count/fill/final-spine boundaries;
final spine capacity is checked against *all filled idrefs*, even if unresolved
refs would later be dropped. Count/fill attribute differences and the C++ cast
to u16 for a resolved manifest index are retained.

The Attributes proposal keeps `next`, adds required immutable `local` and `exact`
first-match lookups over the complete callback view, and implements them in the
native view and test attribute adapter. No attribute collection/truncation is
used. The native adapter retains C Expat provisionally; its heap/internal
allocation behavior is not a no-allocation target parser claim.

## Evidence sources and planned validation

Tests author independent synthetic XML plus complete structured responses/read
traces. Cases cover first/empty container choices; local vs exact attributes;
missing attributes; first nonempty metadata; nested capture loss; cover promotion;
unknown spine references; NCX type/toc-id fallback; nav nested anchors; NCX shared
nested labels; 255 depth saturation; 255 text truncation; malformed/empty XML;
strict entities; short reads; second-pass I/O failure; absent parse buffer;
and container/count-metadata string exhaustion. Eight core tests cover reusable
attributes, media interning after 12 distinct types, count/fill asymmetry, final
spine admission, reopens/cleanup, and XML error priority over metadata OOM.
Native tests exercise 300 attributes and lookups after iterator exhaustion.

The runner reuses Run/owned_process through tests/inflate/run_support.py and
inherited_child for invalid requests. It builds full unmodified PackageParsers,
XmlSax, ZipCatalog and EntityFilter with miniz/Expat for the C++ oracle. The Rust
binary links **only** accepted Expat bridge and three Expat C wrappers. No C++
package/session/reader/filter or C miniz enters the Rust executable.

Each proposed semantic mutant changes one unique typed source site, compiles a
fresh core/host/test harness, then must exit zero with a complete valid LF
response that differs on its explicit witness. A compilation failure, panic,
sanitizer finding, timeout or malformed response fails validation. All seven
controls and their witnesses are source proposals; none has been executed.

Root-owned commands after integration (not executed here):

```sh
# Use the root's approved project-local toolchain and shared Run environment.
python3 -B rust/tests/package_parsers/protocol_controls.py
# Gate check() owns sanitized C++/C, host builds, focused tests, protocol controls,
# paired independent XML cases and all fresh semantic mutations.
python3 tools/check.py
# Existing approved resource build, only if root's integrated firmware changes require it:
python3 tools/firmware-build.py --clean --health-report
```

The root must run approved offline native and RV32 builds; verify all three
nonempty runtime-borrowed exported probe symbols; inspect final integration source
bindings; and obtain fresh root-owned reviews. No target XML backend or physical
hardware behavior is demonstrated by these probes.

## Concrete limits and remaining evidence

This is an implementation candidate, not completed NRUST-40 acceptance. No build
or behavioral result is available. Resource policy is deliberately not changed.
Equal numeric byte-arena budgets are **not comparable** across these designs:
C++ charges typed arrays to its arenas and Rust does not. The equality fixtures
therefore use generous budgets or failures before typed allocation. Separate resource cases encode the different Rust fill/string and C++
pre-reopen allocation failure traces, including I/O precedence in the Rust fill.
Native Expat allocator failure injection, native TOC partial-output preservation under failures, compressed XML
cross-products, pathological changing-source behavior across passes, and complete
resource-domain differential matrices are not yet covered. The core exposes
those paths but coverage must be added by root before claiming them verified.

The C++ allocation API does not initialize manifest array objects; its cover
promotion scans the counted rather than filled range. The transport uses zeroed
backing memory as existing host fixtures do. This is a reference-source hazard,
not evidence for arbitrary uninitialized C++ backing behavior. Rust initializes
the counted slots safely. Very large host-only manifests beyond signed C++ index
range are outside the proposed fixtures; Rust uses Option<usize> for selection.
A native C++ allocation failure before a fill reopen cannot share a read trace
with a Rust string failure *during* fill; do not call that difference a parser
regression. The source suite provides separate expected traces for these allocation domains,
which remain unexecuted.
