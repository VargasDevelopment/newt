# Explicit reader allocation alignment

The installed `2e42846aca0e-regional-experiment` image passed complete flash
readback and preservation checks, but the actual book journey failed on its
first open. The last Activate completed normally and returned to Library.
No timeout, recovery input, retry or subsequent hardware action followed.
The original error is unknown: rejection cleanup resets the reader status,
and the client did not retain unframed Serial admission/error logs.

Read-only inspection of that image exposes a concrete allocation contract
defect. The shared 48 KiB arena is allocated through ordinary nothrow new
(`0x42030cde`), which reaches the pinned C3 malloc path requesting four-byte
alignment (`0x40380d2a`). `PhasedReader` checks both arena bases for sixteen-byte
alignment (`0x4202c81c`) and rejects an invalid workspace as out of memory.
This is a credible hardware-only failure path, not proof of the observed cause.

`AlignedOwner` now constructs reader storage, its owned arenas and page plan
inside explicitly padded byte allocations. A stateless deleter reads the raw
base with `memcpy`, destroys the aligned object, and releases its original
allocation. Embedded fixture storage uses the same ownership boundary.
Admission charges the complete requested block, including the pointer header
and worst-case padding; every arena remains 48 KiB. Borrowed display storage
and its lease contract remain unchanged.

XML allocations also align their payload explicitly. Headers retain the raw
base, charged size and payload size. Realloc caches metadata before the call,
moves retained payload bytes when the new base requires different padding,
then reconstructs the header. Failed realloc preserves the original payload
and accounting. The aggregate 32 KiB XML ceiling charges all requested header
and padding bytes; the separate 32 KiB runtime reserve remains intact.

The source gate recognizes `<utility>` as a standard C++ header for perfect
forwarding. No external adapter, generated-file exclusion, generic line limit,
SDK build flag, selected stack measurement or dependency was added or removed.

Regression coverage deliberately shifts reader raw allocations through all
sixteen address residues. It exercises sixteen-byte placement construction,
move/release/allocation failure, embedded reading, real owned/borrowed external
pages, turns, exact saved-page reopen, settings reflow and retry cleanup in both
orientations. The XML observer cycles raw residues 0/4/8/12 during allocation
and realloc; a growing token checks retention through changing padding.
These are host emulations, not measurements of a physical allocation address.

## Reviewed native allocation costs

Clean preliminary compiles measure unchanged static RAM and RTC. The exact
flash ceilings below include the explicit ownership and realloc logic; final
clean builds must fit these measured bounds. The only selected frame change is
XML resize, 32 to 48 bytes, to retain offsets and lengths across realloc.
All other selected frames remain static at their previous limits. These are
explained correctness costs, not blanket headroom or removed measurements.

| Variant | Static RAM | Application bytes | RTC |
| --- | ---: | ---: | ---: |
| default | 133,248 | 1,237,104 | 300 |
| DEV USB | 134,064 | 1,247,760 | 300 |
| regional | 133,760 | 1,240,928 | 300 |
| regional DEV USB | 134,576 | 1,251,600 | 300 |

The actual C3 owned-reader allocation adds pointer headers and padding rather
than shrinking storage: aggregate admission and largest-block guards account
for that overhead. Linked disassembly and final reports remain distinct from
the physical admission values that the failed journey did not capture.

The previous image and failed private receipt remain frozen in the separate
`newt-reader-heap` checkout. This fix is source-only until an exact rebuilt
candidate is reviewed and authorized. Actual open/page/reopen/recovery evidence
is still required before owner acceptance. Efficiency profiling and battery
energy measurements are outside this stabilization change.
