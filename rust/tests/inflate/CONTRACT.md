# Streaming DEFLATE contract

## Authority and scope

`provenance.json` hashes the complete immutable Git-object files from SDK
cd6f5b831c8fad899723b6d406586f00c575f021. The frozen source is NOT the patched
live vendor tree. The miniz implementation has both Unlicense dedication and
RAD/Valve/Rich Geldreich/Tenacious copyright permission notices; keep the
complete upstream notices. The canonical licences are listed in
`rust/THIRD_PARTY.md`; the pinned SDK retains the original C and header. Do not
label this source simply MIT. The Rust code derives from tinfl under those
retained terms. `provenance.json` remains the captured-stream identity; no copied
reference tree or reference compilation is required.

The compiled SDK root is frozen `src/vendor/miniz_impl.c`, which includes
`epub/MinizConfig.h` and the local miniz.c. It disables compressors, archives,
stdio/time and zlib-compatible names. Its renames prevent capture by ESP ROM
symbols with incompatible state layout. This port replaces low-level tinfl
only; it has no C calls or linking dependency. Heap wrappers, zlib mz_stream,
archive APIs, compression and CRC are intentionally outside this dependency.

## Borrowed API and state

`State::new(BitBuffer)` initializes state; `reset()` resets the coroutine entry
as tinfl_init does. Construct/initialize state in caller storage before target
probing. `decompress(input, output, offset, capacity, flags)` returns status,
input consumed, output produced and running Adler. `adler32()` exposes the
running check. All four original flag bits are retained. Flags are evaluated
at their original decision points, including checksum computation on return.
No dictionary resides in State: the caller owns the initialized output/history.
The three lookups, original trees, code lengths and 457-byte repeat scratch
are fixed State fields. Tree building has only original 16/17-entry local
counter arrays. No heap, global cache, input capacity limit, unsafe Rust,
internal 32KB dictionary, or hidden 47KB local frame. Stack equivalence is
**not established** without codegen and measurement. `new` returns a sizable
value; do not treat that constructor as an in-place initializer.
`initialize_storage(bytes, mode)` writes the same initial fields into an exact
State-sized byte region, rejecting any other extent without mutation. It uses
native field offsets and explicit one-byte enum representations, not a wire
format. The physical storage boundary checks alignment and owns the exclusive
typed loan after initialization; the Core initializer remains safe Rust.

Input and output must be separate allocations/borrows (the safe API enforces
nonaliasing). Keep the same initialized logical output/history across calls.
Nonwrapping calls advance offset by produced bytes and may choose capacity
within the remaining allocation. Ring calls use a >=32768-byte power-of-two
ring, capacity = ring_size - offset, and advance offset modulo ring_size.
Read/consume each produced segment before it can be overwritten. Bytes outside
the writable segment are history and are not modified. The original C also
accepts smaller power-of-two raw rings, and this implementation retains that
mechanical parameter acceptance; such rings do not provide general DEFLATE
history. Reject overflow/out-of-slice parameters as BadParam with zero counts.
A zero output segment is supported. A non-power-of-two ring end is BadParam;
nonwrapping output need not be a power of two. Caller storage may exceed the
logical ring, but the mask derives from offset+capacity exactly as in C.

HAS_MORE_INPUT chooses NeedsMoreInput vs CannotMakeProgress at starvation;
the latter is resumable. Failures from malformed headers/tables/blocks are
terminal. Output can be partial on any status, but Adler is updated only on
nonnegative statuses, matching C even when a negative return emitted bytes.
Done is terminal, yet the C common-exit checksum check runs again on repeated
calls: a mismatched zlib trailer repeats AdlerMismatch while PARSE_ZLIB_HEADER
remains set. This follows the actual C, contrary to its header commentary.
Reset leaves dictionary bytes alone. Ring history must be initialized by the
caller; malformed backwards references may read that initialized history.

Explicit enum phases retain suspended GET_BYTE/GET_BITS/HUFF_FILL, pending
literal/stored bytes, repeat expansion, match length/distance and copy counters.
StoredAwaitInput matters: resuming C state 38 checks input before output.
HUFF_FILL resumption bypasses the ordinary two-byte prefetch. Fast literal
pairs delay publishing the first byte until the second code validates.
Unread complete prefetched bytes are returned except at input-starvation
statuses. These are observable transport semantics, not just output equality.

## Arithmetic and comparison domain

Pinned miniz.h derives MINIZ_HAS_64BIT_REGISTERS from compiler target macros.
LP64 hosts normally use 64-bit bitbuf and four-byte fast prefetch below 30
bits. RV32 uses 32-bit bitbuf and two-byte prefetch below 15 bits, including
between the fast literal pair. State's mode makes both paths selectable on a
host. Its u64 storage is masked after each append in Bits32 mode; all original
values/counters use u32 and output offsets/mask use usize. No claim is made
that u64 storage has optimal RV32 codegen. Host mode32 compares exact per-call
status/in/out/Adler/bytes to the C's original 32-bit-bitbuf branch; mode64 does
likewise for its original 64-bit branch. Cross-mode per-call traces need not
match. Cross-target contract is matching branch + identical call flags,
chunks, initialized history and output offsets/ranges. Portable supported
ring sizes/offsets must also fit RV32 usize. Final decoded bytes must agree
across modes for valid streams. Root must run RV32 itself before claiming
RV32 compile/runtime parity.

The C oracle is only used on disjoint initialized input/output and state.
Its output allocation adds the original 258-byte maximum match padding:
C forms max(out,src)+counter before deciding to copy slowly, so padding keeps
that speculative pointer arithmetic inside the backing allocation. The passed
logical capacity and ring mask are unchanged. No unaligned typed loads are
enabled (pinned default is zero). C structs are initialized to zero before
first tinfl_init; the safe Rust state has typed initialized fields.

There is no blanket equivalence claim for C undefined behavior: overlapping
input/output memcpy, invalid pointers/size arithmetic, uninitialized history,
corrupted state, dynamic lengths outside original arrays, invalid symbol/tree
indexes or invalid shifts are outside the comparison domain. Rust bounds/tree
checks return Failed on detected out-of-domain indexes; `huff` cannot report
an arbitrary undefined C memory read equivalently. Some malformed incomplete
single-symbol trees have zero-length lookup entries and can produce unusual
progress or fail only in the fast path; this translation retains the lookup
and slow/fast distinction. Broader malformed-input totality needs independent
review: the staged code is not yet verified as a hardened decoder.
