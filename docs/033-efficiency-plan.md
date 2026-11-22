# Fixed reading baseline and bounded efficiency ownership

The exact installed image passed the fixed 23-stage reading/resume/recovery
journey. This is the preserved comparison baseline; owner M1 acceptance,
settings/sleep/battery/optical checks and the proposed 3–5 day trial remain
pending. Efficiency work starts from a separate worktree and branch,
`perf/m1-reader-efficiency`, based on
`7d0672a2d5d02c1e9358718e8970291bf79a2e6a`.

## Frozen baseline

Local annotated tag `m1-fixed-reading-7d0672a` points to the unchanged passed
source/evidence head. `artifacts/m1-fixed-reading-baseline.json` records the
compiled scope objects, image, bundle, installation, journey and evidence hashes.
Six exclusive, hash-verified local copies of the image, source bundle, build
manifest, physical artifact/document and final handoff are protected as 0400
files in a 0700 directory at task-5/.baseline/m1-fixed-reading-7d0672a. No raw
device backup or book was copied there. These files are never build outputs.
The original baseline worktree, artifacts and evidence were not modified.

Image `57ba55228181-regional-experiment`: 1,258,736 bytes, SHA256
`e39bcbd491853aedee03bc2d6c77f2e15904cb5d01b97d9badc61585d437111d`.
Installation/readback and both planned startup checks passed. The complete
journey contains 23 stages, 245 operations and 646 ordered CRC-valid responses.
The reader task had 186 observations and minimum free stack 5,752/16,384 bytes.

Latest observed positions are primary spine 1/character **2607** and sibling
spine 1/character **710**. Post-current-journey durable NVS was not recaptured.
Prewrite 1708 and every older device snapshot are historical and must never
replace this newer progress. Last confirmed state is awake Home, reader closed,
opening false and serial closed. No additional hardware scope is active.

## Existing measurements

`artifacts/m1-efficiency-before.json` projects numeric data from the exact
accepted journey, associating every operation with its monotonic protocol
records. Its derivation script is retained locally under .build with its hash
in the artifact. No additional device traffic was used.

| Existing observation | Count | Median s | Maximum s |
| --- | ---: | ---: | ---: |
| Library focus request to terminal | 170 | 1.490322 | 1.883354 |
| Reader next/previous request to terminal | 6 | 1.607892 | 1.792889 |

Library focus minimum is 1.329073 s and nearest-rank p95 is 1.777162 s. Library
entry paint-path receipts range from 2.132 to 2.349 s. Reader-turn paint paths
range from 1.030 to 1.128 s. These paint receipts include rendering and the
blocking panel call; they do not isolate waveform time. Stage heap/largest-block
snapshots are included for all 23 stages. Reader snapshots range from 69,624 to
69,788 free bytes, with a 51,188-byte largest block. They are not transient
minimum-heap measurements. The complete opening profiles remain in docs/032
and the unchanged physical artifact; cold sibling terminals are 19.255118 and
19.362358 s under the existing 20-second cooperative opening budget.

## Source findings and ordered experiments

1. **Reuse the successfully captured current page on repaint.** In
   src/sample_reader.cpp, navigation lays out a page; renderReaderUi then calls
   SampleReader::paint, which calls showPage for the same page again.
   PhasedReader::layout reconstructs metadata and completes chapter layout on
   both calls. The existing 10,820-byte PhasedPagePlan already owns copied runs
   and text after parser teardown. First investigate reusing that validated plan
   for repaint, with no additional page allocation. Publish validity only after
   the complete chapter, finish checks, glyph checks and anchor/page totals pass.
   Invalidate before any attempt that may overwrite it and on close, book/source
   replacement, settings change, cancellation, stale source or parser failure.
   A failed attempt must never paint a partially replaced plan. Keep explicit
   navigation/restore validation semantics and successful-progress publication.
   The non-borrowed/no-plan reader must retain a correct fallback.
2. **Measure remaining whole-chapter work before adding adjacent page storage.**
   Same-spine selection copies already exist, but currently metadata is rebuilt
   on every layout. Investigate safe reuse within one immutable source session.
   A later adjacent-page experiment is capped at two additional plans (21,640
   payload bytes) and must first justify allocation overhead, fragmentation,
   Expat peak and the 48 KiB allocation requirement against actual heap evidence.
   That cap is a proposal, not an approved resource increase or implementation.
   Do not retain SDK/arena/parser pointers, a whole chapter or whole-book index.
   Do not stop parsing at the requested page: trailing failures, exact page
   totals, empty-chapter handling and parser teardown must remain verified.
3. **Attribute Library delay before changing refresh policy.** The renderer
   produces a monochrome frame. firmware/main.cpp requests a full update on
   invalid/grayscale/orientation transitions or the existing nine-fast-paint
   bound; the pinned X3 driver can independently require full synchronization.
   The current evidence does not record each focus move's requested/effective
   waveform. Separate render/cover I/O from blocking refresh with bounded
   development instrumentation before proposing a focus-only update. Preserve
   controller baseline recovery, footer/title/clock updates, both orientations,
   periodic ghosting cleanup and the single-buffer blocking ownership rule.
   No SDK LUT change, extra framebuffer or asynchronous frame overwrite.
4. **Bound decorative cover reuse independently.** Parent may assign the
   separate owner below. Cover-cache speedup is unmeasured; the accepted journey
   does not establish that real prepared covers were present. It must remain
   optional, fail to placeholders and never authorize opening a book.

## Ownership contracts for parallel work

| Workstream | Owner and exclusive source boundary | Required handoff |
| --- | --- | --- |
| Reader layout/current-page reuse | M1 owner/root: include/newt/phased_reader.h, src/phased_reader.cpp, src/sample_reader.cpp, focused phased/borrowed/parse-storage tests | Exact copied-page validity/invalidation rules, read/layout counts, pixel/anchor/status parity, memory and failure evidence |
| Prepared-cover cache | Reserved for parent-selected separate owner: firmware/library_covers.h, cover-specific helper/header and tests; renderer/API changes proposed before touching src/library_ui.cpp or include/newt/library_cover.h | Exact diff/commit based on 7d0672a, capacity and allocation costs, source/sidecar freshness contract, release/disable hooks, fault and pixel tests; no lifecycle/main writes |
| Library refresh attribution and integration | M1 owner/root: firmware/main.cpp, display receipt/refresh helpers, integration hooks and exact candidate gate | Attributed timing, recovery/focus redraw evidence, resource receipts and one combined reviewed image |
| Review/promotion | Independent bounded adversarial and maintainability reviewers; root integrates | Full gates plus exact parent-reviewed next candidate; no simultaneous edits to shared integration files |

Proposed cover-cache ceiling is one visible shelf: at most four 3,384-byte
rasters (13,536 payload bytes), copied bounded 96-byte filenames and metadata,
with **16 KiB total allocation including overhead**. Static RAM/RTC/framebuffer
ceilings remain unchanged. Cache allocation failure falls back safely. Clear
negative entries on explicit shelf/session refresh; capacity never grows with
catalog size. No catalog/filename/renderer raster pointers may be retained.
The cache owner must hand back a synchronous disable-and-release hook; root will
integrate it **before opening starts**, including the opening-message repaint,
and on leaving Library, sleep, detach/rescan/error. No cache allocation or retained
cache may reduce the reader's open-time heap. Root alone edits lifecycle calls.

Every hit needs a bounded copied key and current file binding; same-size,
same-time substitutions and unobserved same-stat sidecar changes remain possible
unless revalidated. Define invalidation explicitly; never claim cheap stat is
content identity. The existing source binding is size plus
`uint64_t(st_mtime) & ~1` (round down two seconds), matching the host preparation
format. NLC2 remains exactly 28+3,384 = 3,412 bytes, 144×188 1bpp, CRC checked
before display. FAT timestamp behavior on the actual card is still unverified.
Full SHA identity, admission, catalog release before open and NVS startup guards
remain authoritative and outside this decorative workstream. Missing, stale,
short, corrupt or changed input must safely fall back. No book/SD/card/private
artwork transfer is authorized by this contract.

## Promotion gates

Record before/after host timing and source-read/layout counts on the same
authored inputs, with exact bytes/settings and both orientations. Verify pixel,
footer, page count, anchor and status parity; lease denial, allocation failure,
trailing XML/inflate/I/O failure, cancellation, empty chapters, stale source,
rejected open and settings/close/reopen invalidation. Run the full host gate and
all four affected clean native resource variants using project-local approved
tools/caches. Preserve measured symbols, original ceilings and offline security
constraints. Two fresh bounded reviews must be clean before an exact-image
proposal (source, build, hash, size, resources and expected benefits).

No new hardware access occurs before a separately reviewed exact candidate
grant from the parent. Before promotion, repeat the unchanged actual 23-stage
journey and record latency, post-stage heap/largest block, reader-stack minimum
and observations against this baseline. Any new peak-heap or render/panel
instrumentation is explicitly distinguished from existing measurements. Keep
the 20-second opening, 30-second wire and 600-second journey budgets. Never
restore historical NVS to manufacture identical initial progress. Owner optics,
physical focus/buttons, settings, sleep/wake, both orientations and battery
remain separate manual gates. SD migration/catalog admission changes, new app
features and the second-milestone SDK work are outside this efficiency scope.
