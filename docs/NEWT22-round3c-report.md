# NEWT-22 round 3c

The clean starting point was `2fa9c1ed6b92d0a91d5f7140bc2b5da33cd234ab`.
Source checkpoint `1682769da698bf25733bdde416d98500df449054` resolves the
selected-frame failures without changing ceilings, selected entries, exclusions,
adapters, partitions, mount capacity, SRAM or RTC. Flash record 42 is the one new
correction record relative to the branch's existing record 41. Engineering review
and the final complete gates are recorded below; this document is not owner or
hardware acceptance.

## Behavior and ownership

The descriptor rule remains generic: every routed durable-state transaction asks
the owning revision session to retire only its optional Pages read lease. Source
and receipt can retain two of the three SD descriptors; the remaining descriptor
belongs to state. A later cached turn reacquires Pages through the existing full
source/receipt/cache validation. Producer ownership and file limits are unchanged.

`ReadingSaveNotice` owns page and destination uncertainty independently in the
existing one-byte recovery field. RTC v7 uses two previously unused recovery bits,
keeping the record at 68 bytes. Actual startup restores the warm live candidate
into recovery and leaves durable `savedProgress` separate. Opening/publishing a
page cannot acknowledge persistence or clear uncertainty. Verified retry results
retire only the acknowledged component. Back and Sleep proceed with a warning
when storage cannot confirm a commit, retaining the prior durable authority.
Bounded readback reconciles lost acknowledgements; unresolved outcomes mean old
or new complete data may restore, never definitely unsaved. Independent Home
preferences retain their durable commit-before-publication workflow.

The reader's center chapter field shows the single `PG SAVE ERR` constant at
16 px only for the explicit Page uncertainty bit. It measures 103 px inside the
140/200 px portrait/landscape fields. Other storage diagnostics show `STORAGE`;
clear state restores the chapter label. The page fraction and normal overall
percent stay in their original positions. The branch's estimated-percent `~`
remains for the Lead's later integration with main. Home reuses its existing hint
for `Last save unconfirmed`, without a new overlay or buffer.

Non-page diagnostics include destination-only uncertainty (even when its detailed
Reading message mentions a page), unavailable/corrupt resume authority, unverified
or unavailable selected-book state, missing/changed SD source, unavailable reader
session, NVS version/recovery/unavailability, and DEV Home-preference restore or
commit problems. `sdSelection.storageProblem()` is included. None is classified
as a page-save error by text matching. If a page uncertainty coexists with one of
these diagnostics, the explicit Page bit takes precedence. Detailed text remains
in Your X3 and the existing unavailable-reader/Sleep explanations.

Checked setters and consumers include startup initialization and selection,
`SleepSession::restore`, source/session admission, post-open publication,
checkpoint completion, Back, Sleep destination retry, canceled sleep, Settings
reflow, Library open/rollback, later periodic saves, producer-deadline saves,
Home projection and footer rendering. The final sweep found a sibling repaint
gap: Page could change while a foreign or destination diagnostic pointer stayed
identical. Periodic and producer wrappers now compare both pointer and Page state.

## Stack and flash

The small notice method was already outlined by GCC, so explicitly marking it
noinline did not reduce the original DEV frame. Extracting post-open publication
reduced 592 to 576 B. Keeping checkpoint selection out of that frame reduced it
to 560 B. Four clean lanes were measured after each production change, including
the repaint fix. The final measured source is `2951c20`:

| Variant | Flash | Growth over record 41 | SRAM | RTC | `setup()` | `openReader()` |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| default | 2,161,808 B | 1,344 B | 133,232 B | 300 B | 384 B | 144 B |
| DEV USB | 2,302,000 B | 1,408 B | 134,080 B | 304 B | 384 B | 560 B |
| regional | 2,168,512 B | 1,360 B | 133,728 B | 300 B | 384 B | 144 B |
| regional DEV USB | 2,310,704 B | 1,408 B | 134,576 B | 304 B | 384 B | 560 B |

All inherited selected entries fit; counts stay 350 / 379 / 352 / 386, with none
removed or missing. Relative to the accepted pre-R3 measurements, DEV `setup()`
decreases 400 to 384 B and DEV `paint()` decreases 48 to 32 B. All other inherited
selected actuals equal their previous values. Frosted Home stays 496 B, Stage
Home 320 B, and the earlier `armCheckpoint` guard stays 96 B in every lane.
The additional boundaries measure: `publishReaderOpen` 16/32/16/32 B,
checkpoint selection 48 B throughout, and `SleepSession::restore` 112 B throughout.
Individual compiler frames do not bound combined call chains or runtime peaks.
Existing ceilings and complete protected history remain unchanged by owner rule.

Flash growth is about 0.06% of these images and remains below the 7,016,448 B
usable partition limit. The four clean base builds of immutable `683fe1d` reproduce
record 41's exact sizes. ELF comparison attributes the change to:

- The outlined restore boundary adds 678 B default/regional or 952 B DEV, while
  `setup()` shrinks 494/746 B respectively.
- Checkpoint selection adds a 142 B symbol and post-open publication adds
  136/168 B; `openReader()` shrinks 242/290 B respectively.
- Destination recovery, save completion, notice publication, rendering and
  repaint branches supply the remaining code. The three canonical notice strings
  now have named symbols; they also replace existing pooled string literals.
- Mapped text grows 900 / 952 / 906 / 952 B, read-only data 32 / 28 / 32 / 24 B,
  and unwind data 412 / 436 / 424 / 436 B. Image-alignment deltas account for the
  remaining 0 / -8 / -2 / -4 B. Debug information and the no-contents virtual
  alignment section are not flashed payload.

`artifacts/newt22-r3c-symbol-delta.json` records all changed symbols and sections.
Raw base/candidate reports and native command logs are archived with hashes;
record 42's report hashes cover decompressed raw JSON. The prior 41 flash records,
24 protected records and source-policy pins are preserved.

## Regression evidence

The warm-wake matrix rejects progress writes and final-intent writes, discards
ordinary RAM, restores actual deep-sleep startup and post-open publication, then
checks durable bytes, acknowledgement, live recovery, warning and later verified
save. Both orientations include cold old-page/new-page controls. Failed Back
followed by verified Home Sleep clears the destination warning while preserving
unresolved page uncertainty and repainting the change. The three original R3
cases independently failed at their intended runtime assertions on `683fe1d`;
that immutable witness remains archived in the R3 evidence.

Library/Read, unavailable heap/parser, failed chapter, Settings/reflow and wake
evidence fixtures now require verified persistence before retiring uncertainty.
Three legacy startup simulations were replaced with the actual `SleepSession`
path to distinguish durable acknowledgement from RTC live state. A cohesive
99-line warm-ACK fixture keeps the host driver below its existing size threshold.
No resource ceiling or negative control was removed. Expectations now distinguish
RTC live recovery from durable acknowledgement.

The new periodic repaint regression first failed on an unchanged foreign
diagnostic pointer, then passed after the fix. Producer tests independently cover
failure setting Page, verified retry clearing Page, and no visible state change;
removing only the Page comparison fails its exact runtime predicate. All nine
cold-start wiring controls compile and fail their named runtime predicates.

The descriptor witness now requires a real cached turn immediately before the
authority transaction: source/receipt/Pages = 1/1/1, held descriptors = 3. The
earlier quiet maintenance could retire Pages before this boundary, allowing a
weak historical witness to pass. The strengthened test preserves all `213c28f`
production inputs and overlays only test support, with its historical fixture
compatibility documented in the archived manifest. Both old reset routes compile
and fail `cached-page escape contract: checkpoint, Sleep and Back must succeed`:
the immediate fourth open is denied before reads, authority load fails, and
checkpoint result is false. On the candidate, authority load and checkpoint
succeed with zero denials, followed by Sleep and Back. Pre/post-save turns each
prove a cache hit with no ordinary-layout fallback.

The full turn-cost matrix passes 24 heterogeneous fixtures, 3,456 observations,
1,152 cold/produced exact pairs and 1,152 cells. Timings are informational host
measurements. Authored footer PNG evidence (PBM can be regenerated) covers normal, Page error and other
storage states in both geometries under `artifacts/newt22-r3c-footer/`.

Fresh Claude Opus 5.5 high review of `683fe1d..a2b522c` found a live Settings
rollback and sample-only stale destination warning. Source `2951c20` keeps the
open reader authoritative; only an unavailable reader reopens pending recovery.
Both geometries cover failed periodic save, Page-uncertain wake and destination-only
wake, followed by further real turns and Settings reflow. The pre-fix runtime
predicate fails on the old guard. The sample source-copy control fails when its
save is incorrectly given SD destination ownership. Verified sample Back clears
Page, notice, Home hint and pending state. Missing page authority still protects
durable bytes while an independently verified preference may reflow the live page.

The same review removed the test-only message-to-state inference fallback:
destination commits now require an explicit notice reference. Failure-to-success
tests retain that owner. Stale comments are corrected. The complete quick gate
also exposed a deleted test-helper export; all consumers now use the shared
fixture helper, and the replay passes. The initial complete Linux run was
interrupted for the reproduced Settings correction; it is not a passing gate.
Fresh Claude Opus 5.5 high review 2 of `683fe1d..e411ddc` is CLEAN and explicitly
approves record 42. It independently ran the actual Settings gate and reverted
only the live-reader guard to reproduce the exact regression. The optional stale
`armCheckpoint` comment remains: Settings is now its sole production caller.
The complete review and its evidence limits are archived under `artifacts/`.

The complete host run then exposed two more stale component expectations:
sample wake must read its live page from recovery while preserving the separate
acknowledgement, and Library sleep must inspect the policy separately from the
uncertainty bits. Book-route exact recovery also keeps its warning/pending state
until a save is verified. Scratch probes pass those corrected expectations and
both original route negative controls. The known-failing full run was interrupted
only after every other phase had finished, with parity still running, and is
archived as an interrupted run. A fresh complete gate follows the fixture repairs.
No firmware or build-procedure change follows source `2951c20`.

The checkpoint transaction driver passes twice through its normal short-path
fixture. A diagnostic redirection under a longer scratch path exceeded the
fixed 80-byte device path buffer; the failure preceded notice handling. The SD
reading-state fixture now uses a shorter temporary prefix and preserves every
fault assertion. The archived path-bound note records the failed and corrected
host setup; no product path bound or adapter is changed.

## Remaining acceptance boundaries

Quick and focused gates PASS on the final production source. The complete Linux
host gate PASSes on `29f4b583f02c93e3a1611c62a0f258eec565e058` in 858.233 s, including ASan/UBSan,
all 20 reviewed renderer frames, all nine cold-start controls, both descriptor
routes and their durable-restore control, Settings/sample controls, checkpoint
transaction crashes and the two reconciled component fixtures. Every-page parity
PASSes 18 fixtures and 13,408 comparisons in 517.335 s. The earlier full turn-cost
matrix was measured on `1682769`; subsequent changes are lifecycle/save ownership
outside the page-turn mechanism. Its host timings remain informational.

All four post-review clean native lanes PASS on the same clean source identity,
with the exact table sizes, unchanged SRAM/RTC and all selected frames present and
within their existing caps. All sixteen live bin/elf/map/partition hashes match
the fresh reports and receipts; each compiler-input hash equals the reviewed
record-42 measurement. DEV build IDs refer to this clean checkpoint. Later
handoff commits contain documentation/evidence only. No main merge or public
operation is authorized or performed. Pinned release results and the last
read-only conflict inspection are recorded in the final handoff below.

Hands-on USB-powered sleep/wake, unplugged battery sleep/wake and cold power-on
remain separate pending checks, each requiring its power condition and reset
evidence. Exact-source Mac ImageIO coverage also remains pending. Host rendering,
clean firmware compilation and engineering review do not establish physical
behavior or owner-acceptance readiness.

## Final integration handoff

Read-only three-tree comparison of `213c28f` / `3edbbe2` / pinned main
`7cc92ab` predicts these textual conflicts; no merge or battery worktree access
was performed:

- `src/reader_chrome.cpp`, one hunk immediately after chapter/page formatting:
  ours puts `PG SAVE ERR`/`STORAGE` in the center and formats the normal percent
  with the inherited optional `~`; main still puts `UNSAVED` in the right column
  and formats plain percent. The combined behavior needs the center-only status,
  normal plain right percent, main's label spelling, and its 14 px/ellipsis
  `drawField`. The spelling and drawing changes merge automatically in this
  comparison; our branch deliberately leaves `~` alone.
- `tests/reader_chrome_test.cpp`, one insertion hunk after `declaredSectionLabels`:
  our authored `writeFooterEvidence` and main's `counters`/`legibleFields` tests
  both belong there. Retain both; update estimated-percent evidence with main's
  plain percent and preserve both-geometry Page/other-storage checks.
- `code-health-baseline.json`, six hunks: our NEWT-22 record 41, followed by this
  round's record 42, overlaps main's distinct NEWT-23 record 41; the four live
  flash limits also differ. Main's record-41 sizes are
  2,162,192 / 2,302,320 / 2,168,880 / 2,311,024 B. This branch's final record-42
  sizes are 2,161,808 / 2,302,000 / 2,168,512 / 2,310,704 B. Preserve both measured
  histories and the protected budgets; the integrated source needs its own four
  clean measurements and reviewed flash accounting. The costs are not additive
  evidence for an unbuilt combination.
- `docs/CODE_HEALTH.md`, one append hunk: retain both resource explanations and
  their distinct review/measurement provenance.
- Twelve generated live files: four `code-health-*.json`, four
  `firmware-identity-*.json`, and four `*nvs-guard-link-verification.json` receipts.
  Regenerate these from the integrated source. The exact paths and hunk counts
  are in `artifacts/newt22-r3c-integration.json`; the complete read-only comparison
  is archived beside it.

`src/frosted_home.cpp`, `docs/056-reader-reliability.md`, the UI helpers, reader
view declaration and open-phase proof checks merge without textual conflict in
that comparison. This is no claim that the combined tree passes validation.
The pinned release gate PASSes both the full adoption history and
`213c28f..3edbbe2`; the final documentation/evidence commit is checked again.
The Lead's later integration must retain both parents and an explicit
Conventional Commit merge subject, followed by the pinned full adoption gate.


Final bounded Claude Opus 5.5 high review 3 is CLEAN on
`683fe1d..1895c46874a1d861682710667d27988755ae0668`, with no actionable findings.
It independently ran both post-review component fixtures and their original route
controls, then proved the sample-ACK and retained-uncertainty oracles reject their
specific production mutants. It confirmed all production source is identical to
`2951c20`, and that post-build changes are documentation/evidence only. Its full
result and limits are archived in `artifacts/newt22-r3c-review3.md.gz`. The pinned
release gate also PASSed the full adoption history and both `213c28f..HEAD` and
`683fe1d..HEAD` at `1895c46`; the final evidence-only commit is checked once more.
