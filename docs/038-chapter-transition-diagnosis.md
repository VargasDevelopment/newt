# Later-chapter failure diagnosis and bounded recovery gate

The owner reported readable pages and snappy turns, then an error during what
they believed was a chapter transition. The actual photo says exactly:
“This book could not open. Your reading nook is still here.” The footer says
“Chapter --”. The resolved Library JPEG was materialized through the supported
helper, verified at 435,773 bytes, and inspected at original resolution. Its
SHA-256 is `6284a283ae204d0fa5abc129cbc4a9fec7f572f2336539eaf84b7809a1d0c886`.
The private photo is not part of this repository.

The photographed chrome is the generic unavailable-reader state. It does not
identify the book, attempted spine, parser status, or failure phase. The owner's
specific cause is **unresolved**. Source, include and firmware trees at installed
`f22f72044de2` match analysis base `c505be373e25d75e1e5ec41eb016ca7423d6db0d`.
FreeInk remains pinned to `cd6f5b831c8fad899723b6d406586f00c575f021`.

## Reproductions and recovery

`python3 -B tests/chapter_transition_test.py` creates five deterministic authored
EPUBs locally and runs the production SampleReader, Reader UI dispatch,
FileBookSource, firmware display-lease configuration and staged publication.
The full file SHA-256 binds the actual ExternalProgressStore and two-slot codec
through a persistent bounded host backend. These are host ASan/UBSan checks;
they are not physical card or NVS evidence.

The valid three-spine book checks two forward chapter crossings, exact previous
last-page pixels/anchors, repeated next first-page pixels, save and fresh reader
reopen at both first and later pages of a later chapter, chapter-menu selection,
first/last book clamps, and bounded source reads. All run with portrait and
landscape settings and the progress bar both enabled and disabled.

| Authored later-chapter condition | Observed status |
| --- | --- |
| Unsupported U+1F9EC | Unsupported (6), exact glyph retained |
| Malformed XML | ParseError (8) |
| Valid XML with a 500,000-byte attribute | ParseError (8), XML budget rejection increases |
| Missing referenced ZIP member | NotFound (9) |
| Display scratch lease refused | OutOfMemory (7) |
| Expat allocation refused | OutOfMemory (7) |
| Source read fails while layout XML is live | IoError (1) |
| Cancel hook signals while layout XML is live | IoError (1) |

Each of the 32 failure/settings combinations preserves the successful semantic
checkpoint and the prior published body frame during the failed navigation
transaction, balances the display lease, releases all XML allocations, saves
unchanged progress without an additional write, and reopens the exact prior body
frame using freshly loaded durable progress. Ordinary subsequent UI painting
still renders the unavailable chrome; the test does not claim that the full UI
keeps displaying prose after failure. No bounds or admission requirements are
relaxed. Existing efficiency and borrowed-storage suites independently cover
long valid chapters and owned-reader frame comparisons.

Pinned XmlSax maps an `XML_Parse` allocation-budget failure to ParseError as
well as mapping malformed syntax to that status. Status alone therefore cannot
distinguish those two conditions. A turn forces metadata selection and full
layout of the new chapter; certified same-chapter turns can use a prefix. This
explains why these authored inputs can read initially and fail only later, but
does not prove that any such input was present in the owner's book.

The installed ordinary turn path does not enter BookOpenScope or the opening
profile scope. These host cancel-hook tests do not establish physical queued
Power cancellation or an opening watchdog deadline on ordinary turns. DIAG
reports reader status and the last successful coordinates after failure;
RENDER_RETURN can acknowledge completed error-chrome painting. Neither proves
successful navigation without the reader-open/status and actual-anchor checks.

## Extended physical procedure

The fixed reading acceptance journey now has 27 stages. `primary_next2` must
cross to a greater actual spine. Previous must return to the exact preceding
anchor. The later recovery turn must reproduce the first crossed anchor. Four
new stages close the reader, reopen that later-chapter checkpoint, return to
the exact prior chapter anchor, and repeat the crossing. Existing live protocol,
device/build/source identity, publication, timeout and close checks still apply.
Boot/sleep checkpoint flags are not treated as ordinary exit-save acknowledgments.

The primary source must already be positioned so its second forward turn
crosses a chapter. This procedure inserts no positioning, reset, NVS restore or
additional exploratory input. A future separately authorized setup can use an
authored fixture or a verified real-book endpoint while preserving current
owner progress. An unsuitable starting checkpoint fails the gate. The old
23-stage receipt cannot pass this extended procedure.

Synthetic negative controls cover same-spine character increases masquerading
as a crossing, failed-reader chrome with a fresh display generation, and lost
or shifted later-chapter reopen anchors even when stage, operation and exact
wire response are all edited consistently. Live simulated failures stop before
further navigation. Successful validation explicitly returns
`chapter_crossing=pass` and `book_completion=unverified`.

Two independent source/evidence reviews completed CLEAN after correcting the
failure harness to resume tracking after its pre-failure durable save. A
calibrated temporary mutation of SampleReader advanced tracking on a failed
turn: the original inactive harness accepted all 32 cases, while the corrected
active harness rejected the mutation at the checkpoint/write-count assertion.
The mutation was confined to temporary host sources and removed after testing.
The complete `python3 -B tools/check.py` host gate passed with exit 0, including
ASan/UBSan, all 27 acceptance tests, recovery regressions, NVS startup guards,
Unicode font proofs and exact PBM/PNG preview checks. Source/include/firmware
and the pinned SDK remain unchanged. No native firmware image or physical
chapter evidence was produced by this source/test-only change.

No hardware phase ran, no firmware behavior changed, no new image was flashed,
and automatic-cover integration remains paused. Actual-book chapter/menu/edge
checks, optical quality, physical latency, memory minima and full-book completion
still require separate owner-authorized physical evidence. The existing 23-stage
timing comparison remains historical evidence within its documented scope.

Current owner progress is unknown. Historical backups must not be restored.
The next decisive evidence is the filename and approximate last working position,
followed by bounded diagnostic metadata or a separately authorized device check.
No private book text, raw device backups or credentials belong in this change.
