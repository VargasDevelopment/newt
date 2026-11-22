# Staged ZIP directory window / byte arena

Executable-intended staging only. No tracked integration; no full FreeInk port claim.
All compilation, runtime/parity, mutation-kill, target, resource, firmware and
hardware evidence is **PENDING**. Only existing rustfmt and Python syntax parsing
were used. No dependencies acquired, tests executed or build tools invoked.

## Boundary

`src/` is safe `no_std`, allocation-free, forbids unsafe, and uses no raw pointers,
self references, retained slice owners or interior mutability. `Arena` borrows
caller storage; `ZipDirectoryWindow` exclusively borrows that arena and its
source. The only 4096-byte cache is an arena allocation, conditional on capacity
and a successful constructor probe. Even an invalid constructor yields a scoped
object whose Drop releases the original mark. Release preserves diagnostics.
Rust's constructor does not return an error that could lose the cleanup scope.

This is a foundation for a future central-directory admission walker. ZIP parsing,
ZIP record validation, inflate, admitEpub, ScheduledBookSource, XML policy,
book engines, filesystems, firmware integration and runtime policy are excluded.
The generic yield function receives a temporary source reborrow, allowing safe
cancellation/handle/size simulations without global aliases. Source adapters may
also perform their own scheduling inside read_at; the window adds no such policy.
The host transport is a separate namespace and uses host vectors only for the
source/output and caller arena. It adds no production cache.

## Safe surface and equivalence domain

* Offsets are aligned relative to the allocation cursor, exactly like FontArena;
  an aligned offset is NOT a promise about typed alignment of the backing address.
  Alignment is explicit (there is no portable Rust max_align_t default).
* ByteRange fields are opaque. A handle belongs to its creating arena/init and
  live nested allocation scope only. Release/reset/init invalidate affected
  handles. Views are tied to a borrow; they cannot escape across mutable arena
  operations. Using a stale or foreign numeric handle cannot create Rust memory
  unsafety, but is outside the logical allocation contract (a reused range may
  otherwise resolve to new bytes). No generation table or runtime engine added.
* Source bytes, output and arena storage must occupy disjoint caller borrow
  domains. The window exclusively owns the source/arena scope. This is stricter
  than some defined C++ aliasing use cases; it is an API restriction, not a claim
  that those C++ cases are undefined. A callback must preserve immutable content
  and binding unless deliberately invalidating the source for failure testing.
* The minimal trait's size is observational, like the C++ const method. Zero reads
  receive a live empty slice derived from a local byte, including constructor
  probes. No null zero-length destination reaches an adapter. Every nonzero read
  is bounded by 4096; signed results are checked before advancing.
* read takes explicit bytes plus Option<slice> to model null and zero requests.
  A too-short slice is an additional nonsticky safe rejection. u32 request lengths
  assume a target with usize at least 32 bits, matching the ESP32/host domain.
* Direct fallback requires an immutable source for its lifetime. It intentionally
  has no repeated size/probe checks or yields. Constructor probe nonzero means
  fallback, never sticky failure. A cache-path zero-byte read performs no checks
  or yields after the initial range/failure check.
* Invalid ranges/requests are nonsticky except constructor range failure;
  physical/guard failures are sticky. size mismatch short-circuits its probe.
  Validation occurs before yield, after yield, and after a successful refill.
  A failed refill cannot publish its scratch bytes; a later-chunk failure CAN
  leave earlier copied output. Direct short-read failure can also expose bytes.
* Arena default models null base; initialized empty storage permits zero alloc.
  Invalid alignment/null base and checked array-byte multiplication overflow do
  not change failed-allocation diagnostics. OOM/rounding overflow retain the
  largest failed byte request. reset/release retain both diagnostics; init clears.
* alloc_array_bytes is checked count times element size only, NOT allocArray<T>,
  placement new, object construction, drop management or a typed allocator.
  copy_nul is explicit-length strdup, preserving embedded NULs and appending one.
  copy_c_string requires an accessible terminator. None models a null input.
  Inaccessible source lengths and unrepresentable len+1 cannot be expressed and
  are safely rejected; no parity claim covers impossible slices or C++ wrapping
  string lengths. Source return values above requested lengths are supported as
  fault reports, but adapters must still respect the destination slice boundary.

## Frozen originals and licenses

`provenance.json` records exact git-show source paths, revisions and SHA-256.
All SDK inputs were frozen from the existing SDK git repository, without network
or acquisition. The C++ oracle uses the unmodified original window and actual
FontArena via the original BookArena reexport. Its include-only epub_admission
shim includes BookStorage/BookArena and substitutes NO algorithm. Frozen Newt
and SDK tests are reference material, not executed or claimed as ported whole.

Newt MIT license: `LICENSE`.
Original FreeInk MIT license AND NOTICE: `frozen/sdk/LICENSE`, `frozen/sdk/NOTICE`.
These files must accompany derivatives. The Rust arena derives from FontArena;
the window derives from Newt. Host harness/expectations are new Newt staging work.

## Future verification, not run

A complete offline executable checker is prepared in `check.py`. Its code and
hand-authored lockfile are **unexecuted**; AST parsing is not checker acceptance.
After the lead releases the machine, from the repository root run:

```sh
python3 -B .build/rust/staged-foundations/zip-window/check.py --jobs 2
```

The lead must wrap that command in its shared flock and 120-second allowance.
This packet implements no lock, waiting or polling service. Builds run sequentially,
with Cargo jobs fixed at two and offline/frozen/locked on every invocation. It
uses only pinned existing `.toolchain` executables, verifies Rust's installed
receipt and files read-only, and acquires nothing. No dependencies or Cargo packages are added; `probe/lib.rs` is compiled directly
with the pinned rustc against the actual target library.

All snapshots, unique fresh mutant targets, caches, binaries and command logs go
under this packet's `out/run-*/`. Failure preserves logs and publishes no pass
JSON. Only a complete run writes `evidence.json`, with source/log hashes, every
independent case, all compiling semantic controls, and optimized target probe
symbols. A former run's evidence never represents a new run.

`tests/protocol.py` checks canonical bounded fields, event framing, constructor/
drop ordering, one output per request and exact request-dependent hex length
(including empty output's trailing space) before semantic comparisons. Signed
fault reports and oversized counts remain grammatical; expected traces check
whether they are correct. `tests/protocol_test.py` includes malformed fixtures
and valid semantic differences. The checker validates every baseline response
before comparing either transport to any authored expectation. Mutants must
compile both real library and real transport freshly and emit valid grammar
before a mismatch against their named independent witness counts as a kill.

Host focused arena/window tests and the borrowed no_std library probe are built
and run/compiled as appropriate. Separately, the RISC-V release static library
retains two externally named, non-inlined runtime-borrowed probe functions;
`nm` must report defined nonempty text symbols for both. Export attributes occur
only in the probe wrapper, use safe Rust slice ABI, and add no pointer operations
or FFI promise. Production code remains unchanged and forbids unsafe code.
This proves target compilation/code retention only: no firmware link, boot,
resource, hardware or whole FreeInk acceptance follows.

The transport input is a 15-integer header followed by `R at bytes present`:
physical size, advertised size, arena capacity, preallocation bytes, short-read
limit, zero-probe signed result, successful-call limit (fail_after), oversized
result flag, explicit-yield action, yield ordinal, read-completion action,
read ordinal, pattern, begin, end. Actions: 0 none, 1 invalidate, 2 size increment,
3 probe becomes nonzero. Patterns: arithmetic, XOR, all zero. Source events are
S size, P probe/result, R offset/request/result, Y explicit yield. A records
constructor used/highwater/failed-allocation bytes; O records success and complete
output hex including unchanged sentinel suffix; D records drop used/highwater/
failed bytes and physical cursor. Zero-length output prints a trailing space.
Transport limits bound host memory, not production admission policy. Fault counts
never write outside requested buffers. Malformed transport is not a product case.

Independent expectations assert exact traces and bytes against EACH transport;
matching each other alone is insufficient. Cases include 31-byte and 3-byte
short reads, cache rereads, cache crossing, final tail, direct 4096 cap, unsupported
and positive probes, capacity boundary, zero/end/empty range, null output,
u64::MAX, nonsticky invalid requests, sticky I/O/oversized/truncated failures,
yield and refill invalidation, size-probe short circuit, partial output, mark
restoration and persistent OOM diagnostics. Focused Rust arena cases add alignment,
NUL copies, reset/init, invalid alignment, array-byte overflow and zero alloc.

`mutations.json` proposes single-site semantic controls. Future runner must check
exactly one anchor, apply exactly one replacement in an isolated copy under this
stage, compile it successfully, preserve protocol syntax and execute its named
expectation. A build failure, crash, timeout or malformed protocol is INVALID,
not a killed semantic mutant. A kill requires valid protocol with a changed
expected value/trace. Baseline and frozen originals must pass first. Do not mutate
originals or claim kills until observed. The prepared runner enforces these rules; all mutation results remain PENDING until execution.
