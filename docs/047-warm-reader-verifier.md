# Temporary verifier scratch during saved reader selection

Saved Read from Home currently loads a committed import receipt, selects canonical
EPUB metadata, and authenticates the selected chapter. It does not recompute the
whole source digest on this path. Library selection continues to recompute and
compare the complete source SHA-256 before reusing that receipt. Missing or
unusable optional state keeps the existing strict admission and recovery paths.

On the installed `04b49c06c4ef-regional-experiment` image, five saved opens had a
3.024-second median and 3.066-second maximum. Canonical metadata took about
795–799 milliseconds. The observed 1.318-second BlockingDisplay phase belongs
to saved Read from Home, rather than Home painting. These are one-device software
completion observations, not optical settling or controlled cold-cache results.

The native compact revision owner keeps its existing 256-byte allocation. During
warm canonical selection only, it can temporarily borrow a disjoint 2048-byte
span immediately after the parser's 48 KiB arena. The display loan must actually
grant the complete 50 KiB extent. An unavailable tail, conflicting owner/index
storage, invalid arithmetic, or declined extended loan falls back to the original
48 KiB loan and 256-byte verifier. Legacy and provisional revisions keep their
original paths. No retained block, raster, catalog pointer, heap allocation, or
new static owner field is introduced.

This batches source calls without caching verification results. Each compact
request still reads and hashes every byte of each consumed 4096-byte block,
compares all 32 digest bytes, and checks the source, cancellation and receipt
owner before accepting its result. The receipt read lease policy is unchanged.
A short positive read must still accumulate the complete authenticated block.
A subsequent same-size substitution must still be rejected by fresh block
verification. Whole-source hashing and strict all-member admission are unchanged.
[Decision 050](050-reader-action-storage.md) later adds caller-lent action
storage that retains verified blocks within one receipt lease. This tail
scratch declines while that loan is active.

The temporary owner captures revision epoch and receipt generation. Scratch is
restored before releasing the parser loan, including failure and callback-driven
replacement. The selection remains unpublished until release and final owner
validation finish. A stale attempt must neither publish a selected frame nor reset
a replacement revision loaded by a callback. Opaque source implementations retain
the existing responsibility to keep their own backing state out of parser storage;
known reader, cache, revision, scratch, page-plan and index spans are checked.

The focused production-owner suite is `tests/reader_tail_scratch_test.py`; it is
part of `tools/check.py`. Native resource measurements and the complete fresh
canonical gate are required on a frozen committed source. Existing RAM and stack
limits remain protected. Any flash cost or new explicit helper stack guards
require measured four-variant evidence and a separate reviewed policy update.
Host source-call reductions do not establish a physical speed improvement.
Display refresh/recovery, approved artwork, settings, durable book identity,
progress authority and the NVS startup guard retain their existing behavior.
