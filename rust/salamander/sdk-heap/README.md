# SDK heap boundary

`newt-sdk-heap` is a local, dependency-free `no_std` library. It exposes the
reviewed `SdkHeap: GlobalAlloc` implementation through four private C functions.
It never registers an allocator. Core and the default target closure have no
heap dependency; adding this workspace member does not add one.

`native/native_alloc.c` calls the existing libc malloc/realloc/free and byte
operations. The SDK remains the heap owner. There is no build script, external
Cargo dependency, arena, scheduler initialization or capability override.
The final owner must link the matching C bridge exactly once under its real
SDK configuration. This is not firmware or physical resource acceptance.

## Allocation policy and cost

`include/newt/aligned_storage.h` states the actual C3 malloc guarantee: four-byte
alignment. Requests aligned to 1, 2 or 4 use malloc/realloc/free directly and
carry **zero bridge metadata**. Host tests use this same threshold even though
host libc supplies stronger alignment. This avoids a host-only max_align_t
assumption silently breaking RV32 objects.

For alignment >4 the exact requested libc block is
`size + sizeof(void*) + alignment - 1`. The payload is rounded within that
block; its immediately preceding bytes save the raw malloc pointer using
memcpy, without requiring a naturally aligned header or type-punning. On C3
this costs four bytes plus up to alignment-1 bytes of padding per allocation,
before existing vendor poison/TLSF overhead. No size field, global ledger or
per-task object is retained. The caller already supplies size and alignment.
This cost buys Rust's full arbitrary power-of-two Layout alignment while
preserving the existing libc heap selection, poison checks, hooks and release
behavior. It is not covered by the separate CoverHeap accounting allowance.

Small-alignment realloc uses native realloc. Over-aligned realloc allocates a
new aligned block, copies exactly min(old_size,new_size), then frees the old
raw block. Failure does not free or modify the original allocation. Successful
shrink may move or fail, which GlobalAlloc permits. Growth bytes are unspecified.
Over-aligned realloc temporarily holds both old and new blocks; this peak heap
cost needs physical acceptance. No native extended-alignment realloc promise
is invented. Zeroed allocation zeroes precisely the requested payload.

Allocation validates nonzero size, nonzero power-of-two alignment and Rust's
rounded-size signed-address bound, then checks header/padding addition before
calling libc. Invalid or unrepresentable requests return null without a heap
call. No multiplication is used. A valid Layout whose bridge overhead cannot
fit also returns null, a permitted allocation failure. Zero-size allocation and
null/zero-size resize are outside the owned-pointer API; zero-size resize is
defensively rejected without freeing. Only exact originating pointers and
original layouts may reach free/resize. Do not pass bridge payloads to libc or
libc payloads to the bridge, even on the headerless path.

## Unsafe and task contract

The only Rust unsafe surface is the GlobalAlloc implementation and four private
FFI declarations. `usize`/`size_t` and data pointers use the actual C ABI; the C3
build asserts 32-bit widths. No struct, Rust enum, native mutex or errno crosses
FFI. C owns only allocations; it retains no borrowed call arguments. A saved raw
pointer is recovered with memcpy, so extended alignment never entails an
unaligned pointer load. Pointer arithmetic stays within checked allocation
bounds. Copies use disjoint allocations and preserve the complete required
prefix. No panic, formatting, Rust allocation, lock or callback is introduced
inside this boundary.

Use ordinary task context after the real SDK heap is initialized and while the
real scheduler/heap remain available. No ISR/NMI, early-startup or shutdown
allocation claim is made. Independent allocations can be called concurrently:
the bridge has no mutable shared state and relies on the existing SDK libc/
heap locking, not a new interrupt mask around malloc. Ownership transfer across
tasks is permitted only after synchronization by the caller, with no concurrent
access/free/resize of a block. SDK failure/tracing hooks must not reenter Rust
allocation or unwind. The actual SDK configuration must remain non-aborting on
allocation failure; target compilation rejects an abort-on-OOM configuration.
Existing panic/OOM handling remains the final binary owner's responsibility.
Fallible Vec callers must continue to use try_reserve; registration alone does
not make infallible allocations recoverable.

## Existing Run integration

The existing `.build/rust/target-allocator-run-preparation/run.py` remains the
only execution entrypoint. Its existing recipe is
`.build/rust/target-allocator-preparation/recipe.py`. The proposed recipe uses
`run.cargo` to build `-p newt-sdk-heap --lib --release` for both
`x86_64-unknown-linux-gnu` and `riscv32imc-unknown-none-elf` from the canonical
workspace snapshot. Cargo is offline/frozen/locked, with the existing two-job
allowance and unchanged workspace release profile. No generated manifests,
production body rewriting or packet-to-crate source copying occurs at runtime.
The ordinary sealed workspace snapshot is the source for both builds.

Host C uses `native/native_alloc.c` with `NEWT_ALLOC_HOST_TEST`; C++ fault and
policy witnesses use `host/host_checks.cpp`, with canonical native headers and
the existing repository aligned-storage comparison header. The Rust host
witness is `host/host_test.rs`; it calls the real GlobalAlloc implementation
without global registration. Link wrappers stay host-only. Its rustc link
command and native flags retain their existing settings.

Target C uses canonical `native/native_alloc.c`, the same actual acquired C3
compiler, SDK flags, include closure, DIO sdkconfig and dependency output as
before. `probe/lib.rs` remains the optional standalone borrowed Vec object;
it alone registers the allocator. Its rustc command uses the Cargo-built RV32
rlib and retains its existing settings. Manually linked witnesses live under
host/ and probe/, outside Cargo's automatic integration-test discovery. No default binary or Core feature selects or registers it.

The existing Run still owns process cleanup, deadline, local output/cache,
source inventories and final tool/source verification. SDK flags/config/tool
seals and the run envelope are unchanged. The objects are not a final SDK link.
Do not raw GNU `-r` release Rust LLVM bitcode, add a fake SDK/panic owner, alter
profiles, relax unresolved symbols or retention rules, or raise resource limits.
Any future final-probe owner must compile its registration once, retain an actual
call to `newt_allocator_borrowed_probe`, and link the real SDK closure. Do not
link both the standalone borrowed object and a second compiled copy of it.

The probe takes disjoint readable input and writable output, exclusively
borrowed for the call. Count must fit output capacity and isize. Empty input
needs no valid addresses; no pointers are retained. Results are 0 copied,
-1 capacity/signed bound and -2 fallible reserve failure. Its final SDK task,
panic/OOM owner, map/import resolution and scheduler lifetime remain pending.

## Source and evidence

The six implementation/witness files are byte-for-byte materializations of
`.build/rust/target-allocator-preparation/{lib.rs,native_alloc.c,native_alloc.h,
host_test.rs,host_checks.cpp,probe.rs}`. `PROVENANCE.md` and `LICENSE` preserve
original Newt MIT authorship and the consulted SDK/policy references; no vendor
implementation is copied. The latest two-thread Rust host witness and C++
release-counter assertions after failed realloc, grow, shrink and final free
are preserved without weakening or replacement.

The owner reports a fresh Astra review with two findings fixed. The existing
`.build/rust/target-allocator-focus-01/evidence.json` records passing host tests
and actual SDK C3 object compilation for the earlier packet. Those results
belong to those exact sources and commands, not to this canonical Cargo path.
Root must rerun the existing proof after adoption and current reader proofs.

The host witnesses cover alignments 1/2/4/8/16/64/256/4096 and odd sizes,
zeroing, invalid/zero/overflow/header-overflow rejection without heap calls,
exact requested metadata cost, malloc/realloc failure, preserved old bytes and
ownership, grow/shrink prefixes and real release. The Rust witness exercises
GlobalAlloc directly and two independent host threads. Host libc concurrency
does not establish SDK task locking or target execution. Semantic negative
controls remain with Root's existing proof owner; this crate adds no runner,
registry or guard.

This preparation executes only Python AST/TOML parsing and, if used, existing
rustfmt. No build, test, gate, network, device, timing, acquisition, commit or
subdelegation is performed. A fresh canonical host/RV32 receipt, final SDK
link/retention, real task behavior, memory/fragmentation/realloc peaks, stack
and physical device acceptance remain separate pending evidence. No profile,
SDK configuration, ceiling, floor, reserve or firmware readiness claim changes.
