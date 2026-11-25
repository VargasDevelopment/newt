# ZIP container contract

Source authority: FreeInk SDK cd6f5b831c8fad899723b6d406586f00c575f021.
PROVENANCE.json hashes each complete original file; all notices remain in frozen/.

## Layout, lifetime and capacity domains

C allocates ZipEntry[] plus names in one byte arena, with ABI-dependent pointer
size/alignment. Rust borrows initialized typed Entry[] separately and allocates
only names (length+1) in accepted Arena. It never reinterprets byte arena as
Rust objects. Rust metadata OOM is `totalEntries > slots.len()` before names;
names allocate sequentially, retain consumed space on failure, and failed open
publishes zero entries. Entry[] and Arena remain exclusively borrowed while a
Catalog exists; arena reset/release/reinit therefore cannot invalidate its
names. Entries returned by reference cannot outlive Catalog. Names expose only
borrow-scoped views. Entry's public CStringRef can be copied for metadata, but
must not be resolved against a different arena; Catalog owns the scope for its
lookup surface, matching accepted Arena's documented caller contract.

CPP_CAP prints sizeof(ZipEntry), arena.used/highWater/failedAllocSize.
RUST_CAP prints slots, name-arena.used and name capacity. These are DIFFERENT
resources. Oracle capacity is `ENTRY_SLOTS*sizeof(ZipEntry)+NAME_BYTES`.
For exact name-capacity/OOM comparisons choose ENTRY_SLOTS equal advertised
entry count; otherwise unused C record budget can be consumed by names.
The corpus's name-OOM and boundary fixtures obey this rule. Metadata-OOM uses
zero slots AND zero bytes. Root must retain both resource outcomes, compare
corresponding allocation demands by domain, and must not claim byte equality.
Rust does not reserve fake C bytes or claim matching high-water values.

C deflate open makes three bump allocations in order (state align8, dictionary
32768 align16, input2048 align16), attempts all three even if one fails, and
returns OOM if any failed. Rust takes `Option<InflateScratch>` containing all
three initialized typed borrows: None is unavailable scratch -> OOM at the same
algorithm stage, Some resets only State. This does not emulate partial C arena
allocation diagnostics: root must measure/map state layout and partial failure
budgets separately if claiming allocation equivalence. The 0/1 host scratch
cases test unavailable/available domains, not each C byte threshold. No resource
reduction/equality or firmware stack assertion is made. Rust dictionaries must
be initialized caller memory (uninitialized C history is not a safe domain).

There is no baked-in entry-count ceiling: caller slots may cover every supported
u16 EOCD count (65535 remains Unsupported). Name and compressed-input limits are
exactly the SDK's 1024 and 2048; the latter is not an entry-size limit. Output
is streamed through the complete 32768 ring; metadata uncompressed size does
not cap deflate delivery, just as C. Scratch scan/sequence arrays are borrowed,
not multi-KB temporaries in runtime functions. Runtime locals are only fixed
22/30/46 byte headers and small state. State construction belongs to caller's
initialization domain; target probe does not construct it on the reader stack.

BookSource must return counts no larger than destination, and <=i32::MAX.
Over-returning C adapters cause out-of-bounds behavior; Rust rejects this safely.
Read buffers larger than i32::MAX are rejected because positive C return counts
are not representable. Repeated operations must keep source content stable as
in C. Source rebinding means construct a new Catalog/Reader; same-source reopen
retains the original reset and error mutation rules. Null C source/entry/name
pointers, unterminated strings, aliasing live storage and C undefined calls are
outside the safe domain; find(None) explicitly models null path.

## Semantic details retained

EOCD reverse 4096 scan plus three-byte overlap; no comment validation, multi-disk
or flags rejection absent from original; ZIP64 sentinel statuses; directory
range check but no invented per-entry dirSize enforcement; 1024 SeqReader with
buffer-aware skip; exact-read loops for EOCD/local headers but single stored
reads; central method accepted until Reader.open; local name/extra authority;
raw sentinel validation and stored mismatch status differences; complete reader
counter reset before validation; stale dataOffset retained on pre-offset errors;
pending delivery before inflate/refill; compressed byte count is bytes fetched,
not merely consumed; final-input flag based on fetched count; decoder counts
update before error; error leaves pending output; totalProduced changes only at
successful deflated read exit; ring wraps after write, pending bytes don't wrap;
no invented CRC or decoded-size checks. Existing Window's optional caching is
not inserted: changing source call patterns would violate the pinned SeqReader
and exact trace contract.

Entry metadata and source content remain stable while a reader is active. Rust
reads a value snapshot of Entry; mutating the original C entry through another
reference is defined C++ usage outside this supported Rust caller domain.

## Newt additions (#60)

These are additive and outside the oracle. No C path reads them, and the
pinned trace, statuses and mutation anchors are unchanged.

- `Entry.flags` keeps the central general-purpose flags. It fills existing
  padding, so `Entry` stays 24 B (32-bit) and 32 B (64-bit).
- `Catalog::directory_offset()` returns where the central directory starts.
- `Status::Damaged` (13) is never returned by Catalog or Reader. Only the
  open-time member check returns it (`core/src/member_check.rs`, reached via
  `canonical::CatalogView::check`), when a local header disagrees with its
  central record.
- `Reader::open` takes `&Checked`, not `&Entry`. Only the entry type in the
  signatures changed, so the compiler refuses any read of a member that
  skipped the open-time check. A `Checked` reads as its `Entry`, so behavior
  and traces are byte-identical, and the oracle and expected files are
  untouched. The pinned harness reads raw catalog entries, as the SDK does, so
  it wraps them with `Checked::unverified`. That exists only under the
  `pinned-harness` feature, which RV32 (firmware) builds refuse to compile.
