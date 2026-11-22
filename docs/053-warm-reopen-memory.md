# Warm reopen memory admission

## Problem

A warm reopen (Home → Read of a book with an imported receipt) normally
takes about 1.4–2 s on the X3. `LibraryReaderSelection::bindSession` first
admitted the receipt-backed path with the `ReaderRevisionSession` (about
1.7 KB) still allocated. The requirement is 128,212 bytes: the 62,676-byte
reader workspace, the 32 KiB XML budget and the 32 KiB runtime reserve
(`firmware/reader_admission.h`). When free heap was about 1.1 KB short, the
selection closed the reader, dropped the session, hashed the whole book and
opened through strict raw admission, which then inflated every ZIP member
(Stage 1 now expands only the required ones; see docs/054). For a
6.4 MB book that exceeds the 20 s open budget: "Opening timed out",
`BookStatus::IoError`.

The fallback was also sticky. The source stayed open with no session, so
every later Home → Read repeated the 20 s path until the Library was browsed
or the device slept.

## The memory is the same

Let `R` be the requirement and `S` the session's allocation
(`sizeof(ReaderRevisionSession)+alignof(std::max_align_t)`, the sizing Home
overview admission already charges). The strict path first releases the
session, so it admits when `free + S >= R`. Warm admission with the session
allocated needs the same total. Releasing the session never helps the
free-heap check. It can help only the largest-block check, by coalescing.

`readerSessionReserveCredit` names this credit. It reads the session's
`ExternalReaderSession::ownedHeapBytes()`, which is `readerOwnerHeapBytes<ReaderRevisionSession>()`
for the receipt owner and zero for anything else.

`S` is the existing nominal sizing, not a measured release. Per-block allocator
overhead can differ by a few bytes, so the equivalence holds to within that
margin.

## Decision tree

`bindSession`, with the source open and the reader closed:

1. **Session present.** `readerWarmHeapBinding` checks `free + S >= R` and the
   uncredited largest block.
   - **Both pass.** Attach the session and return `Ready`. No hash runs here.
   - **Credited free heap short.** This takes precedence. Keep the session,
     detach the reader and return `FreeHeapGuard`. The user promptly sees
     "Reader session unavailable. Position kept." No hash or strict pass
     runs, because strict admission could not succeed either.
   - **Only the largest block short.** Behaviour is unchanged: release the
     session, recheck, hash the full source, compare the digest, recheck and
     bind strict.
2. **No session** (for example, its allocation failed). Unchanged: recheck,
   hash, compare, recheck, bind strict.

`PendingExactResume::attempt` repeats admission just before the open. It
credits the attached session the same way, so a warm binding is not rejected
there. A strict binding has nothing attached and keeps the full requirement.
`attempt` keeps its largest-block precedence. The serial admission line now
also prints `credit=`.

**Stickiness.** `ensureSource` recreates a missing session with `makeSession`
whenever the source is already open, exactly as a fresh source open does.
This covers both an earlier fallback and a failed allocation. It does not
apply inside a Library candidate transaction, which keeps the owner decision
`select()` has just made. If recreation fails, the strict path runs as before.

## What stays identical

- Full-digest binding of the selected identity.
- The receipt digest must equal the selection's full digest, and every
  consumed block is verified.
- A missing, damaged or mismatched receipt reimports with a complete digest;
  otherwise the open falls back to strict.
- A changed full digest is still rejected.
- The strict path's hash and digest comparison, and its pre- and post-hash
  admission rechecks.
- `readerHeapRequirement`, `readerHeapBinding` and its largest-block
  precedence, and the uncredited largest-block check.

## Tradeoff

A warm open admitted with the credit has up to `S` (about 1.7 KB) less reserve
than an uncredited warm open. The session's own allocations, such as
`ChapterPageCache`, metadata staging and receipt/page file handles, were
already drawn from the runtime reserve and still are. The credit applies to
whatever the session's open does: a warm hit, a reimport after a missing or
corrupt receipt, or its internal strict fallback. Before, a heap this tight
always took the strict fallback, which released the session and kept the full
reserve. The reduced headroom is the explicit cost of not timing out.

A book whose receipt import keeps failing (for example, a full or read-only
card) now retries the import on each Read after a fallback. A freshly opened
source already behaved that way. The old sticky null owner had acted as an
accidental negative cache within one source lifetime.

When both constraints fail, the warm path's wake evidence reports
`FreeHeapGuard`, not `LargestBlockGuard`. The user message is the same.

## Evidence

Host tests (`tests/sd_reader_heap_fallback_checks.h`,
`tests/reader_open_phase_proof_checks.h`) model heap snapshots. They prove
the decisions and that no hash or strict admission runs. They are not
device evidence.

**Builds.** Four clean `--health-report` builds; see "Round 2 reader and Home
cost" in `docs/CODE_HEALTH.md`. `bindSession` and `readerHeapBinding` stay
within their selected ceilings.

**Device (owner's X3, regional dev-usb, 2026-10-05).** The image was
`54cf1bc` (tag `evidence/round2-54cf1bc`) plus a diagnostic patch, not in this
tree. The patch holds 3,400 B of heap from boot, which reproduces the
shortfall, and logs the heap low-water mark (`ESP.getMinFreeHeap()`). Each
book ran in one session from a USB-reset boot: a Library open (no receipt,
after an earlier purge), 10 turns, Back, three Home → Read reopens,
`PRESS purge-cache`, a Read that must reimport, three turns and Back.

- **Every open** printed `free=127048 largest=114676 required=128292
  block=49171 credit=1728`. Free heap was 1,244 B short, and the credit
  admitted the warm path. The requirement includes Phase A's extra 80 B
  (docs/054).
- **Warm reopens:** 1.11–1.12 s for book 0 and 1.35–1.36 s for book 24,
  three each. None ran a hash or member admission
  (`Reader hash: us=0`, `members: n=0`).
- **Credited reimport** after the purge: 6.1 s for book 0 and 10.8 s for book
  24. Both opened.
- **Heap low-water:** 47,024 B for book 0 and 46,696 B for book 24, covering
  each whole session, including chapter crossings. No uncredited comparison
  was taken on this image.
- **Not exercised on device:** a Read after a forced strict fallback (session
  recreation). The host tests cover it.

The raw data stays outside the tree (numbers only, no titles or text).
