# CrossInk migration and phased reader evidence

Historical slice: current bounded runtime and trial gates are recorded in
[015-m1-library-trial.md](015-m1-library-trial.md).

The isolated `spike/sd-reader-migration` branch now integrates this phase model
through the shared reader lifecycle, with a library browser, complete-content
SHA-256 checks, separate two-slot per-book progress and validated SD sleep
selection. It is **unflashed**. The initial production C3 build and linked NVS
guard passed before the final empty-cover/routing changes; final combined gates,
fresh review and a new native build remain required for a hardware candidate.
The current runtime read-only host probe opens 60 of 61 backup EPUBs, with one
explicit unsupported case and 58 next-page anchor restores. It does not establish
physical SD heap/stack/latency or all chapters of all books. Aggregate evidence
is in `artifacts/sd-runtime-host-verification.json`. The authored experiment
below is retained as its original narrower evidence.

CrossInk migration is a supported product path to earn, not a manual workaround
or a claim established by the SD/XML probe. A reader should be able to try Newt
with their existing books, locate the same passage, and return to CrossInk
without losing the original library, progress or settings. The current firmware
still opens only the embedded fixture. No migration import or production SD
reader is integrated into the approved image.

## Smallest RAM experiment

`python3 tools/phased-reader-benchmark.py` uses only the original CC0 Small
Lantern fixture, deflated with the existing generator. It compiles the unchanged
FreeInk pin with the existing strict-warning ASan/UBSan host toolchain. The
focused test is also included in `python3 tools/check.py`.

The test opens a temporary `Book` in a 48-KiB arena with a separate 48-KiB parse
arena. It copies the selected ZIP descriptor, href, title and spine count into
an independent owner, replaces the descriptor's arena-owned name pointer, and
destroys the `Book` before resetting and overwriting its arena. That same arena
then holds `ChapterLayoutSession` layout buffers. The descriptor and source stay
alive until the session has been aborted, and only then are both arenas reset.
The source adapter is exercised with 31-byte short reads.

This uses the existing null-ZIP session API, with embedded styles disabled.
It skips image probing/placement, does not implement interactive links, and
does not establish production Unicode rendering. Parse and layout buffers
remain distinct: inflate/XML drive the page callbacks while layout is live.
The test samples Expat payload and both arena cursors inside page callbacks to
demonstrate that overlap. Closing Expat does not itself reset its arena.

The oracle uses the ordinary full-Book layout path first, then compares every
page's text, resolved positions/styles, character start and complete chapter
character count against phased incremental layout. It restores beginning,
interior and final character anchors in both chapters over repeated reopen
cycles. Failure cases cover metadata parser allocation, missing chapter, layout
and parse arena exhaustion, parser allocation during session open, source I/O
at open and during parsing, and cancellation of an active session. A complete
chapter after these cases proves recovery. All cases require zero live Expat
allocations and zero arena cursors after teardown.

Validation: the complete repository host gate passed; the focused sanitized
benchmark passed again after a bounded independent review corrected phase
high-water accounting and exhaustion-case arena checks. The final read-only
review found no remaining actionable issue. No native build, source integration,
device/card access or private-book read occurred in this experiment.

Aggregate measurements are in `artifacts/phased-reader-benchmark.json`.
This authored run compared 31 pages across seven complete chapter layouts,
restored 18 anchors and passed eight cleanup cases. Its host metadata/layout/
parse arena peaks were 694 / 48,328 / 45,824 bytes. Expat's requested-payload
peak was 10,024 bytes; page callbacks observed up to 9,208 live Expat bytes and
a simultaneous arena-plus-Expat sample of 103,360 bytes. The stable selection
owner costs another 808 host bytes, and Expat retained zero bytes at teardown.
The two modeled capacities cost 98,304 bytes. The existing inline filename
catalog must still be charged separately (12,304 host bytes); resetting it does
not free storage. Add Expat payload, copied selection/owner storage, SD/runtime
allocations and allocator overhead. Callback samples are not a continuous
combined peak. Oracle vectors and baseline buffers are host test scaffolding,
outside the proposed reader budget. Existing production metadata/XML peaks
remain separate observations; authored layout does not establish all-book
layout success or C3 heap headroom.

The minimum future runtime increment is a bounded text reader with this phase
boundary, direct page callbacks into the existing framebuffer and chapter
reopen/semantic-anchor restore. It must release the fixture reader first.
No writable cache is needed for that increment. Reflow latency, Unicode and
migration compatibility remain gates. SDK `BookCatalog` and `PageCache` can
later reduce retained metadata and repeated reflow, but require an explicitly
owned writable cache and a measured cold-build budget; they do not remove
parse/layout overlap during a cold build.

## Migration acceptance contract

| Case | Required behavior before calling migration supported |
| --- | --- |
| Supported CrossInk releases/editions | Publish a tested compatibility matrix naming exact release/build, progress schema and setting schema. Unknown versions receive a clear unsupported result; do not guess schemas or silently reset progress. No release is verified by this slice. |
| Book editions and changed content | Identify exact immutable book content independently of filename. Match renamed/moved identical bytes; do not transfer progress automatically across revised EPUBs, abridgements or translations. Require a reviewable passage match when content differs. |
| Changed library paths | Resolve an explicitly chosen source library and its progress records without moving originals. Preserve relative-path provenance in the import result; detect ambiguous duplicate books and path changes. CrossInk's actual paths/schema require separately authorized source evidence. |
| Legacy progress | Retain the original record untouched. Convert supported chapter/position conventions using the verified source format and EPUB spine mapping. Page numbers, percentages, byte offsets and Newt extracted-codepoint offsets are not interchangeable. Report imported, approximate, unsupported or ambiguous per book, with a passage preview for approximate conversion. |
| Unicode anchors | Test multibyte UTF-8, combining sequences, non-Latin text, entity/whitespace normalization, soft hyphens and chapter boundaries. Save the exact content identity plus a well-defined chapter/text anchor; verify the restored visible passage across reflow. Do not equate byte offsets with codepoints or graphemes. Unicode glyph coverage must render that passage. |
| Settings compatibility | Map only verified equivalent font, size, margins, spacing, orientation and progress-bar settings. Keep unsupported values in the source/import report, use explicit Newt defaults, and make any approximation visible. CrossInk fonts/presets are not assumed equivalent to Newt's current ASCII atlas. |
| Credentials | Reading and import remain offline and require no account, cloud service or device-bound credential. Never copy device-bound keys, credentials or unrelated private state into progress records, logs or migration artifacts. |
| Reversible trial | Preserve original book bytes, directories, legacy progress/settings and a verified restorable legacy firmware/recovery route. Keep Newt trial state separate; allow returning to CrossInk with its original state intact. Firmware restoration is a separately authorized device operation, not something this host experiment performs. |
| Failures and retry | Failed/cancelled imports publish no partial replacement of source state. Retry is deterministic for the same source/content, preserves existing Newt progress unless replacement was requested, and reports conflicts before changing it. |

Acceptance needs authored fixtures for each supported legacy schema, a verified
source-to-Newt anchor/settings mapping, then a separately authorized reversible
device trial. Actual CrossInk version/schema/path evidence, production Unicode
coverage and post-SD-mount C3 memory/latency are currently missing. Obtain that
specific evidence before implementing a narrow importer; the current host
benchmark neither reads legacy state nor claims seamless migration is complete.
